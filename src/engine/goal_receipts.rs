//! Immutable, bounded owner goal execution receipts for autonomous development.
 //! Evidence is attributed to a real owner session, not a benchmark reward.
use crate::engine::{jobs::JobContext,verification};
use serde_json::{json,Value};

pub fn record(ctx:&JobContext,id:&str,status:&str,goal:&Value,gaps:&[String])->Value{
    let receipt_id=format!("goal-receipt-{id}");
    if let Some(previous)=ctx.state.store.get("goal_receipt",&receipt_id){
        return previous;
    }
    let session_id=goal["sessionId"].as_str().unwrap_or("");
    let operations=ctx.state.store.all("execution_operation").into_iter()
        .filter(|op|op["sessionId"].as_str()==Some(session_id))
        .collect::<Vec<_>>();
    let success=operations.iter().filter(|op|verification::successful_evidence(op)).count();
    let actions=operations.iter().filter(|op|verification::is_action_evidence(op)).count();
    let runs=ctx.state.store.all("execution").into_iter()
        .filter(|row|row["sessionId"].as_str()==Some(session_id))
        .collect::<Vec<_>>();
    let tokens=runs.iter().map(|row|row["usage"]["totalTokens"].as_u64()
        .or_else(||row["totalTokens"].as_u64()).unwrap_or(0)).sum::<u64>();
    let files=goal["artifacts"].as_array().map(Vec::len).unwrap_or(0);
    let check_count=goal["acceptanceChecks"].as_array().map(Vec::len).unwrap_or(0);
    let reviewer_count=goal["reviewHistory"].as_array().map(Vec::len).unwrap_or(0);
    let accepted=status=="completed" && gaps.is_empty();
    let record=json!({
        "id":receipt_id,"goalId":id,"jobId":ctx.job_id,"taskId":goal["taskId"],
        "status":status,"sessionId":goal["sessionId"],
        "verifiedCompletion":accepted,
        "verificationSource":"independent_reviewer_and_deterministic_acceptance",
        "acceptanceCriteriaCount":check_count,
        "successfulEvidenceCount":success,"actionEvidenceCount":actions,
        "artifactCount":files,"iterations":goal["iterations"],
        "replans":goal["replans"],"reviewerDecisions":reviewer_count,
        "recordedSessionTokens":tokens,
        "workspaceMode":if goal["isolateWorktree"]==true{
            "linked_git_worktree"
        }else{"shared_checkout"},
        "workspaceBaseCommit":goal["gitWorktree"]["baseCommit"],
        "source":"owner_goal","qualityProven":false,
        "independentPairedEvaluator":false,
        "canRewardFrozenRsiEvaluator":false,
        "at":chrono::Utc::now().to_rfc3339()
    });
    let saved=ctx.state.store.put_if_absent("goal_receipt",&record)
        .unwrap_or(record);
    ctx.state.bus.record("goal_receipt",&saved);
    saved
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn receipt_proof_categories_are_not_benchmark_rewards(){
        let sample=json!({"verificationSource":"independent_reviewer_and_deterministic_acceptance",
            "independentPairedEvaluator":false,"canRewardFrozenRsiEvaluator":false});
        assert_ne!(sample["verificationSource"],"paired_rsi_benchmark");
        assert_eq!(sample["canRewardFrozenRsiEvaluator"],false);
    }
}
