//! Independently revalidated historical RSI evidence, separate from the
//! descendant's mutable system modeling and target-selection algorithms.
//! Raw candidate receipts and job inputs, not editable lineage flags, own
//! evaluation eligibility and attribution.
use serde_json::{json,Value};

/// Factorial feedback is reconstructed from a stored, measured candidate
/// and the original generation-job input. A descendant cannot teach its next
/// generation a fabricated experiment phase by changing a history JSON flag.
/// This remains only experimental evidence, not proof of causal improvement.
pub(crate) fn verified_factorial_receipt(
    generation:&Value,candidate:&Value,job:&Value
)->(Value,bool){
    let plan=&generation["factorialExperiment"];
    let profile=crate::engine::rsi_lineage_transfer::profile(&job["input"]);
    let bound=job["kind"]=="rsi_generation"
        && job["id"]==generation["jobId"]
        && job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]
        && plan["source"]=="compiled_open_topology_counterfactual_lab_v1"
        && plan["generatorCommit"]==generation["generatorCommit"]
        && profile.as_ref().is_some_and(|hash|
            plan["comparisonProfileSha256"].as_str()==Some(hash.as_str()))
        && job["input"]["rsiFactorialExperiment"]==*plan
        && candidate["baseCommit"]==generation["generatorCommit"]
        && candidate["evaluation"]["metrics"]["measurements"].as_array()
            .is_some_and(|cases|cases.iter().any(|case|
                case["holdout"]==true
                && case["id"].as_str().is_some_and(|id|!id.is_empty())
                && case["baselinePass"].is_boolean()
                && case["candidatePass"].is_boolean()));
    if !bound{return (Value::Null,false);}
    let paths=candidate["paths"].as_array().cloned().unwrap_or_default();
    let compliance=crate::engine::rsi_intervention_lab::compliance(plan,&paths);
    let eligible=compliance["eligibleForFactorial"]==true;
    (compliance,eligible)
}

/// The planner must derive its adoption flags from the original paired
/// receipts, not from a mutable generation record or a model's claims.
pub(crate) fn revalidated_history(
    state:&crate::state::AppState,history:&[Value]
)->Vec<Value>{
    history.iter().map(|generation|{
        let mut checked=generation.clone();
        // Revalidate the frozen raw paired receipt for BOTH accepted and
        // rejected candidates. Rejected attempts carry useful prequential
        // negative labels; their mutable history flags are not evidence.
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id));
        let measured=candidate.as_ref().is_some_and(|candidate|{
            candidate["pairedComparisonComplete"]==true
                && candidate["id"]==generation["candidateId"]
                && candidate["baseCommit"]==generation["generatorCommit"]
                && candidate["evaluation"]["source"]
                    =="server_invoked_local_paired_git_worktrees"
                && candidate["evaluation"]["evidenceSha256"]
                    ==generation["evaluation"]["evidenceSha256"]
                && candidate["evaluation"]["metrics"]
                    ==generation["evaluation"]["metrics"]
                && candidate["evaluation"]["verdict"]
                    ==generation["evaluation"]["verdict"]
                && crate::engine::rsi::replayed_pair_metrics(state,candidate)
                    .is_some_and(|metrics|metrics==candidate["evaluation"]["metrics"])
        });
        checked["pairedMeasured"]=json!(measured);
        // Bind the frozen-case profile and source-writing actor to the
        // original job rather than trusting editable lineage annotations.
        let job=generation["jobId"].as_str()
            .and_then(|id|state.store.get("job",id))
            .filter(|job|job["kind"]=="rsi_generation"
                && job["id"]==generation["jobId"]
                && job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]);
        if let Some(original)=job.as_ref(){
            checked["comparisonProfileSha256"]=json!(
                crate::engine::rsi_lineage_transfer::profile(&original["input"]));
            checked["modelEcologyExecuted"]=original["rsiModelEcologyExecuted"].clone();
            checked["generatorProviderId"]=original["input"]["providerId"].clone();
            checked["generatorModel"]=original["input"]["model"].clone();
        }else{
            checked["comparisonProfileSha256"]=Value::Null;
            checked["modelEcologyExecuted"]=Value::Null;
            checked["generatorProviderId"]=Value::Null;
            checked["generatorModel"]=Value::Null;
        }
        if measured {
            // The actual changed paths originate in the persisted evaluated
            // candidate, not model-written generation JSON.
            if let Some(candidate)=&candidate {
                checked["changedSourcePaths"]=candidate["paths"].clone();
            }
        }
        let (compliance,factorial_measured)=match (
            measured.then_some(()).and(candidate.as_ref()),job.as_ref()
        ){
            (Some(candidate),Some(job))=>
                verified_factorial_receipt(generation,candidate,job),
            _=>(Value::Null,false),
        };
        checked["factorialCompliance"]=compliance;
        checked["factorialMeasured"]=json!(factorial_measured);
        let valid=if generation["accepted"]==true {
            measured && candidate.as_ref().is_some_and(|candidate|{
                candidate["status"]=="verified"
                    && candidate["evaluation"]["metrics"]["eligible"]==true
                    && candidate["experimentAdoption"]["commit"]==generation["acceptedCommit"]
                    && candidate["experimentAdoption"]["generatorCommit"]==generation["generatorCommit"]
                    && candidate["experimentAdoption"]["productionDeployed"]==false
            })
        }else{false};
        checked["pairedRevalidated"]=json!(valid);
        // Every compiled successor observes the same replayable control that
        // the fixed campaign/API oracle will later use. It is dynamic so a
        // queued control can complete after an earlier generation was stored.
        checked["methodControlEvidence"]=if valid
            &&generation["candidateOrigin"]=="model_driven_detached_worktree" {
            crate::engine::rsi_closure::control_feedback(state,generation)
        }else{Value::Null};
        // Rewrite recorded prediction labels from the still-present original
        // paired receipt and executed plan on every history load. A stale or
        // hand-edited Brier label cannot change the next compiled planner.
        if generation["metaLearning"]["source"]
            =="source_bound_prequential_feedback_v1"{
            checked["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
                &checked["metaPolicy"],&checked);
        }
        checked
    }).collect()
}


