//! Minimal typed Program IR (design 2.4, section 11-3).
//!
//! Nodes: Observe, CallTool, Branch, Loop. The runtime executes CallTool and
//! Branch today; Loop is typed and validated but returns an explicit
//! unsupported error instead of pretending to run. Checkpoints store the
//! program revision, frame stack, resolved inputs and completed/pending
//! operation ids so a future scheduler can resume from an exact position.
use serde_json::{json, Value};

pub const SUPPORTED_REVISION: u32 = 1;

pub fn validate(program: &Value) -> Result<Value, String> {
    let revision = program["revision"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("program.revision is required")?
        .to_string();
    let nodes = program["nodes"]
        .as_array()
        .ok_or("program.nodes must be an array")?;
    if nodes.is_empty() || nodes.len() > 64 {
        return Err("program.nodes must contain 1..64 nodes".into());
    }
    for node in nodes {
        validate_node(node)?;
    }
    Ok(json!({"revision": revision, "nodes": nodes}))
}

fn validate_node(node: &Value) -> Result<(), String> {
    match node["type"].as_str().unwrap_or("") {
        "observe" => {
            if node["id"].as_str().is_none_or(str::is_empty) {
                return Err("observe.id is required".into());
            }
            Ok(())
        }
        "call_tool" => {
            if node["tool"].as_str().is_none_or(str::is_empty) {
                return Err("call_tool.tool is required".into());
            }
            if !node["inputs"].is_object() {
                return Err("call_tool.inputs must be an object".into());
            }
            Ok(())
        }
        "branch" => {
            if node["then"].as_array().is_none() || node["else"].as_array().is_none() {
                return Err("branch.then/else must be arrays".into());
            }
            for child in node["then"].as_array().into_iter().flatten()
                .chain(node["else"].as_array().into_iter().flatten())
            {
                validate_node(child)?;
            }
            Ok(())
        }
        "loop" => {
            if node["body"].as_array().is_none() {
                return Err("loop.body must be an array".into());
            }
            Ok(())
        }
        other => Err(format!("unsupported program node type: {other}")),
    }
}

pub fn new_checkpoint(program: &Value) -> Value {
    json!({
        "schemaVersion": SUPPORTED_REVISION,
        "programRevision": program["revision"],
        "status": "running",
        "frameStack": [],
        "nextIndex": 0,
        "resolvedInputs": {},
        "completedOperationIds": [],
        "pendingOperationIds": [],
    })
}

/// Resolves a literal or `{from, path}` input against checkpoint inputs.
pub fn resolve_inputs(inputs: &Value, resolved: &Value) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    for (key, value) in inputs.as_object().cloned().unwrap_or_default() {
        if let Some(source) = value.get("from").and_then(|value| value.as_str()) {
            let base = resolved
                .get(source)
                .ok_or_else(|| format!("input {key} references unknown observation {source}"))?;
            let selected = match value.get("path").and_then(|value| value.as_str()) {
                Some(path) => base.pointer(path).cloned().unwrap_or(Value::Null),
                None => base.clone(),
            };
            out.insert(key, selected);
        } else {
            out.insert(key, value);
        }
    }
    Ok(Value::Object(out))
}

pub fn predicate_holds(predicate: &Value, resolved: &Value) -> Result<bool, String> {
    match predicate["type"].as_str().unwrap_or("") {
        "exists" => {
            let source = predicate["from"].as_str().unwrap_or("");
            Ok(resolved.get(source).is_some_and(|value| !value.is_null()))
        }
        "equals" => {
            let source = predicate["from"].as_str().unwrap_or("");
            let base = resolved.get(source).unwrap_or(&Value::Null);
            let actual = match predicate["path"].as_str() {
                Some(path) => base.pointer(path).cloned().unwrap_or(Value::Null),
                None => base.clone(),
            };
            Ok(actual == predicate["value"])
        }
        "compare" => {
            let source = predicate["from"].as_str().unwrap_or("");
            let base = resolved.get(source).unwrap_or(&Value::Null);
            let actual = match predicate["path"].as_str() {
                Some(path) => base.pointer(path).cloned().unwrap_or(Value::Null),
                None => base.clone(),
            };
            let left = actual.as_f64().unwrap_or(f64::NAN);
            let right = predicate["value"].as_f64().unwrap_or(f64::NAN);
            Ok(match predicate["op"].as_str().unwrap_or("") {
                "gt" => left > right,
                "gte" => left >= right,
                "lt" => left < right,
                "lte" => left <= right,
                _ => false,
            })
        }
        "not" => Ok(!predicate_holds(&predicate["predicate"], resolved)?),
        "all" => {
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                if !predicate_holds(item, resolved)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "any" => {
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                if predicate_holds(item, resolved)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        other => Err(format!("unsupported predicate type: {other}")),
    }
}

pub fn checkpoint_skip_completed(checkpoint: &Value, node_id: &str) -> bool {
    checkpoint["completedOperationIds"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|value| value.as_str() == Some(node_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Value {
        json!({
            "revision":"p1",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
                {"type":"branch","id":"check",
                 "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
                 "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
                 "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
            ]
        })
    }

    #[test]
    fn validation_accepts_the_minimal_program_and_rejects_unknown_nodes() {
        assert!(validate(&program()).is_ok());
        assert!(validate(&json!({"revision":"p","nodes":[{"type":"nope"}]})).is_err());
        assert!(validate(&json!({"nodes":[]})).is_err());
    }

    #[test]
    fn failure_predicate_selects_the_fallback_branch() {
        let resolved = json!({"try": {"exitCode": 7, "isError": true}});
        let check = &program()["nodes"][1]["predicate"];
        assert!(!predicate_holds(check, &resolved).unwrap());
        let success = json!({"try": {"exitCode": 0}});
        assert!(predicate_holds(check, &success).unwrap());
    }

    #[test]
    fn inputs_can_reference_resolved_observations() {
        let resolved = json!({"try": {"result": {"path": "out.txt"}}});
        let inputs = json!({
            "path": {"from":"try","path":"/result/path"},
            "content":"x"
        });
        assert_eq!(
            resolve_inputs(&inputs, &resolved).unwrap(),
            json!({"path":"out.txt","content":"x"})
        );
        assert!(resolve_inputs(&json!({"p":{"from":"missing"}}), &resolved).is_err());
    }

    #[test]
    fn checkpoint_records_progress_and_skips_completed_operations_on_resume() {
        let mut checkpoint = new_checkpoint(&program());
        checkpoint["completedOperationIds"] = json!(["try"]);
        checkpoint["pendingOperationIds"] = json!(["fallback"]);
        checkpoint["nextIndex"] = json!(1);
        assert!(checkpoint_skip_completed(&checkpoint, "try"));
        assert!(!checkpoint_skip_completed(&checkpoint, "fallback"));
        // The checkpoint survives a JSON round-trip (durable resume contract).
        let round_trip: Value =
            serde_json::from_str(&serde_json::to_string(&checkpoint).unwrap()).unwrap();
        assert_eq!(round_trip["programRevision"], "p1");
        assert_eq!(round_trip["nextIndex"], 1);
        assert_eq!(round_trip["pendingOperationIds"], json!(["fallback"]));
    }
}
