//! One authoritative, source-replayed verdict for RSI campaign progress.
//! The coordinator, API and final audit must never independently redefine
//! what "capability demonstrated" or "recursive completion" means.
use super::*;
use serde_json::{json, Value};

/// An accepted row is not necessarily an independently verified improvement.
/// Report both visible adoption streaks and receipt-backed streaks.
pub(crate) fn evaluate(state:&AppState,campaign:&Value)->Value {
    let history=lineage_history(state,campaign);
    let mut accepted_run=0u64;
    let mut max_accepted=0u64;
    let mut verified_run=0u64;
    let mut max_verified=0u64;
    for generation in &history {
        if generation["accepted"]==true {
            accepted_run+=1;
        }else{
            accepted_run=0;
        }
        max_accepted=max_accepted.max(accepted_run);
        if generation_proof(state,generation).is_some(){
            verified_run+=1;
        }else{
            verified_run=0;
        }
        max_verified=max_verified.max(verified_run);
    }
    // General source evolution is deliberately NOT the RSI completion gate.
    // A native-only sequence, or an A→B transfer without a subsequent
    // model-authored revision of the very method, remains incomplete.
    let source=max_verified>=3 && capability_proven(state,&history);
    let model_source=source && model_driven_capability_proven(state,&history);
    let closure=if model_source {
        crate::engine::rsi_recursive_closure::evidence(state,&history)
    }else{
        None
    };
    let closed=closure.is_some();
    let systemic=closed
        && crate::engine::rsi_system::verified_handoff(&history);
    json!({
        "source":"independent_recursive_method_campaign_verdict_v1",
        "acceptedStreak":accepted_run,
        "maxAcceptedStreak":max_accepted,
        "verifiedStreak":verified_run,
        "maxVerifiedStreak":max_verified,
        "sourceEvolutionDemonstrated":source,
        "modelDrivenCapabilityDemonstrated":model_source,
        "recursiveMethodClosureProven":closed,
        "capabilityDemonstrated":closed,
        "actualCompletion":closed,
        "systemicHandoffProven":systemic,
        "recursiveMethodEvidence":closure,
        "causalAblationProven":false,
        "requiredMethodSequence":"model_authored_method_edit -> compiled_child_method_use_and_independent_other_capability_gain -> feedback_bound_method_rewrite_with_independent_meta_gain"
    })
}

/// Do not duplicate the gate when a later reader computes a projection.
/// Keep persisted evidence and API projection consistent with the same
/// original candidate receipts, even if old campaigns claimed success.
pub(crate) fn apply(state:&AppState,campaign:&mut Value)->Value {
    let verdict=evaluate(state,campaign);
    for key in [
        "acceptedStreak","maxAcceptedStreak","verifiedStreak","maxVerifiedStreak",
        "sourceEvolutionDemonstrated","modelDrivenCapabilityDemonstrated",
        "recursiveMethodClosureProven","capabilityDemonstrated",
        "actualCompletion","systemicHandoffProven","causalAblationProven"
    ] {
        campaign[key]=verdict[key].clone();
    }
    campaign["capabilityVerdict"]=verdict.clone();
    verdict
}
