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
    if !errors.is_empty() {
        return json!({
            "type": "invalid",
            "reason": errors.join("; "),
        });
    }
    if before.is_empty() && after.is_empty() {
        return unverified();
    }
    json!({
        "type": "procedure",
        "appliesTo": value["appliesTo"].clone(),
        "before": before,
        "after": after,
    })
}

fn contains(value: &Value, needle: &str) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.contains(needle))
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
        let path_hit = ["path", "file", "command", "cwd"]
            .iter()
            .any(|key| contains(&arguments[key], path));
        if !path_hit {
            return false;
        }
    }
    true
}

fn applies_to_path(value: &str, request: &str, args: Option<&Value>) -> bool {
    request.contains(value)
        || args.is_some_and(|args| {
            ["path", "file", "command", "cwd"]
                .iter()
                .any(|key| contains(&args[key], value))
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
                .any(|key| contains(&arguments[key], path));
            if !hit {
                continue;
            }
        }
        let require = &entry["require"];
        if operations
            .iter()
            .any(|operation| operation_matches(operation, require))
        {
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

fn read_workspace_file(workspace: &Path, path: &str) -> Option<String> {
    if !safe_relative(path) {
        return None;
    }
    std::fs::read_to_string(workspace.join(path)).ok()
}

fn check_after(condition: &Value, operations: &[Value], workspace: &Path) -> bool {
    match condition["kind"].as_str() {
        Some("tool_called") => operations
            .iter()
            .any(|operation| operation_matches(operation, condition)),
        Some("file_exists") => condition["path"]
            .as_str()
            .and_then(|path| read_workspace_file(workspace, path))
            .is_some(),
        Some("file_equals") => {
            let Some(path) = condition["path"].as_str() else {
                return false;
            };
            let Some(actual) = read_workspace_file(workspace, path) else {
                return false;
            };
            let expected = condition["text"].as_str().unwrap_or("");
            actual.trim_end() == expected.trim_end()
        }
        Some("file_contains") => {
            let Some(path) = condition["path"].as_str() else {
                return false;
            };
            let Some(actual) = read_workspace_file(workspace, path) else {
                return false;
            };
            actual.contains(condition["text"].as_str().unwrap_or(""))
        }
        _ => false,
    }
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
    let before = verification["before"].as_array().cloned().unwrap_or_default();
    let mut triggered = before.is_empty();

    for entry in &before {
        let when = &entry["when"];
        let require = &entry["require"];
        let target = operations
            .iter()
            .position(|operation| operation_matches(operation, when));
        let prerequisite = operations
            .iter()
            .position(|operation| operation_matches(operation, require));
        if target.is_some() || prerequisite.is_some() {
            triggered = true;
        }
        let ok = match (prerequisite, target) {
            (Some(prerequisite), Some(target)) => prerequisite < target,
            (Some(_), None) => false,
            (None, Some(_)) => false,
            (None, None) => true,
        };
        checks.push(json!({
            "kind": "before",
            "require": require,
            "when": when,
            "ok": ok,
            "triggered": target.is_some() || prerequisite.is_some(),
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
        let ok = check_after(condition, operations, workspace);
        checks.push(json!({ "kind": "after", "condition": condition, "ok": ok }));
        if !ok {
            violations.push(json!({
                "kind": "after",
                "message": format!("후속 조건을 충족하지 못했습니다: {}", condition),
            }));
        }
    }

    let followed = triggered && violations.is_empty();
    let status = if !triggered {
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
    fn unverified_strategy_is_never_counted() {
        let strategy = json!({"id":"u","verification":{"type":"unverified"}});
        assert!(!applies(&strategy, "report.txt", None, None));
        let result = evaluate(&strategy, &[], Path::new("."));
        assert_eq!(result["verified"], false);
    }
}
