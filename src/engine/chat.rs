use crate::engine::mcp;
use crate::engine::tools::{self, ToolContext};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

const MAX_ROUNDS: usize = 8;
const MAX_TOOL_CALLS: usize = 40;
const MAX_ELAPSED: Duration = Duration::from_secs(600);
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
    pub reasoning_language: Option<String>,
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
            reasoning_language: string("reasoningLanguage"),
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
    usage: u64,
}

struct ProviderRef {
    name: String,
    base_url: String,
    api_key: String,
    kind: String,
    model: String,
    tool_mode: String,
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
    if !state.begin_session(&session_id) {
        return Err("Session is already running".into());
    }
    let _session_guard = state.session_guard(&session_id);

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
    tokens: u64,
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
    let system = system_prompt(state, request, &request.input);
    let mut messages = vec![json!({ "role": "system", "content": system })];
    messages.extend(history(state, session_id));

    let mut tool_definitions = if request.tools_enabled && provider.tool_mode != "compat" {
        tools::definitions()
    } else {
        Vec::new()
    };
    if request.tools_enabled && provider.tool_mode != "compat" {
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
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| error.to_string())?;

    let mut final_content = String::new();
    let mut final_reasoning = String::new();
    let mut total_tokens: u64 = 0;
    let started_at = std::time::Instant::now();

    for round in 0..MAX_ROUNDS {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        if started_at.elapsed() > MAX_ELAPSED {
            return Err("실행 시간 제한(10분)을 초과했습니다.".into());
        }
        if metrics.lock().unwrap().tool_calls as usize >= MAX_TOOL_CALLS {
            return Err(format!("도구 호출 제한({MAX_TOOL_CALLS}회)을 초과했습니다."));
        }
        {
            let mut counters = metrics.lock().unwrap();
            counters.model_calls += 1;
            counters.iterations = round as u32 + 1;
        }

        let mut completion = stream_completion(
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
        // Intermediate preambles are stored with their tool-call message, so
        // the final message keeps only the last substantive answer.
        if !completion.content.is_empty() {
            final_content = completion.content.clone();
        }
        total_tokens += completion.usage;
        metrics.lock().unwrap().tokens = total_tokens;

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
            break;
        }

        let tool_calls_json = completion.tool_calls.iter().map(|call| json!({
            "id": call.id,
            "type": "function",
            "function": { "name": call.name, "arguments": call.arguments }
        })).collect::<Vec<_>>();
        messages.push(json!({
            "role": "assistant",
            "content": completion.content,
            "tool_calls": tool_calls_json,
        }));
        // Persist the tool-call chain so a restart keeps the exact context the
        // model saw instead of only the final answer.
        if let Some(saved) = state.store.write_message(&json!({
            "session_id": session_id,
            "role": "assistant",
            "content": completion.content,
            "agent_id": request.agent_id,
            "tool_calls": tool_calls_json,
        })) {
            state.bus.message(&saved);
        }

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
                    state,
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

            let tool_content = result.to_string();
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": tool_content,
            }));
            if let Some(saved) = state.store.write_message(&json!({
                "session_id": session_id,
                "role": "tool",
                "content": tool_content,
                "tool_call_id": call.id,
                "agent_id": request.agent_id,
            })) {
                state.bus.message(&saved);
            }
        }
    }

    if request.reasoning_language.as_deref() == Some("ko")
        && crate::engine::translate::needs_korean(&final_reasoning)
    {
        if let Some(korean) = crate::engine::translate::to_korean(&final_reasoning).await {
            final_reasoning = korean;
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
    record["tokens"] = json!(counters.tokens);
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

    let kind = provider
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("openai")
        .to_string();
    let tool_mode = provider
        .get("toolMode")
        .and_then(|value| value.as_str())
        .filter(|value| matches!(*value, "native" | "compat" | "auto"))
        .map(str::to_string)
        .unwrap_or_else(|| if kind == "ollama" { "compat".into() } else { "auto".into() });

    Ok(ProviderRef {
        name: provider
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("Provider")
            .to_string(),
        base_url,
        api_key,
        kind,
        model,
        tool_mode,
    })
}

fn system_prompt(state: &AppState, request: &TurnRequest, query: &str) -> String {
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
    if let Some(context) = context_block(state, query) {
        prompt.push_str("\n\n");
        prompt.push_str(&context);
    }
    if request.reasoning_language.as_deref() == Some("ko") {
        prompt.push_str(
            "\n\nWrite every internal reasoning / thinking step in Korean (한국어).              The final answer must also be in Korean unless the user asks otherwise.",
        );
    }
    if let Some(extra) = &request.system_override {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }
    prompt
}

async fn tool_manual(native: &[Value]) -> String {
    let mut manual = String::from(
        "You cannot use native function calling. To call a tool, output exactly one line:\n\
         <tool_call>{\"name\":\"tool_name\",\"arguments\":{...}}</tool_call>\n\
         Available tools:",
    );
    let mut seen = std::collections::HashSet::new();
    for definition in native {
        let function = &definition["function"];
        let name = function.get("name").and_then(|value| value.as_str()).unwrap_or("");
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        manual.push_str(&format!(
            "\n- {}: {}",
            name,
            function.get("description").and_then(|value| value.as_str()).unwrap_or("")
        ));
    }
    manual
}

/// Parses text-based tool calls used by small models and `toolMode: compat`.
pub fn parse_text_tool_calls(content: &str) -> (Vec<(String, String)>, String) {
    let mut calls = Vec::new();
    let mut cleaned = content.to_string();

    while let Some(start) = cleaned.find("<tool_call>") {
        let Some(end) = cleaned[start..].find("</tool_call>") else { break };
        let end = start + end + "</tool_call>".len();
        let block = cleaned[start + "<tool_call>".len()..end - "</tool_call>".len()].to_string();
        if let Some(call) = parse_tool_json(&block) {
            calls.push(call);
        }
        cleaned.replace_range(start..end, "");
    }

    // ```json {"tool": ...} ``` fences
    if calls.is_empty() {
        let mut rest = String::new();
        let mut cursor = 0;
        while let Some(open) = cleaned[cursor..].find("```") {
            let open = cursor + open;
            let after = open + 3;
            let Some(close) = cleaned[after..].find("```") else { break };
            let close = after + close;
            let body = &cleaned[after..close];
            let body = body.trim_start_matches(|ch| ch == 'j' || ch == 's' || ch == 'o' || ch == 'n' || ch == '\n');
            if let Some(call) = parse_tool_json(body) {
                calls.push(call);
                rest.push_str(&cleaned[cursor..open]);
                cursor = close + 3;
            } else {
                rest.push_str(&cleaned[cursor..close + 3]);
                cursor = close + 3;
            }
        }
        rest.push_str(&cleaned[cursor..]);
        if !calls.is_empty() {
            cleaned = rest;
        }
    }
    (calls, cleaned.trim().to_string())
}

fn parse_tool_json(text: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    let name = value
        .get("name")
        .or_else(|| value.get("tool"))
        .and_then(|item| item.as_str())?
        .to_string();
    let arguments = value
        .get("arguments")
        .or_else(|| value.get("args"))
        .or_else(|| value.get("parameters"))
        .or_else(|| value.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    Some((name, arguments.to_string()))
}

/// Relevant memories and enabled skills are injected every turn, matching the
/// Node engine's memoryContext + enabled-skill behavior.
fn context_block(state: &AppState, query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect();

    let mut memories = state
        .store
        .all("memory")
        .into_iter()
        .filter_map(|memory| {
            let text = memory.get("text").and_then(|v| v.as_str())?.to_string();
            let lower = text.to_lowercase();
            let score = tokens.iter().filter(|token| lower.contains(token.as_str())).count();
            (score > 0).then_some((score, text))
        })
        .collect::<Vec<_>>();
    memories.sort_by(|a, b| b.0.cmp(&a.0));
    memories.truncate(5);

    let mut skills = state
        .store
        .all("skill")
        .into_iter()
        .filter(|skill| skill.get("enabled").and_then(|v| v.as_bool()) != Some(false))
        .filter_map(|skill| {
            let name = skill.get("name").and_then(|v| v.as_str())?.to_string();
            let instructions = skill
                .get("instructions")
                .or_else(|| skill.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .chars()
                .take(700)
                .collect::<String>();
            Some((name, instructions))
        })
        .collect::<Vec<_>>();
    skills.truncate(5);

    if memories.is_empty() && skills.is_empty() {
        return None;
    }
    let mut block = String::from("Relevant context from previous sessions:");
    for (_, text) in &memories {
        block.push_str("\n- 기억: ");
        block.push_str(&text.chars().take(400).collect::<String>());
    }
    if !skills.is_empty() {
        block.push_str("\n\nEnabled skills (follow when applicable):");
        for (name, instructions) in &skills {
            block.push_str(&format!("\n- {name}: {instructions}"));
        }
    }
    Some(block)
}

fn history(state: &AppState, session_id: &str) -> Vec<Value> {
    let mut all = state.store.messages(session_id);
    let mut prefix = Vec::new();
    if let Some(compaction) = state.store.get("session_compaction", session_id) {
        let through = compaction
            .get("throughMessageId")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if let Some(index) = all.iter().position(|row| {
            row.get("id").and_then(|value| value.as_str()) == Some(through)
        }) {
            all.drain(..=index);
            if let Some(summary) = compaction.get("summary").and_then(|value| value.as_str()) {
                prefix.push(json!({
                    "role": "system",
                    "content": format!("이전 대화 요약(압축됨):\n{summary}"),
                }));
            }
        }
    }
    let start = all.len().saturating_sub(HISTORY_LIMIT);
    let mut out = prefix;
    out.extend(all[start..]
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?;
            let content = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match role {
                "assistant" => {
                    let tool_calls = message.get("tool_calls").cloned().filter(|v| !v.is_null());
                    if content.trim().is_empty() && tool_calls.is_none() {
                        return None;
                    }
                    let mut item = json!({ "role": "assistant", "content": content });
                    if let Some(calls) = tool_calls {
                        item["tool_calls"] = calls;
                    }
                    Some(item)
                }
                "user" if !content.trim().is_empty() => {
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
        .collect::<Vec<_>>());
    out
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
            if let Some(total) = piece
                .get("usage")
                .and_then(|usage| usage.get("total_tokens"))
                .and_then(|value| value.as_u64())
            {
                completion.usage = total;
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
                "message_start" => {
                    if let Some(input) = piece
                        .get("message")
                        .and_then(|message| message.get("usage"))
                        .and_then(|usage| usage.get("input_tokens"))
                        .and_then(|value| value.as_u64())
                    {
                        completion.usage += input;
                    }
                }
                "message_delta" => {
                    if let Some(output) = piece
                        .get("usage")
                        .and_then(|usage| usage.get("output_tokens"))
                        .and_then(|value| value.as_u64())
                    {
                        completion.usage += output;
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::parse_text_tool_calls;

    #[test]
    fn parses_xml_style_tool_calls() {
        let (calls, cleaned) = parse_text_tool_calls(
            "확인하겠습니다.\n<tool_call>{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>",
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "workspace_read");
        assert!(calls[0].1.contains("a.txt"));
        assert!(!cleaned.contains("tool_call"));
    }

    #[test]
    fn parses_fenced_json_tool_calls() {
        let (calls, _) = parse_text_tool_calls(
            "```json\n{\"tool\":\"shell_exec\",\"args\":{\"command\":\"ls\"}}\n```",
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell_exec");
    }

    #[test]
    fn ignores_normal_text() {
        let (calls, cleaned) = parse_text_tool_calls("그냥 일반 답변입니다.");
        assert!(calls.is_empty());
        assert_eq!(cleaned, "그냥 일반 답변입니다.");
    }
}
