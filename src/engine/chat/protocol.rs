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

    // Some models emit a DSML-style block instead of native function calls.
    // Parse it when possible, and always remove the block so a failed call is
    // never shown to the owner as a successful answer.
    let (dsml_calls, dsml_cleaned) = parse_dsml_tool_calls(&cleaned, known);
    if !dsml_calls.is_empty() {
        return (dsml_calls, dsml_cleaned);
    }
    if dsml_cleaned != cleaned {
        cleaned = dsml_cleaned;
    }

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

/// A small model may supply exact patch arguments without a native tool
/// envelope. Accept a standalone JSON object or JSON code fence with
/// model-authored source path/range/newText. A missing SHA can only be bound
/// from an actual workspace_read_range receipt in the RSI native runtime;
/// the regular tool executor still enforces a current file-version hash.
pub fn parse_rsi_unwrapped_edit(
    content:&str, available:&std::collections::HashSet<String>,
)->Option<(String,String)> {
    parse_rsi_unwrapped_edit_observed(content,available,|_|None)
}

/// The model proposes source and edit text. Only the native runner may
/// supply a missing file SHA from a completed same-session range read.
pub fn parse_rsi_unwrapped_edit_observed(
    content:&str, available:&std::collections::HashSet<String>,
    observed:impl Fn(&str)->Option<String>,
)->Option<(String,String)> {
    let trimmed=content.trim();
    // One fenced patch remains an explicit tool intent even if the model
    // surrounds it with explanation. Multiple blocks are ambiguous.
    // A Rust code fence or narrative alone can never become a file write.
    let fences=["```json","```JSON"].into_iter()
        .flat_map(|tag|trimmed.match_indices(tag).map(move |(i,_)|(i,tag.len())))
        .collect::<Vec<_>>();
    let source=if fences.len()==1 {
        let (start,len)=fences[0];
        let inner=&trimmed[start+len..];
        inner.split_once("```")?.0.trim()
    } else if fences.is_empty() {trimmed}
    else{return None};
    if source.len()>32*1024 || !source.starts_with('{') || !source.ends_with('}') {
        return None;
    }
    let mut args:Value=serde_json::from_str(source).ok()?;
    // Some models copy sourceSha256 from a read response rather than using
    // expectedSha256. This is still a precise checked-write precondition.
    if args["expectedSha256"].is_null() {
        if let Some(source_hash)=args["sourceSha256"].as_str().map(str::to_string) {
            if source_hash.len()==64 && source_hash.bytes().all(|b|b.is_ascii_hexdigit()) {
                args["expectedSha256"]=json!(source_hash);
            }
        }
    }
    let path=args["path"].as_str().filter(|path|!path.is_empty())?;
    if path.starts_with('/') || path.contains("..") {return None;}
    if ["name","tool","tool_name","function","action"].iter()
        .any(|key|args.get(*key).is_some()) {return None;}
    // The real model sometimes supplies 32 hex digits instead of SHA-256.
    // A non-empty but malformed model hash must not prevent binding the
    // complete revision from a trusted prior read of this exact path.
    // A valid 64-character (possibly stale) hash is preserved for the normal
    // compare-before-write rejection; it is never silently overwritten.
    let hash_valid=args["expectedSha256"].as_str().is_some_and(|hash|
        hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()));
    if !hash_valid {
        if let Some(hash)=observed(path) {
            if hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()) {
                args["expectedSha256"]=json!(hash);
            }
        }
    }
    if available.contains("workspace_replace_function") {
        let sha=args["expectedSha256"].as_str().unwrap_or("");
        if sha.len()==64 && sha.bytes().all(|byte|byte.is_ascii_hexdigit())
            && args["functionName"].as_str().is_some_and(|name|
                !name.is_empty() && name.bytes().all(|b|
                    b.is_ascii_alphanumeric() || b==b'_'))
            && args["newText"].as_str().is_some_and(|text|
                text.len()<=16_000 && text.trim_end().ends_with('}')) {
            return Some(("workspace_replace_function".into(),args.to_string()));
        }
    }
    if available.contains("workspace_replace_range") {
        let sha=args["expectedSha256"].as_str().unwrap_or("");
        if sha.len()==64 && sha.bytes().all(|byte|byte.is_ascii_hexdigit())
            && args["startLine"].as_u64().is_some_and(|line|line>0)
            && args["endLine"].as_u64().is_some_and(|line|
                line>=args["startLine"].as_u64().unwrap_or(u64::MAX))
            && args["newText"].is_string() {
            return Some(("workspace_replace_range".into(),args.to_string()));
        }
    }
    if available.contains("workspace_replace")
        && args["oldText"].as_str().is_some_and(|old|!old.is_empty())
        && args["newText"].is_string() {
        return Some(("workspace_replace".into(),args.to_string()));
    }
    None
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

fn canonical_dsml_tool(name: &str, known: &std::collections::HashSet<String>) -> Option<String> {
    let lower = name.trim().to_lowercase();
    let canonical = match lower.as_str() {
        "bash" | "shell" | "sh" | "terminal" => "shell_exec",
        "read" | "read_file" | "cat" => "workspace_read",
        "write" | "write_file" => "workspace_write",
        "list" | "ls" | "list_files" => "workspace_list",
        "search" | "grep" => "shell_exec",
        _ => lower.as_str(),
    };
    if known.is_empty() || known.contains(canonical) {
        Some(canonical.to_string())
    } else {
        None
    }
}

fn dsml_block_bounds(content: &str) -> Option<(usize, usize)> {
    let invoke = content.find("invoke name=\"")?;
    let start = content[..invoke].rfind('<').unwrap_or(invoke);
    let end = content
        .rfind("calls>")
        .map(|position| position + "calls>".len())
        .or_else(|| {
            content
                .rfind("invoke>")
                .map(|position| position + "invoke>".len())
        })?;
    (end > start).then_some((start, end))
}

/// Parses the DSML-style call block some models emit instead of a native
/// function call. Unknown tools are stripped but not executed.
fn parse_dsml_tool_calls(
    content: &str,
    known: &std::collections::HashSet<String>,
) -> (Vec<(String, String)>, String) {
    let Some((start, end)) = dsml_block_bounds(content) else {
        return (Vec::new(), content.to_string());
    };
    let block = &content[start..end];
    let mut calls = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = block[cursor..].find("invoke name=\"") {
        let name_start = cursor + relative + "invoke name=\"".len();
        let Some(name_end_relative) = block[name_start..].find('"') else {
            break;
        };
        let name_end = name_start + name_end_relative;
        let name = &block[name_start..name_end];
        let after_name = name_end + 1;
        let invoke_end = block[after_name..]
            .find("invoke>")
            .map(|position| after_name + position + "invoke>".len())
            .unwrap_or(block.len());
        let invoke = &block[after_name..invoke_end];
        let mut arguments = serde_json::Map::new();
        let mut parameter_cursor = 0;
        while let Some(relative) = invoke[parameter_cursor..].find("parameter name=\"") {
            let parameter_start =
                parameter_cursor + relative + "parameter name=\"".len();
            let Some(parameter_end_relative) = invoke[parameter_start..].find('"') else {
                break;
            };
            let parameter_end = parameter_start + parameter_end_relative;
            let key = &invoke[parameter_start..parameter_end];
            let Some(gt_relative) = invoke[parameter_end..].find('>') else {
                break;
            };
            let value_start = parameter_end + gt_relative + 1;
            let value_end = invoke[value_start..]
                .find("parameter>")
                .map(|position| value_start + position)
                .unwrap_or(invoke.len());
            arguments.insert(
                key.to_string(),
                json!(invoke[value_start..value_end].trim()),
            );
            parameter_cursor = (value_end + "parameter>".len()).min(invoke.len());
        }
        if let Some(canonical) = canonical_dsml_tool(name, known) {
            calls.push((canonical, Value::Object(arguments).to_string()));
        }
        cursor = invoke_end;
    }
    let mut cleaned = content.to_string();
    cleaned.replace_range(start..end, "");
    (calls, cleaned.trim().to_string())
}

#[cfg(test)]
mod sub1_structured_patch_tests {
    use super::*;
    #[test]
    fn observed_single_rust_function_is_a_grounded_native_intent() {
        let available=std::collections::HashSet::from([
            "workspace_replace_function".to_string()]);
        let text=r#"{"path":"src/engine/decompose.rs","functionName":"split_markers","newText":"fn split_markers() -> Vec<String> { vec![] }"}"#;
        let observed="a".repeat(64);
        let (tool,raw)=parse_rsi_unwrapped_edit_observed(
            text,&available,|path| (path=="src/engine/decompose.rs")
                .then(||observed.clone())).unwrap();
        assert_eq!(tool,"workspace_replace_function");
        let arguments:Value=serde_json::from_str(&raw).unwrap();
        assert_eq!(arguments["expectedSha256"],observed);
        assert!(parse_rsi_unwrapped_edit_observed(text,&available,|_|None).is_none());
        assert!(parse_rsi_unwrapped_edit_observed(
            &text.replace("split_markers","../../untrusted"),&available,
            |_|Some(observed.clone())).is_none());
    }
    #[test]
    fn truncated_model_hash_is_replaced_only_by_real_source_observation() {
        let available=std::collections::HashSet::from([
            "workspace_replace_function".to_string()]);
        let draft=r#"{"path":"src/engine/decompose.rs","functionName":"split_markers","expectedSha256":"b4a48b5d76e4d13a585778d1f0b66a71","newText":"fn split_markers() -> Vec<String> { vec![] }"}"#;
        let full="e".repeat(64);
        let (tool,raw)=parse_rsi_unwrapped_edit_observed(draft,&available,
            |path| (path=="src/engine/decompose.rs").then(||full.clone())).unwrap();
        assert_eq!(tool,"workspace_replace_function");
        let args:Value=serde_json::from_str(&raw).unwrap();
        assert_eq!(args["expectedSha256"],full);
        assert!(parse_rsi_unwrapped_edit_observed(draft,&available,|_|None).is_none());
        let stale=draft.replace("b4a48b5d76e4d13a585778d1f0b66a71",&"c".repeat(64));
        let (_,raw)=parse_rsi_unwrapped_edit_observed(&stale,&available,
            |_|Some(full.clone())).unwrap();
        let stale_args:Value=serde_json::from_str(&raw).unwrap();
        assert_eq!(stale_args["expectedSha256"],"c".repeat(64),
            "A plausible but stale SHA must fail on the native write path");
    }

    #[test]
    fn prose_wrapped_single_source_bound_patch_is_a_real_tool_intent() {
        let mut available=std::collections::HashSet::new();
        available.insert("workspace_replace_range".to_string());
        let sha="a".repeat(64);
        let content=format!(
            "파일의 2번째 줄을 다음 코드로 교정합니다.\n\n```json\n{{\"path\":\"src/engine/target.rs\",\"startLine\":2,\"endLine\":2,\"newText\":\"let x = 2;\",\"sourceSha256\":\"{sha}\"}}\n```\n"
        );
        let (tool,args)=parse_rsi_unwrapped_edit(&content,&available).unwrap();
        assert_eq!(tool,"workspace_replace_range");
        let value:Value=serde_json::from_str(&args).unwrap();
        assert_eq!(value["expectedSha256"],sha);
        assert_eq!(value["path"],"src/engine/target.rs");
        assert!(parse_rsi_unwrapped_edit(&format!("{content}\n{content}"),&available).is_none());
        assert!(parse_rsi_unwrapped_edit(
            &content.replace("src/engine/target.rs","../escape.rs"),&available).is_none());
        assert!(parse_rsi_unwrapped_edit(
            "```rust\nfn example() {}\n```",&available).is_none());
    }
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
