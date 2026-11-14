//! Qwen-Agent's legacy FUNCTION/ARGS wire format, independent of Python runtimes.
//! Strict JSON objects and the runtime's existing tool allowlist remain authoritative.
use serde_json::Value;
use std::collections::HashSet;

const FUNCTION: &str = "✿FUNCTION✿:";
const ARGS: &str = "✿ARGS✿:";
const RESULT: &str = "✿RESULT✿:";
const RETURN: &str = "✿RETURN✿:";
const MAX_LEGACY_BYTES: usize = 65_536;
const MAX_LEGACY_CALLS: usize = 16;

fn record(
    pending: &mut Option<(String, Option<String>)>,
    known: &HashSet<String>,
    calls: &mut Vec<(String, String)>,
) {
    let Some((name, Some(raw))) = pending.take() else {
        return;
    };
    if calls.len() >= MAX_LEGACY_CALLS
        || name.is_empty()
        || (!known.is_empty() && !known.contains(&name))
    {
        return;
    }
    // Qwen-Agent accepts a permissive JSON5 dialect. DOMINION intentionally
    // requires ordinary JSON so malformed or ambiguous arguments never run.
    if let Ok(Value::Object(arguments)) = serde_json::from_str::<Value>(raw.trim()) {
        calls.push((name, Value::Object(arguments).to_string()));
    }
}

/// Decode the original Qwen-Agent FUNCTION/ARGS format to the same internal
/// (tool name, JSON arguments) shape used by native and Hermes tool calls.
/// Returns None when the format is not present; even rejected calls are removed
/// rather than being accidentally interpreted as an answer or bare JSON.
pub(super) fn parse_legacy(
    content: &str,
    known: &HashSet<String>,
) -> Option<(Vec<(String, String)>, String)> {
    if content.len() > MAX_LEGACY_BYTES || !content.contains(FUNCTION) {
        return None;
    }
    // Reasoning blocks are not executable actions. An unfinished reasoning
    // block must not grant its embedded example tool calls authority.
    let start = if content.contains("<think>") {
        content.rfind("</think>")? + "</think>".len()
    } else {
        0
    };
    let (reasoning, executable) = content.split_at(start);
    if !executable.contains(FUNCTION) {
        return None;
    }

    let mut visible = reasoning.to_string();
    let mut pending: Option<(String, Option<String>)> = None;
    let mut calls = Vec::new();
    for line in executable.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(name) = trimmed.strip_prefix(FUNCTION) {
            record(&mut pending, known, &mut calls);
            pending = Some((name.trim().to_string(), None));
        } else if let Some(args) = trimmed.strip_prefix(ARGS) {
            match &mut pending {
                Some((_, slot)) if slot.is_none() => *slot = Some(args.to_string()),
                Some((name, slot)) => {
                    // A repeated ARGS marker means another call to the same
                    // function. Keep the first before starting the next.
                    let next_name = name.clone();
                    record(&mut pending, known, &mut calls);
                    pending = Some((next_name, Some(args.to_string())));
                    let _ = slot;
                }
                None => visible.push_str(line),
            }
        } else if trimmed.starts_with(RESULT) {
            // A model may echo a previous tool result. It is never a new call.
            record(&mut pending, known, &mut calls);
        } else if let Some(answer) = trimmed.strip_prefix(RETURN) {
            record(&mut pending, known, &mut calls);
            visible.push_str(answer.trim_start());
        } else if let Some((_, Some(args))) = pending.as_mut() {
            args.push_str(line);
        } else if pending.is_none() {
            visible.push_str(line);
        }
    }
    record(&mut pending, known, &mut calls);
    Some((calls, visible.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> HashSet<String> {
        HashSet::from(["workspace_read".into(), "workspace_write".into()])
    }

    #[test]
    fn legacy_qwen_single_call_preserves_assistant_preamble() {
        let source = "읽어 보겠습니다.\n✿FUNCTION✿: workspace_read\n✿ARGS✿: {\"path\":\"안내.txt\"}\n";
        let (calls, remaining) = parse_legacy(source, &known()).unwrap();
        assert_eq!(calls, vec![("workspace_read".into(), r#"{"path":"안내.txt"}"#.into())]);
        assert_eq!(remaining, "읽어 보겠습니다.");
    }

    #[test]
    fn legacy_qwen_parallel_calls_keep_order() {
        let source = "✿FUNCTION✿: workspace_read\n✿ARGS✿: {\"path\":\"a\"}\n✿FUNCTION✿: workspace_write\n✿ARGS✿: {\"path\":\"b\",\"content\":\"x\"}";
        let (calls, _) = parse_legacy(source, &known()).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "workspace_read");
        assert_eq!(calls[1].0, "workspace_write");
    }

    #[test]
    fn legacy_qwen_rejects_unlisted_tools_and_invalid_json() {
        let source = "✿FUNCTION✿: shell_exec\n✿ARGS✿: {\"command\":\"echo x\"}\n✿FUNCTION✿: workspace_read\n✿ARGS✿: {path: 'oops'}";
        let (calls, cleaned) = parse_legacy(source, &known()).unwrap();
        assert!(calls.is_empty());
        assert!(cleaned.is_empty());
    }

    #[test]
    fn legacy_qwen_ignores_tool_examples_in_thinking() {
        let source = "<think>\n✿FUNCTION✿: workspace_write\n✿ARGS✿: {\"path\":\"not-run\"}\n</think>\n✿FUNCTION✿: workspace_read\n✿ARGS✿: {\"path\":\"real\"}";
        let (calls, _) = parse_legacy(source, &known()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "workspace_read");
        assert_eq!(serde_json::from_str::<Value>(&calls[0].1).unwrap()["path"], "real");
        assert!(parse_legacy("<think>✿FUNCTION✿: workspace_read\n✿ARGS✿: {}", &known()).is_none());
    }

    #[test]
    fn legacy_qwen_preserves_plain_prose_and_return_text() {
        let source = "✿FUNCTION✿: workspace_read\n✿ARGS✿: {\"path\":\"a\"}\n✿RETURN✿: 읽기를 마쳤습니다.";
        let (calls, remaining) = parse_legacy(source, &known()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(remaining, "읽기를 마쳤습니다.");
    }
}
