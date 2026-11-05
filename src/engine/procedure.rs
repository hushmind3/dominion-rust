//! Generic, task-agnostic procedure compliance.
//!
//! A strategy can carry a machine-checkable `verification` object:
//!
//! ```json
//! {
//!   "type": "procedure",
//!   "appliesTo": { "requestContains": ["report.txt"], "paths": ["report.txt"] },
//!   "before": [
//!     { "when":    { "tool": "workspace_write", "path": "report.txt" },
//!       "require": { "tool": "workspace_write", "path": "audit.txt" } }
//!   ],
//!   "after": [
//!     { "kind": "file_contains", "path": "audit.txt", "text": "report.txt" },
//!     { "kind": "file_equals", "path": "report.txt", "text": "ok" }
//!   ]
//! }
//! ```
//!
//! The engine never hard-codes benchmark filenames. It evaluates whatever
//! paths and tools the learned procedure contains.
use serde_json::{json, Value};
use std::path::{Component, Path};

pub fn unverified() -> Value {
    json!({ "type": "unverified" })
}

fn safe_relative(path: &str) -> bool {
    let candidate = Path::new(path);
    !path.contains('<')
        && !path.contains('>')
        && !path.contains('*')
        && !candidate.is_absolute()
        && !candidate.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir)
        })
}

fn valid_step(value: &Value) -> bool {
    value["tool"].as_str().is_some_and(|tool| !tool.is_empty())
        && value["path"]
            .as_str()
            .is_none_or(|path| !path.is_empty() && safe_relative(path))
}

fn valid_after(value: &Value) -> bool {
    let path_ok = value["path"]
        .as_str()
        .is_some_and(|path| !path.is_empty() && safe_relative(path));
    match value["kind"].as_str() {
        Some("tool_called") => valid_step(value),
        Some("file_exists" | "file_equals" | "file_contains") => path_ok,
        _ => false,
    }
}

fn tool_catalog() -> std::collections::HashSet<String> {
    crate::engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            definition["function"]["name"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

fn validate_bindings(value: &Value, bindings: &Value, nodes: &[Value]) -> Vec<String> {
    let mut errors = Vec::new();
    let node_ids = nodes
        .iter()
        .filter_map(|node| node["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    fn walk(
        value: &Value,
        bindings: &Value,
        node_ids: &std::collections::HashSet<&str>,
        errors: &mut Vec<String>,
    ) {
        match value {
            Value::String(text) if text.starts_with('$') => {
                let name = text
                    .trim_start_matches('$')
                    .split('.')
                    .next()
                    .unwrap_or("");
                if !bindings[name].is_null()
                    || matches!(name, "owner" | "input" | "task")
                    || node_ids.contains(name)
                {
                    return;
                }
                errors.push(format!("undefined binding: {text}"));
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, bindings, node_ids, errors);
                }
            }
            Value::Object(map) => {
                for item in map.values() {
                    walk(item, bindings, node_ids, errors);
                }
            }
            _ => {}
        }
    }
    walk(value, bindings, &node_ids, &mut errors);
    errors
}

fn valid_argument_value(value: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match value {
        Value::String(text) => text.chars().count() <= 4096,
        Value::Array(items) => items
            .iter()
            .all(|item| valid_argument_value(item, depth + 1)),
        Value::Object(map) => map
            .values()
            .all(|item| valid_argument_value(item, depth + 1)),
        _ => true,
    }
}

fn valid_node_arguments(value: &Value) -> bool {
    value.is_object()
        && value.to_string().len() <= 32_768
        && valid_argument_value(value, 0)
}

fn validate_dag(nodes: &[Value], edges: &[Value], bindings: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if nodes.len() > 16 {
        errors.push("DAG nodes exceed 16".into());
    }
    if edges.len() > 32 {
        errors.push("DAG edges exceed 32".into());
    }
    let catalog = tool_catalog();
    let mut ids = std::collections::HashSet::new();
    for node in nodes {
        let Some(id) = node["id"].as_str().filter(|id| !id.is_empty()) else {
            errors.push("DAG node id is missing".into());
            continue;
        };
        if !ids.insert(id) {
            errors.push(format!("duplicate DAG node: {id}"));
        }
        let Some(tool) = node["tool"].as_str().filter(|tool| !tool.is_empty()) else {
            errors.push(format!("DAG node {id} has no tool"));
            continue;
        };
        if !catalog.contains(tool) && !tool.starts_with("mcp__") {
            errors.push(format!("unsupported DAG tool: {tool}"));
        }
        if !valid_node_arguments(&node["arguments"]) {
            errors.push(format!(
                "DAG node {id} arguments exceed bounds or are not an object"
            ));
        }
        errors.extend(validate_bindings(
            &node["arguments"],
            bindings,
            nodes,
        ));
    }
    let mut adjacency: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for edge in edges {
        let Some(from) = edge["from"].as_str() else {
            errors.push("DAG edge has no from".into());
            continue;
        };
        let Some(to) = edge["to"].as_str() else {
            errors.push("DAG edge has no to".into());
            continue;
        };
        if !ids.contains(from) || !ids.contains(to) {
            errors.push(format!("DAG edge references an unknown node: {from}->{to}"));
            continue;
        }
        if from == to {
            errors.push(format!("DAG self dependency: {from}"));
        }
        adjacency.entry(from).or_default().push(to);
    }
    fn visit<'a>(
        node: &'a str,
        adjacency: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        state: &mut std::collections::HashMap<&'a str, u8>,
        depth: usize,
        errors: &mut Vec<String>,
    ) {
        if depth > 8 {
            errors.push("DAG depth exceeds 8".into());
            return;
        }
        match state.get(node).copied().unwrap_or(0) {
            1 => {
                errors.push(format!("DAG cycle at {node}"));
                return;
            }
            2 => return,
            _ => {}
        }
        state.insert(node, 1);
        if let Some(children) = adjacency.get(node) {
            for child in children {
                visit(child, adjacency, state, depth + 1, errors);
            }
        }
        state.insert(node, 2);
    }
    let mut state = std::collections::HashMap::new();
    for id in &ids {
        visit(id, &adjacency, &mut state, 0, &mut errors);
    }
    errors
}

/// Rejects contradictory file equality checks for the same path.
fn contradictory_after(after: &[Value]) -> Vec<String> {
    let mut expected: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::new();
    let mut errors = Vec::new();
    for item in after {
        if item["kind"].as_str() != Some("file_equals") {
            continue;
        }
        let Some(path) = item["path"].as_str() else {
            continue;
        };
        let text = item["text"].as_str().unwrap_or("");
        if let Some(previous) = expected.insert(path, text) {
            if previous != text {
                errors.push(format!("contradictory file_equals for {path}"));
            }
        }
    }
    errors
}

/// Accepts only the documented shape. Anything ambiguous becomes `unverified`;
/// an unverified strategy is never promoted by the probation counter.
pub fn normalize(value: &Value) -> Value {
    if value["type"].as_str() != Some("procedure") {
        return unverified();
    }
    let mut errors = Vec::new();
    if !value["appliesTo"].is_null() && !value["appliesTo"].is_object() {
        errors.push("appliesTo must be an object".to_string());
    }
    let before = match value["before"].as_array() {
        Some(items) => {
            for (index, item) in items.iter().enumerate() {
                if !valid_step(&item["when"]) {
                    errors.push(format!("before[{index}].when is invalid"));
                }
                if !valid_step(&item["require"]) {
                    errors.push(format!("before[{index}].require is invalid"));
                }
            }
            items.clone()
        }
        None if value["before"].is_null() => Vec::new(),
        None => {
            errors.push("before must be an array".to_string());
            Vec::new()
        }
    };
    let after = match value["after"].as_array() {
        Some(items) => {
            for (index, item) in items.iter().enumerate() {
                if !valid_after(item) {
                    errors.push(format!("after[{index}] is invalid"));
                }
            }
            items.clone()
        }
        None if value["after"].is_null() => Vec::new(),
        None => {
            errors.push("after must be an array".to_string());
            Vec::new()
        }
    };
    let nodes = match value["nodes"].as_array() {
        Some(items) => items.clone(),
        None if value["nodes"].is_null() => Vec::new(),
        None => {
            errors.push("nodes must be an array".to_string());
            Vec::new()
        }
    };
    let edges = match value["edges"].as_array() {
        Some(items) => items.clone(),
        None if value["edges"].is_null() => Vec::new(),
        None => {
            errors.push("edges must be an array".to_string());
            Vec::new()
        }
    };
    errors.extend(validate_dag(&nodes, &edges, &value["bindings"]));
    errors.extend(contradictory_after(&after));
    if !errors.is_empty() {
        return json!({
            "type": "invalid",
            "reason": errors.join("; "),
        });
    }
    if before.is_empty() && after.is_empty() && nodes.is_empty() {
        return unverified();
    }
    json!({
        "type": "procedure",
        "appliesTo": value["appliesTo"].clone(),
        "before": before,
        "after": after,
        "nodes": nodes,
        "edges": edges,
        "bindings": value["bindings"].clone(),
    })
}

fn normalize_path(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("./")
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}

fn argument_matches(value: &Value, key: &str, expected: &str) -> bool {
    let Some(actual) = value[key].as_str() else {
        return false;
    };
    match key {
        "path" | "file" | "cwd" => normalize_path(actual) == normalize_path(expected),
        "command" => actual
            .split_whitespace()
            .any(|token| {
                let token = token.trim_matches(|ch: char| ch == '"' || ch == '\'');
                normalize_path(token) == normalize_path(expected)
                    || token.ends_with(&format!("/{expected}"))
            }),
        _ => actual == expected,
    }
}

fn operation_matches(operation: &Value, condition: &Value) -> bool {
    if operation["status"].as_str() != Some("completed") {
        return false;
    }
    if let Some(tool) = condition["tool"].as_str() {
        if operation["tool"].as_str() != Some(tool) {
            return false;
        }
    }
    if let Some(path) = condition["path"].as_str() {
        let arguments = &operation["arguments"];
        if !["path", "file", "command", "cwd"]
            .iter()
            .any(|key| argument_matches(arguments, key, path))
        {
            return false;
        }
    }
    true
}

fn applies_to_path(value: &str, request: &str, args: Option<&Value>) -> bool {
    let request_match = if value.contains('/') {
        normalize_path(request).contains(&normalize_path(value))
    } else {
        request.split_whitespace().any(|token| token == value)
            || request.contains(value)
    };
    request_match
        || args.is_some_and(|args| {
            ["path", "file", "command", "cwd"]
                .iter()
                .any(|key| argument_matches(args, key, value))
        })
}

/// Whether this strategy is intended for the current request or tool call.
pub fn applies(strategy: &Value, request: &str, tool: Option<&str>, args: Option<&Value>) -> bool {
    let verification = &strategy["verification"];
    if verification["type"].as_str() != Some("procedure") {
        return false;
    }
    let applies_to = &verification["appliesTo"];
    let mut groups = 0usize;
    let mut matched = true;
    if let Some(items) = applies_to["requestContains"].as_array() {
        groups += 1;
        matched &= items.iter().any(|item| {
            item.as_str()
                .is_some_and(|needle| request.contains(needle))
        });
    }
    if let Some(items) = applies_to["tools"].as_array() {
        groups += 1;
        matched &= tool.is_some_and(|tool| {
            items
                .iter()
                .any(|item| item.as_str().is_some_and(|name| name == tool))
        });
    }
    if let Some(items) = applies_to["paths"].as_array() {
        groups += 1;
        matched &= items.iter().any(|item| {
            item.as_str()
                .is_some_and(|path| applies_to_path(path, request, args))
        });
    }
    if groups > 0 {
        return matched;
    }
    // No explicit applicability: apply when a declared path/tool is present.
    verification["before"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(verification["after"].as_array().into_iter().flatten())
        .any(|condition| {
            let condition = if condition.get("when").is_some() {
                &condition["when"]
            } else {
                condition
            };
            condition["path"]
                .as_str()
                .is_some_and(|path| applies_to_path(path, request, args))
                || condition["tool"]
                    .as_str()
                    .is_some_and(|name| tool == Some(name))
        })
}

/// Returns a repair instruction when the current tool call would violate a
/// declared prerequisite. The caller must not execute the tool in that case.
pub fn missing_requirement(
    strategy: &Value,
    current_tool: &str,
    arguments: &Value,
    operations: &[Value],
) -> Option<String> {
    let before = strategy["verification"]["before"].as_array()?;
    for entry in before {
        let when = &entry["when"];
        if when["tool"].as_str() != Some(current_tool) {
            continue;
        }
        if let Some(path) = when["path"].as_str() {
            let hit = ["path", "file", "command", "cwd"]
                .iter()
                .any(|key| argument_matches(arguments, key, path));
            if !hit {
                continue;
            }
        }
        let require = &entry["require"];
        let last_target = operations
            .iter()
            .rposition(|operation| operation_matches(operation, when));
        let last_require = operations
            .iter()
            .rposition(|operation| operation_matches(operation, require));
        let satisfied = match (last_require, last_target) {
            (Some(require), Some(target)) => require > target,
            (Some(_), None) => true,
            _ => false,
        };
        if satisfied {
            continue;
        }
        let tool = require["tool"].as_str().unwrap_or("required");
        let path = require["path"].as_str().unwrap_or("");
        return Some(format!(
            "절차 위반: {tool} {path} 작업을 먼저 완료한 뒤 다시 시도하세요."
        ));
    }
    None
}

const MAX_PROCEDURE_FILE_BYTES: u64 = 2 * 1024 * 1024;

enum FileRead {
    Text(String),
    Missing,
    Unknown,
}

fn read_workspace_file(workspace: &Path, path: &str) -> FileRead {
    if !safe_relative(path) {
        return FileRead::Unknown;
    }
    let candidate = workspace.join(path);
    let metadata = match std::fs::symlink_metadata(&candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return FileRead::Missing
        }
        Err(_) => return FileRead::Unknown,
    };
    if metadata.file_type().is_symlink() || metadata.len() > MAX_PROCEDURE_FILE_BYTES {
        return FileRead::Unknown;
    }
    let canonical_workspace = match workspace.canonicalize() {
        Ok(path) => path,
        Err(_) => return FileRead::Unknown,
    };
    let canonical = match candidate.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return FileRead::Missing
        }
        Err(_) => return FileRead::Unknown,
    };
    if !canonical.starts_with(&canonical_workspace) {
        return FileRead::Unknown;
    }
    match std::fs::read_to_string(canonical) {
        Ok(text) => FileRead::Text(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            FileRead::Missing
        }
        Err(_) => FileRead::Unknown,
    }
}

fn check_after(
    condition: &Value,
    operations: &[Value],
    workspace: &Path,
) -> (bool, bool) {
    match condition["kind"].as_str() {
        Some("tool_called") => (
            operations
                .iter()
                .any(|operation| operation_matches(operation, condition)),
            false,
        ),
        Some("file_exists") => {
            let Some(path) = condition["path"].as_str() else {
                return (false, true);
            };
            if !safe_relative(path) {
                return (false, true);
            }
            match std::fs::symlink_metadata(workspace.join(path)) {
                Ok(metadata) if metadata.file_type().is_symlink() => (false, true),
                Ok(_) => (true, false),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    (false, false)
                }
                Err(_) => (false, true),
            }
        }
        Some("file_equals") => {
            let Some(path) = condition["path"].as_str() else {
                return (false, true);
            };
            let expected = condition["text"].as_str().unwrap_or("");
            match read_workspace_file(workspace, path) {
                FileRead::Text(actual) => (
                    actual.trim_end() == expected.trim_end(),
                    false,
                ),
                FileRead::Missing => (false, false),
                FileRead::Unknown => (false, true),
            }
        }
        Some("file_contains") => {
            let Some(path) = condition["path"].as_str() else {
                return (false, true);
            };
            let expected = condition["text"].as_str().unwrap_or("");
            match read_workspace_file(workspace, path) {
                FileRead::Text(actual) => (actual.contains(expected), false),
                FileRead::Missing => (false, false),
                FileRead::Unknown => (false, true),
            }
        }
        _ => (false, true),
    }
}

/// Sort operations by the engine's monotonic sequence, then by timestamp/id
/// for legacy records that predate `opSeq`.
pub fn sort_operations(operations: &mut [Value]) {
    operations.sort_by(|left, right| {
        left["opSeq"]
            .as_u64()
            .unwrap_or(u64::MAX)
            .cmp(&right["opSeq"].as_u64().unwrap_or(u64::MAX))
            .then_with(|| {
                left["startedAt"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(right["startedAt"].as_str().unwrap_or(""))
            })
            .then_with(|| left["id"].as_str().cmp(&right["id"].as_str()))
    });
}

fn resolve_binding(
    value: &Value,
    bindings: &Value,
    request: &str,
    results: &std::collections::HashMap<String, Value>,
) -> Option<Value> {
    match value {
        Value::String(text) if text.starts_with('$') => {
            let rest = text.trim_start_matches('$');
            if matches!(rest, "owner" | "input" | "task") {
                return Some(json!(request));
            }
            if let Some((node, pointer)) = rest.split_once('.') {
                let result = results.get(node)?;
                if pointer.is_empty() {
                    return Some(result.clone());
                }
                let pointer = if pointer.starts_with('/') {
                    pointer.to_string()
                } else {
                    format!("/{}", pointer.replace('.', "/"))
                };
                return result.pointer(&pointer).cloned();
            }
            bindings.get(rest).cloned()
        }
        Value::Array(items) => Some(Value::Array(
            items
                .iter()
                .map(|item| resolve_binding(item, bindings, request, results))
                .collect::<Option<Vec<_>>>()?,
        )),
        Value::Object(map) => {
            let mut resolved = serde_json::Map::new();
            for (key, item) in map {
                resolved.insert(
                    key.clone(),
                    resolve_binding(item, bindings, request, results)?,
                );
            }
            Some(Value::Object(resolved))
        }
        other => Some(other.clone()),
    }
}

pub fn operation_succeeded(operation: &Value) -> bool {
    if operation["status"].as_str() != Some("completed") {
        return false;
    }
    let result = &operation["result"];
    if !result["error"].is_null() {
        return false;
    }
    if result["success"].as_bool() == Some(false) {
        return false;
    }
    if let Some(code) = result["exitCode"].as_i64() {
        if code != 0 {
            return false;
        }
    }
    if let Some(code) = result["status"].as_u64() {
        if code >= 400 {
            return false;
        }
    }
    true
}

/// Selects the next ready DAG node and resolves only literal, owner/input,
/// or previously verified node-result bindings.
pub fn ready_intent(
    strategy: &Value,
    operations: &[Value],
    request: &str,
    results: &std::collections::HashMap<String, Value>,
) -> Option<(String, String, Value)> {
    let verification = &strategy["verification"];
    if verification["type"].as_str() != Some("procedure") {
        return None;
    }
    let nodes = verification["nodes"].as_array()?;
    if nodes.is_empty() {
        return None;
    }
    let Some(strategy_id) = strategy["id"].as_str() else {
        return None;
    };
    let Some(revision) = strategy["revision"].as_str() else {
        return None;
    };
    let completed = operations
        .iter()
        .filter(|operation| {
            operation["origin"].as_str() == Some("procedure")
                && operation["procedureStrategyId"].as_str() == Some(strategy_id)
                && operation["procedureRevision"].as_str() == Some(revision)
                && operation_succeeded(operation)
        })
        .filter_map(|operation| operation["procedureNodeId"].as_str())
        .collect::<std::collections::HashSet<_>>();
    for node in nodes {
        let Some(id) = node["id"].as_str() else {
            continue;
        };
        if completed.contains(id) {
            continue;
        }
        let dependencies = verification["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|edge| edge["to"].as_str() == Some(id))
            .filter_map(|edge| edge["from"].as_str())
            .collect::<Vec<_>>();
        if dependencies.iter().any(|dependency| !completed.contains(dependency)) {
            continue;
        }
        let tool = node["tool"].as_str()?.to_string();
        let arguments = resolve_binding(
            &node["arguments"],
            &verification["bindings"],
            request,
            results,
        )?;
        if !valid_node_arguments(&arguments) {
            continue;
        }
        return Some((id.to_string(), tool, arguments));
    }
    None
}

/// Evaluates a strategy against the completed operations and file state.
/// `verified=false` means the strategy has no machine-checkable procedure.
pub fn evaluate(strategy: &Value, operations: &[Value], workspace: &Path) -> Value {
    let verification = &strategy["verification"];
    let verification_type = verification["type"].as_str().unwrap_or("unverified");
    if verification_type != "procedure" {
        return json!({
            "verified": false,
            "triggered": false,
            "followed": false,
            "status": verification_type,
            "reason": verification["reason"],
            "violations": [verification_type],
            "checks": [],
        });
    }
    let mut violations = Vec::new();
    let mut checks = Vec::new();
    let mut unknown = false;
    let before = verification["before"].as_array().cloned().unwrap_or_default();
    let mut triggered = before.is_empty();

    for entry in &before {
        let when = &entry["when"];
        let require = &entry["require"];
        let targets = operations
            .iter()
            .enumerate()
            .filter(|(_, operation)| operation_matches(operation, when))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let prerequisites = operations
            .iter()
            .enumerate()
            .filter(|(_, operation)| operation_matches(operation, require))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if !targets.is_empty() || !prerequisites.is_empty() {
            triggered = true;
        }
        // Each target call needs a prerequisite after the previous target.
        let mut previous_target = None;
        let mut ok = true;
        for target in &targets {
            let lower = previous_target.map(|index| index + 1).unwrap_or(0);
            if !prerequisites
                .iter()
                .any(|prerequisite| *prerequisite >= lower && *prerequisite < *target)
            {
                ok = false;
                break;
            }
            previous_target = Some(*target);
        }
        if targets.is_empty() && !prerequisites.is_empty() {
            ok = false;
        }
        checks.push(json!({
            "kind": "before",
            "require": require,
            "when": when,
            "ok": ok,
            "triggered": !targets.is_empty() || !prerequisites.is_empty(),
        }));
        if !ok {
            violations.push(json!({
                "kind": "before",
                "message": format!(
                    "{} {} 작업이 {} {} 작업보다 먼저 수행되지 않았습니다.",
                    require["tool"].as_str().unwrap_or("required"),
                    require["path"].as_str().unwrap_or(""),
                    when["tool"].as_str().unwrap_or("target"),
                    when["path"].as_str().unwrap_or("")
                )
            }));
        }
    }

    for condition in verification["after"].as_array().into_iter().flatten() {
        let (ok, cannot_verify) = check_after(condition, operations, workspace);
        unknown |= cannot_verify;
        checks.push(json!({
            "kind": "after", "condition": condition, "ok": ok,
            "unknown": cannot_verify
        }));
        if !ok && !cannot_verify {
            violations.push(json!({
                "kind": "after",
                "message": format!("후속 조건을 충족하지 못했습니다: {}", condition),
            }));
        }
    }

    let nodes = verification["nodes"].as_array().cloned().unwrap_or_default();
    if !nodes.is_empty() {
        triggered = true;
        let strategy_id = strategy["id"].as_str().unwrap_or("");
        let revision = strategy["revision"].as_str().unwrap_or("");
        let mut completed_nodes = std::collections::HashMap::new();
        for (index, operation) in operations.iter().enumerate() {
            if operation["origin"].as_str() != Some("procedure")
                || operation["procedureStrategyId"].as_str() != Some(strategy_id)
                || operation["procedureRevision"].as_str() != Some(revision)
                || !operation_succeeded(operation)
            {
                continue;
            }
            if let Some(node) = operation["procedureNodeId"].as_str() {
                completed_nodes.insert(node.to_string(), index);
            }
        }
        for node in &nodes {
            let id = node["id"].as_str().unwrap_or("");
            let ok = completed_nodes.contains_key(id);
            checks.push(json!({ "kind":"dag_node", "node":id, "ok":ok }));
            if !ok {
                violations.push(json!({
                    "kind":"dag_node",
                    "message": format!("완료 영수증이 없는 DAG node입니다: {id}")
                }));
            }
        }
        for edge in verification["edges"].as_array().into_iter().flatten() {
            let from = edge["from"].as_str().unwrap_or("");
            let to = edge["to"].as_str().unwrap_or("");
            let ok = match (completed_nodes.get(from), completed_nodes.get(to)) {
                (Some(from), Some(to)) => from < to,
                _ => false,
            };
            checks.push(json!({
                "kind":"dag_edge", "from":from, "to":to, "ok":ok
            }));
            if !ok {
                violations.push(json!({
                    "kind":"dag_edge",
                    "message": format!("DAG edge가 순서대로 완료되지 않았습니다: {from}->{to}")
                }));
            }
        }
    }

    let followed = triggered && violations.is_empty() && !unknown;
    let status = if unknown {
        "unknown"
    } else if !triggered {
        "not_triggered"
    } else if followed {
        "followed"
    } else {
        "violated"
    };
    json!({
        "verified": true,
        "triggered": triggered,
        "followed": followed,
        "unknown": unknown,
        "status": status,
        "violations": violations,
        "checks": checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn procedure() -> Value {
        json!({
            "type": "procedure",
            "appliesTo": { "paths": ["report.txt"] },
            "before": [
                { "when": { "tool": "workspace_write", "path": "report.txt" },
                  "require": { "tool": "workspace_write", "path": "audit.txt" } }
            ],
            "after": [
                { "kind": "file_contains", "path": "audit.txt", "text": "report.txt" },
                { "kind": "file_equals", "path": "report.txt", "text": "ok" }
            ]
        })
    }

    fn strategy() -> Value {
        json!({
            "id": "s1",
            "verification": normalize(&procedure()),
        })
    }

    fn operation(tool: &str, path: &str) -> Value {
        json!({
            "id": format!("{tool}:{path}"),
            "tool": tool,
            "arguments": { "path": path, "content": path },
            "status": "completed",
        })
    }

    #[test]
    fn applies_only_to_intended_tasks() {
        let strategy = strategy();
        assert!(applies(&strategy, "report.txt를 만들어", None, None));
        assert!(!applies(&strategy, "다른 파일을 만들어", None, None));
    }

    #[test]
    fn blocks_target_before_prerequisite() {
        let strategy = strategy();
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path": "report.txt", "content": "ok"}),
            &[],
        )
        .is_some());
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path": "report.txt", "content": "ok"}),
            &[operation("workspace_write", "audit.txt")],
        )
        .is_none());
    }

    #[test]
    fn evaluates_order_and_postconditions() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("audit.txt"), "report.txt").unwrap();
        std::fs::write(dir.join("report.txt"), "ok\n").unwrap();
        let operations = vec![
            operation("workspace_write", "audit.txt"),
            operation("workspace_write", "report.txt"),
        ];
        let result = evaluate(&strategy(), &operations, &dir);
        assert_eq!(result["verified"], true);
        assert_eq!(result["followed"], true);
        let reversed = vec![
            operation("workspace_write", "report.txt"),
            operation("workspace_write", "audit.txt"),
        ];
        let result = evaluate(&strategy(), &reversed, &dir);
        assert_eq!(result["followed"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exact_paths_do_not_match_suffixes_or_commands() {
        let condition = json!({"tool":"workspace_write","path":"report.txt"});
        assert!(operation_matches(
            &json!({"tool":"workspace_write","status":"completed","arguments":{"path":"report.txt","content":"ok"}}),
            &condition
        ));
        assert!(!operation_matches(
            &json!({"tool":"workspace_write","status":"completed","arguments":{"path":"report.txt.bak","content":"ok"}}),
            &condition
        ));
        assert!(!operation_matches(
            &json!({"tool":"shell_exec","status":"completed","arguments":{"command":"echo report.txt"}}),
            &condition
        ));
    }

    #[test]
    fn symlink_and_oversized_files_are_rejected() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        let outside = std::env::temp_dir().join(format!("dominion-out-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&outside, "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join("link.txt")).unwrap();
        assert!(matches!(
            read_workspace_file(&dir, "link.txt"),
            FileRead::Unknown
        ));
        let big = "x".repeat((MAX_PROCEDURE_FILE_BYTES + 1) as usize);
        std::fs::write(dir.join("big.txt"), big).unwrap();
        assert!(matches!(
            read_workspace_file(&dir, "big.txt"),
            FileRead::Unknown
        ));
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_file(outside);
    }

    #[test]
    fn one_invalid_condition_rejects_the_whole_revision() {
        let mut value = procedure();
        value["before"][0]["when"] = json!({"path": "report.txt"});
        let normalized = normalize(&value);
        assert_eq!(normalized["type"], "invalid");
        assert!(normalized["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("before[0].when"));
    }

    #[test]
    fn empty_execution_is_not_triggered_not_compliant() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let result = evaluate(&strategy(), &[], &dir);
        assert_eq!(result["triggered"], false);
        assert_eq!(result["followed"], false);
        assert_eq!(result["status"], "not_triggered");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ready_nodes_bind_only_verified_values() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "appliesTo":{"paths":["out.txt"]},
                "bindings":{"literal":"bound"},
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"$prep.path","content":"$literal"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let mut results = std::collections::HashMap::new();
        let (id, tool, _arguments) =
            ready_intent(&strategy, &[], "out.txt", &results).expect("first node");
        assert_eq!(id, "prep");
        assert_eq!(tool, "workspace_write");
        results.insert(
            "prep".into(),
            json!({"path":"audit.txt","written":true}),
        );
        let (id, _, arguments) =
            ready_intent(
                &strategy,
                &[json!({
                    "origin":"procedure","procedureNodeId":"prep","status":"completed",
                    "procedureStrategyId":"harness","procedureRevision":"r1"
                })],
                "out.txt",
                &results,
            )
                .expect("second node");
        assert_eq!(id, "write");
        assert_eq!(arguments["path"], "audit.txt");
        assert_eq!(arguments["content"], "bound");
    }

    #[test]
    fn completed_operation_with_error_is_not_success() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let (id, _, _) = ready_intent(
            &strategy,
            &[json!({
                "origin":"procedure","procedureNodeId":"prep","status":"completed",
                "procedureStrategyId":"harness","procedureRevision":"r1",
                "result":{"error":"Denied by the user"}
            })],
            "out.txt",
            &std::collections::HashMap::new(),
        )
        .expect("errored prerequisite must be retried");
        assert_eq!(id, "prep");
    }

    #[test]
    fn failed_prerequisite_is_not_completed() {
        let strategy = json!({
            "id":"harness",
            "revision":"r1",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[
                    {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                    {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
                ],
                "edges":[{"from":"prep","to":"write"}]
            }))
        });
        let mut results = std::collections::HashMap::new();
        results.insert("prep".into(), json!({"path":"audit.txt"}));
        let (id, _, _) = ready_intent(
            &strategy,
            &[json!({
                "origin":"procedure","procedureNodeId":"prep","status":"failed",
                "result":{"error":"denied"}
            })],
            "out.txt",
            &results,
        )
        .expect("failed prerequisite must be retried");
        assert_eq!(id, "prep");
    }

    #[test]
    fn unresolved_binding_does_not_produce_an_intent() {
        let strategy = json!({
            "id":"harness",
            "verification": normalize(&json!({
                "type":"procedure",
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"$missing"}}]
            }))
        });
        assert!(ready_intent(
            &strategy,
            &[],
            "out.txt",
            &std::collections::HashMap::new()
        )
        .is_none());
    }

    #[test]
    fn dag_cycles_unknowns_and_contradictions_are_rejected() {
        let valid = json!({
            "type":"procedure",
            "appliesTo":{"paths":["out.txt"]},
            "nodes":[
                {"id":"prep","tool":"workspace_write","arguments":{"path":"audit.txt","content":"ok"}},
                {"id":"write","tool":"workspace_write","arguments":{"path":"out.txt","content":"ok"}}
            ],
            "edges":[{"from":"prep","to":"write"}],
            "bindings":{}
        });
        assert_eq!(normalize(&valid)["type"], "procedure");
        let mut cycle = valid.clone();
        cycle["edges"] = json!([{"from":"prep","to":"write"},{"from":"write","to":"prep"}]);
        assert_eq!(normalize(&cycle)["type"], "invalid");
        let mut self_edge = valid.clone();
        self_edge["edges"] = json!([{"from":"prep","to":"prep"}]);
        assert_eq!(normalize(&self_edge)["type"], "invalid");
        let mut unknown = valid.clone();
        unknown["edges"] = json!([{"from":"prep","to":"missing"}]);
        assert_eq!(normalize(&unknown)["type"], "invalid");
        let mut unsupported = valid.clone();
        unsupported["nodes"][0]["tool"] = json!("not_a_real_tool");
        assert_eq!(normalize(&unsupported)["type"], "invalid");
        let mut binding = valid.clone();
        binding["nodes"][0]["arguments"]["content"] = json!("$missing");
        assert_eq!(normalize(&binding)["type"], "invalid");
        let mut contradiction = valid.clone();
        contradiction["after"] = json!([
            {"kind":"file_equals","path":"out.txt","text":"a"},
            {"kind":"file_equals","path":"out.txt","text":"b"}
        ]);
        assert_eq!(normalize(&contradiction)["type"], "invalid");
    }

    #[test]
    fn repeated_target_requires_a_fresh_prerequisite() {
        let strategy = strategy();
        let audit = operation("workspace_write", "audit.txt");
        let report = operation("workspace_write", "report.txt");
        assert!(missing_requirement(
            &strategy,
            "workspace_write",
            &json!({"path":"report.txt","content":"ok"}),
            &[audit.clone(), report.clone()],
        )
        .is_some());
        let mut second_audit = operation("workspace_write", "audit.txt");
        second_audit["id"] = json!("audit-2");
        let second_report = json!({
            "id":"report-2","tool":"workspace_write","status":"completed",
            "arguments":{"path":"report.txt","content":"ok"}
        });
        let result = evaluate(
            &strategy,
            &[audit, report, second_audit, second_report],
            Path::new("."),
        );
        assert_eq!(result["followed"], false);
    }

    #[test]
    fn operations_sort_by_monotonic_sequence() {
        let mut operations = vec![
            json!({"id":"late","opSeq":2,"startedAt":"2026-01-01T00:00:02Z"}),
            json!({"id":"early","opSeq":1,"startedAt":"2026-01-01T00:00:01Z"}),
        ];
        sort_operations(&mut operations);
        assert_eq!(operations[0]["id"], "early");
    }

    #[test]
    fn unreadable_artifacts_are_unknown() {
        let dir = std::env::temp_dir().join(format!("dominion-proc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("dir.txt")).unwrap();
        let strategy = json!({
            "id":"unknown","verification":normalize(&json!({
                "type":"procedure","appliesTo":{"paths":["dir.txt"]},
                "after":[{"kind":"file_equals","path":"dir.txt","text":"x"}]
            }))
        });
        let result = evaluate(&strategy, &[], &dir);
        assert_eq!(result["status"], "unknown");
        assert_eq!(result["followed"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_strategy_is_never_counted() {
        let strategy = json!({"id":"u","verification":{"type":"unverified"}});
        assert!(!applies(&strategy, "report.txt", None, None));
        let result = evaluate(&strategy, &[], Path::new("."));
        assert_eq!(result["verified"], false);
    }
}
