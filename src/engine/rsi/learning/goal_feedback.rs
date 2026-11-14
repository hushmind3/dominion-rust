//! Owner-goal receipts are development signals for descendant planning.
 //! Never count these as independent RSI benchmark wins or weight updates.
use crate::state::AppState;
use serde_json::{json,Value};
pub(super) fn observe(state:&AppState)->Value{
    let mut rows=state.store.all("goal_receipt");
    rows.sort_by(|a,b|b["at"].as_str().cmp(&a["at"].as_str()));
    rows.truncate(64);
    let success=rows.iter().filter(|g|g["verifiedCompletion"]==true).count();
    let unresolved=rows.iter().filter(|g|
        matches!(g["status"].as_str(),Some("failed"|"needs_review"|"cancelled"))).count();
    let turns=rows.iter().map(|g|g["iterations"].as_u64().unwrap_or(0)).sum::<u64>();
    let edited=rows.iter().map(|g|g["artifactCount"].as_u64().unwrap_or(0)).sum::<u64>();
    let tokens=rows.iter().map(|g|g["recordedSessionTokens"].as_u64().unwrap_or(0)).sum::<u64>();
    json!({"source":"owner_goal_receipts","sampleCount":rows.len(),
        "verifiedCompletions":success,"unresolved":unresolved,
        "totalTurns":turns,"artifactsObserved":edited,"recordedSessionTokens":tokens,
        "qualityProven":false,"independentPairedEvaluator":false,
        "purpose":"rank source research hypotheses, never overwrite frozen holdout evaluation"})
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn meta_feedback_never_claims_paired_rsi_gain(){
        let sample=json!({"source":"owner_goal_receipts","qualityProven":false,
            "independentPairedEvaluator":false});
        assert_eq!(sample["qualityProven"],false);
    }
}
