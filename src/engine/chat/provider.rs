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

pub(crate) fn select_provider(
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
                .filter(|id| state.store.get("provider", id)
                    .is_some_and(|provider| provider["enabled"] != false))
                .map(str::to_string)
        });

    let provider = match provider_id {
        Some(id) => state.store.get("provider", &id),
        None => {
            // Auto mode: no explicit provider/agent/session selection. Pick a
            // deterministic tier from the request instead of the first record.
            let messages = session["id"]
                .as_str()
                .map(|id| state.store.messages(id).len())
                .unwrap_or(0);
            let complexity = crate::engine::router::classify(
                &request.input,
                request.attachment_ids.len(),
                messages,
            );
            crate::engine::router::select_provider(state, complexity).or_else(|| {
                state.store.all("provider").into_iter().find(|provider| {
                    provider
                        .get("enabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(true)
                })
            })
        }
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
        .ok_or("이 API에서 사용할 모델을 선택하거나 모델 ID를 입력하세요.")?;

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
        requests_per_minute: route.requests_per_minute,
        kind: route.kind,
        account_id: route.account_id,
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
        // Ollama and Qwen templates expose thinking through enable_thinking.
        // Sending OpenAI's reasoning_effort to those endpoints makes Ollama
        // reject non-thinking models with "does not support thinking".
        if let Some(effort) = effort {
            if !matches!(protocol, "chat_template" | "qwen") {
                body["reasoning_effort"] = json!(effort);
            }
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

/// Estimated input room for the declared model context, leaving its
/// configured generation capacity reserved. No universal character cap.
fn input_char_capacity(tokens: u64, max_output_tokens: Option<u64>) -> usize {
    let reserved = max_output_tokens.unwrap_or(tokens / 8)
        .min(tokens / 4).max(128).min(tokens);
    (tokens.saturating_sub(reserved) as usize).saturating_mul(4)
}

/// Count the fixed prompt without the bodies of prior tool observations.
/// Those are dynamically reduced for each candidate provider at request time.
fn fixed_prompt_chars(messages: &[Value]) -> usize {
    messages.iter().map(|message| {
        if message["role"] == "tool" {
            let mut envelope = message.clone();
            envelope["content"] = json!("");
            envelope.to_string().chars().count()
        } else {
            message.to_string().chars().count()
        }
    }).sum::<usize>()
}

/// Count serialized JSON string characters (without the surrounding quotes).
/// Escaping matters for structured tool results containing JSON or newlines.
fn normalize_assistant_reasoning(messages: &mut [Value], required: bool) {
    for message in messages {
        if message["role"] != "assistant" {
            continue;
        }
        if required {
            if !message["reasoning_content"].is_string() {
                message["reasoning_content"] = json!("");
            }
        } else if let Some(map) = message.as_object_mut() {
            map.remove("reasoning_content");
        }
    }
}

/// Provider protocols require every assistant tool call to be followed
/// immediately by its tool result. Runtime hints are sometimes inserted as
/// system messages while a round is still executing; move those hints after
/// the completed tool exchange without dropping them.
fn order_tool_exchanges(messages: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len());
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        let calls = message["tool_calls"]
            .as_array()
            .filter(|calls| !calls.is_empty());
        if message["role"] != "assistant" || calls.is_none() {
            out.push(message.clone());
            index += 1;
            continue;
        }
        let mut expected = calls
            .unwrap()
            .iter()
            .filter_map(|call| call["id"].as_str().map(str::to_string))
            .collect::<std::collections::BTreeSet<_>>();
        let mut tools = Vec::new();
        let mut hints = Vec::new();
        let mut cursor = index + 1;
        while cursor < messages.len()
            && (messages[cursor]["role"] == "tool" || messages[cursor]["role"] == "system")
        {
            if messages[cursor]["role"] == "tool" {
                if let Some(id) = messages[cursor]["tool_call_id"].as_str() {
                    expected.remove(id);
                }
                tools.push(messages[cursor].clone());
            } else {
                hints.push(messages[cursor].clone());
            }
            cursor += 1;
            if expected.is_empty() {
                break;
            }
        }
        out.push(message.clone());
        out.extend(tools);
        out.extend(hints);
        index = cursor;
    }
    out
}

/// Reflection Beam requires reasoning_content for assistant turns. If an
/// earlier model/legacy record has no original reasoning, keep its *visible
/// evidence* as an explicitly quoted user record, never invent private
/// reasoning and never erase the entire transcript.
fn preserve_reflection_history(messages: Vec<Value>) -> (Vec<Value>, bool) {
    let mut filled = false;
    let mut out = Vec::with_capacity(messages.len());
    for mut message in messages {
        if message["role"] == "assistant"
            && message["reasoning_content"]
                .as_str()
                .is_none_or(|text| text.is_empty())
        {
            // Reflection requires the field on every earlier assistant turn.
            // A space preserves roles, tool IDs and pairing without inventing
            // a private rationale. Real reasoning is never overwritten.
            message["reasoning_content"] = json!(" ");
            filled = true;
        }
        out.push(message);
    }
    (out, filled)
}

pub(super) fn cached_prompt_tokens(usage: &Value) -> Option<u64> {
    usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64)
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens").and_then(Value::as_u64))
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
}

fn escaped_content_chars(text: &str) -> usize {
    text.chars().map(|ch| match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
        '\u{0000}'..='\u{001f}' => 6,
        _ => 1,
    }).sum()
}

/// Trim just the provider-visible copy, retaining the newest observations
/// first. All raw results remain in the runtime/DB and are available when
/// a larger context provider takes over a later round.
fn clip_tool_for_provider(text: &str, allowed_encoded_chars: usize) -> String {
    if escaped_content_chars(text) <= allowed_encoded_chars {
        return text.to_owned();
    }
    const NOTICE: &str = "\n… [this model's context is full; see stored execution_operation]";
    let notice_size = escaped_content_chars(NOTICE);
    if allowed_encoded_chars < notice_size {
        return if allowed_encoded_chars > 0 { "…".to_string() } else { String::new() };
    }
    let mut prefix = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => 1,
        };
        if used.saturating_add(width) > allowed_encoded_chars - notice_size {
            break;
        }
        prefix.push(ch);
        used += width;
    }
    prefix.push_str(NOTICE);
    prefix
}

/// Prepare a different model's copy from the original, unabridged tool
/// observations. No 6k/20k/80k magic limits or local/cloud distinction.
fn fit_tool_context(provider: &ProviderRef, messages: &mut [Value], schema_chars: usize) {
    let Some(tokens) = provider.context_tokens else { return };
    let fixed = fixed_prompt_chars(messages)
        .saturating_add(schema_chars)
        .saturating_add(512);
    let mut available = input_char_capacity(tokens, provider.max_output_tokens)
        .saturating_sub(fixed);
    // Preserve the most recent observation; old tool outputs get the
    // remaining space, with each tool-call ID kept intact.
    for message in messages.iter_mut().rev() {
        if message["role"] != "tool" {
            continue;
        }
        let Some(original) = message["content"].as_str() else { continue };
        let clipped = clip_tool_for_provider(original, available);
        available = available.saturating_sub(escaped_content_chars(&clipped));
        message["content"] = json!(clipped);
    }
}

fn room_route_keys(state: &AppState, session_id: &str) -> Option<std::collections::HashSet<String>> {
    let session = state.store.get("session", session_id)?;
    let room = state.store.get("room", session["roomId"].as_str()?)?;
    let mut keys = std::collections::HashSet::new();
    for entry in room["models"].as_array().into_iter().flatten() {
        if let (Some(provider), Some(model)) =
            (entry["providerId"].as_str(), entry["model"].as_str())
        {
            keys.insert(format!("{provider}|{model}"));
        }
    }
    if !keys.is_empty() {
        return Some(keys);
    }
    for id in room["agentIds"].as_array()? {
        let Some(agent) = id.as_str().and_then(|id| state.store.get("agent", id)) else {
            continue;
        };
        if let (Some(provider), Some(model)) =
            (agent["providerId"].as_str(), agent["model"].as_str())
        {
            keys.insert(format!("{provider}|{model}"));
        }
    }
    Some(keys)
}

async fn fallback_candidates(
    state: &AppState,
    primary: &ProviderRef,
    request: &TurnRequest,
    messages: &[Value],
    tools: &[Value],
) -> Vec<ProviderRef> {
    let fixed_chars = fixed_prompt_chars(messages);
    let native_schema_chars = tools.iter()
        .map(|tool| tool.to_string().chars().count()).sum::<usize>();
    let compat_schema_chars = if tools.is_empty() {
        0
    } else {
        escaped_content_chars(&super::protocol::tool_manual(tools, &request.input).await)
    };
    // A room may only fall back to its own participants' routes. Otherwise a
    // dead free model silently escalates to whatever paid provider happens to
    // rank highest, which is exactly the surprise bill to avoid.
    let room_routes = request
        .session_id
        .as_deref()
        .and_then(|session_id| room_route_keys(state, session_id));
    crate::engine::gateway::ordered_alternatives(state, &primary.id)
        .into_iter()
        .filter(|route| {
            room_routes.as_ref().is_none_or(|allowed| {
                allowed.contains(&format!("{}|{}", route.id, route.model))
            })
        })
        // Native/API and compat/local models can take over the same turn.
        // A smaller context clips *only tool observations* for that attempt;
        // it does not prevent a local fallback when the tool output is large.
        .filter(|route| {
            route.context_tokens.is_none_or(|tokens| {
                let schema_chars = if route.tool_mode == "compat" {
                    compat_schema_chars
                } else {
                    native_schema_chars
                };
                fixed_chars.saturating_add(schema_chars).saturating_add(512)
                    <= input_char_capacity(tokens, route.max_output_tokens)
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
            requests_per_minute: route.requests_per_minute,
            kind: route.kind,
            account_id: route.account_id,
        })
        .collect()
}

/// Returns the completed response and the route which actually generated it.
/// A private relay preserves streaming to the user while detecting whether any
/// output was emitted before a provider failed.
/// Providers that omit usage during streaming still need a cost signal, so
/// prompt/output characters provide a conservative estimate. The flag stored
/// with the message keeps the estimate distinguishable from exact usage.
fn estimate_usage(messages: &[Value], tools: &[Value], completion: &Completion) -> (u64, u64) {
    (
        super::room_context::input_estimate(messages, tools) as u64,
        super::room_context::estimate(&format!("{}{}",completion.content,completion.reasoning)).max(1) as u64,
    )
}

fn fill_estimated_usage(messages:&[Value],tools:&[Value],completion:&mut Completion){
    if !completion.usage_exact {
        let(prompt,output)=estimate_usage(messages,tools,completion);
        completion.prompt_tokens=prompt;
        completion.completion_tokens=output;
        completion.usage=prompt.saturating_add(output);
        completion.cached_prompt_tokens=None;
    }
}

pub(super) async fn stream_completion(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    allow_fallback: bool,
) -> Result<(Completion, ProviderRef), String> {
    let mut candidates = vec![provider.clone()];
    if allow_fallback {
        candidates.extend(fallback_candidates(state, provider, request, messages, tools).await);
    }
    // Keep normal compat sessions untouched. Translate native tool history
    // only when a turn originally started in native mode and falls back to a
    // compat model (also on later rounds of that same turn).
    let original_mode = request.provider_id.as_deref()
        .and_then(|id| state.store.get("provider", id))
        .and_then(|item| crate::engine::gateway::route_from_provider(state, &item, request.model.as_deref()).ok())
        .map(|route| route.tool_mode)
        .unwrap_or_else(|| provider.tool_mode.clone());
    let mut failed = Vec::new();

    for candidate in candidates {
        if state.is_cancelled(session_id) {
            return Err("실행이 중단되었습니다.".into());
        }
        let (relay, mut relay_rx) = tokio::sync::mpsc::unbounded_channel();
        let candidate_started = std::time::Instant::now();
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
        // A two-minute answer is not a usable room participant. Remember it
        // so the next round does not pay the same price again.
        if candidate_started.elapsed() >= std::time::Duration::from_secs(60) {
            crate::engine::gateway::mark_slow(&candidate.base_url, &candidate.model);
        }
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
    // Every provider uses the same participant state and route binding.
    // Adapters only translate the optional cache hint into their wire schema.
    let instructions=messages.iter().filter(|row|row["role"]=="system")
        .map(|row|anthropic_message_text(&row["content"]))
        .collect::<Vec<_>>().join("\n\n");
    // Tool order is part of the execution contract: compatibility menus may
    // use that priority when their schema budget is limited.
    let ordered_tools=tools.to_vec();
    let settings=json!({"thinking":request.thinking,"effort":request.reasoning_effort,
        "tools":ordered_tools,"toolMode":provider.tool_mode});
    let mut continuity=super::continuity::prepare(state,provider,request,session_id,&instructions,&settings)?;
    let (projected, projection)=super::continuity::project(state,provider,messages,&ordered_tools,session_id,&mut continuity);
    let mut completion=stream_completion_once_body(state,client,provider,&projected,&ordered_tools,
        request,session_id,tx,convert_native_history,&mut continuity).await?;
    completion.projection=projection;
    completion.continuity=Some(continuity);
    Ok(completion)
}

async fn stream_completion_once_body(
    state:&AppState,client:&reqwest::Client,provider:&ProviderRef,messages:&[Value],tools:&[Value],
    request:&TurnRequest,session_id:&str,tx:&UnboundedSender<Value>,convert_native_history:bool,
    continuity:&mut Value,
) -> Result<Completion,String> {
    if provider.kind == "cli" {
        let started=std::time::Instant::now();
        let source=provider.base_url.strip_prefix("cli://").ok_or("CLI 제공자 주소가 올바르지 않습니다.")?;
        let mut projected=messages.to_vec();
        if !tools.is_empty(){
            let manual=super::protocol::tool_manual(tools,&request.input).await;
            projected.insert(0,json!({"role":"system","content":manual}));
        }
        let content=crate::engine::cli_model::generate(source,&provider.model,&projected).await?;
        if state.is_cancelled(session_id){return Err("실행이 중단되었습니다.".into());}
        let (calls,visible)=super::protocol::parse_text_tool_calls(&content);
        let mut completed=Completion::default();
        completed.content=visible;
        completed.tool_calls=calls.into_iter().map(|(name,arguments)|ToolCall{id:uuid::Uuid::new_v4().to_string(),name,arguments}).collect();
        if !completed.content.is_empty(){let _=tx.send(json!({"type":"text","text":completed.content}));}
        let (prompt_tokens,completion_tokens)=estimate_usage(&projected,&[],&completed);
        completed.prompt_tokens=prompt_tokens;
        completed.completion_tokens=completion_tokens;
        completed.usage=prompt_tokens+completion_tokens;
        completed.first_delta_ms=Some(started.elapsed().as_millis() as u64);
        crate::engine::gateway::record_usage_tokens(state,&provider.id,&provider.model,false,"chat",Some(session_id),
            prompt_tokens,completion_tokens,completed.usage,true);
        return Ok(completed);
    }
    if provider.kind == "codex" {
        return stream_codex_once(
            state, client, provider, messages, tools, request, session_id, tx, continuity,
        )
        .await;
    }
    if provider.reasoning_protocol == "anthropic" {
        return stream_anthropic_once(
            state, client, provider, messages, tools, request, session_id, tx, continuity,
        )
        .await;
    }
    let started = std::time::Instant::now();
    let compatible = provider.tool_mode == "compat";
    let manual = if compatible && !tools.is_empty() {
        Some(super::protocol::tool_manual(tools, &request.input).await)
    } else {
        None
    };
    let schema_chars = if let Some(text) = manual.as_deref() {
        escaped_content_chars(text)
    } else {
        tools.iter().map(|tool| tool.to_string().chars().count()).sum::<usize>()
    };
    let mut provider_messages = messages.to_vec();
    fit_tool_context(provider, &mut provider_messages, schema_chars);
    if convert_native_history {
        provider_messages = compat_messages(&provider_messages);
    }
    if let Some(manual) = manual {
        if let Some(first_system) = provider_messages.iter_mut()
            .find(|message| message["role"] == "system")
        {
            let existing = first_system["content"].as_str().unwrap_or("");
            first_system["content"] = json!(format!("{existing}\n\n{manual}"));
        }
    }
    // DeepSeek and Reflection require `reasoning_content` on every earlier
    // assistant message. Most other OpenAI-compatible endpoints reject
    // unknown fields, so only send it where the provider requires it.
    let reflection_provider = provider.base_url.contains("api.reflection.ai");
    let requires_reasoning_content =
        provider.reasoning_protocol == "deepseek" || reflection_provider;
    provider_messages = order_tool_exchanges(provider_messages);
    normalize_assistant_reasoning(&mut provider_messages, requires_reasoning_content);
    if reflection_provider {
        let (preserved, quoted_legacy) = preserve_reflection_history(provider_messages);
        provider_messages = preserved;
        if quoted_legacy {
            state.store.audit(
                "reflection.history_preserved",
                json!({
                    "sessionId": session_id,
                    "reason": "assistant reasoning missing; sent placeholder to preserve roles and tool pairing",
                }),
            );
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
    crate::engine::gateway::reserve_generation(
        &provider.base_url,
        &provider.model,
        provider.requests_per_minute,
    )?;
    let _permit = crate::engine::gateway::acquire(&provider.base_url, provider.local).await;
    let builder = crate::engine::gateway::benchmark_request(
        client.post(format!("{}/chat/completions", provider.base_url)),
    )
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", provider.api_key));
    let response=super::continuity::send(state,provider,builder,&body,continuity,super::continuity::CacheWire::Key).await?;

    crate::engine::gateway::verify_benchmark_route(&response)?;

    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            crate::engine::gateway::mark_rate_limited(&provider.base_url, &provider.model);
        }
        let text = response.text().await.unwrap_or_default();
        let lower = text.to_ascii_lowercase();
        if text.contains("1305") || lower.contains("temporarily overloaded") {
            return Err(format!(
                "{}가 혼잡합니다(1305). 다른 무료 모델을 선택하거나 잠시 후 다시 시도하세요.",
                provider.name
            ));
        }
        if matches!(status.as_u16(), 401 | 403) && provider.base_url.contains("cloudflare") {
            return Err(format!(
                "{} 키가 거부되었습니다. Workers AI 권한이 있는 Cloudflare API 토큰인지 확인하세요.",
                provider.name
            ));
        }
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
            if let Some(usage) = piece.get("usage").filter(|value| value.is_object()) {
                completion.cached_prompt_tokens = cached_prompt_tokens(usage);
                let prompt = usage.get("prompt_tokens").and_then(Value::as_u64);
                let generated = usage.get("completion_tokens").and_then(Value::as_u64);
                if let Some(total) = usage.get("total_tokens").and_then(Value::as_u64) {
                    completion.usage = total;
                }
                if let (Some(prompt), Some(generated)) = (prompt, generated) {
                    completion.prompt_tokens = prompt;
                    completion.completion_tokens = generated;
                    if completion.usage == 0 {
                        completion.usage = prompt + generated;
                    }
                    completion.usage_exact = true;
                }
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

    let (prompt_tokens, completion_tokens) = if completion.usage_exact {
        (completion.prompt_tokens, completion.completion_tokens)
    } else {
        estimate_usage(&provider_messages, if compatible { &[] } else { tools }, &completion)
    };
    let total_tokens = if completion.usage > 0 {
        completion.usage
    } else {
        prompt_tokens + completion_tokens
    };
    crate::engine::gateway::record_usage_tokens(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        prompt_tokens,
        completion_tokens,
        total_tokens,
        !completion.usage_exact,
    );
    Ok(completion)
}

fn anthropic_message_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_anthropic_message(out: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last["role"].as_str() == Some(role) {
            if let Some(content) = last["content"].as_array_mut() {
                content.append(&mut blocks);
                return;
            }
        }
    }
    out.push(json!({"role": role, "content": blocks}));
}

/// Converts the engine's OpenAI-style transcript into Anthropic message
/// blocks: system is separate, tool results become user tool_result blocks,
/// and assistant tool calls become tool_use blocks.
fn anthropic_messages(messages: &[Value]) -> (String, Vec<Value>) {
    let mut system = String::new();
    let mut out: Vec<Value> = Vec::new();
    for message in messages {
        match message["role"].as_str().unwrap_or("user") {
            "system" => {
                let text = anthropic_message_text(&message["content"]);
                if !text.trim().is_empty() {
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(&text);
                }
            }
            "tool" => {
                let content = anthropic_message_text(&message["content"]);
                push_anthropic_message(
                    &mut out,
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": message["tool_call_id"].as_str().unwrap_or(""),
                        "content": content,
                    })],
                );
            }
            "assistant" => {
                let mut blocks = Vec::new();
                let text = anthropic_message_text(&message["content"]);
                if !text.trim().is_empty() {
                    blocks.push(json!({"type": "text", "text": text}));
                }
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    let function = &call["function"];
                    let name = function["name"].as_str().unwrap_or("tool");
                    let input = function["arguments"]
                        .as_str()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call["id"].as_str().filter(|id| !id.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                        "name": name,
                        "input": input,
                    }));
                }
                push_anthropic_message(&mut out, "assistant", blocks);
            }
            _ => {
                let text = anthropic_message_text(&message["content"]);
                if !text.trim().is_empty() {
                    push_anthropic_message(&mut out, "user", vec![json!({"type": "text", "text": text})]);
                }
            }
        }
    }
    (system.trim().to_string(), out)
}

fn anthropic_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function["name"].as_str()?;
            Some(json!({
                "name": name,
                "description": function["description"].as_str().unwrap_or(""),
                "input_schema": function["parameters"].clone(),
            }))
        })
        .collect()
}

/// Codex Responses tool schema. The harness stores OpenAI chat-compatible
/// definitions; the Responses wire format moves `name` and `parameters` to the
/// top level of a `function` tool.
fn codex_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function["name"].as_str()?;
            Some(json!({
                "type":"function",
                "name":name,
                "description":function["description"].as_str().unwrap_or(""),
                "parameters":function["parameters"].clone(),
            }))
        })
        .collect()
}

/// Normalizes a Responses `function_call` output item into the shape the
/// harness rebuilds from persisted messages. Extra response fields (`id`,
/// `status`) are deliberately dropped so chain signatures stay stable.
pub(super) fn codex_function_call_item(item: &Value) -> Option<Value> {
    if item["type"].as_str() != Some("function_call") {
        return None;
    }
    let call_id = item["call_id"]
        .as_str()
        .or_else(|| item["id"].as_str())
        .filter(|value| !value.is_empty())?;
    let name = item["name"].as_str().filter(|value| !value.is_empty())?;
    let arguments = item["arguments"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| item["arguments"].to_string());
    Some(json!({
        "type":"function_call",
        "call_id":call_id,
        "name":name,
        "arguments":arguments,
    }))
}

/// Extracts structured Codex tool calls from a final response output or from
/// one completed output item.
pub(super) fn codex_tool_calls(output: &[Value]) -> Vec<super::ToolCall> {
    let mut calls = Vec::new();
    for item in output {
        let Some(normalized) = codex_function_call_item(item) else {
            continue;
        };
        let id = normalized["call_id"].as_str().unwrap_or("").to_string();
        if calls.iter().any(|call: &super::ToolCall| call.id == id) {
            continue;
        }
        calls.push(super::ToolCall {
            id,
            name: normalized["name"].as_str().unwrap_or("").to_string(),
            arguments: normalized["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string(),
        });
    }
    calls
}

/// Converts chat messages into the Responses API shape used by the Codex
/// backend. System text becomes `instructions`; assistant tool calls and tool
/// observations stay structured so the model can continue a real tool loop.
fn codex_input(messages: &[Value]) -> (String, Vec<Value>) {
    let mut instructions = String::new();
    let mut input = Vec::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or("");
        let content = anthropic_message_text(&message["content"]);
        let has_image = message["content"].as_array().is_some_and(|parts| {
            parts.iter().any(|part| part["type"] == "image_url")
        });
        let tool_calls = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if content.trim().is_empty() && !has_image && tool_calls.is_empty() && role != "tool" {
            continue;
        }
        match role {
            "system" => {
                if !instructions.is_empty() {
                    instructions.push_str("\n\n");
                }
                instructions.push_str(&content);
            }
            "assistant" => {
                if !content.trim().is_empty() {
                    input.push(json!({
                        "role":"assistant",
                        "content":[{"type":"output_text","text":content}]
                    }));
                }
                for call in tool_calls {
                    let Some(name) = call["function"]["name"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    let Some(call_id) = call["id"].as_str().filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    input.push(json!({
                        "type":"function_call",
                        "call_id":call_id,
                        "name":name,
                        "arguments":call["function"]["arguments"].as_str().unwrap_or("{}"),
                    }));
                }
            }
            "tool" => {
                let Some(call_id) = message["tool_call_id"].as_str().filter(|value| !value.is_empty())
                else {
                    continue;
                };
                input.push(json!({
                    "type":"function_call_output",
                    "call_id":call_id,
                    "output":content,
                }));
            }
            _ => {
                let parts = if let Some(parts) = message["content"].as_array() {
                    parts.iter().filter_map(|part| match part["type"].as_str() {
                        Some("text") => Some(json!({"type":"input_text","text":part["text"]})),
                        Some("image_url") => {
                            let mut image = json!({"type":"input_image","image_url":part["image_url"]["url"]});
                            if let Some(detail) = part["image_url"].get("detail") {
                                image["detail"] = detail.clone();
                            }
                            Some(image)
                        }
                        _ => None,
                    }).collect::<Vec<_>>()
                } else {
                    vec![json!({"type":"input_text","text":content})]
                };
                input.push(json!({"role":"user","content":parts}));
            }
        }
    }
    if input.is_empty() {
        input.push(json!({
            "role":"user",
            "content":[{"type":"input_text","text":"ping"}]
        }));
    }
    (instructions, input)
}

/// Terminal failure events have identical meaning on HTTP and WebSocket.
pub(super) fn codex_terminal_error(piece: &Value) -> Option<String> {
    let message = match piece["type"].as_str()? {
        "response.failed" => piece["response"]["error"]["message"]
            .as_str().unwrap_or("응답이 실패했습니다.").to_string(),
        "response.incomplete" => format!(
            "응답이 완료되지 않았습니다: {}",
            piece["response"]["incomplete_details"]["reason"].as_str().unwrap_or("unknown")
        ),
        "error" => piece["error"]["message"].as_str().unwrap_or("알 수 없는 오류").to_string(),
        _ => return None,
    };
    Some(format!("Codex: {message}"))
}

/// Codex/ChatGPT subscription models speak the Responses API on a different
/// host and require the OAuth account id, not an API key.
async fn stream_codex_once(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    continuity: &mut Value,
) -> Result<Completion, String> {
    let account_id = provider
        .account_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("Codex OAuth 계정 ID가 없습니다. 모델 관리에서 Codex를 다시 가져오세요.")?;
    let started = std::time::Instant::now();
    let (instructions, input) = codex_input(messages);
    let mut body = json!({
        "model": provider.model,
        "instructions": if instructions.is_empty() { "You are a helpful assistant. Answer in Korean unless the user writes in another language.".to_string() } else { instructions },
        "input": input.clone(),
        "stream": true,
        "store": false,
    });
    if let Some(limit) = provider.max_output_tokens {
        body["max_output_tokens"] = json!(limit);
    }
    let codex_tools = codex_tools(tools);
    if !codex_tools.is_empty() {
        body["tools"] = json!(codex_tools);
        body["tool_choice"] = json!("auto");
    }
    let catalog = state.store.get("codex_model_catalog", &provider.id)
        .filter(|row|row["accountId"].as_str() == Some(account_id));
    // An explicit tier wins over a legacy inherited boolean setting.
    let effort = request.reasoning_effort.as_deref()
        .or_else(|| (request.thinking == Some(false)).then_some("none"));
    if let Some(reasoning) = crate::engine::reasoning::codex_options(catalog.as_ref(), &provider.model, effort)? {
        body["reasoning"] = reasoning;
    }
    let access = crate::engine::codex_oauth::fresh_access(
        state,
        client,
        &provider.id,
        &provider.api_key,
    )
    .await?;
    // Prefer the incremental websocket transport: the identity/persona
    // instructions are still sent every call, but the accumulated transcript
    // is replaced by the new suffix while the connection holds the chain.
    let ws_body=super::continuity::hinted_body(state,provider,&body,continuity,super::continuity::CacheWire::Key);
    let websocket = super::codex_ws::stream(
        state,
        provider,
        account_id,
        &access,
        continuity["transportSessionId"].as_str().unwrap_or(""),
        &ws_body,
        input,
        session_id,
        tx,
    )
    .await;
    match websocket {
        Ok(outcome) => {
            if continuity["wireHintSent"]==true {
                super::continuity::note_hint(state,provider,super::continuity::CacheWire::Key,true,200);
                continuity["cacheHintAcceptance"]=json!(true);
            }
            let mut completion = outcome.completion;
            fill_estimated_usage(messages,tools,&mut completion);
            continuity["incrementalTransport"] = outcome.transport;
            crate::engine::gateway::record_usage_tokens(
                state,
                &provider.id,
                &provider.model,
                provider.local,
                "chat",
                Some(session_id),
                completion.prompt_tokens,
                completion.completion_tokens,
                completion.usage,
                !completion.usage_exact,
            );
            return Ok(completion);
        }
        Err(super::codex_ws::WsError::Fatal(message)) => return Err(message),
        Err(super::codex_ws::WsError::HintRejected(reason)) => {
            super::continuity::note_hint(state,provider,super::continuity::CacheWire::Key,false,400);
            continuity["incrementalTransport"]=json!({"wire":"http","mode":"full-fallback","fallbackReason":reason});
        }
        Err(super::codex_ws::WsError::Fallback(reason)) => {
            continuity["incrementalTransport"] = json!({
                "wire":"http",
                "mode":"full-fallback",
                "fallbackReason":reason,
                "sentInputTokens":super::room_context::estimate(&json!(body["input"]).to_string()),
                "fullInputTokens":super::room_context::estimate(&json!(body["input"]).to_string()),
                "instructionTokens":super::room_context::estimate(&json!(body["instructions"]).to_string()),
                "modelCalls":0
            });
            state.store.audit(
                "codex.ws.fallback",
                json!({"sessionId":session_id,"reason":reason}),
            );
        }
    }
    let builder = client
        .post(format!("{}/responses", provider.base_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {access}"))
        .header("chatgpt-account-id", account_id)
        .header("OpenAI-Beta", "responses=experimental")
        .header("originator", "codex_cli_rs")
        .header("User-Agent", "codex_cli_rs/0.0.0")
        .header("session_id", continuity["transportSessionId"].as_str().unwrap_or(""))
        .header("x-client-request-id", continuity["cacheKey"].as_str().unwrap_or(""))
        .header("Content-Type", "application/json");
    let response=super::continuity::send(state,provider,builder,&body,continuity,super::continuity::CacheWire::Key).await?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if matches!(status.as_u16(), 401 | 403) {
            return Err("Codex OAuth 토큰이 만료되었거나 거부되었습니다. Codex 앱에서 다시 로그인한 뒤 모델 관리에서 다시 가져오세요.".into());
        }
        return Err(format!(
            "{} 호출 실패 (HTTP {status}): {}",
            provider.name,
            text.chars().take(600).collect::<String>()
        ));
    }

    let mut completion = Completion::default();
    let mut buffer = SseBuffer::default();
    let mut stream = response.bytes_stream();
    let mut terminated = false;
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
                break 'stream;
            }
            if payload.is_empty() {
                continue;
            }
            let piece: Value = match serde_json::from_str(&payload) {
                Ok(value) => value,
                Err(_) => continue,
            };
            match piece["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    let delta = piece["delta"].as_str().unwrap_or("");
                    if delta.is_empty() {
                        continue;
                    }
                    if completion.first_delta_ms.is_none() {
                        completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    completion.content.push_str(delta);
                    let _ = tx.send(json!({"type":"text","text":delta}));
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    let delta = piece["delta"].as_str().unwrap_or("");
                    if !delta.is_empty() {
                        if completion.first_delta_ms.is_none() {
                            completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                        }
                        completion.reasoning.push_str(delta);
                        let _ = tx.send(json!({"type":"reasoning","text":delta}));
                    }
                }
                "response.output_item.done" => {
                    if let Some(item) = piece.get("item") {
                        for call in codex_tool_calls(std::slice::from_ref(item)) {
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
                    if let Some(usage) = piece["response"]["usage"].as_object() {
                        completion.cached_prompt_tokens = cached_prompt_tokens(&json!(usage));
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
                    let output = piece["response"]["output"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for call in codex_tool_calls(&output) {
                        if !completion
                            .tool_calls
                            .iter()
                            .any(|existing| existing.id == call.id)
                        {
                            completion.tool_calls.push(call);
                        }
                    }
                    terminated = true;
                    break 'stream;
                }
                "response.failed" | "response.incomplete" => {
                    return Err(codex_terminal_error(&piece).unwrap());
                }
                "error" => {
                    let message = piece["error"]["message"].as_str().unwrap_or("알 수 없는 오류");
                    return Err(format!("Codex: {message}"));
                }
                _ => {}
            }
        }
    }
    if !terminated {
        return Err("Codex 스트림이 완료 신호 없이 종료되었습니다.".into());
    }
    fill_estimated_usage(messages,tools,&mut completion);
    crate::engine::gateway::record_usage_tokens(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        completion.prompt_tokens,
        completion.completion_tokens,
        completion.usage,
        !completion.usage_exact,
    );
    Ok(completion)
}

async fn stream_anthropic_once(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
    continuity: &mut Value,
) -> Result<Completion, String> {
    let started = std::time::Instant::now();
    let (system, provider_messages) = anthropic_messages(messages);
    let mut body = json!({
        "model": provider.model,
        "messages": provider_messages,
        "stream": true,
        "max_tokens": provider.max_output_tokens.unwrap_or(4096),
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    let tools = anthropic_tools(tools);
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    let effort = request.reasoning_effort.as_deref();
    if request.thinking == Some(true)
        || (request.thinking != Some(false) && matches!(effort, Some("low" | "medium" | "high")))
    {
        let budget = match effort {
            Some("low") => 1024,
            Some("medium") => 2048,
            _ => 4096,
        };
        body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
        let max_tokens = body["max_tokens"].as_u64().unwrap_or(4096).max(budget + 1024);
        body["max_tokens"] = json!(max_tokens);
    }

    let _permit = crate::engine::gateway::acquire(&provider.base_url, provider.local).await;
    let builder = client
        .post(format!("{}/messages", provider.base_url))
        .header("Content-Type", "application/json")
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01");
    let response=super::continuity::send(state,provider,builder,&body,continuity,super::continuity::CacheWire::Ephemeral).await?;
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
    let mut blocks = std::collections::BTreeMap::<usize, (String, String, String)>::new();
    let mut buffer = SseBuffer::default();
    let mut stream = response.bytes_stream();
    let mut terminated = false;
    let mut text_pending = String::new();
    let mut in_text_tool_call = false;
    let mut input_tokens = 0u64;
    let mut input_reported = false;
    let mut output_reported = false;
    let mut cache_read_tokens = 0u64;
    let mut cache_read_reported = false;
    let mut cache_write_tokens = 0u64;
    let mut output_tokens = 0u64;

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
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let piece: Value = serde_json::from_str(&payload)
                .map_err(|_| "Anthropic 스트림 JSON이 올바르지 않습니다.")?;
            match piece["type"].as_str().unwrap_or("") {
                "message_start" => {
                    input_reported=piece["message"]["usage"]["input_tokens"].as_u64().is_some();
                    input_tokens = piece["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
                    cache_read_reported = piece["message"]["usage"]["cache_read_input_tokens"].as_u64().is_some();
                    cache_read_tokens = piece["message"]["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    cache_write_tokens = piece["message"]["usage"]["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                }
                "content_block_start" => {
                    let content = &piece["content_block"];
                    if content["type"].as_str() == Some("tool_use") {
                        let index = piece["index"].as_u64().unwrap_or(0) as usize;
                        let initial = content["input"]
                            .as_object()
                            .filter(|input| !input.is_empty())
                            .map(|_| content["input"].to_string())
                            .unwrap_or_default();
                        blocks.insert(
                            index,
                            (
                                content["id"].as_str().unwrap_or("").to_string(),
                                content["name"].as_str().unwrap_or("tool").to_string(),
                                initial,
                            ),
                        );
                        if completion.first_delta_ms.is_none() {
                            completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                        }
                    }
                }
                "content_block_delta" => {
                    let delta = &piece["delta"];
                    match delta["type"].as_str().unwrap_or("") {
                        "text_delta" => {
                            let text = delta["text"].as_str().unwrap_or("");
                            if !text.is_empty() {
                                if completion.first_delta_ms.is_none() {
                                    completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                                }
                                completion.content.push_str(text);
                                stream_visible_text(tx, text, &mut text_pending, &mut in_text_tool_call);
                            }
                        }
                        "thinking_delta" => {
                            let text = delta["thinking"].as_str().unwrap_or("");
                            if !text.is_empty() {
                                if completion.first_delta_ms.is_none() {
                                    completion.first_delta_ms = Some(started.elapsed().as_millis() as u64);
                                }
                                completion.reasoning.push_str(text);
                                let _ = tx.send(json!({"type": "reasoning", "text": text}));
                            }
                        }
                        "input_json_delta" => {
                            let index = piece["index"].as_u64().unwrap_or(0) as usize;
                            if let Some(entry) = blocks.get_mut(&index) {
                                entry.2.push_str(delta["partial_json"].as_str().unwrap_or(""));
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    let index = piece["index"].as_u64().unwrap_or(0) as usize;
                    if let Some((id, name, raw)) = blocks.remove(&index) {
                        let arguments = if raw.trim().is_empty() {
                            json!({})
                        } else {
                            serde_json::from_str::<Value>(&raw)
                                .ok()
                                .filter(Value::is_object)
                                .unwrap_or_else(|| json!({}))
                        };
                        calls.push(ToolCall {
                            id: if id.is_empty() { uuid::Uuid::new_v4().to_string() } else { id },
                            name,
                            arguments: arguments.to_string(),
                        });
                    }
                }
                "message_delta" => {
                    output_reported|=piece["usage"]["output_tokens"].as_u64().is_some();
                    output_tokens = piece["usage"]["output_tokens"].as_u64().unwrap_or(output_tokens);
                }
                "message_stop" => {
                    terminated = true;
                    break 'stream;
                }
                "error" => {
                    return Err(format!("Anthropic 오류: {}", piece["error"]));
                }
                _ => {}
            }
        }
        if completion.content.len() + completion.reasoning.len() > 2_000_000 {
            return Err("모델 응답 크기 제한을 초과했습니다.".into());
        }
    }
    if !in_text_tool_call
        && !text_pending.is_empty()
        && !TEXT_TOOL_OPEN.starts_with(&text_pending)
    {
        let _ = tx.send(json!({"type": "text", "text": text_pending}));
    }
    if !terminated {
        return Err("Anthropic 스트림이 완료 신호 없이 종료되었습니다.".into());
    }
    completion.tool_calls = calls;
    completion.prompt_tokens = input_tokens.saturating_add(cache_read_tokens).saturating_add(cache_write_tokens);
    completion.cached_prompt_tokens = cache_read_reported.then_some(cache_read_tokens);
    completion.completion_tokens = output_tokens;
    completion.usage = completion.prompt_tokens + output_tokens;
    completion.usage_exact = input_reported&&output_reported;
    fill_estimated_usage(messages,&tools,&mut completion);
    crate::engine::gateway::record_usage_tokens(
        state,
        &provider.id,
        &provider.model,
        provider.local,
        "chat",
        Some(session_id),
        completion.prompt_tokens,
        completion.completion_tokens,
        completion.usage,
        !completion.usage_exact,
    );
    Ok(completion)
}

// Keep bytes until a whole SSE event arrives; network chunks may split a
// Korean UTF-8 character or the CRLF separator at any byte.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_input_preserves_attachment_text_and_images() {
        let image = "data:image/png;base64,aW1hZ2U=";
        let (instructions, input) = codex_input(&[
            json!({"role":"system","content":"identity"}),
            json!({"role":"user","content":[
                {"type":"text","text":"첨부를 분석해"},
                {"type":"text","text":"quoted file contents"},
                {"type":"image_url","image_url":{"url":image,"detail":"high"}}
            ]}),
            json!({"role":"user","content":[{"type":"image_url","image_url":{"url":image}}]}),
        ]);
        assert_eq!(instructions, "identity");
        assert_eq!(input.len(), 2, "attachment messages must not disappear");
        assert_eq!(input[0]["content"], json!([
            {"type":"input_text","text":"첨부를 분석해"},
            {"type":"input_text","text":"quoted file contents"},
            {"type":"input_image","image_url":image,"detail":"high"}
        ]));
        assert_eq!(input[1]["content"][0]["image_url"], image);
    }

    #[test]
    fn codex_input_keeps_tool_calls_and_results_structured() {
        let messages = vec![
            json!({"role":"system","content":"identity"}),
            json!({"role":"user","content":"write a file"}),
            json!({
                "role":"assistant","content":"",
                "tool_calls":[{
                    "id":"call-1","type":"function",
                    "function":{"name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}
                }]
            }),
            json!({"role":"tool","tool_call_id":"call-1","content":"ok"}),
        ];
        let (instructions, input) = codex_input(&messages);
        assert_eq!(instructions, "identity");
        assert_eq!(input.len(), 3, "{input:?}");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call-1");
        assert_eq!(input[1]["name"], "workspace_write");
        assert_eq!(input[1]["arguments"], "{\"path\":\"a.txt\"}");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call-1");
        assert_eq!(input[2]["output"], "ok");
    }

    #[test]
    fn codex_function_calls_become_harness_tool_calls_without_duplicates() {
        let output = vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"preamble"}]}),
            json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}),
            json!({"type":"function_call","id":"fc_1d","call_id":"call_1","name":"workspace_write","arguments":"{\"path\":\"a.txt\"}"}),
        ];
        let calls = codex_tool_calls(&output);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "workspace_write");
        assert_eq!(calls[0].arguments, "{\"path\":\"a.txt\"}");
    }

    #[test]
    fn codex_tool_schema_moves_function_fields_to_the_top_level() {
        let tools = vec![json!({
            "type":"function",
            "function":{
                "name":"workspace_write",
                "description":"write",
                "parameters":{"type":"object"}
            }
        })];
        let codex = codex_tools(&tools);
        assert_eq!(codex.len(), 1, "{codex:?}");
        assert_eq!(codex[0]["type"], "function");
        assert_eq!(codex[0]["name"], "workspace_write");
        assert_eq!(codex[0]["description"], "write");
        assert_eq!(codex[0]["parameters"]["type"], "object");
        assert!(codex[0].get("function").is_none(), "{codex:?}");
    }

    #[test]
    fn reflection_quotes_missing_reasoning_without_dropping_old_context() {
        let messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"user","content":"original owner request"}),
            json!({"role":"assistant","content":"old assistant answer","reasoning_content":""}),
            json!({"role":"user","content":"followup"}),
            json!({"role":"assistant","content":"native beam","reasoning_content":"original thinking"}),
            json!({"role":"user","content":"current"}),
        ];
        let (preserved, quoted) = preserve_reflection_history(messages);
        assert!(quoted);
        assert_eq!(preserved.len(), 6);
        assert!(preserved[2]["content"].as_str().unwrap().contains("old assistant answer"));
        assert_eq!(preserved[4]["role"], "assistant");
        assert_eq!(preserved[4]["reasoning_content"], "original thinking");
        assert_eq!(preserved[5]["content"], "current");
    }

    #[test]
    fn runtime_hints_never_split_an_assistant_tool_call_from_its_result() {
        let messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"user","content":"write a file"}),
            json!({"role":"assistant","content":"","tool_calls":[{"id":"call-1","type":"function","function":{"name":"workspace_write","arguments":"{}"}}]}),
            json!({"role":"system","content":"read-before-write hint"}),
            json!({"role":"tool","tool_call_id":"call-1","content":"ok"}),
            json!({"role":"user","content":"continue"}),
        ];
        let ordered = order_tool_exchanges(messages);
        let roles = ordered.iter().map(|message| message["role"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(roles, vec!["system","user","assistant","tool","system","user"], "{ordered:?}");
        assert_eq!(ordered[3]["tool_call_id"], "call-1");
        assert_eq!(ordered[4]["content"], "read-before-write hint");
    }

    #[test]
    fn cache_usage_distinguishes_hit_from_missing_telemetry() {
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":4032}})),Some(4032));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_cache_hit_tokens":1280})),Some(1280));
        assert_eq!(cached_prompt_tokens(&json!({"input_tokens_details":{"cached_tokens":512}})),Some(512));
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens":4049})),None);
        assert_eq!(cached_prompt_tokens(&json!({"prompt_tokens_details":{"cached_tokens":0}})),Some(0));
    }

    #[test]
    fn reflection_requires_reasoning_content_on_assistant_history() {
        let mut messages = vec![
            json!({"role":"system","content":"s"}),
            json!({"role":"assistant","content":"a"}),
            json!({"role":"assistant","content":"b","reasoning_content":"kept"}),
            json!({"role":"user","content":"u"}),
        ];
        normalize_assistant_reasoning(&mut messages, true);
        assert_eq!(messages[1]["reasoning_content"], "", "{messages:?}");
        assert_eq!(messages[2]["reasoning_content"], "kept", "{messages:?}");
        assert!(messages[0].get("reasoning_content").is_none());
        normalize_assistant_reasoning(&mut messages, false);
        assert!(messages[1].get("reasoning_content").is_none(), "{messages:?}");
    }

    #[test]
    fn local_chat_template_never_receives_openai_reasoning_effort() {
        let mut body = json!({"model":"llama3.2:3b"});
        apply_reasoning_options(
            &mut body,
            "chat_template",
            Some(false),
            Some("low"),
        );
        assert!(
            body.get("reasoning_effort").is_none(),
            "Ollama rejects reasoning_effort on non-thinking models: {body}"
        );
        assert_eq!(
            body["chat_template_kwargs"]["enable_thinking"],
            false,
            "{body}"
        );

        let mut qwen = json!({"model":"qwen3:4b"});
        apply_reasoning_options(&mut qwen, "qwen", Some(true), Some("high"));
        assert!(qwen.get("reasoning_effort").is_none(), "{qwen}");
        assert_eq!(qwen["enable_thinking"], true, "{qwen}");

        let mut api = json!({"model":"reasoning-model"});
        apply_reasoning_options(&mut api, "openai", Some(true), Some("high"));
        assert_eq!(api["reasoning_effort"], "high", "{api}");
    }

    #[test]
    fn each_provider_receives_its_own_tool_window_from_full_evidence() {
        let base = ProviderRef {
            id:"local".into(), name:"local".into(), base_url:String::new(),
            api_key:String::new(), model:"small".into(), tool_mode:"compat".into(),
            reasoning_protocol:String::new(), local:true,
            context_tokens:Some(4096), max_output_tokens:Some(512),
            requests_per_minute: None, kind:"openai".into(), account_id:None,
        };
        let original = vec![
            json!({"role":"system","content":"working context"}),
            json!({"role":"tool","tool_call_id":"old","content":"x".repeat(40_000)}),
            json!({"role":"tool","tool_call_id":"latest","content":"y".repeat(9000)}),
        ];
        let mut local_messages = original.clone();
        fit_tool_context(&base, &mut local_messages, 500);
        let local_latest = local_messages[2]["content"].as_str().unwrap();
        assert_eq!(local_latest.chars().count(), 9000, "most recent evidence wins");
        assert!(local_messages[1]["content"].as_str().unwrap().chars().count() < 40_000);
        assert_eq!(local_messages[1]["tool_call_id"], "old");

        let mut api = base.clone();
        api.context_tokens = Some(128_000);
        api.local = false;
        let mut api_messages = original.clone();
        fit_tool_context(&api, &mut api_messages, 500);
        assert_eq!(api_messages, original, "large API receives full evidence");
        assert_eq!(original[1]["content"].as_str().unwrap().chars().count(), 40_000);
    }

    #[test]
    fn json_escaped_tool_output_respects_provider_budget() {
        let original = "\"\n\\세션".repeat(120);
        let clipped = clip_tool_for_provider(&original, 160);
        assert!(escaped_content_chars(&clipped) <= 160);
        assert!(clipped.contains("stored execution_operation"));
    }

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

    #[tokio::test]
    async fn failed_native_cloud_route_switches_to_compat_local_with_tool_manual() {
        use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        async fn local_model(
            State(observed): State<std::sync::Arc<std::sync::Mutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> axum::response::Response {
            observed.lock().unwrap().push(body);
            let payload = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"로컬 대체 완료"}}]})
            );
            ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], payload)
                .into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/chat/completions", post(local_model))
            .with_state(observed.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-mixed-failover-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone(),
        };
        let state = AppState::new(config, crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")));
        state.store.put("session", &json!({"id":"mixed"}));
        state.store.put("provider", &json!({
            "id":"cloud","baseUrl":"http://127.0.0.1:1","model":"cloud-native",
            "toolMode":"native","enabled":true,
        }));
        state.store.put("provider", &json!({
            "id":"local","baseUrl":format!("http://{address}"),"model":"local-compat",
            "toolMode":"compat","enabled":true,"local":true,"contextTokens":8192,
        }));
        let request = TurnRequest::from_value(&json!({
            "input":"도구로 파일 읽기","sessionId":"mixed","providerId":"cloud",
            "autoLearning":false,
        })).unwrap();
        let primary = select_provider(&state,&request,&state.store.get("session","mixed").unwrap()).unwrap();
        let tools = vec![json!({
            "type":"function","function":{"name":"workspace_read","description":"Read a file",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}}}}
        })];
        let messages = vec![
            json!({"role":"system","content":"You are a helper"}),
            json!({"role":"user","content":"도구로 파일 읽기"}),
            json!({"role":"assistant","content":"","tool_calls":[{
                "id":"tc1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"test.txt\"}"}
            }]}),
            json!({"role":"tool","tool_call_id":"tc1","content":"test.txt 내용"}),
        ];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (answer, selected) = tokio::time::timeout(std::time::Duration::from_secs(5),
            stream_completion(&state,&reqwest::Client::new(),&primary,
                &messages,&tools,&request,"mixed",&tx,true)
        ).await.unwrap().unwrap();
        assert_eq!(selected.id,"local");
        assert_eq!(answer.content,"로컬 대체 완료");
        let calls = observed.lock().unwrap();
        assert_eq!(calls.len(),1);
        let body = &calls[0];
        assert!(body.get("tools").is_none());
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("Available tools:"));
        assert!(prompt.contains("workspace_read"));
        assert_eq!(body["messages"][3]["role"],"user");
        assert!(body["messages"][3]["content"].as_str().unwrap().contains("test.txt 내용"));
        server.abort();
        let _ = std::fs::remove_dir_all(root);
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
                &request, "session-failover", &tx, true,
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

    #[tokio::test]
    async fn anthropic_native_stream_translates_messages_tools_and_usage() {
        use axum::{routing::post, Router, Json};
        use std::sync::{Arc, Mutex};
        let observed = Arc::new(Mutex::new(Vec::<Value>::new()));
        let captured = observed.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/messages",
            post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push(json!({
                        "headerKey": headers.get("x-api-key").and_then(|v| v.to_str().ok()),
                        "headerVersion": headers.get("anthropic-version").and_then(|v| v.to_str().ok()),
                        "body": body,
                    }));
                    let events = [
                        json!({"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":0}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"안녕"}}),
                        json!({"type":"content_block_stop","index":0}),
                        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tool_1","name":"workspace_read","input":{}}}),
                        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"x\"}"}}),
                        json!({"type":"content_block_stop","index":1}),
                        json!({"type":"message_delta","usage":{"output_tokens":5}}),
                        json!({"type":"message_stop"}),
                    ];
                    let payload = events
                        .iter()
                        .map(|event| format!("event: x\ndata: {event}\n\n"))
                        .collect::<String>();
                    ([(axum::http::header::CONTENT_TYPE, "text/event-stream")], payload)
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let root = std::env::temp_dir().join(format!("dominion-anthropic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port: 0, data_dir: root.clone(), web_dir: root.join("web"), workspace: root.clone(),
        };
        let state = AppState::new(
            config,
            crate::db::Store::open(&root.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&root.join("keys.json")),
        );
        let provider = ProviderRef {
            id: "anthropic".into(), name: "Anthropic".into(),
            base_url: format!("http://{address}"), api_key: "secret".into(),
            model: "claude-test".into(), tool_mode: "native".into(),
            reasoning_protocol: "anthropic".into(), local: false,
            context_tokens: None, max_output_tokens: Some(4096),
            requests_per_minute: None, kind: "anthropic".into(), account_id: None,
        };
        let request = TurnRequest::from_value(&json!({
            "input":"안녕","sessionId":"s","autoLearning":false
        })).unwrap();
        let messages = vec![
            json!({"role":"system","content":"system rule"}),
            json!({"role":"user","content":"안녕"}),
            json!({"role":"assistant","content":"","tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"x\"}"}}
            ]}),
            json!({"role":"tool","tool_call_id":"call_1","content":"ok"}),
        ];
        let tools = vec![json!({"type":"function","function":{
            "name":"workspace_read","description":"read",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}}}
        }})];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let completion = stream_completion_once(
            &state, &reqwest::Client::new(), &provider, &messages, &tools,
            &request, "s", &tx, false,
        ).await.unwrap();
        assert_eq!(completion.content, "안녕");
        assert_eq!(completion.usage, 15);
        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "workspace_read");
        assert_eq!(
            serde_json::from_str::<Value>(&completion.tool_calls[0].arguments).unwrap(),
            json!({"path":"x"})
        );
        let calls = observed.lock().unwrap();
        let body = &calls[0]["body"];
        assert_eq!(calls[0]["headerKey"], "secret");
        assert_eq!(calls[0]["headerVersion"], "2023-06-01");
        assert_eq!(anthropic_message_text(&body["system"]), "system rule");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(completion.continuity.is_some());
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }
}
