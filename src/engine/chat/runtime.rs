//! Turn lifecycle, cancellation, limits, tool orchestration and durable evidence.
use super::{
    approval::{close_pending_approvals, request_approval},
    context::history,
    provider::{select_provider, stream_completion},
};
use super::{
    parse_text_tool_calls, Metrics, ToolCall, TurnRequest, MAX_ELAPSED, MAX_ROUNDS, MAX_TOKENS, MAX_TOOL_CALLS,
};
use super::evidence::{evidence_for_source, prepare_owner_action};
use crate::engine::adaptation::types::TaskOutcome;
use crate::engine::goals;
use crate::{
    engine::{
        llm, mcp,
        tools::{self, ToolContext},
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[path = "runtime/turn.rs"]
mod turn_body;
use turn_body::run_turn_body;
#[path = "runtime/adaptation.rs"]
mod adaptation;
use adaptation::{run_harness_nodes,failure_observation,environment_fingerprint,
    record_adaptation_error,attempt_model_adaptation};
#[path = "runtime/program.rs"]
mod execution_program;
use execution_program::{restore_program_state,run_program};
#[path = "runtime/tool_catalog.rs"]
mod tool_catalog;
use tool_catalog::{tool_is_read_only,mark_tool_argument_signal,
    normalize_tool_arguments,tool_name,recent_room_tools,select_room_tools,
    select_tool_definitions,trim_tool_definitions};
#[path = "runtime/dispatch.rs"]
mod dispatch;
use dispatch::{frozen_replacements,run_frozen_replacements,dispatch_tool_call,record_soft_tool_error};
#[path = "runtime/planning.rs"]
mod planning;
use planning::{needs_plan,action_plan,review_action};
#[path = "runtime/finalization.rs"]
mod finalization;
pub(super) use finalization::{tool_succeeded,finish_attempt,finalize_execution};
use finalization::{finalization_payload_hash,compliance_summary,compliance_detail,
    determine_task_outcome,record_success_experience,record_selection_snapshot};
#[path = "runtime/session.rs"]
mod session;
use session::{mark_budget_exhausted,mark_run_signals,retitle_if_placeholder,
    ensure_session,record_tool_stat};
#[cfg(test)]
#[path = "runtime/outcome_tests.rs"]
mod outcome_tests;
#[cfg(test)]
#[path = "runtime/harness_tool_selection_tests.rs"]
mod harness_tool_selection_tests;


/// Native RSI workers are bounded to 900 seconds. CPU-only models can need
/// more than the ordinary five-minute request timeout to prefill source and
/// emit a structured tool call; regular chat and short room turns are unchanged.
fn rsi_turn_limit(session: &Value) -> Duration {
    if session["rsiRun"]==true && session["rsiCampaignId"].is_string() {
        Duration::from_secs(840)
    } else {
        MAX_ELAPSED
    }
}

/// Task contracts bound intent to 4096 characters. Keep the full model prompt
/// intact but derive a bounded, content-unique value for the contract.
fn bounded_contract_intent(input: &str) -> String {
    if input.chars().count() <= 4096 {
        return input.to_string();
    }
    format!(
        "{} [{}]",
        input.chars().take(4000).collect::<String>(),
        crate::engine::memory::stable_id(&[input])
    )
}

/// Repository maps, quoted source and prior evidence are context, not tasks.
fn task_input<'a>(session: &'a Value, input: &'a str) -> &'a str {
    if session["rsiRun"]==true {
        session["rsiObjective"].as_str().filter(|text|!text.trim().is_empty()).unwrap_or(input)
    } else {input}
}

fn rsi_output_allowance(spent: u64, input: u64) -> Result<u64,String> {
    let remaining=MAX_TOKENS.saturating_sub(spent).saturating_sub(input);
    if remaining<256 {return Err(format!("토큰 예산({MAX_TOKENS})이 부족해 다음 호출 전에 중단했습니다."));}
    Ok(remaining.min(8192))
}

/// When RSI has spent most of the turn on exploration, stop offering read
/// tools. The model must decide: make the grounded change, or finish without
/// one. This is a decision checkpoint, not a forced write.
fn rsi_should_checkpoint(tool_calls: u64, has_write: bool) -> bool {
    !has_write && tool_calls >= 6
}

/// A real source observation gives the model a grounded edit opportunity.
/// After three successful tool calls, source editing becomes the next phase;
/// if only broad search happened, six calls trigger a read/edit checkpoint.
fn rsi_should_retry_read_only(
    focused_generation: bool, source_observed: bool, has_write: bool, retried: bool,
) -> bool {
    focused_generation && source_observed && !has_write && !retried
}

fn rsi_checkpoint_stage(tool_calls: u64, read_calls: u64, has_write: bool) -> Option<bool> {
    if has_write { return None; }
    if read_calls > 0 && tool_calls >= 3 { return Some(true); }
    rsi_should_checkpoint(tool_calls, has_write).then_some(false)
}

/// Avoid reopening discovery after the first checkpoint. When evidence is
/// already available, the only valid next actions are source edits.
fn rsi_checkpoint_tool(name: &str, edit_only: bool) -> bool {
    if edit_only {
        return matches!(name, "workspace_write" | "workspace_replace" | "workspace_replace_function" | "rsi_system_plan" | "rsi_method_propose");
    }
    matches!(
        name,
        "workspace_read_range" | "workspace_write" | "workspace_replace" | "workspace_replace_function" | "rsi_system_plan" | "rsi_method_propose" | "algorithm_propose"
    )
}

/// The focused editor receives a deliberately narrowed tool menu. Never
/// instruct a 4B model to call workspace_read_range when that tool is absent:
/// the next turn otherwise wastes the only bounded edit retry on an
/// unavailable call instead of using the already indexed source snapshot.
fn rsi_edit_checkpoint_guidance(read_available: bool) -> &'static str {
    if read_available {
        "실제 소스 근거가 있습니다. 새 지능 구조를 rsi_system_plan에 기록하세요. Rust 함수 전체는 workspace_replace_function(path,functionName,newText,expectedSha256)으로, 작은 범위는 workspace_replace(startLine,endLine,newText,expectedSha256)로 수정하세요. 제공된 sourceSha256를 사용하세요. 이전 소스와 불일치하면 workspace_read_range로 현재 줄을 다시 확인하고 expectedSha256를 갱신하세요. 설명만으로 개선은 증명되지 않습니다."
    } else {
        "이번 라운드는 코드 재검색이 아닌 구현 단계입니다. 새 설계는 rsi_system_plan으로 등록하고, 이미 제공된 sourceSha256 근거를 사용해 workspace_replace 또는 workspace_replace_function을 실행하세요. 새 파일은 workspace_write로 생성하세요. 이 단계에서 workspace_read_range는 제공되지 않습니다. 컴파일 및 독립 A/B 검증은 실행기가 담당합니다."
    }
}

/// A finished source write is the handoff to the independent paired
/// evaluator. A model's claimed completion, or even a tool's successful
/// return, is insufficient: the operation must carry an independent SHA-256
/// readback matching the post-image bytes. Only one explicitly focused
/// source target can trigger this early handoff.
fn rsi_patch_ready_for_verification(
    session:&Value,tool:&str,args:&Value,operation:&Value,
)->bool {
    if session["rsiRun"]!=true
        || !session["rsiCampaignId"].is_string()
        || !matches!(tool,"workspace_replace"|"workspace_replace_function")
        || operation["status"]!="completed"
        || operation["result"]["written"]!=true
        || operation["toolReceipt"]["verified"]!=true {
        return false;
    }
    let Some(path)=args["path"].as_str()
        .filter(|path|crate::engine::rsi::safe_path(path)) else {
        return false;
    };
    // Both owner-targeted and autonomous indexed generations may have a
    // fully verified source edit. An owner target is a strict single-file
    // boundary; the autonomous search must prove its target was one of the
    // indexed files chosen BEFORE the model ran.
    if session["rsiExplicitFocus"]==true {
        return session["rsiFocusFiles"].as_array().is_some_and(|files|
            files.len()==1 && files[0].as_str()==Some(path));
    }
    // Recovered compiler-failed patches invalidate old source excerpts. A
    // fresh index and a saved restoration receipt provide the new anchor,
    // while the actual write still requires independent SHA readback.
    let recovered=session["rsiRecoveredPatch"]["status"]=="restored_unverified"
        && session["rsiRecoveredPatch"]["paths"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)));
    session["rsiCodeIndexId"].as_str().is_some_and(|id|!id.is_empty())
        && (session["rsiFocusContextReady"]==true || recovered)
        && session["rsiFocusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(path)))
}

/// Whole-system redesign is a BUNDLE, not "one tool succeeded".
/// The older early-handoff policy stopped as soon as its FIRST verified
/// source file changed, leaving coupled learning, memory and tool edits
/// impossible to finish. A model-authored multi-file contract now keeps the
/// same native turn alive until every declared source has a witnessed write.
/// No simulated success, inferred edit, or unrecorded file earns completion.
fn rsi_complete_system_bundle(session:&Value,operations:&[Value])->bool {
    if session["rsiSystemEvolution"]!=true{return true;}
    let Some(targets)=session["rsiSystemDesign"]["targetFiles"].as_array()
        else{return true;};
    if targets.len()<=1 {return true;}
    targets.iter().all(|target|target.as_str()
        .filter(|path|crate::engine::rsi::safe_path(path))
        .is_some_and(|path|operations.iter().any(|op|{
            op["arguments"]["path"]==path
                && matches!(op["tool"].as_str(),
                    Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))
                && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true
        })))
}

/// Additional bundle-only handoff: a new intelligence module may be
/// created by workspace_write, and a coupled target need not be among the
/// three initial focus excerpts. It is eligible ONLY with an independently
/// witnessed write and an explicit model-authored design path.
fn rsi_verified_bundle_write(session:&Value,args:&Value,op:&Value)->bool {
    // An OWNER-selected single-file boundary always wins. The engine's
    // inherited/learned source-edit checkpoint is not an owner constraint:
    // it must not prevent a model-authored coupled redesign from finishing.
    if session["rsiExplicitFocus"]==true
        && session["rsiPolicyFocus"]!=true {return false;}
    let Some(targets)=session["rsiSystemDesign"]["targetFiles"].as_array()
        .filter(|files|files.len()>1) else{return false;};
    let Some(path)=args["path"].as_str()
        .filter(|p|crate::engine::rsi::safe_path(p)) else{return false;};
    session["rsiSystemEvolution"]==true
        && targets.iter().any(|target|target.as_str()==Some(path))
        && matches!(op["tool"].as_str(),
            Some("workspace_replace"|"workspace_replace_function"|"workspace_write"))
        && op["status"]=="completed"
        && op["result"]["written"]==true
        && op["toolReceipt"]["verified"]==true
}

/// Classify literal native source-tool failures that do not need another LLM
/// planning call. A focused worker will get a bounded read opportunity on
/// the next round. The original error and unsuccessful source write remain
/// in the execution trace; this is not a success claim.
fn rsi_direct_source_repair(
    session:&Value, tool:&str, args:&Value, result:&Value,
)->Option<&'static str> {
    if session["rsiRun"]!=true || session["rsiExplicitFocus"]!=true
        || !session["rsiFocusFiles"].as_array().is_some_and(|files|
            files.len()==1 && files[0]==args["path"]) {return None}
    let error=result["error"].as_str()?;
    if tool=="workspace_write" && error.contains("새 파일 생성 전용") {
        return Some("existing_file_write");
    }
    if !matches!(tool,"workspace_replace"|"workspace_replace_function") {return None}
    if error.contains("endLine") || error.contains("startLine")
        || error.contains("120줄") {return Some("invalid_source_span")}
    if result["staleVersion"]==true || result["preflightBlocked"]==true
        || error.contains("sourceSha256") {
        return Some("source_revision_mismatch");
    }
    None
}

/// If an inherited, persisted RSI procedure already changed the focused
/// source and its bytes were independently read back, hand the changed tree
/// straight to the existing candidate/frozen paired evaluator. Avoid another
/// model round: this is a verified write, NOT an adopted improvement.
fn rsi_inherited_patch_for_evaluation(
    session:&Value, methods:&[Value], operations:&[Value]
)->Option<Value> {
    if session["rsiRun"]!=true || !session["rsiCampaignId"].is_string() {
        return None;
    }
    operations.iter().find_map(|op| {
        if op["origin"]!="procedure"
            || op["tool"]!="workspace_replace"
            || op["status"]!="completed"
            || op["result"]["written"]!=true
            || op["toolReceipt"]["verified"]!=true {
            return None;
        }
        let path=op["arguments"]["path"].as_str()
            .filter(|path|crate::engine::rsi::safe_path(path))?;
        // Owner-focused runs cannot be retargeted. Autonomous generations
        // instead inherit the method's own executable source target.
        if session["rsiExplicitFocus"]==true
            && !session["rsiFocusFiles"].as_array().is_some_and(|files|
                files.len()==1 && files[0].as_str()==Some(path)) {
            return None;
        }
        // The executed node ID is part of the recorded tool provenance.
        // It also permits methods whose target path was bound from a prior
        // DAG node rather than hard-coded into the procedure definition.
        let executed_node=op["procedureNodeId"].as_str()?;
        let method=methods.iter().find(|method| {
            method["rsiMethodExperienceId"].is_string()
                && method["assistanceMode"]=="harness_assisted"
                && method["id"]==op["procedureStrategyId"]
                && method["revision"]==op["procedureRevision"]
                && method["verification"]["nodes"].as_array().is_some_and(|nodes|
                    nodes.iter().any(|node|
                        node["id"]==executed_node
                            && node["tool"]=="workspace_replace"))
        })?;
        Some(json!({
            "methodId":method["id"],
            "methodRevision":method["revision"],
            "operationId":op["id"],
            "path":path,
            "verifiedWrite":true,
            "independentQualityGain":Value::Null
        }))
    })
}

/// A small native-capable model may answer an explicit edit checkpoint
/// with the exact source replacement arguments as JSON instead of a provider
/// function call. Convert ONLY a complete, standalone source edit into a
/// real tool dispatch. The same workspace read/CAS/approval and Git worktree
/// verifier still decide whether any bytes changed.
fn rsi_model_json_edit(
    answer:&str,tools_available:&[Value],session:&Value,
)->Option<ToolCall> {
    if session["rsiRun"]!=true
        || session["rsiCampaignId"].as_str().is_none()
        || session["rsiFocusContextReady"]!=true {
        return None;
    }
    let focus=session["rsiFocusFiles"].as_array()?;
    if focus.len()!=1 {return None;}
    let full=answer.trim();
    if full.len()>32_768 {return None;}
    let text=if full.starts_with("```json") && full.ends_with("```") {
        full.strip_prefix("```json")?.strip_suffix("```")?.trim()
    }else if full.starts_with("```") && full.ends_with("```"){
        full.strip_prefix("```")?.strip_suffix("```")?.trim()
    }else{full};
    let mut args=serde_json::from_str::<Value>(text).ok()?;
    if args.get("name").is_some() || args.get("tool").is_some()
        || args.get("function").is_some() || args.get("action").is_some() {
        return None;
    }
    let path=args["path"].as_str()?;
    if !crate::engine::rsi::safe_path(path)
        || focus[0].as_str()!=Some(path)
        || !tools_available.iter().any(|tool|tool_name(tool)=="workspace_replace") {
        return None;
    }
    let replacement=args["newText"].as_str()?;
    if replacement.trim().is_empty() || replacement.len()>16_384 {return None;}
    let old=args["oldText"].as_str().filter(|v|!v.is_empty());
    let line=|key:&str|args[key].as_u64()
        .or_else(||args[key].as_str()?.parse::<u64>().ok());
    if let Some(old)=old {
        if old==replacement || old.len()>16_384 {return None;}
    } else {
        let (start,end)=(line("startLine")?,line("endLine")?);
        if start==0 || end==0 || start.max(end)-start.min(end)>=120 {return None;}
        if args["expectedSha256"].is_null()
            && args["sourceSha256"].as_str().is_some_and(|sha|
                sha.len()==64 && sha.bytes().all(|v|v.is_ascii_hexdigit())) {
            args["expectedSha256"]=args["sourceSha256"].clone();
        }
        if !args["expectedSha256"].as_str().is_some_and(|sha|
            sha.len()==64 && sha.bytes().all(|v|v.is_ascii_hexdigit())) {
            return None;
        }
    }
    Some(ToolCall{
        id:uuid::Uuid::new_v4().to_string(),
        name:"workspace_replace".into(),
        arguments:args.to_string(),
    })
}

/// Query-based tool discovery must not shrink the RSI toolset: a generation
/// needs read, search and replace together to produce a grounded patch.
fn tool_discovery_applies(is_room: bool, rsi_run: bool) -> bool {
    !is_room && !rsi_run
}

pub async fn run_turn(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, true, true, None).await
}

pub async fn run_turn_background(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, true, None).await
}

/// Resume path for a rejected/woken run: the original owner message is already
/// stored, so the turn must not append a duplicate user message.
pub async fn run_turn_resume(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, false, None).await
}

// Shared room input is already durably stored by the owner-message API.

pub async fn run_room_turn(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    source_message_id: &str,
) -> Result<Option<Value>, String> {
    run_turn_with_policy(state, request, tx, false, false, Some(source_message_id)).await
}

async fn run_turn_with_policy(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    reset_cancel: bool,
    record_input: bool,
    source_message_id: Option<&str>,
) -> Result<Option<Value>, String> {
    let session = ensure_session(state, request)?;
    if record_input && session["roomId"].is_string() {
        return Err("그룹 대화는 그룹 대화 화면 또는 /api/rooms/:id/messages에서 보내세요.".into());
    }
    if !record_input && source_message_id.is_some() && !session["roomId"].is_string() {
        return Err("공용 대화 기록이 연결된 그룹 세션이 필요합니다.".into());
    }
    if !record_input && source_message_id.is_none() {
        let existing = session["id"]
            .as_str()
            .map(|id| state.store.messages(id))
            .unwrap_or_default();
        if existing.is_empty() {
            return Err("재개할 대화 기록이 없습니다.".into());
        }
    }
    let session_id = session
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Session has no id")?
        .to_string();
    if !state.begin_session(&session_id) {
        return Err("Session is already running".into());
    }
    let _session_guard = state.session_guard(&session_id);
    // Durable ownership: a crashed process leaves a lease that startup
    // recovery releases, while a live owner (even across restarts) is refused.
    let _lease = crate::engine::lease::LeaseGuard::acquire(state, &session_id)
        .ok_or("다른 엔진 프로세스가 이 세션을 실행 중입니다.")?;
    if reset_cancel {
        state.clear_cancel(&session_id);
    }
    if state.is_cancelled(&session_id) {
        return Err("실행이 중단되었습니다.".into());
    }
    let generation = state.cancellation_generation(&session_id);
    let mut request = request.clone();
    if request.agent_id.is_none() {
        request.agent_id = session
            .get("agentId")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    let off_requested = request.reasoning_effort.is_none() && request.thinking == Some(false);
    if let Some(id) = &request.agent_id {
        let agent = state.store.get("agent", id)
            .ok_or_else(||format!("에이전트를 찾을 수 없습니다: {id}"))?;
        if request.reasoning_effort.is_none() {
            request.reasoning_effort = agent["reasoningEffort"].as_str()
                .filter(|value|!value.is_empty()).map(str::to_string);
        }
        if request.thinking.is_none() { request.thinking = agent["thinking"].as_bool(); }
    }
    let provider = select_provider(state, &request, &session)?;
    // Codex expresses off as an effort, rather than the legacy boolean.
    // Keep other providers' existing protocol-specific handling unchanged.
    if provider.kind == "codex" && off_requested {
        request.reasoning_effort = Some("none".into());
    }
    if request.reasoning_effort.is_none() {
        request.reasoning_effort = state.store.get("provider", &provider.id)
            .and_then(|record|record["reasoningEffort"].as_str().filter(|value|!value.is_empty()).map(str::to_string));
    }
    request.provider_id = Some(provider.id.clone());
    request.model = Some(provider.model.clone());
    let mut session = session;
    session["providerId"] = json!(provider.id);
    session["model"] = json!(provider.model);
    // A room shares a transcript but each speaker keeps its own identity.
    if session.get("roomId").is_none() {
        session["agentId"] = json!(request.agent_id);
    }
    state
        .store
        .put("session", &session)
        .ok_or("세션 설정을 저장할 수 없습니다.")?;

    let improvement_scope = crate::engine::memory::MemoryScope::for_session(
        &state.store,
        &session_id,
        request.agent_id.as_deref(),
    );
    let execution_id = uuid::Uuid::new_v4().to_string();
    let applied_improvement_ids = crate::engine::improve::injected_ids_with_execution(
        state, &improvement_scope, Some(&execution_id),
    );
    let mut strategy_candidates = crate::engine::improve::active_values_with_execution(
        state, &improvement_scope, Some(&execution_id),
    );
    strategy_candidates.extend(crate::engine::rsi_evolve::method_strategies(state, &session));
    // Derived prompts (goal iterations, room speakers) attach to the root the
    // parent lifecycle created. Only an owner-facing chat turn creates a root.
    let task_id = request.task_id.clone().unwrap_or_else(|| execution_id.clone());
    let parent_task_id = request.parent_task_id.clone();
    let verification_owner = request
        .verification_owner
        .clone()
        .unwrap_or_else(|| "self".into());
    if request.task_id.is_some() {
        let Some(episode) = state.store.get("task_episode", &task_id) else {
            return Err("연결할 상위 작업(task)을 찾을 수 없습니다.".into());
        };
        if episode["contract"]["scope"]["sessionId"].as_str() != Some(session_id.as_str()) {
            return Err("상위 작업이 이 세션에 속하지 않습니다.".into());
        }
        if let Some(expected) = request.task_revision.as_deref() {
            if episode["contract"]["revision"].as_str() != Some(expected) {
                return Err("상위 작업의 계약 revision이 변경되었습니다.".into());
            }
        }
    }
    // The task contract bounds intent to 4096 characters. A self-improvement
    // prompt carries a repository index and can be much larger; keep the full
    // prompt for the model, but derive a bounded, unique contract intent.
    let contract_intent = bounded_contract_intent(task_input(&session,&request.input));
    let root_spec = crate::engine::episode::RootSpec {
        task_id: &task_id,
        intent: &contract_intent,
        family: "chat",
        source: "owner",
        source_message_id: None,
        scope: json!({"partition": "local", "sessionId": session_id}),
        mandatory: Vec::new(),
    };
    let task_revision = crate::engine::episode::revision_for(&root_spec)
        .ok_or("작업 계약을 준비할 수 없습니다.")?;
    let root = crate::engine::episode::root_value(&root_spec)
        .ok_or("작업 계약을 준비할 수 없습니다.")?;
    let execution_document = json!({
        "id": execution_id,
        "taskId": task_id,
        "taskRevision": task_revision,
        "parentTaskId": parent_task_id,
        "sessionId": session_id,
        "agentId": request.agent_id,
        "providerId": request.provider_id,
        "model": request.model,
        "reasoningProtocol":provider.reasoning_protocol,
        "thinkingRequested":request.thinking,"reasoningEffortRequested":request.reasoning_effort,
        "status": "running",
        "phase": "queued",
        "startedAt": chrono::Utc::now().to_rfc3339(),
        "iterations": 0,
        "modelCalls": 0,
        "toolCalls": 0,
        "tokens": 0,
        "request": request.input,
        "appliedImprovementIds": applied_improvement_ids,
        "strategyCandidates": strategy_candidates,
    });
    let attempt = json!({
        "executionId": execution_id,
        "agentId": request.agent_id,
        "verificationOwner": verification_owner,
        "status": "running",
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let execution = state
        .store
        .create_attempt(&root, &execution_document, &attempt)
        .ok_or("실행 기록을 저장할 수 없습니다.")?;
    state.bus.record("execution", &execution);
    crate::engine::run_state::apply(state, &execution_id, "start");
    let metrics = std::sync::Mutex::new(Metrics::default());
    if provider.context_tokens.is_some() {
        if let Some(mut execution) = state.store.get("execution", &execution_id) {
            execution["contextChecked"] = json!(true);
            execution["contextOverflow"] = json!(false);
            state.store.put("execution", &execution);
        }
    }
    let result = tokio::select! {
        result = tokio::time::timeout(rsi_turn_limit(&session), run_turn_body(state, &request, tx, &session_id, &metrics, &execution_id, &task_id, record_input, generation, source_message_id)) =>
            result.unwrap_or_else(|_| Err("실행 시간 제한(10분)을 초과했습니다.".into())),
        // A closed SSE client must not destroy work already in flight. The turn
        // keeps running in its spawned task; only an explicit cancel or the
        // elapsed budget stops it. Results stay durable for later reads.
        _ = wait_cancelled(state, &session_id, generation) => Err("실행이 중단되었습니다.".into()),
    };
    finish_attempt(state, &execution_id, &result, &metrics);
    close_pending_approvals(state, &session_id);

    if request.auto_learning {
        // Do not turn a failed or cancelled execution (including approval
        // denial) into a success-learning sample.
        let task_outcome = state
            .store
            .get("execution", &execution_id)
            .is_some_and(|execution| {
                execution["taskOutcome"] == "succeeded"
                    || (execution["taskOutcome"] == "answered_unverified"
                        && execution["acceptanceText"].is_null())
            });
        if matches!(&result, Ok(Some(_))) && task_outcome {
            crate::engine::learning::enqueue_for_agent_through(
                state,
                &session_id,
                request.agent_id.as_deref(),
                request.provider_id.as_deref().unwrap_or(""),
                request.model.as_deref().unwrap_or(""),
                result.as_ref().ok().and_then(|message|message.as_ref()).and_then(|message|message["id"].as_str()),
            );
        }
        // A correction is worth reflecting on even when the corrective turn
        // itself failed, because the failure is additional evidence.
        if let Some(correction_id) =
            crate::engine::reflect::correction_to_reflect(state, &session_id, &request.input)
        {
            crate::engine::reflect::enqueue(
                state,
                &session_id,
                &correction_id,
                request.agent_id.as_deref(),
                request.provider_id.as_deref().unwrap_or(""),
                request.model.as_deref().unwrap_or(""),
            );
        }
    }
    result
}

async fn wait_cancelled(state: &AppState, session_id: &str, generation: u64) {
    while !state.is_cancelled(session_id) && state.cancellation_generation(session_id) == generation
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

