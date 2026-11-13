//! Anthropic streaming and tool event integration.
//! Active compiled child of provider.rs.
use super::protocols::*;
use super::*;

pub(crate) async fn stream_anthropic_once(
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
    let response=super::super::continuity::send(state,provider,builder,&body,continuity,super::super::continuity::CacheWire::Ephemeral).await?;
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
    crate::engine::gateway::record_cache_usage(state, session_id, completion.prompt_tokens, completion.cached_prompt_tokens, completion.usage_exact);
    Ok(completion)
}

// Keep bytes until a whole SSE event arrives; network chunks may split a
// Korean UTF-8 character or the CRLF separator at any byte.

