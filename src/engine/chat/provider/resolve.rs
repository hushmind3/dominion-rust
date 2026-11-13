//! Provider routing, reasoning options, tool-message fitting and failover.
//! Active compiled child of provider.rs.
use super::*;

pub(super) fn stream_visible_text(
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

pub(in super::super) fn apply_reasoning_options(
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
pub(super) fn transient_provider_error(error: &str) -> bool {
    error.contains(" 연결 실패:")
        || error.contains("스트림 오류:")
        || error.contains("모델 스트림이 완료 신호 없이 종료")
        || error.contains("(HTTP 408")
        || error.contains("(HTTP 429")
        || error.contains("(HTTP 5")
}

/// Estimated input room for the declared model context, leaving its
/// configured generation capacity reserved. No universal character cap.
pub(super) fn input_char_capacity(tokens: u64, max_output_tokens: Option<u64>) -> usize {
    let reserved = max_output_tokens.unwrap_or(tokens / 8)
        .min(tokens / 4).max(128).min(tokens);
    (tokens.saturating_sub(reserved) as usize).saturating_mul(4)
}

/// Count the fixed prompt without the bodies of prior tool observations.
/// Those are dynamically reduced for each candidate provider at request time.
pub(super) fn fixed_prompt_chars(messages: &[Value]) -> usize {
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
pub(super) fn normalize_assistant_reasoning(messages: &mut [Value], required: bool) {
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
pub(super) fn order_tool_exchanges(messages: Vec<Value>) -> Vec<Value> {
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
pub(super) fn preserve_reflection_history(messages: Vec<Value>) -> (Vec<Value>, bool) {
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

pub(in super::super) fn cached_prompt_tokens(usage: &Value) -> Option<u64> {
    usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64)
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens").and_then(Value::as_u64))
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
}

pub(super) fn escaped_content_chars(text: &str) -> usize {
    text.chars().map(|ch| match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
        '\u{0000}'..='\u{001f}' => 6,
        _ => 1,
    }).sum()
}

/// Trim just the provider-visible copy, retaining the newest observations
/// first. All raw results remain in the runtime/DB and are available when
/// a larger context provider takes over a later round.
pub(super) fn clip_tool_for_provider(text: &str, allowed_encoded_chars: usize) -> String {
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
pub(super) fn fit_tool_context(provider: &ProviderRef, messages: &mut [Value], schema_chars: usize) {
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

pub(super) fn room_route_keys(state: &AppState, session_id: &str) -> Option<std::collections::HashSet<String>> {
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

pub(super) async fn fallback_candidates(
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
        escaped_content_chars(&super::super::protocol::tool_manual(tools, &request.input).await)
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
pub(super) fn estimate_usage(messages: &[Value], tools: &[Value], completion: &Completion) -> (u64, u64) {
    (
        super::super::room_context::input_estimate(messages, tools) as u64,
        super::super::room_context::estimate(&format!("{}{}",completion.content,completion.reasoning)).max(1) as u64,
    )
}

pub(super) fn fill_estimated_usage(messages:&[Value],tools:&[Value],completion:&mut Completion){
    if !completion.usage_exact {
        let(prompt,output)=estimate_usage(messages,tools,completion);
        completion.prompt_tokens=prompt;
        completion.completion_tokens=output;
        completion.usage=prompt.saturating_add(output);
        completion.cached_prompt_tokens=None;
    }
}

