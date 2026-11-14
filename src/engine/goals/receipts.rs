//! Independent, immutable per-attempt proof receipt for ordinary autonomous goals.
//! User goals are observations, never rewards in the frozen RSI evaluator.
use crate::engine::{jobs::JobContext,verification};
use serde_json::{json,Value};

pub(super) fn record(ctx:&JobContext,id:&str,status:&str,goal:&Value,evidence:&[Value],
    deliverables:&Value)->Value{
    let receipt_id=format!("goal-receipt:{id}:{}",ctx.job_id);
    if let Some(existing)=ctx.state.store.get("goal_receipt",&receipt_id){return existing;}
    let session=goal["sessionId"].as_str().unwrap_or("");
    let usage=if session.is_empty(){Value::Null}else{ctx.state.store.session_usage(session)};
    let action_count=evidence.iter().filter(|item|verification::is_action_evidence(item)).count();
    let criteria_count=goal["acceptanceChecks"].as_array().map(Vec::len).unwrap_or(0);
    let artifact_count=deliverables["files"].as_array().map(Vec::len).unwrap_or(0);
    let accepted=status=="completed" && goal["gaps"].as_array().is_none_or(Vec::is_empty);
    let progress=goal["progressHistory"].as_array();
    let progress_window=progress.map(Vec::len).unwrap_or(0);
    let productive=progress.map(|rows|rows.iter().filter(|row|row["advanced"]==true).count()).unwrap_or(0);
    let cleared=progress.map(|rows|rows.iter().map(|row|
        row["clearedAcceptanceGaps"].as_u64().unwrap_or(0)).sum::<u64>()).unwrap_or(0);
    let record=json!({
        "id":receipt_id,"goalId":id,"jobId":ctx.job_id,"taskId":goal["taskId"],
        "sessionId":goal["sessionId"],"status":status,
        "verifiedCompletion":accepted,
        "verificationSource":"reviewer_plus_deterministic_acceptance",
        "acceptanceCriteriaCount":criteria_count,
        "successfulEvidenceCount":evidence.len(),"actionEvidenceCount":action_count,
        "artifactCount":artifact_count,
        "iterations":goal["iterations"],"replans":goal["replans"],
        "progressWindowTurns":progress_window,
        "productiveTurnsWindow":productive,
        "unproductiveTurnsWindow":progress_window.saturating_sub(productive),
        "resolvedChecksWindow":cleared,
        "maxExecutorTokens":goal["maxExecutorTokens"],
        "workspaceMode":if goal["isolateWorktree"]==true{"detached_git_worktree"}else{"shared_workspace"},
        "workspaceBaseCommit":goal["gitWorktree"]["baseCommit"],
        "reviewerCallsAvoided":goal["reviewHistory"].as_array()
            .map(|rows|rows.iter().filter(|row|row["skippedForDeterministicGap"]==true).count())
            .unwrap_or(0),
        "planScore":goal["planScore"],"usage":usage,
        "usageIncludesGoalHelperCalls":true,
        "providerBillingVerified":false,
        "usageCaveat":"Provider usage may be estimated or absent. Paid KRW and local compute power are not independently measured.",
        "start":goal["startedAt"],
        "at":chrono::Utc::now().to_rfc3339(),
        "isIndependentBenchmark":false,
        "canRewardFrozenRsiEvaluator":false
    });
    ctx.state.store.put_if_absent("goal_receipt",&record).unwrap_or(record)
}
