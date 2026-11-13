//! Candidate adoption boundary and failed-model lineage recovery.
use super::{
    Digest, Value, JobContext, Path, Sha256,
    rsi, git,
};

pub(super) fn retry_learned_model_failure(
    error:&str, generation:&Value, has_method:bool, recovery_count:u64,
    remaining_seconds:u64, attempts:u64, maximum_attempts:u64,
)->bool {
    let timed_out=error.contains("시간 제한")
        || error.contains("timed out")
        || error.contains("timeout");
    let failed_edit=generation["actionObservation"]["failedEditCalls"]
        .as_u64().unwrap_or(0)>0
        || generation["editActions"].as_array().is_some_and(|rows|
            rows.iter().any(|row|row["status"]=="failed" && row["written"]!=true));
    timed_out && failed_edit && has_method && recovery_count<1
        && remaining_seconds>=120 && attempts<maximum_attempts
}

/// Empty strings are not model errors; a research-only generation sets the
/// field to "" and must not abort the whole campaign.
pub(super) fn generation_model_error(current: &Value) -> Option<&str> {
    current["result"]["rsi"]["modelError"]
        .as_str()
        .filter(|error| !error.trim().is_empty())
}

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["baseCommit"] != base
    {
        return Err("독립 평가를 통과한 현재 세대 후보만 실험 계보에 채택합니다.".into());
    }
    if git(context, lab, &["rev-parse", "HEAD"]).await? != base
        || !git(context, lab, &["status", "--porcelain"])
            .await?
            .is_empty()
    {
        return Err("실험 기준 소스가 변경됐습니다.".into());
    }
    let patch = candidate["patch"].as_str().ok_or("패치 없음")?;
    if format!("{:x}", Sha256::digest(patch.as_bytes()))
        != candidate["patchSha256"].as_str().unwrap_or("")
    {
        return Err("채택 패치 해시 불일치".into());
    }
    let file = lab
        .parent()
        .ok_or("실험 경로 오류")?
        .join(format!("{}.patch", uuid::Uuid::new_v4()));
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let applied = git(
        context,
        lab,
        &["apply", file.to_str().ok_or("패치 경로 오류")?],
    )
    .await;
    let _ = std::fs::remove_file(file);
    applied?;
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: adopt independently measured experimental candidate",
        ],
    )
    .await?;
    git(context, lab, &["rev-parse", "HEAD"]).await
}

// Distinguish completed source-edit tool calls from model text and proposed
// candidates. A tool attempt alone does not prove an independently measured gain.
