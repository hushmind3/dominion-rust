//! Compiler-assisted repair for model-authored RSI source revisions.
//!
//! A failed rustc check may contain machine-applicable suggestions. Try a
//! bounded sequence only on files already changed by the model, in the
//! detached worktree, and keep it ONLY after a second successful check.
//! Never treat compiler repair as an independent quality improvement.
use crate::engine::{rsi, rsi_worktree::{self, Worktree}};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::Path, time::Duration};
use tokio::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Suggestion {
    path: String,
    start: usize,
    end: usize,
    replacement: String,
}

fn suggestions(output: &[u8], changed: &BTreeSet<String>) -> Vec<Suggestion> {
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for line in String::from_utf8_lossy(output).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else { continue };
        if message["reason"] != "compiler-message"
            || message["message"]["level"] != "error" { continue; }
        let diagnostic = &message["message"];
        let mut groups = vec![diagnostic];
        if let Some(children) = diagnostic["children"].as_array() {
            groups.extend(children);
        }
        for group in groups {
            let Some(spans) = group["spans"].as_array() else { continue };
            for span in spans {
                if span["suggestion_applicability"] != "MachineApplicable" {
                    continue;
                }
                let Some(path) = span["file_name"].as_str() else { continue };
                if !rsi::safe_path(path) || !changed.contains(path) { continue; }
                let Some(start) = span["byte_start"].as_u64().and_then(|n| usize::try_from(n).ok()) else { continue };
                let Some(end) = span["byte_end"].as_u64().and_then(|n| usize::try_from(n).ok()) else { continue };
                let Some(replacement) = span["suggested_replacement"].as_str() else { continue };
                if start > end || replacement.len() > 512 { continue; }
                let key = (path.to_string(), start, end, replacement.to_string());
                if seen.insert(key) {
                    found.push(Suggestion {
                        path: path.to_string(), start, end,
                        replacement: replacement.to_string(),
                    });
                }
            }
        }
    }
    found
}

fn rewritten(source: &str, suggestion: &Suggestion) -> Option<String> {
    // rustc reports byte offsets. str::get protects UTF-8 character bounds.
    let before = source.get(..suggestion.start)?;
    let old = source.get(suggestion.start..suggestion.end)?;
    let after = source.get(suggestion.end..)?;
    if old == suggestion.replacement { return None; }
    Some(format!("{before}{}{after}", suggestion.replacement))
}

fn file_under(root: &Path, path: &str) -> Option<std::path::PathBuf> {
    if !rsi::safe_path(path) { return None; }
    let canonical_root = fs::canonicalize(root).ok()?;
    let absolute = fs::canonicalize(root.join(path)).ok()?;
    if !absolute.starts_with(&canonical_root) || !absolute.is_file() { return None; }
    Some(absolute)
}

async fn check(worktree: &Worktree) -> Result<std::process::Output, String> {
    let mut command = Command::new(rsi_worktree::cargo_executable());
    command.current_dir(&worktree.path)
        .args(["check", "--all-targets", "--message-format=json"])
        .kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(360), command.output())
        .await.map_err(|_| "compiler repair check timed out".to_string())?
        .map_err(|error| error.to_string())
}

async fn changed_paths(worktree: &Worktree) -> Result<BTreeSet<String>, String> {
    let mut command = Command::new("git");
    command.current_dir(&worktree.path)
        .args(["diff", "--name-only", "HEAD"])
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await.map_err(|_| "compiler repair git diff timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("compiler repair could not inspect detached source diff".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines()
        .filter(|path| rsi::safe_path(path) && path.ends_with(".rs"))
        .map(str::to_string).collect())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Only after an independently failed compile of a *real model edit*.
/// On unsuccessful repairs every file is restored byte-for-byte, so ordinary
/// model-guided recovery sees exactly the source it wrote.
pub async fn recover(worktree: &Worktree) -> Result<Option<Value>, String> {
    let changed = changed_paths(worktree).await?;
    if changed.is_empty() { return Ok(None); }
    let mut originals = BTreeMap::<String, Vec<u8>>::new();
    let mut edits = Vec::new();
    let mut tried = BTreeSet::new();
    let mut proven = false;
    for _ in 0..=3 {
        let compile = match check(worktree).await {
            Ok(output) => output,
            Err(_) => break,
        };
        if compile.status.success() {
            proven = !edits.is_empty();
            break;
        }
        if edits.len() >= 3 { break; }
        let candidates = suggestions(&compile.stdout, &changed);
        let next = candidates.into_iter().find(|candidate| {
            tried.insert((
                candidate.path.clone(), candidate.start, candidate.end,
                candidate.replacement.clone(),
            )) && file_under(&worktree.path, &candidate.path).is_some()
        });
        let Some(next) = next else { break };
        let Some(path) = file_under(&worktree.path, &next.path) else { break };
        let Ok(bytes) = fs::read(&path) else { break };
        let Ok(source) = std::str::from_utf8(&bytes) else { break };
        let Some(updated) = rewritten(source, &next) else { continue };
        originals.entry(next.path.clone()).or_insert(bytes.clone());
        if fs::write(&path, updated.as_bytes()).is_err() { break; }
        edits.push(json!({
            "path":next.path,
            "startByte":next.start,"endByte":next.end,
            "beforeSha256":digest(&bytes),
            "afterSha256":digest(updated.as_bytes()),
            "source":"rustc_MachineApplicable"
        }));
    }
    if proven {
        return Ok(Some(json!({
            "status":"compiler_verified",
            "origin":"rustc_machine_applicable_on_model_modified_source",
            "edits":edits,"compileCommand":"cargo check --all-targets",
            "qualityProven":false,"productionDeployed":false
        })));
    }
    // Revert *only* this recoverer's edits, not any preceding model change.
    for (relative, bytes) in originals {
        if let Some(path) = file_under(&worktree.path, &relative) {
            fs::write(path, bytes).map_err(|error| error.to_string())?;
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn diagnostic(path: &str, start: usize, end: usize, applicability: &str) -> Vec<u8> {
        json!({"reason":"compiler-message","message":{"level":"error",
            "spans":[],"children":[{"level":"help","spans":[{
                "file_name":path,"byte_start":start,"byte_end":end,
                "suggested_replacement":"*","suggestion_applicability":applicability
            }]}]}}).to_string().into_bytes()
    }
    #[test]
    fn uses_only_machine_applicable_suggestions_on_changed_source() {
        let changed = BTreeSet::from(["src/engine/decompose.rs".to_string()]);
        let accepted = suggestions(
            &diagnostic("src/engine/decompose.rs", 4, 4, "MachineApplicable"),
            &changed);
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].replacement, "*");
        assert!(suggestions(
            &diagnostic("src/engine/decompose.rs", 4, 4, "MaybeIncorrect"),
            &changed).is_empty());
        assert!(suggestions(
            &diagnostic("src/engine/unrelated.rs", 4, 4, "MachineApplicable"),
            &changed).is_empty());
        assert!(suggestions(
            &diagnostic("../Cargo.toml", 4, 4, "MachineApplicable"),
            &changed).is_empty());
    }
    #[test]
    fn byte_offsets_preserve_utf8_and_reject_invalid_boundaries() {
        let sample = "읽고 marker == \"고\"";
        let offset = sample.find("marker").unwrap();
        let suggestion = Suggestion {
            path:"src/engine/decompose.rs".into(),
            start:offset,end:offset,replacement:"*".into(),
        };
        assert_eq!(rewritten(sample,&suggestion).unwrap(),
            "읽고 *marker == \"고\"");
        assert!(rewritten(sample,&Suggestion {
            start:1,..suggestion.clone()
        }).is_none());
        assert!(rewritten(sample,&Suggestion {
            start:usize::MAX,..suggestion
        }).is_none());
    }
    #[test]
    fn disallows_symlink_escape_from_detached_workspace() {
        let root = std::env::temp_dir().join(format!(
            "dominion-rsi-compiler-repair-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/test.rs"),"fn ok() {}").unwrap();
        assert!(file_under(&root,"src/engine/test.rs").is_some());
        assert!(file_under(&root,"src/engine/../Cargo.toml").is_none());
        #[cfg(unix)] {
            std::os::unix::fs::symlink("/etc/hosts",
                root.join("src/engine/escape.rs")).unwrap();
            assert!(file_under(&root,"src/engine/escape.rs").is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
