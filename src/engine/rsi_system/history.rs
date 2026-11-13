//! Receipt-revalidated learning history; no mutable success labels.
use serde_json::{json,Value};

/// A candidate belongs to the actual child that produced its paired diff.
/// A valid receipt from ANOTHER job must never become this generation's
/// experimental feedback or ancestor source evidence.
fn candidate_from_generation(candidate:&Value,generation:&Value)->bool {
    candidate["id"]==generation["candidateId"]
        && candidate["rsiJobId"].as_str().is_some_and(|id|!id.is_empty())
        && candidate["rsiJobId"]==generation["jobId"]
        && candidate["origin"]==generation["candidateOrigin"]
        && candidate["baseCommit"]==generation["generatorCommit"]
}

/// The adoption action stores the bytes hashed from the committed lab source
/// in its own candidate record. A mutable generation summary cannot invent
/// a new adopted method revision by supplying its own acceptedSourceHashes.
fn adopted_source_lineage_matches(candidate:&Value,generation:&Value)->bool {
    let Some(sealed)=candidate["experimentAdoption"]["acceptedSourceHashes"].as_object()
        else{return false;};
    let Some(summary)=generation["acceptedSourceHashes"].as_object()
        else{return false;};
    let Some(paths)=candidate["paths"].as_array()else{return false;};
    !sealed.is_empty() && sealed==summary
        && candidate["experimentAdoption"]["commit"]==generation["acceptedCommit"]
        && candidate["experimentAdoption"]["generatorCommit"]==generation["generatorCommit"]
        && sealed.iter().all(|(path,hash)|
            crate::engine::rsi::safe_path(path)
                && paths.iter().any(|item|item.as_str()==Some(path))
                && hash.as_str().is_some_and(|sha|
                    sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))
}

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
                && candidate_from_generation(candidate,generation)
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
        // Only new, candidate-sealed source adoption receipts may teach
        // compiled descendant methods that an inherited revision exists.
        // Older measured generations remain usable for task-level feedback.
        checked["sourceLineageVerified"]=json!(accepted
            && candidate.as_ref().is_some_and(|candidate|
                adopted_source_lineage_matches(candidate,generation)));
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_lineage_requires_original_child_and_candidate_seal(){
        let path="src/engine/rsi_meta_controller/peer_transfer.rs";
        let hashes=json!({"src/engine/rsi_meta_controller/peer_transfer.rs":
            "b".repeat(64)});
        let generation=json!({"candidateId":"c","jobId":"child-1",
            "candidateOrigin":"model_driven_detached_worktree",
            "generatorCommit":"a".repeat(40),
            "acceptedCommit":"c".repeat(40),
            "acceptedSourceHashes":hashes});
        let candidate=json!({"id":"c","rsiJobId":"child-1",
            "origin":"model_driven_detached_worktree",
            "baseCommit":"a".repeat(40),"paths":[path],
            "experimentAdoption":{"commit":"c".repeat(40),
                "generatorCommit":"a".repeat(40),
                "acceptedSourceHashes":generation["acceptedSourceHashes"]}});
        assert!(candidate_from_generation(&candidate,&generation));
        assert!(adopted_source_lineage_matches(&candidate,&generation));
        let mut foreign=generation.clone();
        foreign["jobId"]=json!("child-2");
        assert!(!candidate_from_generation(&candidate,&foreign));
        let mut inflated=generation.clone();
        inflated["acceptedSourceHashes"][path]=json!("d".repeat(64));
        assert!(!adopted_source_lineage_matches(&candidate,&inflated));
        let mut extra=candidate.clone();
        extra["experimentAdoption"]["acceptedSourceHashes"]
            ["src/engine/memory.rs"]=json!("e".repeat(64));
        assert!(!adopted_source_lineage_matches(&extra,&generation));
    }
}
