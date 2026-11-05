//! Typed Program IR (design 2.4, section 11-3).
//! A single runtime interprets the nodes and persists its observations,
//! completed operation IDs, branch inputs and per-loop iteration positions.
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
    let mut identifiers = std::collections::HashSet::new();
    for node in nodes {
        validate_node(node)?;
        validate_identifiers(node, &mut identifiers)?;
    }
    Ok(json!({"revision": revision, "nodes": nodes}))
}

/// Prevents different tool nodes (including nested branches and loop bodies)
/// from sharing a completed-operation identity after a restart.
fn validate_identifiers(
    node: &Value,
    identifiers: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    let kind = node["type"].as_str().unwrap_or("");
    if let Some(id) = node["id"].as_str().filter(|id| !id.is_empty()) {
        if !identifiers.insert(id.to_string()) {
            return Err(format!("duplicate program node id: {id}"));
        }
    } else if matches!(kind, "call_tool" | "branch" | "loop" | "observe" | "delegate") {
        return Err(format!("{kind}.id is required for durable execution"));
    }
    for field in ["nodes", "then", "else", "body"] {
        for child in node[field].as_array().into_iter().flatten() {
            validate_identifiers(child, identifiers)?;
        }
    }
    Ok(())
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
            for child in node["body"].as_array().into_iter().flatten() {
                validate_node(child)?;
            }
            Ok(())
        }
        "sequence" | "parallel" => {
            if node["nodes"].as_array().is_none() {
                return Err(format!("{}.nodes must be an array", node["type"]));
            }
            for child in node["nodes"].as_array().into_iter().flatten() {
                validate_node(child)?;
            }
            Ok(())
        }
        "verify" => {
            if node["checks"].as_array().is_none() {
                return Err("verify.checks must be an array".into());
            }
            Ok(())
        }
        "model_decision" => Ok(()),
        "delegate" => {
            if node["agentId"].as_str().is_none_or(str::is_empty)
                || node["prompt"].as_str().is_none_or(str::is_empty)
            {
                return Err("delegate.agentId/prompt are required".into());
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
        "loopProgress": {},
        "observationRefs": {},
        "adaptationRevision": 0,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PredicateVerdict {
    True,
    False,
    Unknown,
}

impl PredicateVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_true(self) -> bool {
        matches!(self, Self::True)
    }
}

fn predicate_observation(predicate: &Value, resolved: &Value) -> Result<Option<Value>, String> {
    let source = predicate["from"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("predicate.from is required")?;
    let Some(base) = resolved.get(source) else {
        return Ok(None);
    };
    if base.is_null() {
        return Ok(None);
    }
    match predicate["path"].as_str() {
        Some(path) => Ok(base.pointer(path).cloned()),
        None => Ok(Some(base.clone())),
    }
}

/// Three-valued predicate evaluation. Missing observation fields are Unknown,
/// not False, so they can never refute a hypothesis.
pub fn predicate_verdict(
    predicate: &Value,
    resolved: &Value,
) -> Result<PredicateVerdict, String> {
    match predicate["type"].as_str().unwrap_or("") {
        "exists" => {
            let source = predicate["from"].as_str().unwrap_or("");
            let Some(base) = resolved.get(source) else {
                // `exists` is a scope-complete check over the resolved map.
                return Ok(PredicateVerdict::False);
            };
            if base.is_null() {
                return Ok(PredicateVerdict::False);
            }
            match predicate["path"].as_str() {
                Some(path) => Ok(if base.pointer(path).is_some() {
                    PredicateVerdict::True
                } else {
                    PredicateVerdict::False
                }),
                None => Ok(PredicateVerdict::True),
            }
        }
        "equals" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            Ok(if actual == predicate["value"] {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "compare" => {
            let Some(actual) = predicate_observation(predicate, resolved)? else {
                return Ok(PredicateVerdict::Unknown);
            };
            let (Some(left), Some(right)) =
                (actual.as_f64(), predicate["value"].as_f64())
            else {
                return Ok(PredicateVerdict::Unknown);
            };
            let verdict = match predicate["op"].as_str().unwrap_or("") {
                "gt" => left > right,
                "gte" => left >= right,
                "lt" => left < right,
                "lte" => left <= right,
                other => return Err(format!("unsupported compare op: {other}")),
            };
            Ok(if verdict {
                PredicateVerdict::True
            } else {
                PredicateVerdict::False
            })
        }
        "not" => Ok(match predicate_verdict(&predicate["predicate"], resolved)? {
            PredicateVerdict::True => PredicateVerdict::False,
            PredicateVerdict::False => PredicateVerdict::True,
            PredicateVerdict::Unknown => PredicateVerdict::Unknown,
        }),
        "all" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::False => return Ok(PredicateVerdict::False),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::True => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::True
            })
        }
        "any" => {
            let mut unknown = false;
            for item in predicate["predicates"].as_array().into_iter().flatten() {
                match predicate_verdict(item, resolved)? {
                    PredicateVerdict::True => return Ok(PredicateVerdict::True),
                    PredicateVerdict::Unknown => unknown = true,
                    PredicateVerdict::False => {}
                }
            }
            Ok(if unknown {
                PredicateVerdict::Unknown
            } else {
                PredicateVerdict::False
            })
        }
        other => Err(format!("unsupported predicate type: {other}")),
    }
}

/// Boolean convenience for callers that can treat Unknown as non-satisfied.
/// Branch/Verify must use `predicate_verdict` so Unknown is not silently False.
pub fn predicate_holds(predicate: &Value, resolved: &Value) -> Result<bool, String> {
    Ok(predicate_verdict(predicate, resolved)?.is_true())
}

/// Extracts the first brace-balanced JSON object from model prose.
pub fn parse_json_object(raw: &str) -> Option<Value> {
    let bytes = raw.as_bytes();
    for (start, _) in raw.match_indices('{') {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for index in start..bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escape {
                    escape = false;
                } else if byte == b'\\' {
                    escape = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(value) = serde_json::from_str::<Value>(&raw[start..=index]) {
                            if value.is_object() {
                                return Some(value);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Validates and applies a model-proposed Program revision. Completed external
/// actions are preserved; only pending nodes may be replaced plus new nodes.
pub fn apply_revision_proposal(
    program: &Value,
    checkpoint: &Value,
    proposal: &Value,
    available_tools: &[String],
    max_nodes: usize,
    repair_mode: &str,
) -> Result<Value, String> {
    let base = proposal["baseRevision"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("proposal.baseRevision is required")?;
    if Some(base) != program["revision"].as_str() {
        return Err(format!(
            "proposal baseRevision {base} does not match current revision {}",
            program["revision"]
        ));
    }
    if proposal["reason"].as_str().is_none_or(str::is_empty) {
        return Err("proposal.reason is required".into());
    }
    let completed = checkpoint["completedOperationIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();
    let mut replacements = proposal["replacePendingNodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if repair_mode == "append" {
        // Append-only repair never rewrites pending nodes; it only adds work.
        replacements.clear();
    }
    let additions = proposal["addNodes"].as_array().cloned().unwrap_or_default();
    if replacements.is_empty() && additions.is_empty() {
        return Err("proposal changes no nodes".into());
    }
    let mut replaced_ids = std::collections::BTreeSet::new();
    for node in &replacements {
        let id = node["id"].as_str().filter(|value| !value.is_empty())
            .ok_or("replacement node needs an id")?;
        if completed.contains(id) {
            return Err(format!(
                "cannot replace completed node {id}; completed effects are preserved"
            ));
        }
        validate_node(node)?;
        if !replaced_ids.insert(id.to_string()) {
            return Err(format!("duplicate replacement node id {id}"));
        }
    }
    let mut nodes = Vec::new();
    for node in program["nodes"].as_array().into_iter().flatten() {
        let id = node["id"].as_str().unwrap_or("");
        if let Some(replacement) = replacements.iter().find(|item| item["id"].as_str() == Some(id)) {
            nodes.push(replacement.clone());
        } else {
            nodes.push(node.clone());
        }
    }
    for node in &additions {
        validate_node(node)?;
        nodes.push(node.clone());
    }

    let mut ids = std::collections::BTreeSet::new();
    let mut known = checkpoint["resolvedInputs"]
        .as_object()
        .map(|map| map.keys().cloned().collect::<std::collections::BTreeSet<_>>())
        .unwrap_or_default();
    for node in &nodes {
        let id = node["id"].as_str().filter(|value| !value.is_empty())
            .ok_or("every program node needs an id")?;
        if !ids.insert(id.to_string()) {
            return Err(format!("duplicate program node id {id}"));
        }
        if node["type"].as_str() == Some("call_tool") {
            let tool = node["tool"].as_str().unwrap_or("");
            if !available_tools.iter().any(|item| item == tool) {
                return Err(format!("tool {tool} is not available in this run"));
            }
        }
        // Every `{from}` reference must point to a prior node or a resolved observation.
        if let Some(inputs) = node["inputs"].as_object() {
            for (key, input) in inputs {
                if let Some(source) = input.get("from").and_then(Value::as_str) {
                    if !known.contains(source) && !ids.contains(source) {
                        return Err(format!(
                            "input {key} references unknown observation/node {source}"
                        ));
                    }
                }
            }
        }
        known.insert(id.to_string());
    }
    if nodes.len() > max_nodes {
        return Err(format!(
            "revised program has {} nodes, budget is {max_nodes}",
            nodes.len()
        ));
    }
    validate(&json!({
        "revision": format!("{}:rev", base),
        "nodes": nodes,
    }))?;
    Ok(json!({
        "revision": format!("{}:rev:{}", base, crate::engine::memory::stable_id(&[&proposal.to_string()])),
        "nodes": nodes,
        "appliedProposal": proposal,
        "parentRevision": base,
    }))
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
    fn durable_program_rejects_duplicate_and_missing_effect_ids() {
        let mut duplicate = program();
        duplicate["nodes"][1]["else"][0]["id"] = json!("try");
        assert!(validate(&duplicate).is_err());
        let mut missing = program();
        missing["nodes"][0].as_object_mut().unwrap().remove("id");
        assert!(validate(&missing).is_err());
        let mut loop_missing = program();
        loop_missing["nodes"].as_array_mut().unwrap().push(json!({
            "type":"loop","maxIterations":2,
            "predicate":{"type":"exists","from":"loop_condition"},
            "body":[{"type":"call_tool","id":"write","tool":"workspace_write","inputs":{"path":"out","content":"x"}}]
        }));
        assert!(validate(&loop_missing).is_err());
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
