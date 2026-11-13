//! Evidence-producing, isolated source worktree for model-driven DOMINION revisions.
//! A worktree protects the live checkout from ordinary workspace writes; it is
//! NOT a process/OS security sandbox. No candidate is automatically promoted.
use crate::engine::{jobs::JobContext, rsi, tools};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::{Path,PathBuf},time::Duration};
use tokio::process::Command;

#[path = "rsi/worktree/candidate.rs"]
mod candidate;
use candidate::*;
pub use candidate::{Worktree, prepare, restore_failed_candidate, finish,
    repair_feedback, compiler_repair_context};
pub(crate) use candidate::{unsafe_audit_for_patch, unsafe_audit_check};

#[path = "rsi/worktree/promotion.rs"]
mod promotion;
use promotion::*;
pub use promotion::{apply_to_sub, apply_campaign_to_sub};
pub(crate) use promotion::is_work_branch;

#[path = "rsi/worktree/evaluation.rs"]
mod evaluation;
use evaluation::*;
pub use evaluation::{compare, cleanup};
pub(crate) use evaluation::compare_at;


const GIT_LIMIT: Duration = Duration::from_secs(30);
const CHECK_LIMIT: Duration = Duration::from_secs(360);

/// launchd starts the engine with a minimal PATH that often omits
/// `~/.cargo/bin` and Homebrew. Resolve the tool explicitly so RSI can build
/// even when the service environment is narrower than the user's shell.
fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}

pub(super) fn cargo_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_CARGO") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".cargo/bin/cargo"));
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/cargo"),
        PathBuf::from("/usr/local/bin/cargo"),
        PathBuf::from("/usr/bin/cargo"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("cargo"))
}

pub(super) fn node_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_NODE") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".volta/bin/node"));
        let nvm = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm) {
            for entry in entries.flatten() {
                candidates.push(entry.path().join("bin/node"));
            }
        }
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        PathBuf::from("/usr/bin/node"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("node"))
}

pub(super) fn npm_executable() -> PathBuf {
    if let Some(explicit) = std::env::var_os("DOMINION_NPM") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(home) = &home {
        candidates.push(home.join(".volta/bin/npm"));
        candidates.push(home.join(".local/bin/npm"));
        let nvm = home.join(".nvm/versions/node");
        if let Ok(entries) = std::fs::read_dir(nvm) {
            for entry in entries.flatten() {
                candidates.push(entry.path().join("bin/npm"));
            }
        }
    }
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/npm"),
        PathBuf::from("/usr/local/bin/npm"),
        PathBuf::from("/usr/bin/npm"),
    ]);
    first_existing(candidates).unwrap_or_else(|| PathBuf::from("npm"))
}
// tools::run_command retains at most 80_004 bytes of stdout. Reject anything
// near that boundary rather than hashing/approving a silently truncated diff.
const MAX_CAPTURE_BYTES: usize = 79_000;

pub fn metric_cases()->Vec<Value>{
    [
        ("memory-quality","memory","memory_ranking",false),
        ("long-recall-quality","memory","long_owner_recall",true),
        ("planning-quality","planning","planning_quality",true),
        ("search-quality","improver","improvement_search",true),
        ("group-chat-input-cost","context","group_chat_context_efficiency",true),
    ].into_iter().map(|(id,domain,name,holdout)|json!({
        "id":id,"domain":domain,"holdout":holdout,"filter":format!("rsi_frozen_oracle::{name}"),
        "metrics":[
            {"key":"quality","direction":"higher","minRelativeGain":0.02,"minAbsoluteGain":0.025},
            {"key":"elapsedMs","direction":"lower","minRelativeGain":0.2,"minAbsoluteGain":1.0,"maxRelativeRegression":0.25}
        ]
    })).map(|mut case|{
        if case["id"]=="planning-quality" {
            // Independent planning evaluation contains 13 varied cases.
            // Require at least two newly correct examples (~2/13 gain),
            // not one overfitted benchmark phrase.
            case["metrics"][0]["minRelativeGain"]=json!(0.2);
            case["metrics"][0]["minAbsoluteGain"]=json!(0.14);
        }
        if case["id"]=="long-recall-quality"{
            case["metrics"].as_array_mut().unwrap().push(json!({"key":"contextChars","direction":"lower",
                "minRelativeGain":0.1,"minAbsoluteGain":20.0,"maxRelativeRegression":0.1}));
        }
        if case["id"]=="group-chat-input-cost"{
            case["metrics"]=json!([
                {"key":"quality","direction":"higher","minRelativeGain":0.02},
                {"key":"contextChars","direction":"lower","minRelativeGain":0.05,"minAbsoluteGain":50.0,"maxRelativeRegression":0.05}
            ]);
        }
        case
    }).collect()
}

/// Evidence from earlier autonomous source runs, reintroduced to the next
/// generation. A successful build is explicitly NOT behavioral improvement.
pub fn recent_feedback(state:&crate::state::AppState)->String{
    let mut runs=state.store.all("rsi_run");
    runs.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let mut lines=Vec::<String>::new();
    for run in runs.into_iter().take(5){
        let job_id=run["jobId"].as_str().unwrap_or("");
        let objective=state.store.get("job",job_id)
            .and_then(|job|job["input"]["rsiObjective"].as_str().map(str::to_string))
            .unwrap_or_else(||"목표 기록 없음".into());
        let checks_ok=run["checksPassed"]==true;
        let invalid=run["captureError"].as_str().unwrap_or("");
        let model_err=run["modelError"].as_str().unwrap_or("");
        let broken=run["checks"].as_array().map(|checks|checks.iter()
            .filter(|check|check["passed"]!=true)
            .filter_map(|check|check["label"].as_str())
            .take(3).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let result=if checks_ok{"정적 검사 통과(실제 성능 향상 미검증)"}
            else {"정적 검사 실패 또는 실행되지 않음"};
        let paired_verdict=run["paired"]["verdict"].as_str().unwrap_or("비교 안 함");
        let error=if !invalid.is_empty(){invalid}
            else if !model_err.is_empty(){model_err}
            else if !broken.is_empty(){broken.as_str()}else{"증거 없음"};
        lines.push(format!(
            "- 과거 목표: {}; 결과: {}; A/B 검증: {}; 오류: {}; 후보: {}",
            objective.chars().take(220).collect::<String>(),
            result,paired_verdict,error.chars().take(220).collect::<String>(),
            run["candidateId"].as_str().unwrap_or("없음"),
        ));
    }
    if lines.is_empty(){"과거 자기개선 실행 증거 없음".into()}
    else {lines.join("\n")}
}

#[cfg(test)]
#[path = "rsi/worktree/tests.rs"]
mod tests;
