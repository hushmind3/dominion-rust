//! Executable A -> B -> A return target from independently replayed B.
//! This is an editable descendant source selector, NOT the frozen evaluator.
use serde_json::{json,Value};

fn sha(v:&Value,n:usize)->bool{
    v.as_str().is_some_and(|s|s.len()==n
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}

/// A changed meta-method A actually ran in B, where B independently
/// improved a distinct capability. Feed that observation back into a
/// version-pinned rewrite of A, in the newly compiled descendant.
pub(crate) fn next_method_return(history:&[Value],frontier:&Value,commit:&str)->Value {
    // A couple of failed C attempts may retry without counting as adopted
    // progress. A new accepted B or method revision invalidates this signal.
    let Some(b)=history.iter().rev().take(4).find(|row|
        row["accepted"]==true && row["sourceLineageVerified"]==true
            && row["acceptedCommit"]==commit) else{return Value::Null;};
    let inherited=&b["metaMethodInheritance"];
    let observation=&b["metaMethodOutcome"];
    let metrics=&b["evaluation"]["metrics"];
    if b["pairedRevalidated"]!=true
        || b["candidateOrigin"]!="model_driven_detached_worktree"
        || b["domain"]=="improver"
        || !sha(&b["evaluation"]["evidenceSha256"],64)
        || metrics["eligible"]!=true || metrics["regressions"]!=0
        || metrics["improvedHoldouts"].as_u64().unwrap_or(0)==0
        || !metrics["improvedHoldoutDomains"].as_array().is_some_and(|rows|
            rows.iter().any(|r|r.as_str().is_some_and(|d|d!="improver")))
        || inherited["source"]!="source_bound_compiled_meta_method_inheritance_v1"
        || inherited["rebuildConfirmed"]!=true
        || inherited["observedPlanDifference"]!=true
        || inherited["chosenTargetActuallyFocused"]!=true
        || observation["source"]!="observed_compiled_meta_method_successor_outcome_v1"
        || observation["independentlyAssessed"]!=true
        || observation["independentCandidateAdopted"]!=true
        || observation["actuallyFocused"]!=true
        || observation["inheritedParentCommit"]!=inherited["parentAcceptedCommit"] {
        return Value::Null;
    }
    let Some(options)=frontier.as_array()else{return Value::Null;};
    let Some(changes)=inherited["sourceChanges"].as_array()else{return Value::Null;};
    for change in changes {
        let (Some(path),Some(hash))=(
            change["path"].as_str(),
            change["acceptedSourceSha256"].as_str()
        )else{continue};
        if !crate::engine::rsi::safe_path(path)
            || !sha(&change["acceptedSourceSha256"],64)
            || b["acceptedSourceHashes"][path].is_string()
            || b["systemDesign"]["targetFiles"].as_array().is_some_and(
                |rows|rows.iter().any(|row|row.as_str()==Some(path))) {
            continue;
        }
        let Some(target)=options.iter().find(|row|
            row["role"]=="improvement_methods"
                && row["domain"]=="improver"
                && row["path"]==path
                && row["sourceSha256"]==hash) else{continue};
        let retries=history.iter().rev().take(4).filter(|row|
            row["accepted"]!=true
                && row["generatorCommit"]==commit
                && row["focusFiles"].as_array().is_some_and(|files|
                    files.iter().any(|file|file.as_str()==Some(path))))
            .count();
        if retries>=2 {continue;}
        return json!({
            "source":"replayed_independent_downstream_method_feedback_v1",
            "target":target,
            "methodSourcePath":path,
            "methodSourceSha256":hash,
            "inheritedMethodCommit":inherited["parentAcceptedCommit"],
            "downstreamAcceptedCommit":b["acceptedCommit"],
            "downstreamCandidateId":b["candidateId"],
            "downstreamGenerationId":b["id"],
            "downstreamChildJobId":b["jobId"],
            "downstreamEvidenceSha256":b["evaluation"]["evidenceSha256"],
            "downstreamImprovedDomains":metrics["improvedHoldoutDomains"],
            "observedCompiledMethodUse":true,
            "independentPairedAdoption":true,
            "causalMethodSuperiorityProven":false
        });
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_replayed_adopted_downstream_improvement_returns_to_same_method() {
        let path="src/engine/rsi_meta_controller/peer_transfer.rs";
        let b=json!({
            "id":"b","jobId":"child-b","candidateId":"cand-b",
            "accepted":true,"pairedRevalidated":true,
            "sourceLineageVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":"c".repeat(40),
            "domain":"memory","systemDesign":{"targetFiles":["src/engine/memory.rs"]},
            "evaluation":{"evidenceSha256":"e".repeat(64),"metrics":{
                "eligible":true,"regressions":0,"improvedHoldouts":1,
                "improvedHoldoutDomains":["memory"]}},
            "acceptedSourceHashes":{"src/engine/memory.rs":"f".repeat(64)},
            "metaMethodInheritance":{
                "source":"source_bound_compiled_meta_method_inheritance_v1",
                "rebuildConfirmed":true,"observedPlanDifference":true,
                "chosenTargetActuallyFocused":true,
                "parentAcceptedCommit":"a".repeat(40),
                "sourceChanges":[{"path":path,
                    "acceptedSourceSha256":"b".repeat(64)}]},
            "metaMethodOutcome":{
                "source":"observed_compiled_meta_method_successor_outcome_v1",
                "independentlyAssessed":true,"independentCandidateAdopted":true,
                "actuallyFocused":true,"inheritedParentCommit":"a".repeat(40)}
        });
        let frontier=json!([{"role":"improvement_methods",
            "domain":"improver","path":path,"sourceSha256":"b".repeat(64)}]);
        let result=next_method_return(&[b.clone()],&frontier,&"c".repeat(40));
        assert_eq!(result["methodSourcePath"],path);
        assert_eq!(result["downstreamCandidateId"],"cand-b");
        let mut fake=b.clone();
        fake["sourceLineageVerified"]=json!(false);
        assert!(next_method_return(&[fake],&frontier,&"c".repeat(40)).is_null());
        let mut wrong=frontier.clone();
        wrong[0]["sourceSha256"]=json!("d".repeat(64));
        assert!(next_method_return(&[b],&wrong,&"c".repeat(40)).is_null());
    }
}
