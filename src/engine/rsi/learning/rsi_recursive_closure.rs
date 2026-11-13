//! A strict, source-replayed closure gate for a complete model-driven RSI.
//!
//! Three accepted patches are only source evolution. A recursive METHOD
//! closure requires a model-authored meta source edit, an evaluated child using
//! the new method on another capability, and a next child rewriting the very
//! method based on independently measured downstream feedback.
//! This audit never invokes a model or validator and never upgrades a claim
//! from generated prose to an observed gain.
use crate::{engine::{rsi,rsi_operator_provenance},state::AppState};
use serde_json::{json,Value};

fn hex(s:&str,n:usize)->bool {
    s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())
}

/// Returns true ONLY for consecutive generations whose real source adoption,
/// model/tool provenance and original frozen paired outcomes form a closed
/// meta -> downstream -> meta improvement lineage.
pub(crate) fn evidence(state:&AppState,history:&[Value])->Option<Value> {
    for window in history.windows(3) {
        let [invent,apply,rewrite]=window else {continue};
        if !rsi::capability_proven(state,window)
            || !rsi::model_driven_capability_proven(state,window) {
            continue;
        }
        if invent["acceptedCommit"]!=apply["generatorCommit"]
            || apply["acceptedCommit"]!=rewrite["generatorCommit"] {
            continue;
        }
        let selected=&apply["improvementAlgebraPlan"]["selected"];
        let rule=&selected["rule"];
        let Some(method_path)=rule["method"]["path"].as_str()
            .filter(|path|rsi::safe_path(path)) else {continue};
        let Some(method_hash)=apply["operatorExecution"]["methodSourceSha256"].as_str()
            .filter(|hash|hex(hash,64)) else {continue};
        // The rewritten meta method must be the SAME bytes which the child
        // actually ran, not a method merely present in a README/plan.
        if invent["acceptedSourceHashes"][method_path]!=method_hash
            || !rsi_operator_provenance::verified(state,apply,rule)
            || apply["operatorExecution"]["targetPath"]==method_path {
            continue;
        }
        let Some(task_candidate)=apply["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
        let receipt=&task_candidate["evaluation"]["evidenceSha256"];
        if !receipt.as_str().is_some_and(|s|hex(s,64)) {continue;}
        let Some(family)=apply["operatorExecution"]["methodFamilyId"].as_str()
            .filter(|s|hex(s,64))else{continue};
        // Re-evolving the operator is a new source edit. The original paired
        // evaluator must accept it as a meta improvement, not just compilation.
        let next_rule=&rewrite["improvementAlgebraPlan"]["selected"]["rule"];
        if rewrite["operatorExecution"]["targetPath"]!=method_path
            || !rsi_operator_provenance::verified(state,rewrite,next_rule)
            || rewrite["acceptedSourceHashes"][method_path].as_str()
                .is_none_or(|hash|!hex(hash,64)||hash==method_hash)
            || rewrite["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)==0 {
            continue;
        }
        let Some(next_candidate)=rewrite["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)) else{continue};
        if next_candidate["target"]!="improver" {continue;}
        // This is the actual downstream evidence consumed by the compiled
        // successor's method learner. A successor that never USED the task
        // feedback did not recursively improve its improvement procedure.
        let linked=rewrite["methodGeneralization"]["operatorLearning"].as_array()
            .is_some_and(|rows|rows.iter().any(|row|
                row["methodFamilyId"]==family
                && row["independentReplayedTrials"].as_u64().unwrap_or(0)>0
                && row["adoptedDescendantTrials"].as_u64().unwrap_or(0)>0
                && row["independentReceiptSha256"].as_array()
                    .is_some_and(|receipts|receipts.contains(receipt))));
        if linked {
            // These are ORIGINAL source and paired-evaluation identifiers,
            // not a synthesis of the model's explanation or a count of plans.
            return Some(json!({
                "source":"replayed_model_method_A_B_A_v1",
                "observedClosure":true,
                "causalAblationProven":false,
                "methodFamilyId":family,
                "methodPath":method_path,
                "parentMethodSha256":method_hash,
                "rewrittenMethodSha256":rewrite["acceptedSourceHashes"][method_path],
                "downstreamPairedReceiptSha256":receipt,
                "generations":[
                    {"phase":"A_method_edit",
                     "jobId":invent["jobId"],
                     "generatorCommit":invent["generatorCommit"],
                     "acceptedCommit":invent["acceptedCommit"],
                     "candidateId":invent["candidateId"]},
                    {"phase":"B_compiled_method_use_other_capability_gain",
                     "jobId":apply["jobId"],
                     "generatorCommit":apply["generatorCommit"],
                     "acceptedCommit":apply["acceptedCommit"],
                     "candidateId":apply["candidateId"],
                     "targetPath":apply["operatorExecution"]["targetPath"]},
                    {"phase":"A_method_rewrite_from_B_evidence",
                     "jobId":rewrite["jobId"],
                     "generatorCommit":rewrite["generatorCommit"],
                     "acceptedCommit":rewrite["acceptedCommit"],
                     "candidateId":rewrite["candidateId"],
                     "feedbackMethodFamilyId":family}
                ]
            }));
        }
    }
    None
}

/// Every consumer that needs a yes/no answer delegates to the SAME
/// source-replayed, receipt-bound proof object.
pub(crate) fn proven(state:&AppState,history:&[Value])->bool {
    evidence(state,history).is_some()
}

#[cfg(test)]
#[path = "rsi_recursive_closure/tests.rs"]
mod tests;
