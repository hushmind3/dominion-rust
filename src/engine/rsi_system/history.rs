//! Receipt-revalidated learning history; no mutable success labels.
use serde_json::{json,Value};

/// Rebuild experimental learning labels from original immutable paired
/// receipts. A mutable generation JSON can report either a fabricated win
/// or a fabricated loss; neither should train a descendant's meta-method.
pub(crate) fn revalidated_method_history(
    state:&crate::state::AppState,history:&[Value]
)->Vec<Value>{
    let mut checked=history.iter().map(|generation|{
        let mut checked=generation.clone();
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id));
        let measured=candidate.as_ref().is_some_and(|candidate|
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
                    .is_some_and(|metrics|metrics==candidate["evaluation"]["metrics"]));
        checked["pairedMeasured"]=json!(measured);
        if !measured {
            // Preserve operational failure details, but never invent a
            // benchmark outcome or Brier training label without a receipt.
            checked["evaluation"]["verdict"]=Value::Null;
            // Do not leave fabricated public-case measurements available to
            // the prequential learner after rejecting their source receipt.
            checked["evaluation"]["metrics"]=Value::Null;
            checked["metaLearning"]=Value::Null;
        }
        let accepted=measured && generation["accepted"]==true
            && candidate.as_ref().is_some_and(|candidate|
                candidate["status"]=="verified"
                    && candidate["evaluation"]["metrics"]["eligible"]==true
                    && candidate["experimentAdoption"]["commit"]
                        ==generation["acceptedCommit"]
                    && candidate["experimentAdoption"]["generatorCommit"]
                        ==generation["generatorCommit"]
                    && candidate["experimentAdoption"]["productionDeployed"]==false
                    && crate::engine::rsi_system::design_implemented(
                        &generation["systemDesign"],
                        &candidate["paths"].as_array().cloned().unwrap_or_default()));
        checked["accepted"]=json!(accepted);
        checked["pairedRevalidated"]=json!(accepted);
        if measured {
            // Feedback is recomputed against the revalidated measurements,
            // never inherited as an untrusted historical score.
            checked["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
                &checked["metaPolicy"],&checked);
        }
        checked
    }).collect::<Vec<_>>();
    // Reconstruct inherited method evidence AFTER replaying original frozen
    // receipts, so mutable generation fields cannot fabricate a learned win.
    let independently_checked=checked.clone();
    for row in &mut checked {
        let observed=crate::engine::rsi_system::inherited_method_receipt(
            &independently_checked,&row["systemBlueprint"],&row["metaPolicy"],
            row["generatorCommit"].as_str().unwrap_or(""),&row["focusFiles"]);
        if observed.is_null()
            || row["metaMethodInheritance"]!=observed
            || row["systemBlueprintVerified"]!=true
            || row["descendantPlan"]["source"]
                !="compiled_descendant_system_controller_v3"
            || row["descendantPlan"]["systemBlueprint"]!=row["systemBlueprint"] {
            row["metaMethodInheritance"]=Value::Null;
            row["metaMethodOutcome"]=Value::Null;
            continue;
        }
        row["metaMethodInheritance"]=observed;
        row["metaMethodOutcome"]=
            crate::engine::rsi_system::observed_method_outcome(row);
    }
    checked
}

