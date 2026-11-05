//! Controlled self-evolution of the Rust harness itself.
//!
//! A proposal is a unified diff against this repository. The engine never
//! edits the running tree directly: it creates an isolated git worktree,
//! applies the patch, runs the real build/test commands, records the result,
//! and only then may create a dedicated promotion branch. The running binary
//! and `main` are never modified by this module.
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const ALLOWED_PREFIXES: &[&str] = &["src/", "tests/", "web/src/", "Cargo.toml"];
const MAX_PATCH_BYTES: usize = 300_000;

fn repo_root() -> Option<PathBuf> {
    let configured = std::env::var("DOMINION_SOURCE_WORKSPACE")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.join(".git").exists());
    if let Some(path) = configured {
        return Some(path);
    }
    let current = std::env::current_dir().ok()?;
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&current)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn cargo_bin() -> String {
    if let Ok(path) = std::env::var("CARGO") {
        if !path.trim().is_empty() {
            return path;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".cargo/bin/cargo");
        if candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    "cargo".into()
}

fn validate_patch(patch: &str) -> Result<(), String> {
    if patch.trim().is_empty() {
        return Err("patch is empty".into());
    }
    if patch.len() > MAX_PATCH_BYTES {
        return Err("patch is too large".into());
    }
    if patch.contains("GIT binary patch") {
        return Err("binary patches are rejected".into());
    }
    if !patch.contains("@@") {
        return Err("patch has no hunks".into());
    }
    let mut files = Vec::new();
    for line in patch.lines() {
        let path = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
            .or_else(|| line.strip_prefix("diff --git a/").and_then(|rest| rest.split(" b/").next()));
        if let Some(path) = path {
            let path = path.trim();
            if path.is_empty() || path == "/dev/null" {
                continue;
            }
            if !ALLOWED_PREFIXES
                .iter()
                .any(|prefix| path == *prefix || path.starts_with(prefix))
            {
                return Err(format!("patch touches a disallowed path: {path}"));
            }
            files.push(path.to_string());
        }
        if line.starts_with("rename from ") || line.starts_with("rename to ") {
            return Err("rename patches are rejected".into());
        }
        if line.starts_with("old mode ") || line.starts_with("new mode ") {
            return Err("mode-change patches are rejected".into());
        }
        if line.contains("../") {
            return Err("patch contains a path traversal".into());
        }
    }
    if files.is_empty() {
        return Err("patch names no allowed files".into());
    }
    Ok(())
}

fn current_branch(repo: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn head(repo: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

async fn run(
    program: &str,
    args: &[&str],
    dir: &Path,
    extra_env: &[(&str, String)],
    timeout_secs: u64,
) -> (bool, String) {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("CI", "1");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = tokio::time::timeout(Duration::from_secs(timeout_secs), command.output()).await;
    match output {
        Ok(Ok(output)) => {
            let mut log = String::from_utf8_lossy(&output.stdout).to_string();
            log.push_str(&String::from_utf8_lossy(&output.stderr));
            let tail = log
                .lines()
                .rev()
                .take(120)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            (output.status.success(), tail)
        }
        Ok(Err(error)) => (false, error.to_string()),
        Err(_) => (false, format!("{program} timed out after {timeout_secs}s")),
    }
}

fn test_counts(log: &str) -> (u64, u64) {
    let mut passed = 0;
    let mut failed = 0;
    for line in log.lines() {
        let line = line.trim();
        let rest = if let Some(rest) = line.strip_prefix("test result: ok. ") {
            rest
        } else if let Some(rest) = line.strip_prefix("test result: FAILED. ") {
            rest
        } else {
            continue;
        };
        let parts = rest.split(';').collect::<Vec<_>>();
        if let Some(count) = parts.first().and_then(|part| part.split_whitespace().next()) {
            passed += count.parse::<u64>().unwrap_or(0);
        }
        if let Some(count) = parts.get(1).and_then(|part| part.split_whitespace().next()) {
            failed += count.parse::<u64>().unwrap_or(0);
        }
    }
    (passed, failed)
}

pub fn list(state: &AppState) -> Value {
    let mut items = state.store.all("harness_proposal");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
    });
    json!({
        "proposals": items,
        "sourceRoot": repo_root().map(|path| path.to_string_lossy().to_string()),
        "selfUpdateEnabled": std::env::var("DOMINION_ALLOW_SELF_UPDATE").as_deref() == Ok("1"),
    })
}

pub fn propose(state: &AppState, body: &Value) -> Result<Value, String> {
    let patch = body["patch"].as_str().unwrap_or("");
    validate_patch(patch)?;
    let reason = body["reason"].as_str().unwrap_or("").trim();
    if reason.is_empty() {
        return Err("proposal.reason is required".into());
    }
    let repo = repo_root().ok_or("source repository is not available")?;
    let id = format!(
        "harness-proposal-{}",
        crate::engine::memory::stable_id(&[reason, patch])
    );
    if let Some(existing) = state.store.get("harness_proposal", &id) {
        return Ok(existing);
    }
    let record = json!({
        "id": id,
        "reason": reason,
        "patch": patch,
        "status": "proposed",
        "baseRevision": head(&repo),
        "baseBranch": current_branch(&repo),
        "evidenceIds": body["evidenceIds"].clone(),
        "predictedBenefit": body["predictedBenefit"].clone(),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    state
        .store
        .put("harness_proposal", &record)
        .ok_or_else(|| "proposal could not be stored".into())
        .map(|saved| {
            state.bus.record("harness_proposal", &saved);
            state.store.audit(
                "harness.proposed",
                json!({"id": saved["id"], "baseRevision": saved["baseRevision"]}),
            );
            saved
        })
}

/// Builds and tests the candidate in an isolated worktree. The running tree is
/// untouched; a failed candidate keeps its logs for the next proposal.
pub async fn validate(state: &AppState, id: &str) -> Result<Value, String> {
    let Some(mut proposal) = state.store.get("harness_proposal", id) else {
        return Err("proposal not found".into());
    };
    if !matches!(
        proposal["status"].as_str(),
        Some("proposed" | "rejected" | "validated")
    ) {
        return Err(format!("cannot validate status {}", proposal["status"]));
    }
    let repo = repo_root().ok_or("source repository is not available")?;
    let base = head(&repo);
    if proposal["baseRevision"].as_str() != Some(base.as_str()) {
        return Err("source HEAD changed; re-propose against the current revision".into());
    }
    if current_branch(&repo) == "main" {
        return Err("self-evolution refuses to run on main".into());
    }
    validate_patch(proposal["patch"].as_str().unwrap_or(""))?;

    let worktree = state
        .config
        .data_dir
        .join("harness-evolution")
        .join(id)
        .join("worktree");
    let _ = std::fs::remove_dir_all(&worktree);
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    proposal["status"] = json!("validating");
    proposal["validatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("harness_proposal", &proposal);
    state.bus.record("harness_proposal", &proposal);

    let (added, log) = run(
        "git",
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_string_lossy().as_ref(),
            &base,
        ],
        &repo,
        &[],
        120,
    )
    .await;
    if !added {
        proposal["status"] = json!("rejected");
        proposal["failure"] = json!(log);
        state.store.put("harness_proposal", &proposal);
        return Ok(proposal);
    }

    let patch_path = worktree.join(".dominion-proposal.patch");
    std::fs::write(&patch_path, proposal["patch"].as_str().unwrap_or(""))
        .map_err(|error| error.to_string())?;
    let (applied, apply_log) = run(
        "git",
        &["apply", "--whitespace=nowarn", ".dominion-proposal.patch"],
        &worktree,
        &[],
        120,
    )
    .await;
    let _ = std::fs::remove_file(&patch_path);
    if !applied {
        let _ = run(
            "git",
            &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
            &repo,
            &[],
            120,
        )
        .await;
        proposal["status"] = json!("rejected");
        proposal["failure"] = json!(apply_log);
        state.store.put("harness_proposal", &proposal);
        state.bus.record("harness_proposal", &proposal);
        return Ok(proposal);
    }

    let target_dir = repo.join("target").to_string_lossy().to_string();
    let cargo = cargo_bin();
    proposal["logs"] = json!([]);
    let (built, build_log) = run(
        &cargo,
        &["build", "--release"],
        &worktree,
        &[("CARGO_TARGET_DIR", target_dir.clone())],
        1800,
    )
    .await;
    let mut logs = vec![format!("$ cargo build --release\n{build_log}")];
    if !built {
        let _ = run(
            "git",
            &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
            &repo,
            &[],
            120,
        )
        .await;
        proposal["status"] = json!("rejected");
        proposal["failure"] = json!("candidate build failed");
        proposal["logs"] = json!(logs);
        state.store.put("harness_proposal", &proposal);
        state.bus.record("harness_proposal", &proposal);
        return Ok(proposal);
    }

    let (tested, test_log) = run(
        &cargo,
        &["test", "--release"],
        &worktree,
        &[("CARGO_TARGET_DIR", target_dir)],
        2400,
    )
    .await;
    logs.push(format!("$ cargo test --release\n{test_log}"));
    let (passed, failed) = test_counts(&test_log);
    proposal["candidateTests"] = json!({"passed": passed, "failed": failed});
    if !tested || failed > 0 {
        proposal["status"] = json!("rejected");
        proposal["failure"] = json!("candidate tests failed");
        proposal["logs"] = json!(logs);
    } else {
        proposal["status"] = json!("validated");
        proposal["logs"] = json!(logs);
        proposal["validation"] = json!({
            "build": "passed",
            "tests": "passed",
            "passedCount": passed,
            "failedCount": failed,
            "isolatedWorktree": worktree.to_string_lossy(),
            "baselineRevision": base,
        });
    }
    let _ = run(
        "git",
        &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
        &repo,
        &[],
        120,
    )
    .await;
    let saved = state
        .store
        .put("harness_proposal", &proposal)
        .ok_or("proposal could not be saved")?;
    state.bus.record("harness_proposal", &saved);
    state.store.audit(
        "harness.validated",
        json!({"id": id, "status": saved["status"], "tests": saved["candidateTests"]}),
    );
    Ok(saved)
}

/// Promotes a validated patch onto a dedicated branch. `main` and the running
/// binary are never touched; the caller decides when to build/restart.
pub async fn promote(state: &AppState, id: &str) -> Result<Value, String> {
    if std::env::var("DOMINION_ALLOW_SELF_UPDATE").as_deref() != Ok("1") {
        return Err("self-update is disabled (DOMINION_ALLOW_SELF_UPDATE=1 required)".into());
    }
    let Some(mut proposal) = state.store.get("harness_proposal", id) else {
        return Err("proposal not found".into());
    };
    if proposal["status"].as_str() != Some("validated") {
        return Err("only a validated proposal can be promoted".into());
    }
    let repo = repo_root().ok_or("source repository is not available")?;
    if current_branch(&repo) == "main" {
        return Err("self-evolution refuses to promote on main".into());
    }
    let base = head(&repo);
    if proposal["baseRevision"].as_str() != Some(base.as_str()) {
        return Err("source HEAD changed after validation; revalidate".into());
    }
    let branch = format!("autoevolve/{}", id);
    let worktree = state
        .config
        .data_dir
        .join("harness-evolution")
        .join(id)
        .join("promotion");
    let _ = std::fs::remove_dir_all(&worktree);
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let (created, log) = run(
        "git",
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_string_lossy().as_ref(),
            &base,
        ],
        &repo,
        &[],
        120,
    )
    .await;
    if !created {
        return Err(format!("could not create promotion worktree: {log}"));
    }
    let patch_path = worktree.join(".dominion-proposal.patch");
    std::fs::write(&patch_path, proposal["patch"].as_str().unwrap_or(""))
        .map_err(|error| error.to_string())?;
    let (applied, apply_log) = run(
        "git",
        &["apply", "--whitespace=nowarn", ".dominion-proposal.patch"],
        &worktree,
        &[],
        120,
    )
    .await;
    let _ = std::fs::remove_file(&patch_path);
    if !applied {
        let _ = run(
            "git",
            &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
            &repo,
            &[],
            120,
        )
        .await;
        return Err(format!("promotion patch apply failed: {apply_log}"));
    }
    let message = format!(
        "autoevolve: {}\n\nproposal: {}\nbase: {}",
        proposal["reason"].as_str().unwrap_or("verified harness change"),
        id,
        base
    );
    let (staged, stage_log) = run("git", &["add", "-A"], &worktree, &[], 120).await;
    if !staged {
        let _ = run(
            "git",
            &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
            &repo,
            &[],
            120,
        )
        .await;
        return Err(format!("promotion staging failed: {stage_log}"));
    }
    let (committed, commit_log) = run(
        "git",
        &["commit", "-m", &message],
        &worktree,
        &[],
        120,
    )
    .await;
    let _ = run(
        "git",
        &["worktree", "remove", "--force", worktree.to_string_lossy().as_ref()],
        &repo,
        &[],
        120,
    )
    .await;
    if !committed {
        return Err(format!("promotion commit failed: {commit_log}"));
    }
    let commit = std::process::Command::new("git")
        .args(["rev-parse", &branch])
        .current_dir(&repo)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    proposal["status"] = json!("promoted");
    proposal["promotion"] = json!({
        "branch": branch,
        "commit": commit,
        "baseRevision": base,
        "requiresRestart": true,
        "mainUntouched": true,
    });
    proposal["promotedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state
        .store
        .put("harness_proposal", &proposal)
        .ok_or("proposal could not be saved")?;
    state.bus.record("harness_proposal", &saved);
    state.store.audit(
        "harness.promoted",
        json!({"id": id, "branch": saved["promotion"]["branch"], "commit": saved["promotion"]["commit"]}),
    );
    Ok(saved)
}

pub fn rollback(state: &AppState, id: &str) -> Result<Value, String> {
    let Some(mut proposal) = state.store.get("harness_proposal", id) else {
        return Err("proposal not found".into());
    };
    let repo = repo_root().ok_or("source repository is not available")?;
    if let Some(branch) = proposal["promotion"]["branch"].as_str() {
        let _ = std::process::Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(&repo)
            .output();
    }
    proposal["status"] = json!("rolled_back");
    proposal["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state
        .store
        .put("harness_proposal", &proposal)
        .ok_or("proposal could not be saved")?;
    state.bus.record("harness_proposal", &saved);
    state.store.audit("harness.rolled_back", json!({"id": id}));
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-evolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn patch(path: &str) -> String {
        format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,1 +1,1 @@\n-old\n+new\n"
        )
    }

    #[test]
    fn patch_validation_allows_only_harness_source_paths() {
        assert!(validate_patch(&patch("src/engine/program.rs")).is_ok());
        assert!(validate_patch(&patch("web/src/pages/Chat.tsx")).is_ok());
        assert!(validate_patch(&patch("tests/e2e.rs")).is_ok());
        assert!(validate_patch(&patch("scripts/deploy.sh")).is_err());
        assert!(validate_patch(&patch("../outside.rs")).is_err());
        assert!(validate_patch("not a patch").is_err());
        assert!(validate_patch("GIT binary patch\n@@ -1 +1 @@").is_err());
    }

    #[test]
    fn proposal_requires_reason_and_valid_patch() {
        let (state, dir) = state();
        assert!(propose(&state, &json!({"patch": patch("src/engine/program.rs")})).is_err());
        assert!(propose(
            &state,
            &json!({"reason":"improve", "patch": patch("scripts/x.sh")})
        )
        .is_err());
        let saved = propose(
            &state,
            &json!({"reason":"improve program", "patch": patch("src/engine/program.rs")}),
        )
        .unwrap();
        assert_eq!(saved["status"], "proposed");
        assert_eq!(list(&state)["proposals"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn promotion_requires_the_explicit_self_update_flag() {
        let (state, dir) = state();
        std::env::remove_var("DOMINION_ALLOW_SELF_UPDATE");
        let saved = propose(
            &state,
            &json!({"reason":"improve program", "patch": patch("src/engine/program.rs")}),
        )
        .unwrap();
        let error = promote(&state, saved["id"].as_str().unwrap()).await.unwrap_err();
        assert!(error.contains("DOMINION_ALLOW_SELF_UPDATE"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
