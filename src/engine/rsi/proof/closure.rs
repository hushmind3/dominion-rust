//! Evidence of a closed A -> B -> A improvement loop.
//!
//! Kept outside the mutable planner: a claimed plan or a generation summary
//! cannot prove that descendants actually used inherited improvements.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};
use sha2::{Digest,Sha256};

#[path = "source_roles.rs"]
mod source_roles;
use self::source_roles::method_source;
#[path = "method_control.rs"]
mod method_control;

fn changed_paths<'a>(candidate: &'a Value) -> Vec<&'a str> {
    candidate["paths"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect()
}

/// The original candidate and frozen evaluation, not a generation's mutable
/// accepted/meta labels, decide whether this step has evidence.
fn measured_step(state: &AppState, generation: &Value) -> Option<(Value, Value)> {
    let candidate = state.store.get("rsi_candidate", generation["candidateId"].as_str()?)?;
    if generation["accepted"] != true
        || candidate["status"] != "verified"
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["baseCommit"] != generation["generatorCommit"]
        || candidate["experimentAdoption"]["generatorCommit"] != generation["generatorCommit"]
        || candidate["experimentAdoption"]["commit"] != generation["acceptedCommit"]
        || candidate["evaluation"]["evidenceSha256"] != generation["evaluation"]["evidenceSha256"]
        || !rsi::verified_pair_receipt(state, &candidate)
    { return None; }
    let job = state.store.get("job",generation["jobId"].as_str()?)?;
    if job["kind"] != "rsi_generation"
        || job["id"] != generation["jobId"]
        || job["input"]["rsiGeneratorCommit"] != generation["generatorCommit"]
        || job["result"]["rsi"]["candidateId"] != generation["candidateId"]
    { return None; }
    Some((candidate,job))
}

/// Requires the *running descendant binary* to have selected and persisted
/// an executable method, and the worker to have produced the measured patch.
/// Simply copying a predecessor's plan into a generation JSON is insufficient.
fn executed_descendant(job: &Value, generation: &Value) -> bool {
    let revision=job["input"]["rsiGeneratorCommit"].as_str().unwrap_or("");
    let edit_revision=job["input"]["rsiEditPolicy"]["revision"].as_str().unwrap_or("");
    let meta_revision=job["input"]["rsiMetaPolicy"]["revision"].as_str().unwrap_or("");
    !revision.is_empty() && !edit_revision.is_empty() && !meta_revision.is_empty()
        && job["input"]["rsiDescendantPlan"]["source"]=="compiled_descendant_system_plan_v1"
        && job["input"]["rsiDescendantPlan"]["generatorCommit"]==revision
        && job["rsiCompiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && job["rsiCompiledPlanExecuted"]["generatorCommit"]==revision
        && job["rsiCompiledPlanExecuted"]["domain"]==generation["domain"]
        && job["rsiMetaPolicyExecuted"]["source"]=="running_compiled_descendant_meta_controller"
        && job["rsiMetaPolicyExecuted"]["generatorCommit"]==revision
        && job["rsiMetaPolicyExecuted"]["metaRevision"]==meta_revision
        && job["rsiEditPolicyDecision"]["producer"]=="running_descendant_binary"
        && job["rsiEditPolicyDecision"]["generatorCommit"]==revision
        && job["rsiEditPolicyDecision"]["methodRevision"]==edit_revision
        && job["rsiDomain"]==generation["domain"]
}

fn method_gain(candidate: &Value) -> bool {
    let paths=changed_paths(candidate);
    candidate["target"]=="improver"
        && !paths.is_empty() && paths.iter().all(|path|method_source(path))
        && candidate["evaluation"]["metrics"]["metaHoldoutGains"]
            .as_u64().unwrap_or(0)>0
        && candidate["evaluation"]["metrics"]["improvedHoldoutDomains"]
            .as_array().is_some_and(|domains|domains.iter().any(|v|v=="improver"))
        // A faster no-op planner is not evidence that its decisions improved.
        // Require at least one frozen improver task that the predecessor
        // failed and the revised method actually passed.
        && candidate["evaluation"]["metrics"]["measurements"]
            .as_array().is_some_and(|cases|cases.iter().any(|case|
                case["domain"]=="improver" && case["holdout"]==true
                    && case["baselinePass"]==false && case["candidatePass"]==true))
}

fn cross_domain_gain(candidate: &Value) -> bool {
    let paths=changed_paths(candidate);
    !paths.is_empty() && !paths.iter().any(|path|method_source(path))
        && candidate["evaluation"]["metrics"]["improvedHoldoutDomains"]
            .as_array().is_some_and(|domains|domains.iter().any(|v|v.as_str()
                .is_some_and(|domain| domain!="improver")))
}

/// A->B->A: an independently measured improver source revision, a descendant
/// which actually executes the inherited binary and improves B under the
/// frozen evaluator, and that descendant's own independently measured
/// re-edit of the source of improvement methods.
///
/// This demonstrates a bounded, replayable loop; it does not claim indefinite
/// intelligence growth or that all changed behavior is causally attributable
/// to the original source edit.
/// Expose an immutable control target to the ordinary RSI start endpoint.
/// A fake generation id cannot choose an arbitrary historical source commit.
pub(crate) fn control_anchor(state:&AppState,treated:&Value)->Option<Value>{
    let (b,b_job)=measured_step(state,treated)?;
    if !cross_domain_gain(&b)
        || !executed_descendant(&b_job,treated)
        || treated["candidateOrigin"]!="model_driven_detached_worktree" {
        return None;
    }
    let campaign=treated["campaignId"].as_str()?;
    let attempt=treated["attempt"].as_u64()?;
    let predecessor=state.store.all("rsi_generation").into_iter().find(|g|
        g["campaignId"]==campaign
        && g["attempt"].as_u64().is_some_and(|n|n.checked_add(1)==Some(attempt))
        && g["acceptedCommit"]==treated["generatorCommit"])?;
    let (a,_)=measured_step(state,&predecessor)?;
    if !method_gain(&a){return None;}
    Some(json!({
        "source":"original_method_source_control_anchor_v1",
        "treatedGenerationId":treated["id"],
        "treatedCampaignId":campaign,
        "treatedJobId":b_job["id"],
        "treatedCandidateId":b["id"],
        "methodCandidateId":a["id"],
        "methodOffGeneratorCommit":predecessor["generatorCommit"],
        "methodOnGeneratorCommit":predecessor["acceptedCommit"]
    }))
}

/// Dynamically replay a B descendant's independent A-off evidence. A
/// mutable campaign flag cannot supply this feedback; every read resolves
/// original accepted source receipts, the worker, and matched control jobs.
pub(crate) fn control_feedback(state:&AppState,treated:&Value)->Value{
    let Some(anchor)=control_anchor(state,treated) else{return Value::Null};
    let Some((candidate,job))=measured_step(state,treated) else{return Value::Null};
    let Some(predecessor)=state.store.all("rsi_generation").into_iter().find(|g|
        g["campaignId"]==anchor["treatedCampaignId"]
            &&g["acceptedCommit"]==anchor["methodOnGeneratorCommit"]
            &&g["generatorCommit"]==anchor["methodOffGeneratorCommit"])
        else{return Value::Null};
    let feedback=method_control::evidence(state,&predecessor,treated,&job,&candidate);
    if feedback["treatedCandidateId"]!=candidate["id"]{return Value::Null;}
    feedback
}

/// Authoritative bounded A(method)->B(capability)->A2(method)->B2(capability)
/// closure. Each method revision must produce a *new*, independently paired
/// successor capability advantage against its own pre-method code generation.
/// A2 must have consumed the real B feedback in its compiled descendant plan;
/// sequence alone is not proof of causal recurrence.
pub(crate) fn evidence(state: &AppState, history: &[Value]) -> Value {
    let mut pending=Value::Null;
    for window in history.windows(4) {
        let [a_row,b_row,a2_row,b2_row] = window else {continue};
        let (Some((a,a_job)),Some((b,b_job)),Some((a2,a2_job)),
             Some((b2,b2_job)))=(
            measured_step(state,a_row),measured_step(state,b_row),
            measured_step(state,a2_row),measured_step(state,b2_row)
        ) else {continue};
        if a_row["acceptedCommit"]!=b_row["generatorCommit"]
            || b_row["acceptedCommit"]!=a2_row["generatorCommit"]
            || a2_row["acceptedCommit"]!=b2_row["generatorCommit"]
            || !method_gain(&a) || !method_gain(&a2)
            || !cross_domain_gain(&b) || !cross_domain_gain(&b2)
            || b_row["candidateOrigin"]!="model_driven_detached_worktree"
            || b2_row["candidateOrigin"]!="model_driven_detached_worktree"
            || !executed_descendant(&b_job,b_row)
            || !executed_descendant(&a2_job,a2_row)
            || !executed_descendant(&b2_job,b2_row)
            || a_job["input"]["rsiGeneratorCommit"]!=a_row["generatorCommit"]
            || b_job["input"]["rsiGeneratorCommit"]!=a_row["acceptedCommit"]
            || a2_job["input"]["rsiGeneratorCommit"]!=b_row["acceptedCommit"]
            || b2_job["input"]["rsiGeneratorCommit"]!=a2_row["acceptedCommit"] {
            continue;
        }
        // The second method rewrite must have *received* B's original
        // independently evaluated candidate as compiled policy feedback.
        // Persisted claims in mutable generation rows cannot replace the
        // actual worker job and meta-policy source provenance.
        let observed=&a2_job["rsiMethodControlFeedbackExecuted"];
        let observed_prompt=a2_job["input"]["prompt"].as_str().unwrap_or("");
        let prompt_hash=format!("{:x}",Sha256::digest(observed_prompt.as_bytes()));
        if observed["source"]!="compiled_descendant_method_feedback_delivery_v1"
            || observed["deliveredToModelTurn"]!=true
            || observed["promptSha256"]!=prompt_hash
            || !observed_prompt.contains(b["id"].as_str().unwrap_or(""))
            || observed["treatedCandidateId"]!=b["id"]
            || observed["generatorCommit"]!=a2_row["generatorCommit"]
            || observed["metaRevision"]!=a2_job["rsiMetaPolicyExecuted"]["metaRevision"]
            || a2_job["input"]["rsiMetaPolicy"]["methodControlFeedback"]["treatedCandidateId"]!=b["id"]{
            continue;
        }
        let first_control=method_control::evidence(state,a_row,b_row,&b_job,&b);
        let second_control=method_control::evidence(state,a2_row,b2_row,&b2_job,&b2);
        let first_good=first_control["measuredAdvantage"]==true;
        let second_good=second_control["measuredAdvantage"]==true;
        if !(first_good && second_good){
            pending=json!({
                "candidateIds":[a["id"],b["id"],a2["id"],b2["id"]],
                "initialMethodControl":first_control,
                "revisedMethodControl":second_control,
                "feedbackActuallyDelivered":true,
                "controlsCompleted":first_control["matchedControlFound"]==true
                    && second_control["matchedControlFound"]==true,
                "proven":false
            });
            continue;
        }
        return json!({
            "proven":true,
            "source":"replayed_four_generation_reflexive_method_closure_v1",
            "shape":"method_A -> capability_B -> revised_method_A2 -> capability_B2",
            "candidateIds":[a["id"],b["id"],a2["id"],b2["id"]],
            "generatorCommits":[a_row["generatorCommit"],b_row["generatorCommit"],
                a2_row["generatorCommit"],b2_row["generatorCommit"]],
            "acceptedCommits":[a_row["acceptedCommit"],b_row["acceptedCommit"],
                a2_row["acceptedCommit"],b2_row["acceptedCommit"]],
            "methodSources":[
                changed_paths(&a).into_iter().filter(|p|method_source(p)).collect::<Vec<_>>(),
                changed_paths(&a2).into_iter().filter(|p|method_source(p)).collect::<Vec<_>>()
            ],
            "observedCompiledMethodFeedback":observed,
            "methodControl":first_control,
            "initialMethodControl":first_control,
            "revisedMethodControl":second_control,
            "independentFrozenPairedEvaluation":true,
            "sourceLevelMethodRevisionsProven":true,
            "unboundedSelfImprovementProven":false,
            "causalEffectIsolatedByRandomization":false
        });
    }
    json!({
        "proven":false,
        "methodControl":pending["initialMethodControl"],
        "revisedMethodControl":pending["revisedMethodControl"],
        "pendingClosure":pending,
        "source":"replayed_four_generation_reflexive_method_closure_v1",
        "requires":"four consecutive independently adopted source revisions A(method), B(capability), A2(method), B2(capability); A2 compiled worker consumes original B feedback; independently measured A-off vs B and A2-off vs B2 each show holdout gains without reverse failures"
    })
}

#[cfg(test)]
#[path = "../../../../tests/unit/engine_rsi_closure_source_tests.rs"]
mod source_tests;
