//! Bounded SSE framing and optional text tool-call protocol for compatible local models.
use serde_json::{json, Value};

pub(super) async fn tool_manual(native: &[Value]) -> String {
    let mut manual = String::from(
        "You cannot use native function calling. To call a tool, output exactly one line:\n\
         <tool_call>{\"name\":\"tool_name\",\"arguments\":{...}}</tool_call>\n\
         Available tools:",
    );
    let mut seen = std::collections::HashSet::new();
    for definition in native {
        let function = &definition["function"];
        let name = function
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        manual.push_str(&format!(
            "\n- {}: {}\n  arguments: {}",
            name,
            function
                .get("description")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            function.get("parameters").cloned().unwrap_or(json!({}))
        ));
    }
    manual
}

// Parses text-based tool calls used by small models and `toolMode: compat`.

pub fn parse_text_tool_calls(content: &str) -> (Vec<(String, String)>, String) {
    let mut calls = Vec::new();
    let mut cleaned = content.to_string();

    while let Some(start) = cleaned.find("<tool_call>") {
        let Some(end) = cleaned[start..].find("</tool_call>") else {
            break;
        };
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
            let Some(close) = cleaned[after..].find("```") else {
                break;
            };
            let close = after + close;
            let body = &cleaned[after..close];
            let body = body.trim_start_matches(|ch| {
                ch == 'j' || ch == 's' || ch == 'o' || ch == 'n' || ch == '\n'
            });
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

#[derive(Default)]
pub(super) struct SseBuffer(Vec<u8>);

impl SseBuffer {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.0.len() + bytes.len() > 2_000_000 {
            return Err("SSE 이벤트 크기 제한을 초과했습니다.".into());
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Result<Option<String>, String> {
        let lf = self
            .0
            .windows(2)
            .position(|part| part == b"\n\n")
            .map(|index| (index, 2));
        let crlf = self
            .0
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .map(|index| (index, 4));
        let Some((index, size)) = [lf, crlf].into_iter().flatten().min_by_key(|part| part.0) else {
            return Ok(None);
        };
        let bytes: Vec<u8> = self.0.drain(..index + size).collect();
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "모델 스트림에 잘못된 UTF-8이 있습니다.".into())
    }
}
