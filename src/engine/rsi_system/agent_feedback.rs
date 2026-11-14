//! Ordinary owner-goal outcomes as research hypotheses, never frozen RSI rewards.
//! The descendant sees bounded aggregate failures on the actual agent harness.
use crate::state::AppState;
use serde_json::{json,Value};

fn aggregate(rows:&[Value])->Value{
    let total=rows.len();
    let completed=rows.iter().filter(|item|
        item["status"]=="completed" && item["verifiedCompletion"]==true).count();
    let unresolved=rows.iter().filter(|item|
        matches!(item["status"].as_str(),Some("failed"|"needs_review"|"cancelled"))).count();
    let skipped=rows.iter().map(|item|item["reviewerCallsAvoided"].as_u64().unwrap_or(0)).sum::<u64>();
    let attempts=rows.iter().map(|item|item["iterations"].as_u64().unwrap_or(0)).sum::<u64>();
    let executor_tokens=rows.iter().map(|item|
        item["usage"]["totalTokens"].as_u64().unwrap_or(0)).sum::<u64>();
    json!({"source":"owner_goal_receipts","sampleCount":total,
        "completedAttempts":completed,"unresolvedAttempts":unresolved,
        "totalIterations":attempts,"recordedSessionTokens":executor_tokens,
        "reviewerCallsAvoided":skipped,
        "qualityProven":false,"independentPairedEvaluator":false,
        "purpose":"candidate prioritization and descendant source inspection only"})
}
pub(super) fn observe(state:&AppState)->Value{
    let mut rows=state.store.all("goal_receipt");
    rows.sort_by(|a,b|b["at"].as_str().cmp(&a["at"].as_str()));
    rows.truncate(64);
    aggregate(&rows)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn normal_task_observations_cannot_be_promoted_to_frozen_benchmark_gains(){
        let summary=aggregate(&[json!({"status":"completed","verifiedCompletion":true,
            "iterations":2,"reviewerCallsAvoided":1,"usage":{"totalTokens":500}}),
            json!({"status":"needs_review","verifiedCompletion":false,"iterations":3})]);
        assert_eq!(summary["sampleCount"],2);
        assert_eq!(summary["completedAttempts"],1);
        assert_eq!(summary["unresolvedAttempts"],1);
        assert_eq!(summary["qualityProven"],false);
        assert_eq!(summary["independentPairedEvaluator"],false);
    }
}
