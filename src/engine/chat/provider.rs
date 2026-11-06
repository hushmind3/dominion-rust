//! OpenAI-compatible provider resolution, native thinking controls and HTTP streaming.
use super::{protocol::SseBuffer, Completion, ProviderRef, ToolCall, TurnRequest, MAX_TOOL_CALLS};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

const TEXT_TOOL_OPEN: &str = "<tool_call>";
const TEXT_TOOL_CLOSE: &str = "</tool_call>";

/// Streams visible text while hiding text-protocol tool calls. Small local
/// models emit `<tool_call>...` as ordinary content; without this buffer the
/// user sees the raw JSON before the tool actually runs.
fn stream_visible_text(
    tx: &UnboundedSender<Value>,
    content: &str,
    pending: &mut String,
    in_tool: &mut bool,
) {
    pending.push_str(content);
    loop {
        if *in_tool {
            if let Some(end) = pending.find(TEXT_TOOL_CLOSE) {
                pending.drain(..end + TEXT_TOOL_CLOSE.len());
                *in_tool = false;
                continue;
            }
            break;
        }
        if let Some(start) = pending.find(TEXT_TOOL_OPEN) {
            let before = pending[..start].to_string();
            if !before.is_empty() {
                let _ = tx.send(json!({ "type": "text", "text": before }));
            }
            pending.drain(..start + TEXT_TOOL_OPEN.len());
            *in_tool = true;
            continue;
        }
        // A tag may be split across deltas. Hold only a suffix that can still
        // become `<tool_call>`.
        let hold = (1..TEXT_TOOL_OPEN.len())
            .rev()
            .find(|length| {
                pending.len() >= *length && pending.ends_with(&TEXT_TOOL_OPEN[..*length])
            })
            .unwrap_or(0);
        let emit = pending.len() - hold;
        if emit > 0 {
            let text = pending[..emit].to_string();
            let _ = tx.send(json!({ "type": "text", "text": text }));
            pending.drain(..emit);
        }
        break;
    }
}

pub(super) fn select_provider(
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
            if session["roomId"].is_string() {
                return None;
            }
            session
                .get("providerId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });

    let provider = match provider_id {
        Some(id) => state.store.get("provider", &id),
        None => state.store.all("provider").into_iter().find(|provider| {
            provider
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(true)
        }),
    };

    let Some(provider) = provider else {
        return Err(
            "모델 제공자가 설정되지 않았습니다. 모델 관리에서 OpenAI 호환 제공자를 추가하세요."
                .into(),
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
                .filter(|agent| {
                    agent["providerId"]
                        .as_str()
                        .is_none_or(|provider_id| provider_id == id)
                })
                .and_then(|agent| agent.get("model"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            (session["providerId"].as_str() == Some(id.as_str())
                && session["agentId"].as_str() == request.agent_id.as_deref())
            .then(|| {
                session["model"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .flatten()
        })
        .or_else(|| {
            provider
                .get("model")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "gpt-4o-mini".into());

    let route = crate::engine::gateway::route_from_provider(state, &provider, Some(&model))?;
    Ok(ProviderRef {
        id: route.id,
        name: route.name,
        base_url: route.base_url,
        api_key: route.api_key,
        model: route.model,
        tool_mode: route.tool_mode,
        reasoning_protocol: route.reasoning_protocol,
        local: route.local,
        context_tokens: route.context_tokens,
        max_output_tokens: route.max_output_tokens,
    })
}

pub(super) fn apply_reasoning_options(
    body: &mut Value,
    protocol: &str,
    thinking: Option<bool>,
    effort: Option<&str>,
) {
    if protocol == "deepseek" {
        let enabled = if thinking == Some(false) || effort == Some("none") {
            Some(false)
        } else {
            thinking
        };
        if let Some(enabled) = enabled {
            body["thinking"] = json!({"type":if enabled{"enabled"}else{"disabled"}});
        }
        if enabled != Some(false) {
            if let Some(effort) = effort {
                body["reasoning_effort"] = json!(match effort {
                    "minimal" | "low" => "low",
                    _ => "high",
                });
            }
        }
    } else {
        if let Some(effort) = effort {
            body["reasoning_effort"] = json!(effort);
        }
        if let Some(thinking) = thinking.or_else(|| (effort == Some("none")).then_some(false)) {
            match protocol {
                "chat_template" => {
                    body["chat_template_kwargs"] = json!({"enable_thinking":thinking})
                }
                "qwen" => body["enable_thinking"] = json!(thinking),
                _ => {}
            }
        }
    }
}

/// Retry transient provider/stream failures only before any content was
/// published to the client. Replaying a partially streamed answer could repeat
/// visible text, reasoning, or a tool request, so that case always stops.
fn transient_provider_error(error: &str) -> bool {
    error.contains(" 연결 실패:")
        || error.contains("스트림 오류:")
        || error.contains("모델 스트림이 완료 신호 없이 종료")
        || error.contains("(HTTP 408")
        || error.contains("(HTTP 429")
        || error.contains("(HTTP 5")
}

fn fallback_candidates(
    state: &AppState,
    primary: &ProviderRef,
    request: &TurnRequest,
    messages: &[Value],
    tools: &[Value],
) -> Vec<ProviderRef> {
    // Include tool schemas and non-text message parts when checking the
    // candidate model's actual declared context capacity.
    let approx_chars = messages.iter()
        .map(|message| message.to_string().chars().count())
        .sum::<usize>()
        .saturating_add(tools.iter().map(|tool| tool.to_string().chars().count()).sum::<usize>());
    crate::engine::gateway::ordered_alternatives(state, &primary.id)
        .into_iter()
        // Compat/local and native/API routes can both handle the same
        // logical turn. Adapt the messages and tool protocol per candidate.
        .filter(|route| {
            route.context_tokens.is_none_or(|tokens| {
                let reserve = route.max_output_tokens.unwrap_or(tokens / 8)
                    .min(tokens / 4).max(128).min(tokens);
                approx_chars <= (tokens.saturating_sub(reserve) as usize).saturating_mul(4)
            })
        })
        .filter(|route| {
            if request.attachment_ids.is_empty() {
                return true;
            }
            // Never silently send an image turn to a text-only route.
            state.store.get("provider", &route.id).is_some_and(|item| {
                item["capabilities"]["vision"] == true
                    || item["capabilities"]["images"] == true
            })
        })
        .map(|route| ProviderRef {
            id: route.id,
            name: route.name,
            base_url: route.base_url,
            api_key: route.api_key,
            model: route.model,
            tool_mode: route.tool_mode,
            reasoning_protocol: route.reasoning_protocol,
            local: route.local,
            context_tokens: route.context_tokens,
            max_output_tokens: route.max_output_tokens,
        })
        .collect()
}

/// Returns the completed response and the route which actually generated it.
/// A private relay preserves streaming to the user while detecting whether any
/// output was emitted before a provider failed.
pub(super) async fn stream_completion(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
) -> Result<(Completion, ProviderRef), String> {
    let mut candidates = vec![provider.clone()];
    candidates.extend(fallback_candidates(state, provider, request, messages, tools));
    // Keep normal compat sessions untouched. Translate native tool history
    // only when a turn originally started in native mode and falls back to a
    // compat model (also on later rounds of that same turn).
    let original_mode = request.provider_id.as_deref()
        .and_then(|id| state.store.get("provider", id))
        .and_then(|item| crate::engine::gateway::route_from_provider(state, &item, None).ok())
        .map(|route| route.tool_mode)
        .unwrap_or_else(|| provider.tool_mode.clone());
    let mut failed = Vec::new();

    for candidate in candidates {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let (relay, mut relay_rx) = tokio::sync::mpsc::unbounded_channel();
        let (result, exposed) = {
            let attempt = stream_completion_once(
                state, client, &candidate, messages, tools, request, session_id, &relay,
                candidate.tool_mode == "compat" && original_mode != "compat",
            );
            tokio::pin!(attempt);
            let mut exposed = false;
            let result = loop {
                tokio::select! {
                    biased;
                    event = relay_rx.recv() => {
                        if let Some(event) = event {
                            exposed = true;
                            let _ = tx.send(event);
                        }
                    }
                    complete = &mut attempt => break complete,
                }
            };
            // The model may finish and enqueue its last delta in the same tick.
            // Forward those deltas before deciding whether replay is safe.
            while let Ok(event) = relay_rx.try_recv() {
                exposed = true;
                let _ = tx.send(event);
            }
            (result, exposed)
        };
        match result {
            Ok(completion) => {
                if candidate.id != provider.id {
                    state.store.audit("chat.provider_failover", json!({
                        "sessionId":session_id, "from":provider.id, "to":candidate.id,
                        "model":candidate.model, "previousFailures":failed,
                    }));
                    let _ = tx.send(json!({
                        "type":"provider_failover",
                        "from":provider.id, "to":candidate.id,
                        "model":candidate.model,
                    }));
                }
                return Ok((completion, candidate));
            }
            Err(error) => {
                // No replay after partial visible content or a policy/cancel
                // error. The next provider is only allowed to retry a clean
                // transient failure with zero externally observed output.
                if exposed || !transient_provider_error(&error) || state.is_cancelled(session_id) {
                    return Err(error);
                }
                failed.push(json!({"providerId":candidate.id,"error":error}));
            }
        }
    }
    Err(format!("사용 가능한 제공자가 모두 실패했습니다: {}", json!(failed)))
}

/// Compatibility models consume text protocol rather than OpenAI tool-call
/// message roles. Convert native tool history into ordinary labeled evidence,
/// preserving it across cloud-to-local failover.
fn compat_messages(messages: &[Value]) -> Vec<Value> {
    messages.iter().map(|message| {
        match message["role"].as_str() {
            Some("tool") => json!({
                "role":"user",
                "content": format!(
                    "Tool observation (tool_call_id={}): {}",
                    message["tool_call_id"].as_str().unwrap_or("unknown"),
                    message["content"].as_str().unwrap_or(""),
                ),
            }),
            Some("assistant") if message["tool_calls"].is_array() => {
                let calls = message["tool_calls"].as_array().unwrap();
                let content = message["content"].as_str().unwrap_or("");
                json!({
                    "role":"assistant",
                    "content": format!(
                        "{content}\nTool requests issued: {}",
                        serde_json::to_string(calls).unwrap_or_default()
                    ),
                })
            }
            _ => message.clone(),
        }
    }).collect()
}

async fn stream_completion_once(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    convert_native_history: bool,
) -> Result<Completion, String> {
    let started = std::time::Instant::now();
    let compatible = provider.tool_mode == "compat";
    let mut provider_messages = if convert_native_history {
        compat_messages(messages)
    } else {
        messages.to_vec()
    };
    if compatible && !tools.is_empty() {
        let manual = super::protocol::tool_manual(tools, &request.input).await;
        if let Some(first_system) = provider_messages.iter_mut()
            .find(|message| message["role"] == "system")
        {
            let existing = first_system["content"].as_str().unwrap_or("");
            first_system["content"] = json!(format!("{existing}\n\n{manual}"));
        }
    }
    for message in &mut provider_messages {
        if message["role"] == "assistant" {
            if provider.reasoning_protocol == "deepseek" {
                if message.get("reasoning_content").is_none() {
                    message["reasoning_content"] = json!("");
                }
            } else if let Some(map) = message.as_object_mut() {
                map.remove("reasoning_content");
            }
        }
    }
    let mut body = json!({
        "model": provider.model,
        "messages": provider_messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !compatible && !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(limit) = provider.max_output_tokens {
        body["max_tokens"] = json!(limit);
    }
    apply_reasoning_options(
        &mut body,
        &provider.reasoning_protocol,
        request.thinking,
        request.reasoning_effort.as_deref(),
    );

    // The same admission slot is shared with helper (non-streaming) calls, so
    // one local model instance is never asked to run two generations at once.
    let _permit = crate::engine::gateway::acquire(&provider.base_url, provider.local).await;
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
    let mut buffer = SseBuffer::default();
    let mut stream = response.bytes_stream();
    let mut terminated = false;
    let mut text_pending = String::new();
    let mut in_text_tool_call = false;

    'stream: while let Some(chunk) = stream.next().await {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let chunk = chunk.map_err(|error| format!("스트림 오류: {error}"))?;
        buffer.push(&chunk)?;

        while let Some(block) = buffer.pop()? {
            let payload = block
                .lines()
                .filter(|line| line.starts_with("data:"))
                .map(|line| line[5..].trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            if payload == "[DONE]" {
                terminated = true;
                break 'stream;
            }
            if payload.is_empty() {
                continue;
            }
            let piece: Value = serde_json::from_str(&payload)
                .map_err(|_| "모델 스트림 JSON이 올바르지 않습니다.")?;
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
            if let Some(reason) = piece["choices"][0]["finish_reason"].as_str() {
                if reason == "content_filter" {
                    return Err("모델 응답이 콘텐츠 필터로 중단되었습니다.".into());
                }
                // Small/local models often hit the token limit. Keep the partial
                // answer instead of failing the whole turn.
                terminated = true;
            }
            let Some(delta) = piece
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
            else {
                continue;
            };
            if completion.first_delta_ms.is_none()
                && (delta["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
                    || delta["reasoning_content"]
                        .as_str()
                        .or_else(|| delta["reasoning"].as_str())
                        .is_some_and(|text| !text.is_empty())
                    || delta["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty()))
            {
                completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
            }

            if let Some(content) = delta.get("content").and_then(|value| value.as_str()) {
                if !content.is_empty() {
                    completion.content.push_str(content);
                    stream_visible_text(tx, content, &mut text_pending, &mut in_text_tool_call);
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
                    if index >= MAX_TOOL_CALLS {
                        return Err("모델이 너무 많은 도구 호출을 반환했습니다.".into());
                    }
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
        if completion.content.len()
            + completion.reasoning.len()
            + calls.iter().map(|call| call.arguments.len()).sum::<usize>()
            > 2_000_000
        {
            return Err("모델 응답 크기 제한을 초과했습니다.".into());
        }
    }
    // Flush text held for a possible tag, unless it is a dangling prefix.
    if !in_text_tool_call
        && !text_pending.is_empty()
        && !TEXT_TOOL_OPEN.starts_with(&text_pending)
    {
        let _ = tx.send(json!({ "type": "text", "text": text_pending }));
    }
    if !terminated {
        return Err("모델 스트림이 완료 신호 없이 종료되었습니다.".into());
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

    crate::engine::gateway::record_usage_total(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        completion.usage,
    );
    Ok(completion)
}

// Keep bytes until a whole SSE event arrives; network chunks may split a
// Korean UTF-8 character or the CRLF separator at any byte.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_tool_history_can_be_replayed_to_a_compat_local_model() {
        let before = vec![
            json!({"role":"system","content":"rules"}),
            json!({"role":"assistant","content":"","tool_calls":[{
                "id":"call_1","type":"function","function":{"name":"file_read","arguments":"{}"}
            }]}),
            json!({"role":"tool","tool_call_id":"call_1","content":"파일 내용"}),
        ];
        let compatible = compat_messages(&before);
        assert_eq!(compatible[2]["role"], "user");
        assert!(compatible[2]["content"].as_str().unwrap().contains("파일 내용"));
        assert!(compatible[1]["content"].as_str().unwrap().contains("file_read"));
        assert!(compatible[1].get("tool_calls").is_none());
    }

    #[test]
    fn only_transient_failures_are_replayed() {
        assert!(transient_provider_error("remote 연결 실패: refused"));
        assert!(transient_provider_error("remote 호출 실패 (HTTP 503 Service Unavailable):"));
        assert!(transient_provider_error("모델 스트림이 완료 신호 없이 종료되었습니다."));
        assert!(!transient_provider_error("모델 응답이 콘텐츠 필터로 중단되었습니다."));
        assert!(!transient_provider_error("실행이 중단되었습니다."));
        assert!(!transient_provider_error("remote 호출 실패 (HTTP 400 Bad Request):"));
    }

    #[tokio::test]
    async fn transient_failure_uses_alternative_without_repeating_visible_output() {
        use axum::{routing::post, response::IntoResponse, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/chat/completions", post(|| async {
            let events = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"대체 모델 정상 응답"}}]})
            );
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], events).into_response()
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-failover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone(),
        };
        let state = AppState::new(config, crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")));
        state.store.put("session", &json!({"id":"session-failover"}));
        state.store.put("provider", &json!({
            "id":"primary","baseUrl":"http://127.0.0.1:1","model":"broken",
            "toolMode":"native","enabled":true,
        }));
        state.store.put("provider", &json!({
            "id":"alternative","baseUrl":format!("http://{address}"),"model":"working",
            "toolMode":"native","enabled":true,
        }));
        let request = TurnRequest::from_value(&json!({
            "input":"응답해","sessionId":"session-failover","providerId":"primary","autoLearning":false,
        })).unwrap();
        let primary = select_provider(&state, &request, &state.store.get("session","session-failover").unwrap()).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (answer, selected) = tokio::time::timeout(std::time::Duration::from_secs(5),
            stream_completion(
                &state, &reqwest::Client::new(), &primary,
                &[json!({"role":"user","content":"응답해"})], &[],
                &request, "session-failover", &tx,
            )
        ).await.unwrap().unwrap();
        assert_eq!(selected.id, "alternative");
        assert_eq!(answer.content, "대체 모델 정상 응답");
        let mut output = String::new();
        while let Ok(event) = rx.try_recv() {
            if event["type"] == "text" {
                output.push_str(event["text"].as_str().unwrap_or(""));
            }
        }
        assert_eq!(output, "대체 모델 정상 응답");
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hides_text_tool_calls_across_split_deltas() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut pending = String::new();
        let mut in_tool = false;
        for piece in [
            "파일을 ",
            "<tool",
            "_call>{\"name\":\"workspace_write\",",
            "\"arguments\":{}}</tool",
            "_call>",
            "생성했습니다.",
        ] {
            stream_visible_text(&tx, piece, &mut pending, &mut in_tool);
        }
        if !in_tool && !pending.is_empty() {
            tx.send(json!({"type":"text","text":pending})).unwrap();
        }
        drop(tx);
        let mut text = String::new();
        while let Ok(event) = rx.try_recv() {
            text.push_str(event["text"].as_str().unwrap_or(""));
        }
        assert_eq!(text, "파일을 생성했습니다.");
        assert!(!text.contains("tool_call"));
    }
}
