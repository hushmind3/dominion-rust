//! Evidence-grounded, non-weight recovery guidance from verified native Runs.
//! Durable tool and verification receipts are the source of truth.
use crate::{
    domain::{Run, RunStatus},
};
use serde::Serialize;
use std::collections::BTreeMap;
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

/// Historical observations ONLY, never a claim of causal strategy lift.
/// "Completed" and independently verified success are intentionally separate.
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct StrategyEvaluation {
    pub strategy_id:String,
    pub loop_id:String,
    pub observed_runs:usize,
    pub completed_runs:usize,
    pub artifact_verified_runs:usize,
    pub failed_runs:usize,
    pub cancelled_runs:usize,
    pub approval_paused_runs:usize,
    pub uncertain_effect_runs:usize,
    pub model_calls:usize,
    pub model_usage_unreported:usize,
    pub external_unmetered_calls:usize,
    pub input_tokens:u64,
    pub output_tokens:u64,
    pub cached_input_tokens:u64,
    pub observed_cost_krw:f64,
    pub steps:u64,
    pub source_note:&'static str,
}
pub fn evaluate_observed(runs:&[Run],repository:Option<&str>)
    ->Vec<StrategyEvaluation>{
    const CAUTION:&str="historical association only; no held-out control, unknown provider tariffs and external CLI charges are not zero";
    let mut rows=BTreeMap::<(String,String),StrategyEvaluation>::new();
    for run in runs{
        if run.model_only||run.team.is_some(){continue;}
        if repository.is_some_and(|scope|run.repository.as_deref()!=Some(scope)){
            continue;
        }
        let id=run.events.iter().find(|e|e.kind=="algorithm_bound")
            .map(|e|e.message.clone()).unwrap_or("legacy_unbound".into());
        let loop_id=run.events.iter().find(|e|e.kind=="loop_bound")
            .map(|e|e.message.clone()).unwrap_or("legacy_unbound".into());
        let row=rows.entry((id.clone(),loop_id.clone())).or_insert_with(||StrategyEvaluation{
            strategy_id:id,loop_id,observed_runs:0,completed_runs:0,
            artifact_verified_runs:0,failed_runs:0,cancelled_runs:0,
            approval_paused_runs:0,uncertain_effect_runs:0,
            model_calls:0,model_usage_unreported:0,
            external_unmetered_calls:0,input_tokens:0,output_tokens:0,
            cached_input_tokens:0,observed_cost_krw:0.0,steps:0,
            source_note:CAUTION
        });
        row.observed_runs+=1;
        row.completed_runs+=usize::from(run.status==RunStatus::Completed);
        row.failed_runs+=usize::from(run.status==RunStatus::Failed);
        row.cancelled_runs+=usize::from(run.status==RunStatus::Cancelled);
        row.approval_paused_runs+=usize::from(
            run.status==RunStatus::AwaitingApproval);
        if run.status==RunStatus::Completed&&run.events.iter().any(|e|
            (e.kind=="verification"
                &&crate::verification::strict_pass(&e.message))
            ||(e.kind=="artifact_evidence_checked"
                &&e.detail.as_ref().and_then(|v|v["satisfied"].as_bool())==Some(true))){
            row.artifact_verified_runs+=1;
        }
        if run.in_flight.is_some()||!run.read_batch_in_flight.is_empty()
            ||run.events.iter().any(|e|matches!(e.kind.as_str(),
                "tool_outcome_uncertain"|"team_patch_outcome_uncertain")){
            row.uncertain_effect_runs+=1;
        }
        row.model_calls+=run.events.iter().filter(|e|e.kind=="model").count();
        row.model_usage_unreported+=run.events.iter().filter(|e|
            e.kind=="model"&&e.detail.as_ref().and_then(|v|
                v["usage_reported"].as_bool())!=Some(true)).count();
        row.external_unmetered_calls+=run.events.iter()
            .filter(|e|e.kind=="external_agent_unmetered").count();
        row.input_tokens=row.input_tokens.saturating_add(run.input_tokens);
        row.output_tokens=row.output_tokens.saturating_add(run.output_tokens);
        row.cached_input_tokens=row.cached_input_tokens
            .saturating_add(run.cached_input_tokens);
        row.observed_cost_krw+=run.cost_krw;
        row.steps=row.steps.saturating_add(run.steps as u64);
    }
    rows.into_values().collect()
}
#[cfg(test)]
mod observed_evaluation_tests{
    use super::*;
    use crate::domain::CreateRun;
    #[test]
    fn model_claim_does_not_count_as_a_verified_strategy_success(){
        let mut run=Run::new(CreateRun{instruction:"Change code".into(),
            budget_krw:Some(100.0),max_steps:Some(3)},3).unwrap();
        run.strategy_learning=true;
        run.status=RunStatus::Completed;
        run.answer=Some("I verified every change successfully".into());
        run.event("algorithm_bound","ReactNative",None);
        run.event("loop_bound","ReAct",None);
        let rows=evaluate_observed(&[run.clone()],None);
        assert_eq!(rows[0].completed_runs,1);
        assert_eq!(rows[0].artifact_verified_runs,0);
        assert_eq!(rows[0].source_note.contains("historical"),true);
        run.event("verification","PASS",None);
        assert_eq!(evaluate_observed(&[run],None)[0].artifact_verified_runs,1);
    }
}
