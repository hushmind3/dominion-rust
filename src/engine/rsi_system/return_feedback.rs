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

/// After A -> B -> A', require a *new compiled child* to put A' back
/// to work on the SAME downstream capability. A' is an adopted executable
/// source revision, not a declarative policy name.
pub(crate) fn next_capability_reapplication(
    checked:&[Value],frontier:&Value,commit:&str
)->Value {
    let Some(c)=checked.last().filter(|row|
        row["accepted"]==true
            && row["sourceLineageVerified"]==true
            && row["pairedRevalidated"]==true
            && row["acceptedCommit"]==commit
            && row["metaPolicy"]["mode"]=="close_observed_downstream_method_loop"
            && row["returnMethodFeedbackExecuted"]==true
            && row["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)>0)else{return Value::Null;};
    let Some(b)=checked[..checked.len()-1].last().filter(|row|
        row["accepted"]==true && row["sourceLineageVerified"]==true
            && row["pairedRevalidated"]==true
            && row["acceptedCommit"]==c["generatorCommit"]
            && row["id"]==c["downstreamMethodFeedback"]["downstreamGenerationId"]
            && row["candidateId"]==c["downstreamMethodFeedback"]["downstreamCandidateId"]
            && row["evaluation"]["evidenceSha256"]
                ==c["downstreamMethodFeedback"]["downstreamEvidenceSha256"]
            && row["domain"]!="improver")else{return Value::Null;};
    let revalidated=next_method_return(&checked[..checked.len()-1],
        &c["descendantPlan"]["architectureFrontier"],
        c["generatorCommit"].as_str().unwrap_or(""));
    if revalidated.is_null()
        || c["downstreamMethodFeedback"]!=revalidated
        || c["descendantPlan"]["downstreamMethodFeedback"]!=revalidated {
        return Value::Null;
    }
    let Some(method_path)=revalidated["methodSourcePath"].as_str()else{
        return Value::Null;
    };
    let new_method=&c["acceptedSourceHashes"][method_path];
    if !sha(new_method,64) || new_method==&revalidated["methodSourceSha256"]
        || !c["systemDesign"]["targetFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|file.as_str()==Some(method_path))) {
        return Value::Null;
    }
    let Some(options)=frontier.as_array()else{return Value::Null;};
    let Some(targets)=b["systemDesign"]["targetFiles"].as_array()else{
        return Value::Null;
    };
    for target in options {
        let Some(path)=target["path"].as_str()else{continue};
        if target["domain"]!=b["domain"]
            || target["role"]=="improvement_methods"
            || !targets.iter().any(|t|t.as_str()==Some(path))
            || target["sourceSha256"]!=b["acceptedSourceHashes"][path]
            || !sha(&target["sourceSha256"],64) {
            continue;
        }
        return json!({
            "source":"replayed_revised_method_capability_reapplication_v1",
            "target":target,
            "originalDownstreamDomain":b["domain"],
            "originalDownstreamGenerationId":b["id"],
            "originalDownstreamCandidateId":b["candidateId"],
            "originalDownstreamEvidenceSha256":b["evaluation"]["evidenceSha256"],
            "revisedMethodGenerationId":c["id"],
            "revisedMethodCandidateId":c["candidateId"],
            "revisedMethodCommit":c["acceptedCommit"],
            "revisedMethodSourcePath":method_path,
            "revisedMethodSourceSha256":new_method,
            "originalMethodSourceSha256":revalidated["methodSourceSha256"],
            "originalDownstreamTargetSourceSha256":target["sourceSha256"],
            "causalImprovementProven":false
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

    #[test]
    fn adopted_method_revision_must_reenter_the_original_downstream_domain() {
        let method="src/engine/rsi_meta_controller/peer_transfer.rs";
        let task="src/engine/memory.rs";
        let initial=json!({
            "id":"b","candidateId":"candidate-b",
            "accepted":true,"pairedRevalidated":true,
            "sourceLineageVerified":true,
            "acceptedCommit":"c".repeat(40),
            "domain":"memory","systemDesign":{"targetFiles":[task]},
            "acceptedSourceHashes":{"src/engine/memory.rs":"f".repeat(64)},
            "evaluation":{"evidenceSha256":"e".repeat(64),"metrics":{
                "eligible":true,"regressions":0,"improvedHoldouts":1,
                "improvedHoldoutDomains":["memory"]}},
            "metaMethodInheritance":{
                "source":"source_bound_compiled_meta_method_inheritance_v1",
                "rebuildConfirmed":true,"observedPlanDifference":true,
                "chosenTargetActuallyFocused":true,
                "parentAcceptedCommit":"a".repeat(40),
                "sourceChanges":[{"path":method,
                    "acceptedSourceSha256":"b".repeat(64)}]},
            "metaMethodOutcome":{
                "source":"observed_compiled_meta_method_successor_outcome_v1",
                "independentlyAssessed":true,"independentCandidateAdopted":true,
                "actuallyFocused":true,
                "inheritedParentCommit":"a".repeat(40)}
        });
        let c_frontier=json!([{"role":"improvement_methods",
            "domain":"improver","path":method,"sourceSha256":"b".repeat(64)}]);
        let feedback=next_method_return(&[initial.clone()],&c_frontier,&"c".repeat(40));
        assert!(feedback.is_object());
        let c=json!({
            "id":"c","candidateId":"candidate-c",
            "accepted":true,"pairedRevalidated":true,
            "sourceLineageVerified":true,
            "acceptedCommit":"d".repeat(40),
            "generatorCommit":"c".repeat(40),
            "metaPolicy":{"mode":"close_observed_downstream_method_loop"},
            "returnMethodFeedbackExecuted":true,
            "downstreamMethodFeedback":feedback,
            "descendantPlan":{"architectureFrontier":c_frontier,
                "downstreamMethodFeedback":feedback},
            "systemDesign":{"targetFiles":[method]},
            "acceptedSourceHashes":{
                "src/engine/rsi_meta_controller/peer_transfer.rs":"d".repeat(64)},
            "evaluation":{"metrics":{"metaHoldoutGains":1}}
        });
        let d_frontier=json!([{"role":"memory","domain":"memory",
            "path":task,"sourceSha256":"f".repeat(64)}]);
        let reapply=next_capability_reapplication(
            &[initial.clone(),c.clone()],&d_frontier,&"d".repeat(40));
        assert_eq!(reapply["target"]["path"],task);
        assert_eq!(reapply["revisedMethodSourcePath"],method);
        let mut changed=c.clone();
        changed["evaluation"]["metrics"]["metaHoldoutGains"]=json!(0);
        assert!(next_capability_reapplication(
            &[initial.clone(),changed],&d_frontier,&"d".repeat(40)).is_null());
        let mut wrong=d_frontier;
        wrong[0]["sourceSha256"]=json!("0".repeat(64));
        assert!(next_capability_reapplication(
            &[initial,c],&wrong,&"d".repeat(40)).is_null());
    }
}
