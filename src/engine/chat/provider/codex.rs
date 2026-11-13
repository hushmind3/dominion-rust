//! Codex streaming and structured tool results.
//! Active compiled child of provider.rs.
use super::*;

pub(crate) async fn stream_codex_once(
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
    let ws_body=super::super::continuity::hinted_body(state,provider,&body,continuity,super::super::continuity::CacheWire::Key);
    let websocket = super::super::codex_ws::stream(
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
                super::super::continuity::note_hint(state,provider,super::super::continuity::CacheWire::Key,true,200);
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
        Err(super::super::codex_ws::WsError::Fatal(message)) => return Err(message),
        Err(super::super::codex_ws::WsError::HintRejected(reason)) => {
            super::super::continuity::note_hint(state,provider,super::super::continuity::CacheWire::Key,false,400);
            continuity["incrementalTransport"]=json!({"wire":"http","mode":"full-fallback","fallbackReason":reason});
        }
        Err(super::super::codex_ws::WsError::Fallback(reason)) => {
            continuity["incrementalTransport"] = json!({
                "wire":"http",
                "mode":"full-fallback",
                "fallbackReason":reason,
                "sentInputTokens":super::super::room_context::estimate(&json!(body["input"]).to_string()),
                "fullInputTokens":super::super::room_context::estimate(&json!(body["input"]).to_string()),
                "instructionTokens":super::super::room_context::estimate(&json!(body["instructions"]).to_string()),
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
    let response=super::super::continuity::send(state,provider,builder,&body,continuity,super::super::continuity::CacheWire::Key).await?;
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

