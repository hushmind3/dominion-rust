//! Compact presentation of the existing tool registry. No tool search model,
//! extra round trip or separate execution path is introduced.
use super::ToolCall;
use serde_json::{json, Value};

const DISPATCH: &str = "dominion_tool";

fn name(tool: &Value) -> &str {
    tool["function"]["name"].as_str().unwrap_or("")
}

fn signature(schema: &Value) -> String {
    if let Some(values) = schema["enum"].as_array() {
        return values
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("|");
    }
    match schema["type"].as_str().unwrap_or("any") {
        "object" => {
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            let fields = schema["properties"]
                .as_object()
                .map(|fields| {
                    fields
                        .iter()
                        .map(|(key, value)| {
                            format!(
                                "{key}{}:{}",
                                if required.contains(&json!(key)) {
                                    ""
                                } else {
                                    "?"
                                },
                                signature(value)
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            format!("{{{}}}", fields.join(","))
        }
        "array" => format!("[{}]", signature(&schema["items"])),
        kind => kind.to_string(),
    }
}

/// Keep every tool reachable, including MCP tools. Names, descriptions and
/// parameter types come from the runtime registry, never an intent keyword list.
pub(super) fn compact(tools: &[Value]) -> Vec<Value> {
    if tools.is_empty() {
        return Vec::new();
    }
    let registry = crate::engine::tools::definitions();
    let (builtin, mut external): (Vec<_>, Vec<_>) = tools.iter().cloned().partition(|tool| {
        registry
            .iter()
            .any(|registered| name(registered) == name(tool))
    });
    if builtin.is_empty() {
        return external;
    }
    let catalog = builtin
        .iter()
        .map(|tool| {
            format!(
                "{} {} — {}",
                name(tool),
                signature(&tool["function"]["parameters"]),
                tool["function"]["description"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    external.insert(0,json!({"type":"function","function":{
        "name":DISPATCH,
        "description":format!("Execute an available tool using its exact name and arguments. ? means optional. Existing permissions and approvals apply.\n{catalog}"),
        "parameters":{"type":"object","properties":{
            "name":{"type":"string","enum":builtin.iter().map(name).collect::<Vec<_>>()},
            "arguments":{"type":"string","description":"A JSON-encoded object with the selected tool arguments"}
        },"required":["name","arguments"],"additionalProperties":false}
    }}));
    external
}

/// Explicit resource references are engine-observable context. Keep native
/// schemas for these tasks without reading files or calling a discovery model.
pub(super) fn for_request(tools: &[Value], query: &str, workspace: &std::path::Path) -> Vec<Value> {
    let readers = query
        .split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(|ch: char| {
                matches!(ch, '`' | '\'' | '"' | '(' | ')' | '[' | ']' | ',')
            });
            // Korean particles can be attached to a file name. Only filesystem
            // metadata is inspected; source contents remain behind normal tools.
            word.char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(word.len()))
                .rev()
                .filter(|end| *end > 0)
                .find_map(|end| {
                    let path = std::path::Path::new(&word[..end]);
                    workspace
                        .join(path)
                        .is_file()
                        .then_some(if path.is_absolute() {
                            "file_read"
                        } else {
                            "workspace_read"
                        })
                })
        })
        .collect::<std::collections::HashSet<_>>();
    // A concrete resource task keeps the original native schemas. Compact
    // presentation is for open conversation, not a new mandatory tool protocol.
    if !readers.is_empty() || tools.iter().any(|tool| query.contains(name(tool))) {
        return tools.to_vec();
    }
    compact(tools)
}

pub(super) fn uses_dispatch(tools: &[Value]) -> bool {
    tools.iter().any(|tool| name(tool) == DISPATCH)
}

/// Stored history uses the actual tool name. Only the provider's copy uses
/// the compact protocol; IDs and corresponding tool results stay unchanged.
pub(super) fn encode_history(messages: &[Value], tools: &[Value]) -> Vec<Value> {
    let names = crate::engine::tools::definitions()
        .iter()
        .map(|tool| name(tool).to_string())
        .collect::<Vec<_>>();
    let mut messages = messages.to_vec();
    for message in &mut messages {
        for call in message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if tools.iter().any(|tool| {
                call["function"]["name"] == name(tool)
                    && names
                        .iter()
                        .any(|name| name == super::tool_catalog::name(tool))
            }) {
                if let Ok(arguments) = serde_json::from_str::<Value>(
                    call["function"]["arguments"].as_str().unwrap_or(""),
                ) {
                    let wrapped =
                        json!({"name":call["function"]["name"],"arguments":arguments.to_string()});
                    call["function"]["name"] = json!(DISPATCH);
                    call["function"]["arguments"] = json!(wrapped.to_string());
                }
            }
        }
    }
    messages
}

/// Decode before the shared permission, procedure, approval and execution
/// pipeline. A malformed/unknown dispatch stays invalid and cannot widen access.
pub(super) fn decode(call: &mut ToolCall, tools: &[Value]) {
    if call.name != DISPATCH {
        return;
    }
    let Ok(body) = serde_json::from_str::<Value>(&call.arguments) else {
        return;
    };
    let Some(selected) = body["name"]
        .as_str()
        .filter(|selected| tools.iter().any(|tool| name(tool) == *selected))
    else {
        return;
    };
    let arguments = match &body["arguments"] {
        Value::String(text) => serde_json::from_str::<Value>(text).ok(),
        Value::Object(_) => Some(body["arguments"].clone()),
        _ => None,
    };
    let Some(arguments) = arguments.filter(Value::is_object) else {
        return;
    };
    call.name = selected.to_string();
    call.arguments = arguments.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cited_files_preload_the_original_reader_without_reading_contents() {
        let root =
            std::env::temp_dir().join(format!("dominion-tool-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("input.json"),
            "PRIVATE_FILE_CONTENT_NOT_IN_SCHEMA",
        )
        .unwrap();
        let tools = crate::engine::tools::definitions();
        let presented = for_request(&tools, "input.json를 확인해줘", &root);
        let reader = presented
            .iter()
            .find(|tool| name(tool) == "workspace_read")
            .unwrap();
        assert_eq!(
            reader,
            tools
                .iter()
                .find(|tool| name(tool) == "workspace_read")
                .unwrap()
        );
        assert!(!json!(presented)
            .to_string()
            .contains("PRIVATE_FILE_CONTENT_NOT_IN_SCHEMA"));
        let absolute = for_request(&tools, root.join("input.json").to_str().unwrap(), &root);
        assert!(absolute.iter().any(|tool| name(tool) == "file_read"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compact_dispatch_keeps_every_tool_and_round_trips_native_history() {
        let tools = crate::engine::tools::definitions();
        let compact = compact(&tools);
        assert!(json!(compact).to_string().len() < json!(tools).to_string().len());
        for tool in &tools {
            assert!(compact[0]["function"]["description"]
                .as_str()
                .unwrap()
                .contains(name(tool)));
        }
        let mut call = ToolCall {
            id: "one".into(),
            name: DISPATCH.into(),
            arguments:
                json!({"name":"file_write","arguments":json!({"path":"/tmp/test","content":"value"}).to_string()})
                    .to_string(),
        };
        decode(&mut call, &tools);
        assert_eq!(call.name, "file_write");
        let history = vec![
            json!({"role":"assistant","tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}),
        ];
        let wire = encode_history(&history, &tools);
        assert_eq!(wire[0]["tool_calls"][0]["id"], "one");
        assert_eq!(wire[0]["tool_calls"][0]["function"]["name"], DISPATCH);
        assert_eq!(
            history[0]["tool_calls"][0]["function"]["name"],
            "file_write"
        );
        let mut invalid = ToolCall {
            id: "two".into(),
            name: DISPATCH.into(),
            arguments: json!({"name":"not_registered","arguments":{}}).to_string(),
        };
        decode(&mut invalid, &tools);
        assert_eq!(invalid.name, DISPATCH);
    }
}
