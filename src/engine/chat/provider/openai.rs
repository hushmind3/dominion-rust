//! OpenAI-compatible SSE transport and usage metering; provider and cache identity remain defined by the shared gateway.
use super::*;

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
        // Interactive room rotations may skip a participant that took too
        // long. Normal chat and especially RSI/code-edit runs must be able
        // to use a slow-but-working local model on the next tool round.
        // Otherwise one successful 60s prefill blacklists the model for 15m
        // and aborts the very run that just used it successfully.
        let is_room = state.store.get("session", session_id)
            .is_some_and(|session| session["roomId"].is_string());
        if should_mark_slow_provider(candidate_started.elapsed(), is_room) {
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
pub(super) fn compat_messages(messages: &[Value]) -> Vec<Value> {
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

pub(super) async fn stream_completion_once(
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

pub(super) async fn stream_completion_once_body(
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
        if state.store.get("session", session_id)
            .is_some_and(|session| rsi_edit_only_tool_menu(&session, tools)) {
            body["tool_choice"] = json!("required");
        }
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

