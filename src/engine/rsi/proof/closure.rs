//! Evidence of a closed A -> B -> A improvement loop.
//!
//! Kept outside the mutable planner: a claimed plan or a generation summary
//! cannot prove that descendants actually used inherited improvements.
use crate::{engine::rsi, state::AppState};
use serde_json::{json, Value};

fn method_source(path: &str) -> bool {
    matches!(path,
        "src/engine/rsi_evolve.rs"
        | "src/engine/rsi_meta_controller.rs"
        | "src/engine/rsi_prequential.rs"
        | "src/engine/rsi_recursive_rewrite.rs"
        | "src/engine/rsi_principle_learning.rs"
        | "src/engine/rsi_system.rs"
        | "src/engine/algorithms.rs"
    ) || (path.starts_with("src/engine/rsi/planner/")
        || path.starts_with("src/engine/rsi/learning/")
        || path.starts_with("src/engine/rsi/meta/"))
        && path.ends_with(".rs")
}

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
        && paths.iter().any(|path|method_source(path))
        && candidate["evaluation"]["metrics"]["metaHoldoutGains"]
            .as_u64().unwrap_or(0)>0
        && candidate["evaluation"]["metrics"]["improvedHoldoutDomains"]
            .as_array().is_some_and(|domains|domains.iter().any(|v|v=="improver"))
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
pub(crate) fn evidence(state: &AppState, history: &[Value]) -> Value {
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
        return json!({
            "proven":true,
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
        "source":"replayed_method_capability_method_closed_loop_v1",
        "requires":"three contiguous independently accepted source revisions: method A, descendant B, descendant re-improvement of method A, with original jobs executing the compiled descendant policies"
    })
}
