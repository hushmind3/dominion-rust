//! Normal campaign -> independently queued method-off control.
//! An accepted A(method) followed by a measured B(capability) automatically
//! schedules the PRE-A compiled source on the SAME B task. The ordinary
//! rsi_evolve worker, fixed-provider route and frozen evaluator own execution.
//! The control is never adopted. This module selects work, not proof.
use crate::{engine::{jobs, rsi_closure},state::AppState};
use serde_json::{json,Value};

fn scheduled_input(original:&Value, anchor:&Value, treated:&Value)->Option<Value>{
    let source=anchor["methodOffGeneratorCommit"].as_str()?;
    let treated_job=anchor["treatedJobId"].as_str()?;
    if source.len()!=40||!source.bytes().all(|ch|ch.is_ascii_hexdigit())
        ||treated["id"]!=treated_job
        ||treated["kind"]!="rsi_generation"
        ||treated["input"]["rsiPromptBase"].as_str()
            .is_none_or(|s|s.trim().is_empty())
        ||treated["input"]["rsiFrozenCases"].as_array()
            .is_none_or(|cases|cases.is_empty()){
        return None;
    }
    // A 1:1 preflight model budget on the control, billed as a separate
    // explicitly agreed experiment rather than hidden in the original budget.
    // Using the same route and effective limit prevents an underfunded control
    // from masquerading as evidence of superiority.
    let remaining=treated["input"]["rsiRemainingCampaignTokens"].as_u64();
    if remaining.is_some_and(|n|n<1000 ||n>2_000_000) {return None;}
    let mut input=original.clone();
    input["prompt"]=treated["input"]["rsiPromptBase"].clone();
    input["rsiObjective"]=treated["input"]["rsiObjective"].clone();
    input["rsiFrozenCases"]=treated["input"]["rsiFrozenCases"].clone();
    input["reasoningEffort"]=treated["input"]["reasoningEffort"].clone();
    input["providerId"]=treated["input"]["providerId"].clone();
    input["model"]=treated["input"]["model"].clone();
    input["rsiSourceBranch"]=treated["input"]["rsiSourceBranch"].clone();
    input["rsiSourceDir"]=original["rsiSourceDir"].clone();
    input["rsiBaseCommit"]=original["rsiBaseCommit"].clone();
    input["rsiCausalControlOf"]=anchor["treatedJobId"].clone();
    input["rsiCausalControlGeneration"]=anchor["treatedGenerationId"].clone();
    input["rsiCausalControlCampaign"]=anchor["treatedCampaignId"].clone();
    input["rsiCausalControlCommit"]=anchor["methodOffGeneratorCommit"].clone();
    input["rsiSystemEvolution"]=json!(true);
    input["rsiNativeInduction"]=json!(false);
    input["rsiModelEvolution"]=json!(false);
    input["rsiExpandAfterGain"]=json!(false);
    input["rsiAutoMethodControl"]=json!(false);
    input["rsiStopAfterProof"]=json!(false);
    input["rsiAutonomous"]=json!(false);
    input["rsiFocusFiles"]=json!([]);
    input["rsiContinueFrom"]=Value::Null;
    input["rsiPreviousDecision"]=Value::Null;
    input["rsiTrialAlgorithmId"]=Value::Null;
    input["selectionPolicy"]=json!("automatic");
    input["rsiNativeApproved"]=json!(true);
    input["limits"]=json!({
        "maxAttempts":1,
        "tokenBudget":remaining,
        "timeBudgetSeconds":original["limits"]["timeBudgetSeconds"]
    });
    Some(input)
}

/// Called from the normal accepted-generation path, *after* the B source
/// adoption and paired receipts are persisted but before campaign exit.
/// Uses a stable job ID so crash recovery and replay cannot bill the same
/// accepted B twice. The normal job scheduler runs the control campaign;
/// no parallel ad-hoc model, fake receipt or new evaluator is involved.
pub(super) fn queue_if_needed(
    state:&AppState, original:&Value, campaign:&Value, generation:&Value
)->Option<Value>{
    if original["rsiAutoMethodControl"]!=true
        ||campaign["controlRun"]==true
        ||generation["accepted"]!=true{return None;}
    let anchor=rsi_closure::control_anchor(state,generation)?;
    let treated=state.store.get("job",anchor["treatedJobId"].as_str()?)?;
    let control=scheduled_input(original,&anchor,&treated)?;
    let generation_id=anchor["treatedGenerationId"].as_str()?;
    let control_id=format!("rsi-auto-method-control-{generation_id}");
    let job=jobs::enqueue_once(state,&control_id,"rsi_evolve",control)?;
    state.store.audit("rsi.method_control.auto_queued",json!({
        "campaignId":campaign["id"],"generationId":generation["id"],
        "controlJobId":job["id"],"preMethodCommit":anchor["methodOffGeneratorCommit"],
        "treatedJobId":anchor["treatedJobId"],
        "source":"accepted_generation_automatic_control_v1",
        "notYetMeasured":true
    }));
    Some(json!({
        "source":"accepted_generation_automatic_control_v1",
        "controlJobId":job["id"],"treatedGenerationId":anchor["treatedGenerationId"],
        "methodOffGeneratorCommit":anchor["methodOffGeneratorCommit"],
        "status":job["status"],"proven":false
    }))
}
#[cfg(test)]
#[path = "../../../tests/unit/engine_rsi_auto_control_tests.rs"]
mod tests;
