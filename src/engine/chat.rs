use crate::engine::mcp;
use crate::engine::tools::{self, ToolContext};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

const MAX_ROUNDS: usize = 8;
const HISTORY_LIMIT: usize = 40;

/// One logical turn: a user prompt, optional agent/provider/model overrides.
/// Workflows, rooms and the chat UI all go through this.
#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub session_id: Option<String>,
    pub input: String,
    pub agent_id: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tools_enabled: bool,
    pub thinking: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub system_override: Option<String>,
}

impl TurnRequest {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let text = value
            .get("input")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            return Err("메시지가 비어 있습니다.".into());
        }
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            session_id: string("sessionId"),
            input: text,
            agent_id: string("agentId"),
            provider_id: string("providerId"),
            model: string("model"),
            tools_enabled: value
                .get("toolsEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            thinking: value.get("thinking").and_then(|item| item.as_bool()),
            reasoning_effort: string("reasoningEffort"),
            system_override: string("systemOverride"),
        })
    }
}

#[derive(Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct Completion {
    content: String,
    reasoning: String,
    tool_calls: Vec<ToolCall>,
}

struct ProviderRef {
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    model: String,
}

/// SSE entry point used by `POST /api/chat`.
pub async fn run(state: AppState, input: Value, tx: UnboundedSender<Value>) {
    let session_hint = input
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let result = match TurnRequest::from_value(&input) {
        Ok(request) => run_turn(&state, &request, &tx).await,
        Err(message) => Err(message),
    };

    match result {
        Ok(Some(message)) => {
            let _ = tx.send(json!({
                "type": "assistant_saved",
                "messageId": message.get("id"),
                "sessionId": message.get("session_id"),
            }));
            let _ = tx.send(json!({
                "type": "done",
                "sessionId": message.get("session_id").cloned().unwrap_or(json!(session_hint)),
            }));
        }
        Ok(None) => {
            let _ = tx.send(json!({ "type": "done", "sessionId": session_hint }));
        }
        Err(message) => {
            let _ = tx.send(json!({ "type": "error", "message": message }));
            let _ = tx.send(json!({ "type": "done", "sessionId": session_hint }));
        }
    }
}

/// Executes a full turn (model + tools + persistence) without owning the SSE
/// transport, so background jobs can reuse exactly the same engine.
pub async fn run_turn(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
) -> Result<Option<Value>, String> {
    let session = ensure_session(state, request)?;
    let session_id = session
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("Session has no id")?
        .to_string();
    state.clear_cancel(&session_id);

    let execution = state.store.put(
        "execution",
        &json!({
            "sessionId": session_id,
            "agentId": request.agent_id,
            "providerId": request.provider_id,
            "model": request.model,
            "status": "running",
            "startedAt": chrono::Utc::now().to_rfc3339(),
            "iterations": 0,
            "modelCalls": 0,
            "toolCalls": 0,
            "tokens": 0,
        }),
    );
    let execution_id = execution
        .as_ref()
        .and_then(|record| record.get("id"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if let Some(record) = &execution {
        state.bus.record("execution", record);
    }

    let metrics = std::sync::Mutex::new(Metrics::default());
    let result =
        run_turn_body(state, request, tx, &session_id, &metrics, &execution_id).await;
    finalize_execution(state, &execution_id, &result, &metrics);
    result
}

#[derive(Default)]
struct Metrics {
    model_calls: u32,
    tool_calls: u32,
    iterations: u32,
}

async fn run_turn_body(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    metrics: &std::sync::Mutex<Metrics>,
    execution_id: &str,
) -> Result<Option<Value>, String> {
    let session = state
        .store
        .get("session", session_id)
        .ok_or("Session not found")?;

    let user_message = state.store.write_message(&json!({
        "session_id": session_id,
        "role": "user",
        "content": request.input,
        "agent_id": request.agent_id,
    }));
    if let Some(message) = &user_message {
        state.bus.message(message);
    }
    retitle_if_placeholder(state, &session, &request.input);

    let provider = select_provider(state, request, &session)?;
    let system = system_prompt(state, request);
    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(history(state, session_id));

    let mut tool_definitions = if request.tools_enabled {
        tools::definitions()
    } else {
        Vec::new()
    };
    if request.tools_enabled {
        tool_definitions.extend(mcp::definitions(state).await);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| error.to_string())?;

    let mut final_content = String::new();
    let mut final_reasoning = String::new();

    for round in 0..MAX_ROUNDS {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        {
            let mut counters = metrics.lock().unwrap();
            counters.model_calls += 1;
            counters.iterations = round as u32 + 1;
        }

        let completion = stream_completion(
            state,
            &client,
            &provider,
            &messages,
            &tool_definitions,
            request,
            session_id,
            tx,
        )
        .await?;

        if !completion.reasoning.is_empty() {
            final_reasoning.push_str(&completion.reasoning);
        }
        if !completion.content.is_empty() {
            final_content.push_str(&completion.content);
        }

        if completion.tool_calls.is_empty() {
            break;
        }

        messages.push(json!({
            "role": "assistant",
            "content": completion.content,
            "tool_calls": completion.tool_calls.iter().map(|call| json!({
                "id": call.id,
                "type": "function",
                "function": { "name": call.name, "arguments": call.arguments }
            })).collect::<Vec<_>>(),
        }));

        for call in &completion.tool_calls {
            if tx.is_closed() {
                return Ok(None);
            }
            if state.is_cancelled(session_id) {
                return Err("실행이 중단되었습니다.".into());
            }
            let arguments: Value =
                serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
            let started_at = chrono::Utc::now().to_rfc3339();
            let _ = tx.send(json!({ "type": "tool_start", "name": call.name, "id": call.id }));

            let needs_approval = if call.name.starts_with("mcp__") {
                mcp::requires_approval(state, &call.name)
            } else {
                tools::requires_approval(&call.name)
            };
            let allowed = if needs_approval {
                request_approval(state, tx, &call.name, &arguments).await
            } else {
                true
            };

            let (result, ok) = if allowed {
                let context = ToolContext {
                    workspace: &state.config.workspace,
                    store: &state.store,
                };
                let outcome = if call.name.starts_with("mcp__") {
                    mcp::call(state, &call.name, &arguments).await
                } else {
                    tools::execute(&context, &call.name, &arguments).await
                };
                match outcome {
                    Ok(value) => {
                        let _ = tx.send(json!({
                            "type": "tool_end",
                            "name": call.name,
                            "id": call.id,
                        }));
                        (value, true)
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

            metrics.lock().unwrap().tool_calls += 1;
            state.store.audit(
                "tool.call",
                json!({
                    "tool": call.name,
                    "args": arguments,
                    "allowed": allowed,
                    "round": round,
                }),
            );
            let operation = state.store.put(
                "execution_operation",
                &json!({
                    "runId": execution_id,
                    "callId": call.id,
                    "tool": call.name,
                    "status": if ok { "completed" } else { "failed" },
                    "startedAt": started_at,
                    "finishedAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            if let Some(operation) = operation {
                state.bus.record("execution_operation", &operation);
            }

            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": result.to_string(),
            }));
        }
    }

    if final_content.trim().is_empty() && final_reasoning.trim().is_empty() {
        return Ok(None);
    }

    let saved = state.store.write_message(&json!({
        "session_id": session_id,
        "role": "assistant",
        "content": final_content,
        "reasoning": final_reasoning,
        "agent_id": request.agent_id,
    }));
    if let Some(message) = &saved {
        state.bus.message(message);
    }
    Ok(saved)
}

fn finalize_execution(
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
    record["modelCalls"] = json!(counters.model_calls);
    record["toolCalls"] = json!(counters.tool_calls);
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

fn select_provider(
    state: &AppState,
    request: &TurnRequest,
    session: &Value,
) -> Result<ProviderRef, String> {
    let agent = request
        .agent_id
        .as_deref()
        .and_then(|id| state.store.get("agent", id));

    let provider_id = request
        .provider_id
        .clone()
        .or_else(|| {
            agent
                .as_ref()
                .and_then(|agent| agent.get("providerId"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            session
                .get("providerId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });

    let provider = match provider_id {
        Some(id) => state.store.get("provider", &id),
        None => state
            .store
            .all("provider")
            .into_iter()
            .find(|provider| {
                provider
                    .get("enabled")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(true)
            }),
    };

    let Some(provider) = provider else {
        return Err(
            "모델 제공자가 설정되지 않았습니다. 모델 관리에서 OpenAI 호환 제공자를 추가하세요.".into(),
        );
    };

    if provider.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
        return Err("선택한 제공자가 비활성 상태입니다.".into());
    }

    let id = provider
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let base_url = provider
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return Err("제공자에 baseUrl 이 없습니다.".into());
    }

    let model = request
        .model
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            agent
                .as_ref()
                .and_then(|agent| agent.get("model"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            provider
                .get("model")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "gpt-4o-mini".into());

    let api_key = state
        .read_secret(&format!("provider:{id}"))
        .or_else(|| {
            provider
                .get("apiKey")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();

    Ok(ProviderRef {
        name: provider
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("Provider")
            .to_string(),
        base_url,
        api_key,
        kind: provider
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("openai")
            .to_string(),
        model,
    })
}

fn system_prompt(state: &AppState, request: &TurnRequest) -> String {
    let mut prompt = String::from(
        "You are DOMINION, an agentic AI workspace running on a Rust engine. \
         Answer in Korean unless the user writes in another language. \
         You can call real tools. Never claim a tool action succeeded unless it actually ran. \
         When a tool fails, report the failure honestly.",
    );
    prompt.push_str(&format!(
        "\nWorkspace root: {}\nCurrent date: {}",
        state.config.workspace.display(),
        chrono::Local::now().format("%Y-%m-%d")
    ));
    if let Some(agent_id) = &request.agent_id {
        if let Some(agent) = state.store.get("agent", agent_id) {
            if let Some(extra) = agent.get("systemPrompt").and_then(|value| value.as_str()) {
                if !extra.trim().is_empty() {
                    prompt.push_str("\n\nAgent instructions:\n");
                    prompt.push_str(extra);
                }
            }
        }
    }
    if let Some(extra) = &request.system_override {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }
    prompt
}

fn history(state: &AppState, session_id: &str) -> Vec<Value> {
    let all = state.store.messages(session_id);
    let start = all.len().saturating_sub(HISTORY_LIMIT);
    all[start..]
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?;
            let content = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match role {
                "user" | "assistant" if !content.trim().is_empty() => {
                    Some(json!({ "role": role, "content": content }))
                }
                "tool" => Some(json!({
                    "role": "tool",
                    "tool_call_id": message.get("tool_call_id").cloned().unwrap_or(Value::Null),
                    "content": content,
                })),
                _ => None,
            }
        })
        .collect()
}

async fn stream_completion(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
) -> Result<Completion, String> {
    if provider.kind == "anthropic" {
        return stream_anthropic(state, client, provider, messages, tools, request, session_id, tx)
            .await;
    }

    let mut body = json!({
        "model": provider.model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(effort) = &request.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    if request.thinking == Some(false) {
        body["chat_template_kwargs"] = json!({ "enable_thinking": false });
    }

    let response = client
        .post(format!("{}/chat/completions", provider.base_url))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("{} 연결 실패: {error}", provider.name))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "{} 호출 실패 (HTTP {status}): {}",
            provider.name,
            text.chars().take(600).collect::<String>()
        ));
    }

    let mut completion = Completion::default();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let chunk = chunk.map_err(|error| format!("스트림 오류: {error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        buffer = buffer.replace("\r\n", "\n");

        while let Some(separator) = buffer.find("\n\n") {
            let block: String = buffer.drain(..separator + 2).collect();
            let payload = block
                .lines()
                .filter(|line| line.starts_with("data:"))
                .map(|line| line[5..].trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(piece) = serde_json::from_str::<Value>(&payload) else {
                continue;
            };
            if let Some(error) = piece.get("error") {
                return Err(format!("모델 오류: {error}"));
            }
            let Some(delta) = piece
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
            else {
                continue;
            };

            if let Some(content) = delta.get("content").and_then(|value| value.as_str()) {
                if !content.is_empty() {
                    completion.content.push_str(content);
                    let _ = tx.send(json!({ "type": "text", "text": content }));
                }
            }

            let reasoning = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !reasoning.is_empty() {
                completion.reasoning.push_str(reasoning);
                let _ = tx.send(json!({ "type": "reasoning", "text": reasoning }));
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|value| value.as_array()) {
                for call in tool_calls {
                    let index = call
                        .get("index")
                        .and_then(|value| value.as_u64())
                        .unwrap_or(calls.len() as u64) as usize;
                    while calls.len() <= index {
                        calls.push(ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                    }
                    let slot = &mut calls[index];
                    if let Some(id) = call.get("id").and_then(|value| value.as_str()) {
                        if !id.is_empty() {
                            slot.id = id.to_string();
                        }
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) = function.get("name").and_then(|value| value.as_str()) {
                            slot.name.push_str(name);
                        }
                        if let Some(arguments) =
                            function.get("arguments").and_then(|value| value.as_str())
                        {
                            slot.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }
    }

    completion.tool_calls = calls
        .into_iter()
        .filter(|call| !call.name.is_empty())
        .map(|mut call| {
            if call.id.is_empty() {
                call.id = uuid::Uuid::new_v4().to_string();
            }
            call
        })
        .collect();

    Ok(completion)
}

/// Anthropic Messages API streaming adapter.
async fn stream_anthropic(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
) -> Result<Completion, String> {
    let system = messages
        .iter()
        .filter(|message| message.get("role").and_then(|value| value.as_str()) == Some("system"))
        .filter_map(|message| message.get("content").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("\n\n");

    let converted: Vec<Value> = messages
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?;
            if role == "system" {
                return None;
            }
            let content = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            Some(json!({ "role": if role == "assistant" { "assistant" } else { "user" }, "content": content }))
        })
        .collect();

    let mut body = json!({
        "model": provider.model,
        "system": system,
        "messages": converted,
        "max_tokens": 4096,
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools
            .iter()
            .filter_map(|definition| {
                let function = definition.get("function")?;
                Some(json!({
                    "name": function.get("name")?,
                    "description": function.get("description"),
                    "input_schema": function.get("parameters"),
                }))
            })
            .collect::<Vec<_>>());
    }
    let _ = request;

    let response = client
        .post(format!("{}/messages", provider.base_url))
        .header("content-type", "application/json")
        .header("x-api-key", provider.api_key.clone())
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("{} 연결 실패: {error}", provider.name))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!(
            "{} 호출 실패 (HTTP {status}): {}",
            provider.name,
            text.chars().take(600).collect::<String>()
        ));
    }

    let mut completion = Completion::default();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let chunk = chunk.map_err(|error| format!("스트림 오류: {error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        buffer = buffer.replace("\r\n", "\n");
        while let Some(separator) = buffer.find("\n\n") {
            let block: String = buffer.drain(..separator + 2).collect();
            let payload = block
                .lines()
                .filter(|line| line.starts_with("data:"))
                .map(|line| line[5..].trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            if payload.is_empty() {
                continue;
            }
            let Ok(piece) = serde_json::from_str::<Value>(&payload) else {
                continue;
            };
            let kind = piece.get("type").and_then(|value| value.as_str()).unwrap_or("");
            match kind {
                "content_block_delta" => {
                    let delta = &piece["delta"];
                    match delta.get("type").and_then(|value| value.as_str()) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(|value| value.as_str()) {
                                if !text.is_empty() {
                                    completion.content.push_str(text);
                                    let _ = tx.send(json!({ "type": "text", "text": text }));
                                }
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(text) = delta.get("thinking").and_then(|value| value.as_str())
                            {
                                if !text.is_empty() {
                                    completion.reasoning.push_str(text);
                                    let _ = tx.send(json!({ "type": "reasoning", "text": text }));
                                }
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) =
                                delta.get("partial_json").and_then(|value| value.as_str())
                            {
                                if let Some(call) = calls.last_mut() {
                                    call.arguments.push_str(partial);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_start" => {
                    if let Some(block) = piece.get("content_block") {
                        if block.get("type").and_then(|value| value.as_str()) == Some("tool_use") {
                            calls.push(ToolCall {
                                id: block
                                    .get("id")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                name: block
                                    .get("name")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                arguments: String::new(),
                            });
                        }
                    }
                }
                "error" => {
                    return Err(format!("모델 오류: {}", piece.get("error").cloned().unwrap_or_default()));
                }
                _ => {}
            }
        }
    }

    completion.tool_calls = calls
        .into_iter()
        .filter(|call| !call.name.is_empty())
        .map(|mut call| {
            if call.id.is_empty() {
                call.id = uuid::Uuid::new_v4().to_string();
            }
            call
        })
        .collect();
    Ok(completion)
}

async fn request_approval(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    tool: &str,
    args: &Value,
) -> bool {
    let policy = state
        .store
        .get("policy", "default")
        .unwrap_or_else(|| json!({}));
    if policy
        .get("autoApproveAll")
        .and_then(|value| value.as_bool())
        == Some(true)
    {
        return true;
    }
    if let Some(list) = policy.get("autoApprove").and_then(|value| value.as_array()) {
        if list.iter().any(|entry| entry.as_str() == Some(tool)) {
            return true;
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    state
        .approvals
        .lock()
        .unwrap()
        .insert(id.clone(), sender);

    let record = state.store.put(
        "approval",
        &json!({
            "id": id,
            "tool": tool,
            "args": args,
            "status": "pending",
            "requestedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state
        .bus
        .approval(record.as_ref().unwrap_or(&json!({})));
    let _ = tx.send(json!({
        "type": "approval",
        "id": id,
        "tool": tool,
        "args": args,
    }));

    let outcome = tokio::time::timeout(Duration::from_secs(120), receiver).await;
    state.approvals.lock().unwrap().remove(&id);

    let (allowed, status) = match outcome {
        Ok(Ok(true)) => (true, "approved"),
        Ok(Ok(false)) => (false, "denied"),
        Ok(Err(_)) => (false, "cancelled"),
        Err(_) => (false, "expired"),
    };

    if let Some(mut record) = state.store.get("approval", &id) {
        record["status"] = json!(status);
        record["decision"] = json!(if allowed { "allow" } else { "deny" });
        record["resolvedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("approval", &record);
    }
    allowed
}
