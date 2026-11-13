//! Anthropic and Codex message conversion and tool adapters.
//! Active compiled child of provider.rs.
use super::*;

pub(super) fn anthropic_message_text(value: &Value) -> String {
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

pub(super) fn push_anthropic_message(out: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
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
pub(super) fn anthropic_messages(messages: &[Value]) -> (String, Vec<Value>) {
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

pub(super) fn anthropic_tools(tools: &[Value]) -> Vec<Value> {
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
pub(super) fn codex_tools(tools: &[Value]) -> Vec<Value> {
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
pub(in super::super) fn codex_function_call_item(item: &Value) -> Option<Value> {
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
pub(in super::super) fn codex_tool_calls(output: &[Value]) -> Vec<super::super::ToolCall> {
    let mut calls = Vec::new();
    for item in output {
        let Some(normalized) = codex_function_call_item(item) else {
            continue;
        };
        let id = normalized["call_id"].as_str().unwrap_or("").to_string();
        if calls.iter().any(|call: &super::super::ToolCall| call.id == id) {
            continue;
        }
        calls.push(super::super::ToolCall {
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
pub(super) fn codex_input(messages: &[Value]) -> (String, Vec<Value>) {
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
pub(in super::super) fn codex_terminal_error(piece: &Value) -> Option<String> {
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
