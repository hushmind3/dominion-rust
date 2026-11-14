//! Build-tool aware acceptance inference for AUTONOMOUS goals. This module
//! never runs a command: it makes requested verification mandatory so the
//! executor cannot call an edit "complete" without a genuine tool receipt.
use serde_json::{json,Value};
use std::path::Path;

fn asks_for_tests(goal: &str) -> bool {
    let lower=goal.to_lowercase();
    crate::engine::text_intent::has_any(&lower, &[
        "테스트", "스위트", "test", "tests", "testing", "pytest",
        "cargo test", "go test",
    ])
}

/// Adds at most two deterministic, user-requested project checks. The
/// commands are constant strings; user text is never interpolated into shell.
/// A project without an explicit test entry point is left unchanged rather
/// than demanding an impossible check.
pub(crate) fn inferred_checks(workspace: &Path, goal: &str) -> Vec<Value> {
    if !asks_for_tests(goal) { return Vec::new(); }
    let mut commands:Vec<&'static str>=Vec::new();
    if workspace.join("Cargo.toml").is_file() {
        commands.push("cargo test");
    }
    if workspace.join("go.mod").is_file() {
        commands.push("go test ./...");
    }
    let package_path=workspace.join("package.json");
    if std::fs::metadata(&package_path).is_ok_and(|meta|meta.is_file() && meta.len()<=128*1024) {
        if let Ok(data)=std::fs::read(package_path) {
            if let Ok(package)=serde_json::from_slice::<Value>(&data) {
                let test=package["scripts"]["test"].as_str().unwrap_or("");
                if !test.trim().is_empty() && !test.contains("no test specified") {
                    let manager=package["packageManager"].as_str().unwrap_or("");
                    commands.push(if manager.starts_with("pnpm@") {"pnpm test"}
                        else if manager.starts_with("yarn@") {"yarn test"}
                        else {"npm test"});
                }
            }
        }
    }
    if workspace.join("pytest.ini").is_file() && commands.is_empty() {
        commands.push("pytest");
    }
    commands.into_iter().take(2).map(|command|json!({
        "type":"command_succeeded","command":command,
        "source":"goal_project_test_inference_v1",
    })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicitly_requested_tests_become_completion_gates() {
        let root=std::env::temp_dir().join(format!("goal-check-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"),"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        assert!(inferred_checks(&root,"코드를 수정하세요").is_empty());
        assert_eq!(inferred_checks(&root,"구현하고 테스트까지 수행하세요")[0]["command"],"cargo test");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn embedded_english_test_substrings_do_not_force_a_build() {
        let root=std::env::temp_dir().join(format!("goal-lexical-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        for phrase in [
            "summarize the latest profile thread",
            "prepare a protest article",
            "review the testament",
        ] {
            assert!(inferred_checks(&root, phrase).is_empty(), "unexpected test gate: {phrase}");
        }
        for phrase in ["run tests", "add testing before delivery", "코드 테스트까지 수행"] {
            assert_eq!(inferred_checks(&root, phrase)[0]["command"], "cargo test");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn node_check_requires_real_script_and_uses_declared_manager() {
        let root=std::env::temp_dir().join(format!("goal-js-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"),r#"{"packageManager":"pnpm@10","scripts":{"test":"vitest run"}}"#).unwrap();
        assert_eq!(inferred_checks(&root,"run tests")[0]["command"],"pnpm test");
        std::fs::write(root.join("package.json"),r#"{"scripts":{"test":"echo no test specified"}}"#).unwrap();
        assert!(inferred_checks(&root,"run tests").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
