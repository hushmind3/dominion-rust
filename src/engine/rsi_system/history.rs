//! Receipt-revalidated learning history; no mutable success labels.
use super::*;

/// Rebuild experimental learning labels from original immutable paired
/// receipts. A mutable generation JSON can report either a fabricated win
/// or a fabricated loss; neither should train a descendant's meta-method.
pub(crate) fn revalidated_method_history(
    state:&crate::state::AppState,history:&[Value]
)->Vec<Value>{
    history.iter().map(|generation|{
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
    }).collect()
}

