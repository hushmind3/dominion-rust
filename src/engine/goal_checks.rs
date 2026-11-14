//! Deterministic verification obligations for owner-requested coding tests.
//! This module never runs shell commands and never interpolates user text into
//! a command. It freezes required test receipts before agent execution.
use serde_json::{json, Value};
use std::path::Path;

fn english_test_word(text: &str) -> bool {
    let lower=text.to_lowercase();
    let mut word=String::new();
    for ch in lower.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_alphabetic() {
            word.push(ch);
        } else {
            if matches!(word.as_str(),"test"|"tests"|"testing"|"pytest") {
                return true;
            }
            word.clear();
        }
    }
    false
}
fn tests_requested(goal:&str)->bool {
    goal.contains("테스트") || goal.contains("스위트") || english_test_word(goal)
}

/// Infer *only* commands with declared project evidence and explicit testing
/// intent. No arbitrary shell text is ever taken from the natural-language
/// goal. The actual executor must run each command for the task to complete.
pub(crate) fn inferred_checks(workspace:&Path,goal:&str)->Vec<Value>{
    if !tests_requested(goal){return vec![];}
    let mut commands:Vec<&'static str>=Vec::new();
    if workspace.join("Cargo.toml").is_file(){commands.push("cargo test");}
    if workspace.join("go.mod").is_file(){commands.push("go test ./...");}
    let manifest=workspace.join("package.json");
    if std::fs::metadata(&manifest).is_ok_and(|m|m.is_file() && m.len()<=128*1024){
        if let Ok(bytes)=std::fs::read(&manifest){
            if let Ok(value)=serde_json::from_slice::<Value>(&bytes){
                let script=value["scripts"]["test"].as_str().unwrap_or("");
                if !script.trim().is_empty() && !script.contains("no test specified"){
                    let manager=value["packageManager"].as_str().unwrap_or("");
                    commands.push(if manager.starts_with("pnpm@"){"pnpm test"}
                        else if manager.starts_with("yarn@"){"yarn test"}
                        else {"npm test"});
                }
            }
        }
    }
    if commands.is_empty() && workspace.join("pytest.ini").is_file(){
        commands.push("pytest");
    }
    commands.into_iter().take(2).map(|cmd|json!({
        "type":"command_succeeded","command":cmd,
        "source":"goal-project-test-contract-v1"
    })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inferred_project_commands_require_request_and_actual_manifest(){
        let root=std::env::temp_dir().join(format!("goal-verifier-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
        assert!(inferred_checks(&root,"코드만 수정하세요").is_empty());
        assert_eq!(inferred_checks(&root,"코드를 수정하고 테스트하세요")[0]["command"],"cargo test");
        assert_eq!(inferred_checks(&root,"run tests")[0]["command"],"cargo test");
        for other in ["summarize latest updates","write a protest article","review the testament"] {
            assert!(inferred_checks(&root,other).is_empty(),"{other}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn js_test_entrypoint_is_not_invented(){
        let root=std::env::temp_dir().join(format!("goal-js-check-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"),
            r#"{"packageManager":"pnpm@10","scripts":{"test":"vitest run"}}"#).unwrap();
        assert_eq!(inferred_checks(&root,"run tests")[0]["command"],"pnpm test");
        std::fs::write(root.join("package.json"),
            r#"{"scripts":{"test":"echo no test specified"}}"#).unwrap();
        assert!(inferred_checks(&root,"run tests").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}