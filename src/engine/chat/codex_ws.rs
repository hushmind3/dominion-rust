//! Incremental Responses transport for ChatGPT subscription (Codex) models.
//!
//! The Codex backend keeps a conversation chain only for the lifetime of a
//! WebSocket connection: `previous_response_id` is rejected after the socket
//! closes or when it belongs to another connection. This module therefore owns
//! the socket per participant session, tracks which input items the remote
//! chain already holds, and sends only the new suffix on later calls.
//!
//! `instructions` are not inherited through `previous_response_id` (measured:
//! a directive absent from the continuation request is not applied), so the
//! full identity/persona prefix is re-sent on every call. Only accumulated
//! transcript items are reduced.
use super::{Completion, ProviderRef};
use crate::state::AppState;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The backend closes sockets after 60 minutes; recycle earlier so a turn
/// never starts on a connection that is about to expire.
const MAX_CONNECTION_AGE: Duration = Duration::from_secs(50 * 60);
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_CHAINS: usize = 64;

static REGISTRY: LazyLock<Mutex<HashMap<String, Arc<AsyncMutex<Chain>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Plan {
    /// No compatible prefix: send the complete request on a fresh chain.
    Full,
    /// Send items from this offset onward; earlier items live in the chain.
    Delta(usize),
}

/// A transport item is compared by meaning that the next request can rebuild:
/// role plus visible text. Ids and JSON envelope details are deliberately
/// ignored because the harness re-renders them deterministically.
pub(super) fn item_signature(item: &Value) -> (String, String) {
    match item["type"].as_str() {
        Some("function_call") => (
            "function_call".to_string(),
            format!(
                "{}|{}|{}",
                item["call_id"].as_str().unwrap_or(""),
                item["name"].as_str().unwrap_or(""),
                item["arguments"].as_str().unwrap_or("")
            ),
        ),
        Some("function_call_output") => (
            "function_call_output".to_string(),
            format!(
                "{}|{}",
                item["call_id"].as_str().unwrap_or(""),
                item["output"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| item["output"].to_string())
            ),
        ),
        _ => {
            let role = item["role"].as_str().unwrap_or("").to_string();
            if let Some(parts) = item["content"].as_array() {
                if parts.iter().any(|part| part["type"] == "input_image") {
                    // Image identity, detail and ordering are part of history.
                    return (role, json!(parts).to_string());
                }
            }
            let text = item["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            (role, text)
        }
    }
}

/// A prefix match is the only safe incremental form. Any edit, truncation or
/// reordering means the remote chain diverged, so the full request is resent.
pub(super) fn plan_chain(chain: &[(String, String)], incoming: &[(String, String)]) -> Plan {
    if chain.is_empty() || chain.len() >= incoming.len() {
        return Plan::Full;
    }
    if chain
        .iter()
        .zip(incoming.iter())
        .all(|(previous, current)| previous == current)
    {
        Plan::Delta(chain.len())
    } else {
        Plan::Full
    }
}

/// Assistant items the server added to the chain. The next request rebuilds
/// these from persisted messages, so they must be part of the comparison base.
fn assistant_items(response: &Value, fallback_text: &str) -> Vec<Value> {
    let mut items = Vec::new();
    for item in response["output"].as_array().into_iter().flatten() {
        if item["type"].as_str() == Some("function_call") {
            if let Some(normalized) = super::provider::codex_function_call_item(item) {
                items.push(normalized);
            }
            continue;
        }
        if item["type"].as_str() != Some("message") || item["role"].as_str() != Some("assistant") {
            continue;
        }
        let text = item["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|part| part["type"].as_str() == Some("output_text"))
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            items.push(json!({
                "role":"assistant",
                "content":[{"type":"output_text","text":text}]
            }));
        }
    }
    if items.is_empty() && !fallback_text.is_empty() {
        items.push(json!({
            "role":"assistant",
            "content":[{"type":"output_text","text":fallback_text}]
        }));
    }
    items
}

struct Chain {
    socket: Option<Socket>,
    last_response_id: Option<String>,
    items: Vec<(String, String)>,
    opened: Option<Instant>,
    used: Instant,
}

impl Chain {
    fn new() -> Self {
        Self {
            socket: None,
            last_response_id: None,
            items: Vec::new(),
            opened: None,
            used: Instant::now(),
        }
    }

    fn reset(&mut self) {
        self.socket = None;
        self.last_response_id = None;
        self.items.clear();
        self.opened = None;
    }
}

#[derive(Debug)]
pub(super) enum WsError {
    /// A rejected optional hint must be disabled without retrying it twice.
    HintRejected(String),
    /// The socket cannot be used for this call; retry the complete request.
    Fallback(String),
    /// The model/backend rejected the request itself; surface it like HTTP.
    Fatal(String),
}

pub(super) struct Outcome {
    pub completion: Completion,
    pub transport: Value,
}

fn websocket_url(base_url: &str) -> String {
    let http = format!("{}/responses", base_url.trim_end_matches('/'));
    if let Some(rest) = http.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = http.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        http
    }
}

async fn connect(
    provider: &ProviderRef,
    account_id: &str,
    access: &str,
    transport_session_id: &str,
) -> Result<Socket, String> {
    let url = websocket_url(&provider.base_url);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| format!("Codex WebSocket 주소 오류: {error}"))?;
    let headers = request.headers_mut();
    let insert = |headers: &mut tokio_tungstenite::tungstenite::http::HeaderMap,
                  name: &'static str,
                  value: String| {
        let value = HeaderValue::from_str(&value)
            .map_err(|error| format!("Codex WebSocket 헤더 오류: {error}"))?;
        headers.insert(name, value);
        Ok::<(), String>(())
    };
    insert(headers, "Authorization", format!("Bearer {access}"))?;
    insert(headers, "chatgpt-account-id", account_id.to_string())?;
    insert(
        headers,
        "OpenAI-Beta",
        "responses_websockets=2026-02-06".into(),
    )?;
    insert(headers, "originator", "codex_cli_rs".into())?;
    insert(headers, "User-Agent", "codex_cli_rs/0.0.0".into())?;
    insert(headers, "session_id", transport_session_id.to_string())?;
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| "Codex WebSocket 연결 시간 초과".to_string())?
    .map_err(|error| format!("Codex WebSocket 연결 실패: {error}"))?;
    Ok(socket)
}

fn registry_key(provider: &ProviderRef, account_id: &str, transport_session_id: &str) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        provider.id, provider.model, provider.base_url, account_id, transport_session_id
    )
}

fn chain_for(key: &str) -> Arc<AsyncMutex<Chain>> {
    let mut registry = REGISTRY.lock().unwrap();
    registry.retain(|_, chain| {
        chain
            .try_lock()
            .map(|guard| guard.used.elapsed() < Duration::from_secs(60 * 60))
            .unwrap_or(true)
    });
    if registry.len() >= MAX_CHAINS && !registry.contains_key(key) {
        if let Some(oldest) = registry
            .iter()
            .min_by_key(|(_, chain)| {
                chain
                    .try_lock()
                    .map(|guard| guard.used)
                    .unwrap_or_else(|_| Instant::now())
            })
            .map(|(name, _)| name.clone())
        {
            registry.remove(&oldest);
        }
    }
    registry
        .entry(key.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(Chain::new())))
        .clone()
}

pub(super) fn forget(key: &str) {
    REGISTRY.lock().unwrap().remove(key);
}

/// Sends one Responses request over the participant's socket. On a transport
/// failure the chain is discarded and the complete request is retried once on
/// a new socket; if that also fails the caller falls back to HTTP.
pub(super) async fn stream(
    state: &AppState,
    provider: &ProviderRef,
    account_id: &str,
    access: &str,
    transport_session_id: &str,
    base: &Value,
    input: Vec<Value>,
    session_id: &str,
    tx: &UnboundedSender<Value>,
) -> Result<Outcome, WsError> {
    if std::env::var("DOMINION_CODEX_WS")
        .is_ok_and(|value| matches!(value.as_str(), "off" | "http"))
    {
        return Err(WsError::Fallback(
            "DOMINION_CODEX_WS=off (forced HTTP baseline)".into(),
        ));
    }
    let key = registry_key(provider, account_id, transport_session_id);
    let chain = chain_for(&key);
    let mut guard = chain.lock().await;
    guard.used = Instant::now();
    let incoming = input
        .iter()
        .map(item_signature)
        .collect::<Vec<(String, String)>>();
    let plan = plan_chain(&guard.items, &incoming);
    match exchange(
        state,
        provider,
        account_id,
        access,
        transport_session_id,
        base,
        &input,
        plan,
        session_id,
        tx,
        &mut guard,
    )
    .await
    {
        Ok((completion, response_id, output, delta_items)) => {
            let mut chain_items = incoming;
            chain_items.extend(output.iter().map(item_signature));
            guard.items = chain_items;
            guard.last_response_id = Some(response_id.clone());
            guard.used = Instant::now();
            Ok(Outcome {
                completion,
                transport: json!({
                    "wire":"websocket",
                    "mode":if delta_items == 0 {"full"} else {"delta"},
                    "deltaItems":input.len().saturating_sub(delta_items),
                    "chainReusedItems":delta_items,
                    "chainItems":guard.items.len(),
                    "sentInputTokens":super::room_context::estimate(&json!(&input[delta_items..]).to_string()),
                    "fullInputTokens":super::room_context::estimate(&json!(input).to_string()),
                    "instructionTokens":super::room_context::estimate(&json!(base["instructions"]).to_string()),
                    "responseId":response_id,
                    "modelCalls":0
                }),
            })
        }
        Err(WsError::Fatal(message)) => {
            guard.reset();
            Err(WsError::Fatal(message))
        }
        Err(WsError::HintRejected(message)) => {
            guard.reset();
            Err(WsError::HintRejected(message))
        }
        Err(WsError::Fallback(first_error)) => {
            guard.reset();
            // A fresh socket has no chain, so the retry must carry every item.
            let retry = exchange(
                state,
                provider,
                account_id,
                access,
                transport_session_id,
                base,
                &input,
                Plan::Full,
                session_id,
                tx,
                &mut guard,
            )
            .await;
            match retry {
                Ok((completion, response_id, output, _)) => {
                    let mut chain_items = incoming;
                    chain_items.extend(output.iter().map(item_signature));
                    guard.items = chain_items;
                    guard.last_response_id = Some(response_id.clone());
                    guard.used = Instant::now();
                    state.store.audit(
                        "codex.ws.reconnected",
                        json!({"sessionId":session_id,"reason":first_error}),
                    );
                    Ok(Outcome {
                        completion,
                        transport: json!({
                            "wire":"websocket",
                            "mode":"full-after-reconnect",
                            "deltaItems":input.len(),
                            "chainReusedItems":0,
                            "chainItems":guard.items.len(),
                            "sentInputTokens":super::room_context::estimate(&json!(input).to_string()),
                            "fullInputTokens":super::room_context::estimate(&json!(input).to_string()),
                            "instructionTokens":super::room_context::estimate(&json!(base["instructions"]).to_string()),
                            "responseId":response_id,
                            "reconnectReason":first_error,
                            "modelCalls":0
                        }),
                    })
                }
                Err(WsError::Fatal(message)) => {
                    guard.reset();
                    Err(WsError::Fatal(message))
                }
                Err(WsError::HintRejected(message)) => {
                    guard.reset();
                    Err(WsError::HintRejected(message))
                }
                Err(WsError::Fallback(second_error)) => {
                    guard.reset();
                    forget(&key);
                    Err(WsError::Fallback(format!(
                        "{first_error}; reconnect: {second_error}"
                    )))
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn exchange(
    state: &AppState,
    provider: &ProviderRef,
    account_id: &str,
    access: &str,
    transport_session_id: &str,
    base: &Value,
    input: &[Value],
    plan: Plan,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    chain: &mut Chain,
) -> Result<(Completion, String, Vec<Value>, usize), WsError> {
    if chain
        .opened
        .is_some_and(|opened| opened.elapsed() > MAX_CONNECTION_AGE)
    {
        chain.reset();
    }
    if chain.socket.is_none() {
        let socket = connect(provider, account_id, access, transport_session_id)
            .await
            .map_err(WsError::Fallback)?;
        chain.socket = Some(socket);
        chain.opened = Some(Instant::now());
        chain.last_response_id = None;
        chain.items.clear();
    }
    let (start, previous) = match plan {
        Plan::Full => (0usize, None),
        Plan::Delta(start) => match chain.last_response_id.clone() {
            Some(previous) => (start, Some(previous)),
            // Recycling a socket invalidates the plan computed for the old
            // connection. No previous response means the entire input is due.
            None => (0, None),
        },
    };
    let mut body = base.clone();
    body["type"] = json!("response.create");
    body["input"] = json!(&input[start..]);
    body["stream"] = json!(true);
    body["store"] = json!(false);
    body["client_metadata"] = json!({
        "session_id":transport_session_id,
        "thread_id":transport_session_id
    });
    if let Some(previous) = previous {
        body["previous_response_id"] = json!(previous);
    } else {
        body.as_object_mut()
            .map(|object| object.remove("previous_response_id"));
    }
    let socket = chain
        .socket
        .as_mut()
        .ok_or_else(|| WsError::Fallback("Codex WebSocket 연결이 없습니다.".to_string()))?;
    socket
        .send(Message::Text(body.to_string()))
        .await
        .map_err(|error| WsError::Fallback(format!("Codex WebSocket 전송 실패: {error}")))?;

    let mut output_started = false;
    let result =
        receive_completion(state, socket, session_id, tx, start, &mut output_started).await;
    match result {
        Err(WsError::Fallback(message) | WsError::HintRejected(message)) if output_started => Err(WsError::Fatal(format!(
            "Codex 응답 도중 연결이 끊겼습니다. 부분 응답은 자동 재시도하지 않습니다: {message}"
        ))),
        other => other,
    }
}

async fn receive_completion(
    state: &AppState,
    socket: &mut Socket,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    start: usize,
    output_started: &mut bool,
) -> Result<(Completion, String, Vec<Value>, usize), WsError> {
    let started = Instant::now();
    let mut completion = Completion::default();
    loop {
        if state.is_cancelled(session_id) {
            return Err(WsError::Fatal("실행이 중단되었습니다.".into()));
        }
        let receive = tokio::time::timeout(IDLE_TIMEOUT, socket.next());
        tokio::pin!(receive);
        let message = loop {
            tokio::select! {
                result = &mut receive => break result
                    .map_err(|_| WsError::Fallback("Codex WebSocket 응답 시간 초과".to_string()))?,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if state.is_cancelled(session_id) {
                        return Err(WsError::Fatal("실행이 중단되었습니다.".into()));
                    }
                }
            }
        };
        let Some(message) = message else {
            return Err(WsError::Fallback(
                "Codex WebSocket 연결이 종료되었습니다.".to_string(),
            ));
        };
        let message = message
            .map_err(|error| WsError::Fallback(format!("Codex WebSocket 수신 실패: {error}")))?;
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Message::Ping(payload) => {
                // Long model turns can leave the socket otherwise write-idle.
                // Answer keepalive pings explicitly; a missing pong surfaces as
                // a 1011 timeout and forces a full-chain resend.
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| {
                        WsError::Fallback(format!("Codex WebSocket pong 전송 실패: {error}"))
                    })?;
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(frame) => {
                let reason = frame
                    .map(|frame| format!("{} {}", frame.code, frame.reason))
                    .unwrap_or_default();
                return Err(WsError::Fallback(format!(
                    "Codex WebSocket 연결이 닫혔습니다: {reason}"
                )));
            }
            _ => continue,
        };
        let Ok(piece) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match piece["type"].as_str().unwrap_or("") {
            "response.output_text.delta" => {
                let delta = piece["delta"].as_str().unwrap_or("");
                if !delta.is_empty() {
                    if completion.first_delta_ms.is_none() {
                        completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    completion.content.push_str(delta);
                    *output_started = true;
                    let _ = tx.send(json!({"type":"text","text":delta}));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let delta = piece["delta"].as_str().unwrap_or("");
                if !delta.is_empty() {
                    if completion.first_delta_ms.is_none() {
                        completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    completion.reasoning.push_str(delta);
                    *output_started = true;
                    let _ = tx.send(json!({"type":"reasoning","text":delta}));
                }
            }
            "response.output_item.done" => {
                if let Some(item) = piece.get("item") {
                    for call in super::provider::codex_tool_calls(std::slice::from_ref(item)) {
                        *output_started = true;
                        if !completion
                            .tool_calls
                            .iter()
                            .any(|existing| existing.id == call.id)
                        {
                            completion.tool_calls.push(call);
                        }
                    }
                }
            }
            "response.completed" => {
                let response_id = piece["response"]["id"].as_str().unwrap_or("").to_string();
                let output = piece["response"]["output"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                for call in super::provider::codex_tool_calls(&output) {
                    if !completion
                        .tool_calls
                        .iter()
                        .any(|existing| existing.id == call.id)
                    {
                        completion.tool_calls.push(call);
                    }
                }
                if let Some(usage) = piece["response"]["usage"].as_object() {
                    completion.cached_prompt_tokens =
                        super::provider::cached_prompt_tokens(&json!(usage));
                    completion.prompt_tokens =
                        usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
                    completion.completion_tokens =
                        usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
                    completion.usage = usage
                        .get("total_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(completion.prompt_tokens + completion.completion_tokens);
                    completion.usage_exact = true;
                }
                if response_id.is_empty() {
                    return Err(WsError::Fallback(
                        "Codex WebSocket 응답에 ID가 없습니다.".to_string(),
                    ));
                }
                let output_items = assistant_items(&json!({"output":output}), &completion.content);
                return Ok((completion, response_id, output_items, start));
            }
            "response.failed" | "response.incomplete" => {
                if super::continuity::rejects_hint(reqwest::StatusCode::BAD_REQUEST,
                    &piece["response"].to_string(), "prompt_cache_key") {
                    return Err(WsError::HintRejected("Codex 캐시 힌트가 지원되지 않습니다.".into()));
                }
                return Err(WsError::Fatal(
                    super::provider::codex_terminal_error(&piece).unwrap(),
                ));
            }
            "error" => {
                if super::continuity::rejects_hint(reqwest::StatusCode::BAD_REQUEST,
                    &piece.to_string(), "prompt_cache_key") {
                    return Err(WsError::HintRejected("Codex 캐시 힌트가 지원되지 않습니다.".into()));
                }
                let code = piece["error"]["code"].as_str().unwrap_or("");
                let param = piece["error"]["param"].as_str().unwrap_or("");
                let message = piece["error"]["message"].as_str().unwrap_or("알 수 없는 오류");
                if code == "previous_response_not_found" || param == "previous_response_id" {
                    return Err(WsError::Fallback(format!(
                        "Codex 이전 응답 없음: {message}"
                    )));
                }
                return Err(WsError::Fatal(format!("Codex: {message}")));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: &str, text: &str) -> Value {
        json!({"role":role,"content":[{"type":"text","text":text}]})
    }

    #[test]
    fn tool_item_signatures_include_call_identity_and_output() {
        let call_a = json!({
            "type":"function_call","call_id":"call-a",
            "name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"
        });
        let call_b = json!({
            "type":"function_call","call_id":"call-b",
            "name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"
        });
        let output = json!({
            "type":"function_call_output","call_id":"call-a","output":"ok"
        });
        assert_ne!(item_signature(&call_a), item_signature(&call_b));
        assert_ne!(item_signature(&call_a), item_signature(&output));
        assert_eq!(item_signature(&call_a).0, "function_call");
        assert_eq!(item_signature(&output).0, "function_call_output");
    }

    #[test]
    fn replacing_an_image_invalidates_the_remote_chain() {
        let first = json!({"role":"user","content":[
            {"type":"input_text","text":"describe"},
            {"type":"input_image","image_url":"data:image/png;base64,original"}
        ]});
        let mut edited = first.clone();
        edited["content"][1]["image_url"] = json!("data:image/png;base64,changed");
        let chain = vec![item_signature(&first)];
        let incoming = vec![
            item_signature(&edited),
            item_signature(&item("user", "next")),
        ];
        assert_eq!(plan_chain(&chain, &incoming), Plan::Full);
    }

    #[test]
    fn a_matching_prefix_sends_only_the_new_suffix() {
        let chain = vec![("user".to_string(), "one".to_string())];
        let incoming = vec![
            ("user".to_string(), "one".to_string()),
            ("assistant".to_string(), "two".to_string()),
        ];
        assert_eq!(plan_chain(&chain, &incoming), Plan::Delta(1));
    }

    #[test]
    fn edited_or_truncated_history_resends_everything() {
        let chain = vec![("user".to_string(), "one".to_string())];
        assert_eq!(
            plan_chain(&chain, &[("user".to_string(), "changed".to_string())]),
            Plan::Full
        );
        assert_eq!(plan_chain(&chain, &[]), Plan::Full);
        assert_eq!(
            plan_chain(&chain, &[("user".to_string(), "one".to_string())]),
            Plan::Full,
            "no new item means no incremental request"
        );
    }

    #[test]
    fn assistant_output_is_rebuilt_from_the_messages_the_next_call_will_send() {
        let response = json!({"output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"안녕"}]},
            {"type":"reasoning","summary":[]}
        ]});
        let items = assistant_items(&response, "ignored");
        assert_eq!(items.len(), 1);
        assert_eq!(item_signature(&items[0]), ("assistant".to_string(), "안녕".to_string()));
    }

    #[test]
    fn websocket_url_keeps_the_codex_backend_path() {
        assert_eq!(
            websocket_url("https://chatgpt.com/backend-api/codex"),
            "wss://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(websocket_url("http://127.0.0.1:1/v1/"), "ws://127.0.0.1:1/v1/responses");
    }

    #[test]
    fn cached_usage_is_unknown_when_absent_not_zero() {
        let completion = Completion::default();
        assert_eq!(completion.cached_prompt_tokens, None);
    }

    fn test_state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-ws-audit-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), root)
    }

    async fn fixture(
        events: Vec<Value>,
    ) -> (
        ProviderRef,
        tokio::sync::mpsc::UnboundedReceiver<Value>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let provider = ProviderRef {
            id: uuid::Uuid::new_v4().to_string(),
            name: "fixture".into(),
            base_url: format!("http://{}", listener.local_addr().unwrap()),
            api_key: "fixture".into(),
            model: "fixture".into(),
            tool_mode: "native".into(),
            reasoning_protocol: "openai".into(),
            local: false,
            context_tokens: None,
            max_output_tokens: None,
            requests_per_minute: None,
            kind: "codex".into(),
            account_id: Some("fixture".into()),
        };
        let server = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    tx.send(serde_json::from_str(&text).unwrap()).unwrap();
                    if events.is_empty() {
                        std::future::pending::<()>().await;
                    }
                    for event in &events {
                        socket.send(Message::Text(event.to_string())).await.unwrap();
                    }
                    if !events
                        .iter()
                        .any(|event| event["type"] == "response.completed")
                    {
                        let _ = socket.close(None).await;
                        break;
                    }
                }
            }
        });
        (provider, rx, server)
    }

    #[tokio::test]
    async fn unsupported_cache_hint_is_not_retried_as_a_broken_connection() {
        let (state,root)=test_state();
        let (provider,mut requests,server)=fixture(vec![json!({"type":"error","error":{
            "code":"invalid_parameter","param":"prompt_cache_key","message":"prompt_cache_key is not supported"}})]).await;
        let (tx,_)=tokio::sync::mpsc::unbounded_channel();
        let result=stream(&state,&provider,"fixture","fixture","session",
            &json!({"prompt_cache_key":"stable-key"}),vec![item("user","hello")],"session",&tx).await;
        assert!(matches!(result,Err(WsError::HintRejected(_))));
        assert_eq!(requests.recv().await.unwrap()["prompt_cache_key"],"stable-key");
        assert!(requests.try_recv().is_err(),"do not open another socket to send the same rejected hint");
        server.abort();std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn recycling_an_expired_socket_resends_the_full_history() {
        let (state, root) = test_state();
        let (provider, mut requests, server) = fixture(vec![
            json!({"type":"response.output_text.delta","delta":"reply"}),
            json!({"type":"response.completed","response":{"id":"resp-1","output":[]}}),
        ])
        .await;
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let first = vec![item("user", "original request")];
        stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &json!({}),
            first.clone(),
            "session",
            &tx,
        )
        .await
        .unwrap();
        requests.recv().await.unwrap();
        let key = registry_key(&provider, "fixture", "session");
        chain_for(&key).lock().await.opened =
            Some(Instant::now() - MAX_CONNECTION_AGE - Duration::from_secs(1));
        let incoming = vec![
            first[0].clone(),
            item("assistant", "reply"),
            item("user", "follow up"),
        ];
        let outcome = stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &json!({}),
            incoming.clone(),
            "session",
            &tx,
        )
        .await
        .unwrap();
        let second = requests.recv().await.unwrap();
        assert_eq!(
            second["input"],
            json!(incoming),
            "a new socket cannot inherit old items"
        );
        assert!(second.get("previous_response_id").is_none());
        assert_eq!(outcome.transport["mode"], "full");
        forget(&key);
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn interrupted_output_is_not_retried_or_accepted_as_complete() {
        for terminal in [
            None,
            Some(
                json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}),
            ),
        ] {
            let (state, root) = test_state();
            let mut events = vec![json!({"type":"response.output_text.delta","delta":"partial"})];
            events.extend(terminal);
            let (provider, mut requests, server) = fixture(events).await;
            let (tx, mut output) = tokio::sync::mpsc::unbounded_channel();
            let result = stream(
                &state,
                &provider,
                "fixture",
                "fixture",
                "session",
                &json!({}),
                vec![item("user", "request")],
                "session",
                &tx,
            )
            .await;
            assert!(
                matches!(result, Err(WsError::Fatal(_))),
                "partial output must surface as failure"
            );
            requests.recv().await.unwrap();
            assert!(
                requests.try_recv().is_err(),
                "a partial generation must not be replayed"
            );
            assert_eq!(output.try_recv().unwrap()["text"], "partial");
            assert!(output.try_recv().is_err());
            forget(&registry_key(&provider, "fixture", "session"));
            server.abort();
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn cancelling_a_stalled_socket_does_not_wait_for_network_data() {
        let (state, root) = test_state();
        let (provider, mut requests, server) = fixture(Vec::new()).await;
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let body = json!({});
        let request = stream(
            &state,
            &provider,
            "fixture",
            "fixture",
            "session",
            &body,
            vec![item("user", "request")],
            "session",
            &tx,
        );
        let cancel = async {
            requests.recv().await.unwrap();
            state.cancel_session("session");
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(request, cancel)
        })
        .await
        .expect("cancellation must interrupt a stalled socket");
        assert!(matches!(result, Err(WsError::Fatal(_))));
        forget(&registry_key(&provider, "fixture", "session"));
        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}
