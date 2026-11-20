//! Evidence-grounded, non-weight recovery guidance from verified native Runs.
//! Durable tool and verification receipts are the source of truth.
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
