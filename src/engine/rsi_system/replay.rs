//! Reconstruct independent paired candidate receipts for descendant policy
//! learning. Reject labels not backed by the original stored evaluation.
use super::*;

/// Reconstruct source-bound training labels from the original immutable
/// paired record. Rejected edits can inform prediction, but model-written
/// generation flags cannot create an outcome or repair their own evidence.
pub(crate) fn revalidated_history(
    state:&crate::state::AppState,history:&[Value]
)->Vec<Value>{
    history.iter().map(|generation|{
        let mut checked=generation.clone();
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id));
        let job_matches=generation["jobId"].as_str()
            .and_then(|id|state.store.get("job",id)).is_some_and(|job|
                job["kind"]=="rsi_generation"
                && job["input"]["rsiControllerPlan"]==generation["controllerPlan"]
                && job["input"]["rsiMetaPolicy"]==generation["metaPolicy"]
                && job["input"]["rsiFocusFiles"]==generation["focusFiles"]
                && job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]);
        let measured=job_matches && candidate.as_ref().is_some_and(|candidate|
            candidate["pairedComparisonComplete"]==true
                && candidate["id"]==generation["candidateId"]
                && candidate["baseCommit"]==generation["generatorCommit"]
                && candidate["origin"]==generation["candidateOrigin"]
                && candidate["evaluation"]["source"]==
                    "server_invoked_local_paired_git_worktrees"
                && candidate["evaluation"]["evidenceSha256"]
                    ==generation["evaluation"]["evidenceSha256"]
                && candidate["evaluation"]["metrics"]
                    ==generation["evaluation"]["metrics"]
                && candidate["evaluation"]["verdict"]
                    ==generation["evaluation"]["verdict"]
                && crate::engine::rsi::replayed_pair_metrics(state,candidate)
                    .is_some_and(|metrics|metrics==candidate["evaluation"]["metrics"]));
        checked["pairedMeasured"]=json!(measured);
        checked["pairedReceiptReplayed"]=json!(measured);
        checked["changedSourcePaths"]=if measured {
            candidate.as_ref().map(|candidate|candidate["paths"].clone())
                .unwrap_or(Value::Null)
        }else{Value::Null};
        let adopted=measured && generation["accepted"]==true
            && candidate.as_ref().is_some_and(|candidate|
                candidate["status"]=="verified"
                && candidate["evaluation"]["metrics"]["eligible"]==true
                && candidate["experimentAdoption"]["commit"]==generation["acceptedCommit"]
                && candidate["experimentAdoption"]["generatorCommit"]==generation["generatorCommit"]
                && candidate["experimentAdoption"]["productionDeployed"]==false);
        checked["pairedRevalidated"]=json!(adopted);
        // Rebuild the meta experimental-choice provenance from the ORIGINAL
        // compiled worker. Never train a source-mutating policy on a copied
        // generation field that claims a control arm ran.
        let original=generation["jobId"].as_str()
            .and_then(|id|state.store.get("job",id));
        let policy_bound=job_matches && original.as_ref().is_some_and(|job|
            crate::engine::rsi_evolve::meta_experiment_policy::decode(
                &job["input"]["rsiExperimentPolicy"],
                job["input"]["rsiGeneratorCommit"].as_str().unwrap_or(""),
                &job["input"]).is_some()
            && job["rsiExperimentPolicyExecuted"]["source"]
                =="running_compiled_descendant_experiment_designer_v1"
            && job["rsiExperimentPolicyExecuted"]["generatorCommit"]
                ==generation["generatorCommit"]
            && job["rsiExperimentPolicyExecuted"]["policySha256"]
                ==job["input"]["rsiExperimentPolicy"]["signatureSha256"]
            && job["rsiExperimentPolicyExecuted"]["protocol"]
                ==job["input"]["rsiExperimentPolicy"]["selectedProtocol"]);
        checked["experimentPolicy"]=if policy_bound {
            original.as_ref().map(|job|job["input"]["rsiExperimentPolicy"].clone())
                .unwrap_or(Value::Null)
        }else{Value::Null};
        checked["experimentPolicyExecuted"]=if policy_bound {
            original.as_ref().map(|job|job["rsiExperimentPolicyExecuted"].clone())
                .unwrap_or(Value::Null)
        }else{Value::Null};
        // Always reconstruct controlled-method labels from the original OFF
        // job and BOTH independent paired candidate receipts. Descendant
        // learning must never trust a copied methodAblation JSON claim.
        checked["methodAblation"]=if adopted {
            crate::engine::rsi_evolve::measured_method_ablation(state,&checked)
        } else {Value::Null};
        if checked["metaPolicy"]["source"]=="compiled_editable_meta_controller_v1" {
            checked["metaLearning"]=crate::engine::rsi_prequential::feedback(
                &checked["metaPolicy"],&checked);
            checked["predictionRepairEvidence"]=
                crate::engine::rsi_prequential::repair_execution_evidence(&checked);
        }
        checked
    }).collect()
}

