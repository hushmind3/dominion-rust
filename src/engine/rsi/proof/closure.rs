//! Evidence of a closed A -> B -> A improvement loop.
//!
//! Kept outside the mutable planner: a claimed plan or a generation summary
//! cannot prove that descendants actually used inherited improvements.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};

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

pub(crate) fn evidence(state: &AppState, history: &[Value]) -> Value {
    let mut pending_control=Value::Null;
    for window in history.windows(3) {
        let [first, second, third] = window else { continue };
        let (Some((a, a_job)),Some((b,b_job)),Some((c,c_job))) = (
            measured_step(state,first),measured_step(state,second),
            measured_step(state,third)
        ) else {continue};
        if first["acceptedCommit"]!=second["generatorCommit"]
            || second["acceptedCommit"]!=third["generatorCommit"]
            || !method_gain(&a)
            || !cross_domain_gain(&b)
            || !method_gain(&c)
            || !executed_descendant(&b_job,second)
            || !executed_descendant(&c_job,third)
            || b_job["input"]["rsiGeneratorCommit"]!=first["acceptedCommit"]
            || c_job["input"]["rsiGeneratorCommit"]!=second["acceptedCommit"]
            || a_job["input"]["rsiGeneratorCommit"]!=first["generatorCommit"]
        {continue;}
        let control=method_control::evidence(state,first,second,&b_job,&b);
        if control["measuredAdvantage"]!=true {
            pending_control=control;
            continue;
        }
        return json!({
            "methodControl":control,
            "source":"replayed_method_capability_method_closed_loop_v1",
            "shape":"improver_A -> different_capability_B -> improver_A",
            "candidateIds":[a["id"],b["id"],c["id"]],
            "generatorCommits":[first["generatorCommit"],second["generatorCommit"],third["generatorCommit"]],
            "acceptedCommits":[first["acceptedCommit"],second["acceptedCommit"],third["acceptedCommit"]],
            "improverMethodSources":[
                changed_paths(&a).into_iter().filter(|p|method_source(p)).collect::<Vec<_>>(),
                changed_paths(&c).into_iter().filter(|p|method_source(p)).collect::<Vec<_>>()
            ],
            "descendantObservedDecisions":[
                {"domain":b_job["rsiDomain"],"editPolicyRevision":b_job["rsiEditPolicyDecision"]["methodRevision"],"metaRevision":b_job["rsiMetaPolicyExecuted"]["metaRevision"]},
                {"domain":c_job["rsiDomain"],"editPolicyRevision":c_job["rsiEditPolicyDecision"]["methodRevision"],"metaRevision":c_job["rsiMetaPolicyExecuted"]["metaRevision"]}
            ],
            "independentFrozenPairedEvaluation":true,
            "unboundedSelfImprovementProven":false
        });
    }
    json!({
        "proven":false,
        "methodControl":pending_control,
        "source":"replayed_method_capability_method_closed_loop_v1",
        "requires":"three contiguous independently accepted source revisions: method A, descendant B, descendant re-improvement of method A, with original jobs executing the compiled descendant policies AND a matched, independently replayed method-off control outperformable on frozen holdouts"
    })
}

#[cfg(test)]
#[path = "../../../../tests/unit/engine_rsi_closure_source_tests.rs"]
mod source_tests;
