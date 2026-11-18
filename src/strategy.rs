//! Evidence-grounded, non-weight strategy transfer across RSI and agent runs.
//! Persisted Run/Improvement records remain the source of truth: no duplicate
//! index can falsely promote a candidate that was never adopted.
use crate::{
    domain::{Run, RunStatus},
};
use serde::Serialize;
use uuid::Uuid;

mod transfer;
pub use transfer::{RecoveryProof,rank,rank_for_run,prompt as transfer_prompt};

#[derive(Clone, Debug, Serialize)]
pub struct StrategyEvidence {
    pub source_id: Uuid,
    pub source: String,
    pub repository: String,
    pub goal: String,
    pub principle: String,
    pub when_to_use: String,
    pub failure_mode: String,
    pub files: Vec<String>,
    pub outcome: String,
    pub baseline_score: Option<f64>,
    pub resulting_score: Option<f64>,
    pub attempts: usize,
    pub cost_krw: f64,
    pub updated_at_ms: u64,
    /// Linked failure, repaired resource and subsequent auditor + postflight.
    #[serde(skip_serializing_if="Option::is_none")]
    pub recovery:Option<RecoveryProof>,
}

fn bounded(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

/// A strategy is eligible only after an actual failure receipt, an
/// independently observed same-file repair, a read postflight and PASS
/// recorded after the effect. A model-written plan is not evidence, and
/// completed without receipts is not eligible for cross-run transfer.
pub(crate) fn from_run(run: &Run) -> Option<StrategyEvidence> {
    if !run.strategy_learning || run.status != RunStatus::Completed {
        return None;
    }
    let repository=run.repository.as_ref()?;
    let proof=transfer::recovery_proof(run)?;
    let principle=format!(
        "Observed {} after {}, corrected the same workspace file,          and performed a read postflight with {} evidence.",
        proof.failure_kind,proof.failed_tool,proof.verification_basis);
    Some(StrategyEvidence {
        source_id:run.id,source:"trace_recovery_2026".into(),
        repository:repository.clone(),
        goal:bounded(&run.instruction,400),
        principle:bounded(&principle,420),
        when_to_use:format!(
            "Only the same repository, failed tool, failure family and exact              resource; source verification at event seq {}",proof.verification_seq),
        failure_mode:format!(
            "Prior failure {} at event seq {}, call {}",
            proof.failure_kind,proof.failure_seq,proof.failed_call_id),
        files:vec![proof.affected_path.clone()],
        outcome:if proof.verification_basis=="deterministic_exact_artifact"{
            "artifact_verified".into()
        }else{"verified".into()},
        baseline_score:None,resulting_score:None,
        attempts:run.steps,cost_krw:run.cost_krw,
        updated_at_ms:run.updated_at_ms,recovery:Some(proof)
    })
}

/// Bounded lower-trust contextual evidence; never a system instruction.
pub fn guidance(records:&[StrategyEvidence]) -> String {
    records.iter().map(|e|format!(
        "[{}:{}] {}. Observed={} attempts={} score={:?}->{:?}; when={}; caution={}",
        e.source,e.source_id,bounded(&e.principle,250),e.outcome,
        e.attempts,e.baseline_score,e.resulting_score,
        bounded(&e.when_to_use,150),bounded(&e.failure_mode,170)
    )).collect::<Vec<_>>().join("\n").chars().take(1800).collect()
}

/// A completed candidate is comparable only when its own isolated Run has
/// independent verification AND at least one actual successful tool receipt.
/// An LLM's chosen plan, narrative or optimistic score is never the outcome.
pub(crate) fn trial_report(parent:&Run,children:&[Run])->Option<serde_json::Value>{
    use serde_json::{json,Value};
    let launch=parent.events.iter().rev().find(|e|e.kind=="strategy_trials_launched")?;
    if parent.events.iter().any(|e|e.kind=="strategy_trials_evaluated"
        && e.detail.as_ref().is_some_and(|d|d["launch_seq"]==launch.seq)){return None;}
    let candidates=launch.detail.as_ref()?.get("candidates")?.as_array()?;
    if candidates.len()!=2{return None;}
    let mut outcomes=Vec::new();
    for candidate in candidates{
        let run_id=candidate["run_id"].as_str()
            .and_then(|s|Uuid::parse_str(s).ok())?;
        let child=children.iter().find(|r|r.id==run_id&&r.parent_run_id==Some(parent.id))?;
        if !child.terminal()||child.in_flight.is_some()||child.waiting_for_children
            ||!parent.settled_child_ids.contains(&run_id) {
            // A completed-looking child may still be actively finalizing its
            // own receipts. Never compare before its reserved bill settles.
            return None;
        }
        // The *last* verdict must pass, after the last modified artifact.
        // A successful read is not evidence that a proposed code fix exists.
        let verdict=child.events.iter().rev()
            .find(|e|e.kind=="verification");
        let last_write=child.events.iter().rev().find(|e|
            e.kind=="tool_result" &&e.detail.as_ref().is_some_and(|v|
                matches!(v["name"].as_str(),Some("write_file"|"edit_file"))));
        let verified=child.status==RunStatus::Completed
            &&verdict.is_some_and(|e|crate::verification::strict_pass(&e.message));
        let artifact_changed=last_write.is_some_and(|e|verdict.is_some_and(|v|v.seq>e.seq));
        let successful_tools=child.events.iter().filter(|e|e.kind=="tool_result"
            ||(e.kind=="tool_reconciled"
                &&e.detail.as_ref().is_some_and(|v|v["succeeded"]==true))).count();
        let independent=child.strategy_candidate
            &&child.repository==parent.repository
            &&child.base_commit==parent.base_commit
            &&child.provider_id==parent.provider_id
            &&child.model_name==parent.model_name;
        let actual_cost=child.cost_krw+child.delegated_spent_krw;
        let eligible=verified&&artifact_changed&&independent&&successful_tools>0
            &&actual_cost.is_finite()&&actual_cost>=0.0;
        outcomes.push(json!({
            "id":child.id,"label":candidate["label"],"status":child.status,
            "verifiedByModel":verified,
            "observedArtifactEdit":artifact_changed,
            "sameBaseModelAndWorkspacePolicy":independent,
            "successfulToolReceipts":successful_tools,
            "eligibleForComparison":eligible,
            "actualCostKrw":actual_cost,"modelSteps":child.steps,
            "failureCount":child.events.iter().filter(|e|e.kind=="tool_error"
                ||e.kind=="tool_admission_rejected").count(),
            "independentWorkspace":child.repository.is_some(),
            "note":"Verifier PASS and tool receipts are observable; acceptance tests and artifact inspection still govern adoption."
        }));
    }
    let eligible=outcomes.iter().filter(|o|o["eligibleForComparison"]==true)
        .collect::<Vec<&Value>>();
    // Pareto dominance rather than an invented 5-point diagnosis bonus.
    // When price, model steps and failures trade off, return NO winner.
    let winner=match eligible.as_slice(){
        [one]=>one["id"].clone(),
        [a,b]=>{
            let dominates=|x:&Value,y:&Value|{
                let xc=x["actualCostKrw"].as_f64().unwrap_or(f64::INFINITY);
                let yc=y["actualCostKrw"].as_f64().unwrap_or(f64::INFINITY);
                let xs=x["modelSteps"].as_u64().unwrap_or(u64::MAX);
                let ys=y["modelSteps"].as_u64().unwrap_or(u64::MAX);
                let xf=x["failureCount"].as_u64().unwrap_or(u64::MAX);
                let yf=y["failureCount"].as_u64().unwrap_or(u64::MAX);
                xc<=yc && xs<=ys && xf<=yf && (xc<yc || xs<ys || xf<yf)
            };
            if dominates(a,b){a["id"].clone()}
            else if dominates(b,a){b["id"].clone()}
            else{Value::Null}
        },
        _=>Value::Null,
    };
    Some(json!({"launch_seq":launch.seq,"candidates":outcomes,
        "provisionalWinner":winner,
        "decisionBasis":"verified artifacts plus Pareto dominance on measured KRW, model steps and tool failures; tradeoffs produce no forced winner",
        "adopted":false,"requiresApprovalForPatchAdoption":true,
        "missingGroundTruth":"LLM verifier PASS alone is not a held-out test; local build/test and artifact inspection still determine actual adoption"}))
}

#[cfg(test)]
mod runtime_trials_tests{
    use super::*;
    use crate::domain::{CreateRun,Run};
    use serde_json::json;
    fn sample()->Run{Run::new(CreateRun{
        instruction:"Fix both files and test".into(),
        budget_krw:Some(1000.0),max_steps:Some(8)},8).unwrap()}
    #[test]
    fn actual_verified_children_compete_by_recorded_cost_not_weighted_labels(){
        let mut parent=sample();
        parent.repository=Some("repo".into());parent.base_commit=Some("sha".into());
        parent.provider_id="provider".into();parent.model_name="model".into();
        let mut a=sample();a.parent_run_id=Some(parent.id);
        let mut b=sample();b.parent_run_id=Some(parent.id);
        for child in [&mut a,&mut b]{
            child.repository=parent.repository.clone();child.base_commit=parent.base_commit.clone();
            child.provider_id=parent.provider_id.clone();child.model_name=parent.model_name.clone();
            child.strategy_candidate=true;
        }
        a.status=RunStatus::Completed;a.cost_krw=40.0;a.steps=6;
        b.status=RunStatus::Completed;b.cost_krw=90.0;b.steps=9;
        a.event("tool_result","write completed",Some(json!({"name":"write_file"})));
        b.event("tool_result","write completed",Some(json!({"name":"write_file"})));
        a.event("verification","PASS",None);
        b.event("verification","PASS",None);
        parent.settled_child_ids=vec![a.id,b.id];
        parent.event("strategy_trials_launched","Two isolated candidates",Some(json!({
            "candidates":[{"run_id":a.id,"label":"alternative architecture"},
                {"run_id":b.id,"label":"minimal fix"}]
        })));
        let report=trial_report(&parent,&[a.clone(),b.clone()]).unwrap();
        assert_eq!(report["provisionalWinner"],json!(a.id));
        assert_eq!(report["adopted"],false);
        // Cheap but unverified is not a success and cannot win.
        a.status=RunStatus::Failed;a.cost_krw=1.0;
        assert_eq!(trial_report(&parent,&[a.clone(),b.clone()]).unwrap()
            ["provisionalWinner"],json!(b.id));
        b.events.retain(|e|e.kind!="verification");
        assert!(trial_report(&parent,&[a.clone(),b.clone()]).unwrap()
            ["provisionalWinner"].is_null());
    }
    #[test]
    fn trial_comparison_waits_for_settlement_and_declines_cost_speed_tradeoff(){
        let mut parent=sample();
        parent.repository=Some("repo".into());parent.base_commit=Some("commit".into());
        parent.provider_id="p".into();parent.model_name="m".into();
        let mut children=Vec::new();
        for (cost,steps) in [(20.0,8),(75.0,4)]{
            let mut c=sample();
            c.parent_run_id=Some(parent.id);c.strategy_candidate=true;
            c.repository=parent.repository.clone();c.base_commit=parent.base_commit.clone();
            c.provider_id=parent.provider_id.clone();c.model_name=parent.model_name.clone();
            c.status=RunStatus::Completed;c.cost_krw=cost;c.steps=steps;
            c.event("tool_result","real edit",Some(json!({"name":"edit_file"})));
            c.event("verification","PASS",None);
            children.push(c);
        }
        parent.event("strategy_trials_launched","started",Some(json!({"candidates":[
            {"run_id":children[0].id,"label":"fast cheap"},
            {"run_id":children[1].id,"label":"fast costly"}]})));
        assert!(trial_report(&parent,&children).is_none(),
            "comparison before child budget settlement is a race");
        parent.settled_child_ids=children.iter().map(|c|c.id).collect();
        let report=trial_report(&parent,&children).unwrap();
        assert!(report["provisionalWinner"].is_null(),
            "budget vs speed is a tradeoff, not proof of superiority");
        assert_eq!(report["candidates"][0]["eligibleForComparison"],true);
        children[0].strategy_candidate=false;
        let report=trial_report(&parent,&children).unwrap();
        assert_eq!(report["provisionalWinner"],json!(children[1].id),
            "a non-isolated candidate cannot win");
    }

    #[test]
    fn incomplete_or_unrelated_children_cannot_fabricate_a_comparison(){
        let mut parent=sample();let mut child=sample();
        child.parent_run_id=Some(parent.id);
        let another=sample();
        parent.event("strategy_trials_launched","test",Some(json!({
            "candidates":[{"run_id":child.id,"label":"A"},
                {"run_id":another.id,"label":"B"}]
        })));
        assert!(trial_report(&parent,&[child.clone(),another]).is_none());
        parent.event("strategy_trials_evaluated","already observed",
            Some(json!({"launch_seq":parent.events.last().unwrap().seq})));
        assert!(trial_report(&parent,&[child]).is_none());
    }
}
