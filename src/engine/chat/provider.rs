//! OpenAI-compatible provider resolution, native thinking controls and HTTP streaming.
use super::{protocol::SseBuffer, Completion, ProviderRef, ToolCall, TurnRequest, MAX_TOOL_CALLS};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

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
        .unwrap_or_else(|| {
            if kind == "ollama" || provider["local"].as_bool() == Some(true) {
                "compat".into()
            } else {
                "auto".into()
            }
        });

    let reasoning_protocol = provider["reasoningProtocol"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            if model.to_ascii_lowercase().contains("deepseek") {
                "deepseek".into()
            } else if kind == "ollama" || id == "dominion-native" {
                "chat_template".into()
            } else {
                "openai".into()
            }
        });
    Ok(ProviderRef {
        id,
        name: provider
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("Provider")
            .to_string(),
        base_url,
        api_key,
        model,
        tool_mode,
        reasoning_protocol,
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

pub(super) async fn stream_completion(
    state: &AppState,
    client: &reqwest::Client,
    provider: &ProviderRef,
    messages: &[Value],
    tools: &[Value],
    request: &TurnRequest,
    session_id: &str,
    tx: &UnboundedSender<Value>,
) -> Result<Completion, String> {
    let started = std::time::Instant::now();
    let mut provider_messages = messages.to_vec();
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
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    apply_reasoning_options(
        &mut body,
        &provider.reasoning_protocol,
        request.thinking,
        request.reasoning_effort.as_deref(),
    );

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
                if matches!(reason, "length" | "content_filter") {
                    return Err(format!("모델 응답이 완료되지 않았습니다: {reason}"));
                }
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

    Ok(completion)
}

// Keep bytes until a whole SSE event arrives; network chunks may split a
// Korean UTF-8 character or the CRLF separator at any byte.
