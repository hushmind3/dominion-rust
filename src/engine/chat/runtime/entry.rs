//! Active turn/room entrypoints and streaming orchestration.
//! Active Rust subtree of chat::runtime.
use super::adaptation::*;
use super::*;

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

pub(crate) async fn run_turn_with_policy(
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
    let elapsed_limit=elapsed_limit_for_session(&session);
    let result = tokio::select! {
        result = tokio::time::timeout(elapsed_limit, run_turn_body(state, &request, tx, &session_id, &metrics, &execution_id, &task_id, record_input, generation, source_message_id)) =>
            result.unwrap_or_else(|_| Err(format!("실행 시간 제한({}분)을 초과했습니다.",elapsed_limit.as_secs()/60))),
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

