//! Evidence-grounded, non-weight strategy transfer across RSI and agent runs.
//! Persisted Run/Improvement records remain the source of truth: no duplicate
//! index can falsely promote a candidate that was never adopted.
use crate::{
    domain::{Run, RunStatus},
};
use serde::Serialize;
use std::{cmp::Ordering, collections::HashSet};
use uuid::Uuid;

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
}

fn bounded(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase().split(|c:char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| s.chars().count() >= 3 && s.chars().count() <= 36)
        .filter(|s| !matches!(*s,
            "from" | "with" | "have" | "into" | "this" | "that" | "agent" |
            "code" | "improve" | "please" | "source" | "file" | "files" |
            "수정하고" | "개선하고" | "만들어라" | "프로젝트"))
        .take(35).map(str::to_owned).collect()
}

fn relevant(query: &str, candidate: &StrategyEvidence) -> u32 {
    let needed = words(query);
    if needed.is_empty() { return 0; }
    let title = candidate.goal.to_lowercase();
    let method = candidate.principle.to_lowercase();
    let files = candidate.files.join(" ").to_lowercase();
    needed.iter().map(|term| {
        if title.contains(term) {5}
        else if method.contains(term) {3}
        else if files.contains(term) {2}
        else if candidate.when_to_use.to_lowercase().contains(term) {1}
        else {0}
    }).sum()
}

pub(crate) fn from_run(run: &Run) -> Option<StrategyEvidence> {
    if !run.strategy_learning || run.status != RunStatus::Completed { return None; }
    let repository=run.repository.as_ref()?;
    let verified=run.events.iter().any(|e|e.kind=="verification"
        && crate::verification::strict_pass(&e.message));
    let receipts=run.events.iter().filter(|e|e.kind=="tool_result").count();
    if !verified || receipts==0 { return None; }
    let method=run.plan.as_deref().unwrap_or("");
    if method.trim().is_empty() { return None; }
    Some(StrategyEvidence {
        source_id:run.id, source:"agent".into(),repository:repository.clone(),
        goal:bounded(&run.instruction,400),principle:bounded(method,420),
        when_to_use:"Previously completed with independently verified tool receipts".into(),
        failure_mode:String::new(),files:Vec::new(),outcome:"verified".into(),
        baseline_score:None,resulting_score:None,attempts:run.steps,
        cost_krw:run.cost_krw,updated_at_ms:run.updated_at_ms
    })
}

/// Rank the compact proof records, without cloning every historical agent
/// transcript on each model turn. Never combine unrelated repositories.
pub fn rank(
    evidence: Vec<StrategyEvidence>, repository:&str, query:&str, max:usize,
) -> Vec<StrategyEvidence> {
    if repository.trim().is_empty() || repository=="global" || max==0 {return Vec::new();}
    let mut ranked=evidence.into_iter()
        .filter(|e|e.repository==repository)
        .filter_map(|e|{
            let overlap=relevant(query,&e);
            if overlap==0 {None} else {
                let outcome_bonus=match e.outcome.as_str(){
                    "adopted"=>3.0,
                    "verified"=>1.0,
                    _=>-2.0,
                };
                let measured_gain=match (e.baseline_score,e.resulting_score) {
                    (Some(a),Some(b)) if b>a => (b-a).ln_1p().min(3.0),
                    _=>0.0,
                };
                Some((overlap as f64 + outcome_bonus + measured_gain,e))
            }
        }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)
        .then_with(||b.1.updated_at_ms.cmp(&a.1.updated_at_ms)));
    let mut out=Vec::new();
    let mut warnings=0usize;
    for (_,e) in ranked {
        if e.outcome=="rejected" {
            if warnings>=1 {continue;}
            warnings+=1;
        }
        out.push(e);
        if out.len()>=max.min(6) {break;}
    }
    out
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
