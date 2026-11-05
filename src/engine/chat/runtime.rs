//! Turn lifecycle, cancellation, limits, tool orchestration and durable evidence.
use super::{
    approval::{close_pending_approvals, request_approval},
    context::{history, system_prompt},
    protocol::tool_manual,
    provider::{select_provider, stream_completion},
};
use super::{
    parse_text_tool_calls, Metrics, ToolCall, TurnRequest, MAX_ELAPSED, MAX_ROUNDS, MAX_TOKENS, MAX_TOOL_CALLS,
};
use super::evidence::{evidence_for_source, prepare_owner_action};
use crate::engine::goals;
use crate::{
    engine::{
        mcp,
        tools::{self, ToolContext},
    },
    state::AppState,
};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

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
    if !record_input && !session["roomId"].is_string() {
        return Err("공용 대화 기록이 연결된 그룹 세션이 필요합니다.".into());
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
    if let Some(id) = &request.agent_id {
        if state.store.get("agent", id).is_none() {
            return Err(format!("에이전트를 찾을 수 없습니다: {id}"));
        }
    }
    let provider = select_provider(state, &request, &session)?;
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

    let execution = state
        .store
        .put(
            "execution",
            &json!({
                "sessionId": session_id,
                "agentId": request.agent_id,
                "providerId": request.provider_id,
                "model": request.model,
                "reasoningProtocol":provider.reasoning_protocol,
                "thinkingRequested":request.thinking,"reasoningEffortRequested":request.reasoning_effort,
                "status": "running",
                "startedAt": chrono::Utc::now().to_rfc3339(),
                "iterations": 0,
                "modelCalls": 0,
                "toolCalls": 0,
                "tokens": 0,
            }),
        )
        .ok_or("실행 기록을 저장할 수 없습니다.")?;
    let execution_id = execution
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
        .ok_or("실행 ID가 없습니다.")?
        .to_string();
    state.bus.record("execution", &execution);

    let metrics = std::sync::Mutex::new(Metrics::default());
    let result = tokio::select! {
        result = tokio::time::timeout(MAX_ELAPSED, run_turn_body(state, &request, tx, &session_id, &metrics, &execution_id, record_input, generation, source_message_id)) =>
            result.unwrap_or_else(|_| Err("실행 시간 제한(10분)을 초과했습니다.".into())),
        _ = wait_cancelled(state, &session_id, generation) => Err("실행이 중단되었습니다.".into()),
        _ = tx.closed() => Err("응답 연결이 종료되어 실행이 중단되었습니다.".into()),
    };
    finalize_execution(state, &execution_id, &result, &metrics);
    close_pending_approvals(state, &session_id);

    if request.auto_learning && matches!(&result, Ok(Some(_))) {
        crate::engine::learning::enqueue(
            state,
            &session_id,
            request.provider_id.as_deref().unwrap_or(""),
            request.model.as_deref().unwrap_or(""),
        );
    }
    result
}

async fn wait_cancelled(state: &AppState, session_id: &str, generation: u64) {
    while !state.is_cancelled(session_id) && state.cancellation_generation(session_id) == generation
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn run_turn_body(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    metrics: &std::sync::Mutex<Metrics>,
    execution_id: &str,
    record_input: bool,
    generation: u64,
    source_message_id: Option<&str>,
) -> Result<Option<Value>, String> {
    let turn_started = std::time::Instant::now();
    let session = state
        .store
        .get("session", session_id)
        .ok_or("Session not found")?;

    if record_input {
        let attachments =
            crate::engine::attachments::validate_ids(state, session_id, &request.attachment_ids)?;
        let user_message = state
            .store
            .write_message(&json!({
                "session_id": session_id,"role": "user","content": request.input,
                "agent_id": request.agent_id,"metadata":{"authorKind":"owner","attachments":attachments},
            }))
            .ok_or("사용자 메시지를 저장할 수 없습니다.")?;
        state.bus.message(&user_message);
        retitle_if_placeholder(state, &session, &request.input);
    }
    let reply_to = if let Some(id) = source_message_id {
        let source = state
            .store
            .message(id)
            .ok_or("주인의 원문 메시지를 찾을 수 없습니다.")?;
        if source["session_id"] != session_id
            || source["role"] != "user"
            || source["metadata"]["authorKind"] != "owner"
        {
            return Err("그룹 대화 원문과 실행이 일치하지 않습니다.".into());
        }
        Some(id.to_string())
    } else {
        state
            .store
            .messages(session_id)
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["id"].as_str().map(str::to_string))
    };
    // The acceptance baseline is captured BEFORE any tool runs. For rooms the
    // original owner message is the only source of truth; the synthesized
    // group prompt is never treated as an instruction.
    let acceptance_source = match source_message_id {
        Some(id) => state
            .store
            .message(id)
            .and_then(|message| message["content"].as_str().map(str::to_string)),
        None => Some(request.input.clone()),
    };
    let acceptance = acceptance_source
        .as_deref()
        .and_then(|text| prepare_owner_action(state, text));
    let mut acceptance_retries = 0usize;

    let mut metadata = json!({"authorKind":"agent","roomId":session.get("roomId"),"replyToMessageId":reply_to,"executionId":execution_id,"reasoningLanguageRequested":request.reasoning_language,"reasoningTranslated":false});

    let provider = select_provider(state, request, &session)?;
    let system = system_prompt(state, request, session_id, &request.input);
    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(history(state, session_id, request.agent_id.as_deref()));
    metrics.lock().unwrap().context_elapsed_ms = turn_started.elapsed().as_millis() as u64;

    let mut tool_definitions = if request.tools_enabled {
        tools::definitions()
    } else {
        Vec::new()
    };
    if request.tools_enabled {
        tool_definitions.extend(mcp::definitions(state).await);
    }
    if request.tools_enabled && provider.tool_mode == "compat" {
        // Weak/local models without native function calling get a text
        // protocol instead of silently losing tool access.
        let manual = tool_manual(&tool_definitions).await;
        if let Some(first) = messages.first_mut() {
            let existing = first
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            first["content"] = json!(format!("{existing}\n\n{manual}"));
        }
        tool_definitions.clear();
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| error.to_string())?;

    let mut final_content = String::new();
    let mut final_reasoning = String::new();
    let mut last_reasoning = String::new();
    let mut total_tokens: u64 = 0;
    let started_at = std::time::Instant::now();
    metrics.lock().unwrap().context_elapsed_ms = turn_started.elapsed().as_millis() as u64;
    let mut finished = false;

    for round in 0..MAX_ROUNDS {
        if state.is_cancelled(session_id) || state.cancellation_generation(session_id) != generation
        {
            return Err("실행이 중단되었습니다.".into());
        }
        if started_at.elapsed() > MAX_ELAPSED {
            return Err("실행 시간 제한(10분)을 초과했습니다.".into());
        }
        if metrics.lock().unwrap().tool_calls as usize >= MAX_TOOL_CALLS {
            return Err(format!(
                "도구 호출 제한({MAX_TOOL_CALLS}회)을 초과했습니다."
            ));
        }
        {
            let mut counters = metrics.lock().unwrap();
            counters.model_calls += 1;
            counters.iterations = round as u32 + 1;
        }

        let model_started = std::time::Instant::now();
        let model_offset_ms = turn_started.elapsed().as_millis() as u64;
        let completion_result = stream_completion(
            state,
            &client,
            &provider,
            &messages,
            &tool_definitions,
            request,
            session_id,
            tx,
        )
        .await;
        {
            let mut counters = metrics.lock().unwrap();
            counters.model_elapsed_ms += model_started.elapsed().as_millis() as u64;
            if counters.first_token_ms.is_none() {
                if let Ok(completion) = &completion_result {
                    counters.first_token_ms =
                        completion.first_delta_ms.map(|ms| model_offset_ms + ms);
                }
            }
        }
        let mut completion = completion_result?;

        if !request.tools_enabled && !completion.tool_calls.is_empty() {
            return Err("도구가 비활성인 대화에서 모델이 도구 실행을 요청했습니다.".into());
        }

        if !completion.reasoning.is_empty() {
            final_reasoning.push_str(&completion.reasoning);
        }
        last_reasoning = completion.reasoning.clone();
        // Intermediate preambles are stored with their tool-call message, so
        // the final message keeps only the last substantive answer.
        total_tokens += completion.usage;
        metrics.lock().unwrap().tokens = total_tokens;
        if total_tokens > MAX_TOKENS {
            return Err(format!(
                "토큰 예산({MAX_TOKENS})을 초과했습니다. 요청을 나눠 다시 시도하세요."
            ));
        }

        if completion.tool_calls.is_empty()
            && request.tools_enabled
            && provider.tool_mode != "native"
        {
            let (parsed, cleaned) = parse_text_tool_calls(&completion.content);
            if !parsed.is_empty() {
                completion.content = cleaned;
                completion.tool_calls = parsed
                    .into_iter()
                    .map(|(name, arguments)| ToolCall {
                        id: uuid::Uuid::new_v4().to_string(),
                        name,
                        arguments,
                    })
                    .collect();
            }
        }

        if completion.tool_calls.is_empty() {
            final_content = completion.content;

            // Ordinary-chat evidence gate: a model cannot complete an action
            // request by claiming success without successful tool results.
            if let Some((acceptance_text, checks)) = &acceptance {
                let evidence = reply_to
                    .as_deref()
                    .map(|source| evidence_for_source(state, session_id, source, execution_id))
                    .unwrap_or_default();
                let gaps = goals::check_acceptance(state, acceptance_text, checks, &evidence);
                if !gaps.is_empty() {
                    if !request.tools_enabled {
                        return Err(format!(
                            "도구가 비활성 상태라 요청한 작업을 실제로 수행할 수 없습니다: {}",
                            gaps.join("; ")
                        ));
                    }
                    let tool_calls = metrics.lock().unwrap().tool_calls as usize;
                    if acceptance_retries < 2
                        && round + 1 < MAX_ROUNDS
                        && tool_calls + 1 < MAX_TOOL_CALLS
                    {
                        acceptance_retries += 1;
                        let _ = tx.send(json!({
                            "type": "acceptance_retry",
                            "attempt": acceptance_retries,
                            "gaps": gaps,
                        }));
                        state.store.audit(
                            "acceptance.retry",
                            json!({
                                "sessionId": session_id,
                                "executionId": execution_id,
                                "round": round,
                                "gaps": gaps,
                            }),
                        );
                        messages.push(json!({
                            "role": "user",
                            "content": format!(
                                "자동 검증 결과 아직 완료되지 않았습니다:\n- {}\n\n말로만 완료하지 말고, 지금 실제 도구를 호출해 빠진 작업을 수행한 뒤 결과를 확인하세요.",
                                gaps.join("\n- ")
                            )
                        }));
                        continue;
                    }
                    state.store.audit(
                        "acceptance.failed",
                        json!({
                            "sessionId": session_id,
                            "executionId": execution_id,
                            "gaps": gaps,
                        }),
                    );
                    return Err(format!(
                        "실행 증거가 부족하여 완료하지 못했습니다: {}",
                        gaps.join("; ")
                    ));
                }
            }

            finished = true;
            break;
        }

        let tool_calls_json = completion
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments }
                })
            })
            .collect::<Vec<_>>();
        messages.push(json!({
            "role": "assistant",
            "content": completion.content,
            "reasoning_content": completion.reasoning,
            "tool_calls": tool_calls_json,
        }));
        // Persist the tool-call chain so a restart keeps the exact context the
        // model saw instead of only the final answer.
        let saved = state
            .store
            .write_message(&json!({
                "session_id": session_id,
                "role": "assistant",
                "content": completion.content,
                "reasoning": completion.reasoning,
                "agent_id": request.agent_id,
                "tool_calls": tool_calls_json,
                "metadata": metadata,
            }))
            .ok_or("도구 호출 메시지를 저장할 수 없습니다.")?;
        state.bus.message(&saved);

        let mut seen_ids = std::collections::HashSet::new();
        if completion
            .tool_calls
            .iter()
            .any(|call| !seen_ids.insert(&call.id))
        {
            return Err("모델이 중복된 도구 호출 ID를 반환했습니다.".into());
        }
        for (call_index, call) in completion.tool_calls.iter().enumerate() {
            if tx.is_closed() {
                return Err("응답 연결이 종료되어 실행이 중단되었습니다.".into());
            }
            if metrics.lock().unwrap().tool_calls as usize >= MAX_TOOL_CALLS {
                return Err(format!(
                    "도구 호출 제한({MAX_TOOL_CALLS}회)을 초과했습니다."
                ));
            }
            if state.is_cancelled(session_id)
                || state.cancellation_generation(session_id) != generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let arguments: Value = serde_json::from_str(&call.arguments)
                .map_err(|_| format!("{} 도구 인수가 올바른 JSON이 아닙니다.", call.name))?;
            if !arguments.is_object() {
                return Err("도구 인수는 객체여야 합니다.".into());
            }
            let started_at = chrono::Utc::now().to_rfc3339();
            let tool_started = std::time::Instant::now();
            let _ = tx.send(json!({ "type": "tool_start", "name": call.name, "id": call.id }));

            let needs_approval = if call.name.starts_with("mcp__") {
                mcp::requires_approval(state, &call.name)
            } else {
                tools::requires_approval(&call.name)
            };
            let allowed = if needs_approval {
                request_approval(state, tx, session_id, &call.name, &arguments).await
            } else {
                true
            };

            let operation_id = format!("{execution_id}:{round}:{call_index}:{}", call.id);
            state
                .store
                .put(
                    "execution_operation",
                    &json!({
                        "id": operation_id, "runId": execution_id, "sessionId": session_id,
                        "callId": call.id, "tool": call.name, "arguments": arguments,
                        "status": "running", "startedAt": started_at,
                    }),
                )
                .ok_or("도구 실행 시작을 저장할 수 없습니다.")?;
            let (result, ok) = if allowed {
                let context = ToolContext {
                    workspace: &state.config.workspace,
                    store: &state.store,
                    state,
                    session_id,
                    agent_id: request.agent_id.as_deref(),
                };
                let outcome = if call.name.starts_with("mcp__") {
                    mcp::call(state, &call.name, &arguments).await
                } else {
                    tools::execute(&context, &call.name, &arguments).await
                };
                match outcome {
                    Ok(value) if tool_succeeded(&value) => {
                        let _ = tx.send(json!({
                            "type": "tool_end",
                            "name": call.name,
                            "id": call.id,
                            "arguments": arguments,
                            "result": value,
                        }));
                        (value, true)
                    }
                    Ok(value) => {
                        let _ = tx.send(json!({ "type": "tool_error", "name": call.name,
                            "id": call.id, "error": "도구가 실패 결과를 반환했습니다.", "result": value }));
                        (value, false)
                    }
                    Err(error) => {
                        let _ = tx.send(json!({
                            "type": "tool_error",
                            "name": call.name,
                            "id": call.id,
                            "error": error,
                        }));
                        (json!({ "error": error }), false)
                    }
                }
            } else {
                let _ = tx.send(json!({
                    "type": "tool_error",
                    "name": call.name,
                    "id": call.id,
                    "error": "사용자가 실행을 거부했습니다.",
                }));
                (json!({ "error": "Denied by the user" }), false)
            };

            {
                let mut counters = metrics.lock().unwrap();
                counters.tool_calls += 1;
                counters.tool_elapsed_ms += tool_started.elapsed().as_millis() as u64;
            }
            record_tool_stat(state, &call.name, ok);
            state.store.audit(
                "tool.call",
                json!({
                    "tool": call.name,
                    "args": arguments,
                    "allowed": allowed,
                    "round": round,
                }),
            );
            let operation = state
                .store
                .put(
                    "execution_operation",
                    &json!({
                        "id": operation_id,
                        "runId": execution_id,
                        "sessionId": session_id,
                        "callId": call.id,
                        "tool": call.name,
                        "arguments": arguments,
                        "result": result,
                        "status": if ok { "completed" } else { "failed" },
                        "startedAt": started_at,
                        "finishedAt": chrono::Utc::now().to_rfc3339(),
                    }),
                )
                .ok_or("도구 수행 결과를 저장하지 못했습니다. 실행 기록을 확인하세요.")?;
            state.bus.record("execution_operation", &operation);

            let tool_content = result.to_string();
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": tool_content,
            }));
            let saved = state
                .store
                .write_message(&json!({
                    "session_id": session_id,
                    "role": "tool",
                    "content": tool_content,
                    "tool_call_id": call.id,
                    "agent_id": request.agent_id,
                    "metadata": {"authorKind":"tool","roomId":session.get("roomId"),"executionId":execution_id},
                }))
                .ok_or("도구 결과 메시지를 저장할 수 없습니다.")?;
            state.bus.message(&saved);
        }
    }

    if !finished {
        return Err(format!(
            "모델 반복 제한({MAX_ROUNDS}회)에 도달하여 실행을 마치지 못했습니다."
        ));
    }

    // Preserve provider reasoning as received. Display language requests are
    // best effort; never send private reasoning to a hidden translation API.
    metadata["providerReasoning"] = json!(last_reasoning);

    if final_content.trim().is_empty() && final_reasoning.trim().is_empty() {
        return Err("모델이 빈 응답을 반환했습니다.".into());
    }

    let saved = state
        .store
        .write_message(&json!({
            "session_id": session_id,
            "role": "assistant",
            "content": final_content,
            "reasoning": final_reasoning,
            "agent_id": request.agent_id,
            "metadata": metadata,
        }))
        .ok_or("최종 응답을 저장할 수 없습니다.")?;
    state.bus.message(&saved);
    Ok(Some(saved))
}

pub(super) fn tool_succeeded(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) != Some(true)
        && value.get("success").and_then(Value::as_bool) != Some(false)
        && value.get("error").is_none_or(Value::is_null)
        && value
            .get("exitCode")
            .is_none_or(|code| code.as_i64() == Some(0))
        && value
            .get("status")
            .and_then(Value::as_u64)
            .is_none_or(|code| code < 400)
}

pub(super) fn finalize_execution(
    state: &AppState,
    execution_id: &str,
    result: &Result<Option<Value>, String>,
    metrics: &std::sync::Mutex<Metrics>,
) {
    if execution_id.is_empty() {
        return;
    }
    let Some(mut record) = state.store.get("execution", execution_id) else {
        return;
    };
    let counters = metrics.lock().unwrap();
    record["iterations"] = json!(counters.iterations);
    record["tokens"] = json!(counters.tokens);
    record["modelCalls"] = json!(counters.model_calls);
    record["toolCalls"] = json!(counters.tool_calls);
    record["firstTokenMs"] = json!(counters.first_token_ms);
    record["modelElapsedMs"] = json!(counters.model_elapsed_ms);
    record["toolElapsedMs"] = json!(counters.tool_elapsed_ms);
    record["contextElapsedMs"] = json!(counters.context_elapsed_ms);
    record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    match result {
        Ok(_) => record["status"] = json!("completed"),
        Err(message) if message.contains("중단") => {
            record["status"] = json!("cancelled");
            record["reason"] = json!(message);
        }
        Err(message) => {
            record["status"] = json!("failed");
            record["reason"] = json!(message);
        }
    }
    for mut operation in state.store.all("execution_operation") {
        if operation["runId"] == execution_id && operation["status"] == "running" {
            operation["status"] = json!("interrupted");
            operation["outcomeUnknown"] = json!(true);
            operation["reason"] = json!("실행이 중단되어 도구 결과를 확정할 수 없습니다.");
            operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            if let Some(saved) = state.store.put("execution_operation", &operation) {
                state.bus.record("execution_operation", &saved);
            }
        }
    }
    if let Some(saved) = state.store.put("execution", &record) {
        state.bus.record("execution", &saved);
    }
}

fn retitle_if_placeholder(state: &AppState, session: &Value, text: &str) {
    let title = session
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    if !title.is_empty() && title != "새 대화" {
        return;
    }
    let mut updated = session.clone();
    updated["title"] = json!(text.chars().take(40).collect::<String>());
    if let Some(saved) = state.store.put("session", &updated) {
        state.bus.record("session", &saved);
    }
}

fn ensure_session(state: &AppState, request: &TurnRequest) -> Result<Value, String> {
    if let Some(id) = &request.session_id {
        if let Some(session) = state.store.get("session", id) {
            return Ok(session);
        }
        return Err("세션을 찾을 수 없습니다.".into());
    }
    let title = request.input.chars().take(40).collect::<String>();
    let session = state.store.put(
        "session",
        &json!({
            "title": title,
            "agentId": request.agent_id,
            "providerId": request.provider_id,
            "model": request.model,
        }),
    );
    session.ok_or_else(|| "세션을 만들 수 없습니다.".to_string())
}

fn record_tool_stat(state: &AppState, name: &str, ok: bool) {
    state.store.increment_counters(
        "tool_stat",
        name,
        &["attempts", if ok { "successes" } else { "failures" }],
    );
}
