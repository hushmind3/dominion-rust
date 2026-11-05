//! Bounded SSE framing and optional text tool-call protocol for compatible local models.
use serde_json::{json, Value};

pub(super) async fn tool_manual(native: &[Value], query: &str) -> String {
    let mut manual = String::from(
        "You cannot use native function calling. To call a tool, output exactly one line:\n\
         <tool_call>{\"name\":\"tool_name\",\"arguments\":{...}}</tool_call>\n\
         Available tools:",
    );
    let lower = query.to_lowercase();
    let mut ranked = native.to_vec();
    ranked.sort_by_key(|definition| {
        let name = definition["function"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let mut score = 10u8;
        if (name.contains("workspace") || name.contains("file"))
            && ["파일", "폴더", "저장", "읽", "작성", "만들", "수정", "삭제"]
                .iter()
                .any(|term| lower.contains(term))
        {
            score += 10;
        }
        if (name.contains("shell") || name.contains("terminal"))
            && ["실행", "명령", "빌드", "테스트", "설치"]
                .iter()
                .any(|term| lower.contains(term))
        {
            score += 12;
        }
        if (name.contains("web") || name.contains("http"))
            && ["검색", "웹", "http", "url", "찾아"]
                .iter()
                .any(|term| lower.contains(term))
        {
            score += 10;
        }
        if (name.contains("memory") || name.contains("skill"))
            && ["기억", "메모리", "스킬", "학습"]
                .iter()
                .any(|term| lower.contains(term))
        {
            score += 10;
        }
        std::cmp::Reverse(score)
    });
    let mut seen = std::collections::HashSet::new();
    for definition in ranked.iter().take(10) {
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
    manual.push_str("\nExample: <tool_call>{\"name\":\"workspace_read\",\"arguments\":{\"path\":\"a.txt\"}}</tool_call>");
    manual
}

// Parses text-based tool calls used by small models and `toolMode: compat`.

pub fn parse_text_tool_calls(content: &str) -> (Vec<(String, String)>, String) {
    parse_text_tool_calls_known(content, &std::collections::HashSet::new())
}

pub fn parse_text_tool_calls_known(
    content: &str,
    known: &std::collections::HashSet<String>,
) -> (Vec<(String, String)>, String) {
    let mut calls = Vec::new();
    let mut cleaned = content.to_string();

    // A small model often forgets the closing tag. Parse the remainder too.
    if let Some(start) = cleaned.find("<tool_call>") {
        if !cleaned[start..].contains("</tool_call>") {
            let body = cleaned[start + "<tool_call>".len()..].to_string();
            if let Some(call) = parse_tool_json_known(&body, known) {
                calls.push(call);
                cleaned.replace_range(start.., "");
            }
        }
    }

    while let Some(start) = cleaned.find("<tool_call>") {
        let Some(end) = cleaned[start..].find("</tool_call>") else {
            break;
        };
        let end = start + end + "</tool_call>".len();
        let block = cleaned[start + "<tool_call>".len()..end - "</tool_call>".len()].to_string();
        if let Some(call) = parse_tool_json_known(&block, known) {
            calls.push(call);
            cleaned.replace_range(start..end, "");
        } else {
            cleaned.replace_range(start..end, "");
        }
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
            if let Some(call) = parse_tool_json_known(body, known) {
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
    // Bare JSON objects are common with 4B models. Scan balanced objects and
    // only accept shapes that look like a known tool call.
    if calls.is_empty() {
        let mut removals = Vec::new();
        for (start, end) in json_objects(&cleaned) {
            let body = &cleaned[start..=end];
            if let Some(call) = parse_tool_json_known(body, known) {
                calls.push(call);
                removals.push((start, end));
            }
        }
        for (start, end) in removals.into_iter().rev() {
            cleaned.replace_range(start..=end, "");
        }
    }

    (calls, cleaned.trim().to_string())
}

fn json_objects(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'{' {
            index += 1;
            continue;
        }
        let start = index;
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;
        while index < bytes.len() {
            let ch = bytes[index];
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == b'\\' {
                    escaped = true;
                } else if ch == b'"' {
                    in_string = false;
                }
            } else if ch == b'"' {
                in_string = true;
            } else if ch == b'{' {
                depth += 1;
            } else if ch == b'}' {
                depth -= 1;
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            index += 1;
        }
        if let Some(end) = end {
            out.push((start, end));
            index = end + 1;
        } else {
            index = start + 1;
        }
    }
    out
}

fn parse_tool_json_known(
    text: &str,
    known: &std::collections::HashSet<String>,
) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    parse_tool_value(&value, known)
}

fn parse_tool_value(
    value: &Value,
    known: &std::collections::HashSet<String>,
) -> Option<(String, String)> {
    let function = value.get("function").unwrap_or(&Value::Null);
    let name = ["name", "tool", "tool_name", "action"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .or_else(|| function.get("name").and_then(Value::as_str))?
        .to_string();
    if !known.is_empty() && !known.contains(&name) {
        return None;
    }
    let raw = value
        .get("arguments")
        .or_else(|| value.get("args"))
        .or_else(|| value.get("parameters"))
        .or_else(|| value.get("input"))
        .or_else(|| function.get("arguments"));
    let arguments = match raw {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text)
            .unwrap_or_else(|_| json!({ "value": text })),
        Some(value) => value.clone(),
        None => {
            let mut map = serde_json::Map::new();
            for (key, value) in value.as_object()? {
                if !["name", "tool", "tool_name", "action", "function"].contains(&key.as_str()) {
                    map.insert(key.clone(), value.clone());
                }
            }
            Value::Object(map)
        }
    };
    arguments
        .is_object()
        .then(|| (name, arguments.to_string()))
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
