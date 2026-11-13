//! Frozen-receipt-gated worktree adoption and successor directives.
//! Compiled RSI child module; external APIs are re-exported by rsi_evolve.
use super::*;

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || crate::engine::rsi::verify_paired_receipt(&context.state,candidate).is_err()
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

/// Derive an executable next-generation intent from observed outcomes.
/// Merely recording research is not progress: the following worker receives
/// a concrete implementation target, and repeated failed implementations
/// force a different hypothesis instead of another read-only research round.
pub(super) fn improvement_directive(history:&[Value]) -> Value {
    let Some(last)=history.last() else {
        return json!({"mode":"discover","sourceGenerationId":null,
            "reason":"no_recorded_generation","attemptedAgain":false});
    };
    let previous_mode=last["improvementDirective"]["mode"].as_str().unwrap_or("");
    let verdict=last["evaluation"]["verdict"].as_str().unwrap_or("");
    let has_patch=last["candidateId"].is_string()
        || last["attemptedCandidateId"].is_string();
    let new_findings=last["newFindings"].as_u64().unwrap_or(0);
    let mode=if last["accepted"]==true {
        "extend_verified_capability"
    } else if last["improvementActionObservation"]["status"]=="provider_interrupted" {
        // Transport failure is not evidence that the model chose to ignore
        // the patch instruction. Retry the same concrete action if possible,
        // under the existing user token/time/attempt limits.
        if matches!(previous_mode,
            "implement_research"|"change_approach"|"repair_counterexample"|"revise_candidate") {
            previous_mode
        } else if new_findings>0 {"implement_research"} else {"discover"}
    } else if last["improvementActionObservation"]["status"]=="not_followed"
        && !matches!(previous_mode,""|"discover"|"change_approach") {
        "change_approach"
    } else if matches!(verdict,"regressed"|"build_failed") {
        if previous_mode=="repair_counterexample" {"change_approach"}
        else {"repair_counterexample"}
    } else if has_patch {
        if matches!(previous_mode,"revise_candidate"|"repair_counterexample") {
            "change_approach"
        } else {
            "revise_candidate"
        }
    } else if new_findings>0 {
        if previous_mode=="implement_research" {
            "change_approach"
        } else {
            "implement_research"
        }
    } else {
        "change_approach"
    };
    let measured_failures=last["evaluation"]["metrics"]["measurements"].as_array()
        .into_iter().flatten()
        .filter(|measurement|measurement["holdout"]==true
            && (measurement["candidatePass"]==false
                || measurement["metrics"].as_array().is_some_and(|rows|
                    rows.iter().any(|row|row["regressed"]==true))))
        .filter_map(|row|row["id"].as_str())
        .take(8).collect::<Vec<_>>();
    json!({
        "mode":mode,
        "sourceGenerationId":last["id"],
        "sourceJobId":last["jobId"],
        "domain":last["domain"],
        "candidateId":last["candidateId"],
        "attemptedCandidateId":last["attemptedCandidateId"],
        "candidateCapture":last["candidateCapture"],
        "evidenceHashes":last["knowledgeArtifactHashes"].as_array().map(|rows|
            rows.iter().take(8).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "failureCaseIds":measured_failures,
        "previousVerdict":last["evaluation"]["verdict"],
        "previousError":last["error"].as_str().unwrap_or("").chars().take(600).collect::<String>(),
        "previousFocusFiles":last["focusFiles"].as_array().map(|rows|
            rows.iter().take(4).cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "attemptedAgain":matches!(previous_mode,"implement_research"|"revise_candidate"|"repair_counterexample"),
        "newFindings":new_findings
    })
}

/// A concrete owner-selected repair already has an executable source target.
/// Spend the first real-model generation editing it, not rediscovering the
/// same file; later generations still follow observed success/failure.
pub(super) fn generation_improvement_directive(input:&Value,history:&[Value])->Value{
    if history.is_empty() && input["rsiAutonomous"]==false
        && input["rsiFocusFiles"].as_array().is_some_and(|paths|!paths.is_empty())
    {
        let goal=input["rsiObjective"].as_str().unwrap_or("").to_lowercase();
        if ["fix","repair","modify","implement","patch","고쳐","고치","수정","구현","교정","개선"]
            .iter().any(|verb|goal.contains(*verb))
        {
            return json!({"mode":"implement_research",
                "sourceGenerationId":null,
                "reason":"explicit_owner_patch_request_with_source",
                "attemptedAgain":false});
        }
    }
    improvement_directive(history)
}

/// Preserve chronological ordering across new millisecond RFC3339 records
/// and older SQLite-created timestamps (YYYY-MM-DD HH:MM:SS, UTC).
pub(super) fn generation_completion_millis(row:&Value)->i64 {
    row["completedAt"].as_str()
        .and_then(|text|chrono::DateTime::parse_from_rfc3339(text).ok())
        .map(|date|date.timestamp_millis())
        .or_else(||row["createdAt"].as_str().and_then(|text|
            chrono::NaiveDateTime::parse_from_str(text,"%Y-%m-%d %H:%M:%S")
                .ok().map(|date|date.and_utc().timestamp_millis())))
        .unwrap_or(i64::MIN)
}

