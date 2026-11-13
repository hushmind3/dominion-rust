//! The independent A -> B -> A proof gate. It reads original candidate
//! receipts and the ACTUAL child job, never trusts an agent's success label.
//! Model weight updates are neither required nor permitted by this path.
use crate::{engine::{rsi,rsi_system},state::AppState};
use serde_json::{json,Value};

fn hash(v:&Value,len:usize)->bool {
    v.as_str().is_some_and(|s|s.len()==len
        && s.bytes().all(|b|b.is_ascii_hexdigit()))
}

/// A generation whose real child produced the independently paired candidate
/// and whose exact accepted source hashes are recorded on that candidate.
fn source_adoption(state:&AppState,g:&Value)->Option<(Value,Value)> {
    if g["accepted"]!=true || g["pairedRevalidated"]!=true
        || g["sourceLineageVerified"]!=true
        || g["systemEvolution"]!=true || g["systemBlueprintVerified"]!=true
        || g["metaPolicyExecuted"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || !hash(&g["generatorCommit"],40)
        || !hash(&g["acceptedCommit"],40)
        || !hash(&g["generatorBinarySha256"],64) {
        return None;
    }
    let candidate=state.store.get("rsi_candidate",g["candidateId"].as_str()?)?;
    if candidate["rsiJobId"]!=g["jobId"]
        || candidate["origin"]!=g["candidateOrigin"]
        || candidate["baseCommit"]!=g["generatorCommit"]
        || candidate["status"]!="verified"
        || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || candidate["evaluation"]["metrics"]!=g["evaluation"]["metrics"]
        || candidate["evaluation"]["evidenceSha256"]!=g["evaluation"]["evidenceSha256"]
        || candidate["experimentAdoption"]["commit"]!=g["acceptedCommit"]
        || candidate["experimentAdoption"]["generatorCommit"]!=g["generatorCommit"]
        || candidate["experimentAdoption"]["acceptedSourceHashes"]!=g["acceptedSourceHashes"]
        || candidate["experimentAdoption"]["productionDeployed"]!=false
        || !rsi::verified_pair_receipt(state,&candidate)
        || candidate["evaluation"]["metrics"]["regressions"]!=0 {
        return None;
    }
    let job=state.store.get("job",g["jobId"].as_str()?)?;
    if job["id"]!=g["jobId"]
        || job["kind"]!="rsi_generation"
        || job["input"]["rsiGeneratorCommit"]!=g["generatorCommit"]
        || job["input"]["rsiDescendantPlan"]!=g["descendantPlan"]
        || job["input"]["rsiMetaPolicy"]!=g["metaPolicy"]
        || job["rsiSystemBlueprintVerified"]!=true
        || job["rsiMetaPolicyExecuted"]!=true {
        return None;
    }
    Some((candidate,job))
}

fn holdout_gain(candidate:&Value,domain:&str)->bool {
    let metrics=&candidate["evaluation"]["metrics"];
    metrics["improvedHoldouts"].as_u64().unwrap_or(0)>0
        && metrics["improvedHoldoutDomains"].as_array()
            .is_some_and(|rows|rows.iter().any(|row|
                row.as_str().is_some_and(|v|if domain=="improver"{
                    v=="improver"
                }else{v!="improver"})))
        && metrics["measurements"].as_array().is_some_and(|rows|
            rows.iter().any(|row|
                row["holdout"]==true
                && row["baselinePass"]==false
                && row["candidatePass"]==true
                && row["domain"].as_str().is_some_and(|v|
                    if domain=="improver"{v=="improver"}else{v!="improver"})))
}

fn changed(candidate:&Value,path:&str)->bool{
    candidate["paths"].as_array().is_some_and(|files|
        files.iter().any(|row|row.as_str()==Some(path)))
}

/// Four CONSECUTIVE improved and adopted source revisions: A edits a
/// compiled method, B uses A for a new task, A' rewrites the SAME method
/// from B's real measured outcome, and B' must run the revised A' policy
/// to improve the original downstream task family again.
pub(crate) fn evidence(state:&AppState,history:&[Value])->Value {
    // Re-play receipts, not stored success/failure flags, even when the
    // coordinator or API has copied a campaign into a display summary.
    let checked=rsi_system::revalidated_method_history(state,history);
    for (index,group) in checked.windows(4).enumerate() {
        let [a,b,c,d]=group else{continue};
        let (Some((ca,_ja)),Some((cb,jb)),Some((cc,jc)),Some((cd,jd)))=(
            source_adoption(state,a),source_adoption(state,b),
            source_adoption(state,c),source_adoption(state,d))else{continue};
        if a["acceptedCommit"]!=b["generatorCommit"]
            || b["acceptedCommit"]!=c["generatorCommit"]
            || c["acceptedCommit"]!=d["generatorCommit"]
            || a["generatorBinarySha256"]==b["generatorBinarySha256"]
            || b["generatorBinarySha256"]==c["generatorBinarySha256"]
            || c["generatorBinarySha256"]==d["generatorBinarySha256"]
            || ca["target"]!="improver" || cb["target"]=="improver"
            || cc["target"]!="improver" || cd["target"]=="improver"
            || !holdout_gain(&ca,"improver")
            || !holdout_gain(&cb,"other")
            || !holdout_gain(&cc,"improver")
            || !holdout_gain(&cd,"other")
            || d["domain"]!=b["domain"]
            || d["metaPolicy"]["mode"]!="reapply_revised_method_to_capability"
            || d["methodReapplicationExecuted"]!=true
            || jd["rsiMetaReapplicationExecuted"]!=true
            || jd["input"]["rsiMetaPolicy"]!=d["metaPolicy"]
            || d["metaMethodInheritance"]["parentAcceptedCommit"]
                !=c["acceptedCommit"]
            || d["metaMethodInheritance"]["observedPlanDifference"]!=true
            || d["metaMethodInheritance"]["chosenTargetActuallyFocused"]!=true
            || d["metaMethodOutcome"]["independentCandidateAdopted"]!=true
            || jd["rsiMetaMethodInheritance"]!=d["metaMethodInheritance"]
            || b["metaMethodInheritance"]["source"]
                !="source_bound_compiled_meta_method_inheritance_v1"
            || b["metaMethodInheritance"]["parentAcceptedCommit"]!=a["acceptedCommit"]
            || b["metaMethodInheritance"]["observedPlanDifference"]!=true
            || b["metaMethodInheritance"]["chosenTargetActuallyFocused"]!=true
            || b["metaMethodOutcome"]["independentCandidateAdopted"]!=true
            || jb["rsiMetaMethodInheritance"]!=b["metaMethodInheritance"]
            || c["returnMethodFeedbackExecuted"]!=true
            || jc["rsiMetaReturnExecuted"]!=true
            || c["metaPolicy"]["mode"]!="close_observed_downstream_method_loop"
            || jc["input"]["rsiFocusFiles"]!=c["descendantPlan"]["focusFiles"]
            || c["downstreamMethodFeedback"]!=c["descendantPlan"]["downstreamMethodFeedback"] {
            continue;
        }
        let expected=rsi_system::next_method_return(
            &checked[..index+2],
            &c["descendantPlan"]["architectureFrontier"],
            c["generatorCommit"].as_str().unwrap_or(""));
        if expected.is_null()
            || c["downstreamMethodFeedback"]!=expected
            || jc["input"]["rsiDescendantPlan"]["downstreamMethodFeedback"]!=expected {
            continue;
        }
        let Some(path)=expected["methodSourcePath"].as_str()else{continue};
        let before=&a["acceptedSourceHashes"][path];
        let after=&c["acceptedSourceHashes"][path];
        if !changed(&ca,path) || !changed(&cc,path)
            || changed(&cb,path)
            || !hash(before,64) || !hash(after,64) || before==after
            || &expected["methodSourceSha256"]!=before
            || expected["downstreamEvidenceSha256"]!=b["evaluation"]["evidenceSha256"]
            || !jc["input"]["rsiFocusFiles"].as_array()
                .is_some_and(|paths|paths.iter().any(|row|row.as_str()==Some(path))) {
            continue;
        }
        let expected_reapplication=rsi_system::next_capability_reapplication(
            &checked[..index+3],
            &d["descendantPlan"]["architectureFrontier"],
            d["generatorCommit"].as_str().unwrap_or(""));
        let Some(target)=expected_reapplication["target"]["path"].as_str()else{
            continue;
        };
        if expected_reapplication.is_null()
            || d["methodReapplication"]!=expected_reapplication
            || d["descendantPlan"]["methodReapplication"]!=expected_reapplication
            || jd["input"]["rsiDescendantPlan"]["methodReapplication"]
                !=expected_reapplication
            || d["metaPolicy"]["target"]!=expected_reapplication["target"]
            || !changed(&cb,target) || !changed(&cd,target)
            || changed(&cc,target)
            || expected_reapplication["originalDownstreamEvidenceSha256"]
                !=b["evaluation"]["evidenceSha256"]
            || &expected_reapplication["revisedMethodSourceSha256"]!=after
            || b["acceptedSourceHashes"][target]
                !=expected_reapplication["originalDownstreamTargetSourceSha256"]
            || d["acceptedSourceHashes"][target]
                ==b["acceptedSourceHashes"][target]
            || jd["input"]["rsiFocusFiles"].as_array().is_none_or(|files|
                !files.iter().any(|f|f.as_str()==Some(target))) {
            continue;
        }
        return json!({
            "source":"replayed_executed_method_capability_method_capability_closure_v2",
            "proven":true,
            "shape":"method_A -> capability_B -> improved_method_A -> improved_capability_B",
            "candidateIds":[ca["id"],cb["id"],cc["id"],cd["id"]],
            "jobIds":[a["jobId"],b["jobId"],c["jobId"],d["jobId"]],
            "acceptedCommits":[a["acceptedCommit"],b["acceptedCommit"],c["acceptedCommit"],d["acceptedCommit"]],
            "methodSourcePath":path,
            "methodSourceBeforeSha256":before,
            "methodSourceAfterSha256":after,
            "downstreamIndependentReceipt":b["evaluation"]["evidenceSha256"],
            "reapplicationIndependentReceipt":d["evaluation"]["evidenceSha256"],
            "reappliedCapabilitySourcePath":target,
            "actualCompiledReturnExecution":true,
            "actualCompiledReapplicationExecution":true,
            "independentPairedRevalidation":true,
            "causalMethodSuperiorityProven":false,
            "unboundedGrowthProven":false
        });
    }
    json!({"source":"replayed_executed_method_capability_method_capability_closure_v2",
        "proven":false,"requires":"four contiguous independently adopted revisions: method A -> assessed B using A -> measured revision A' using B evidence -> assessed B' running revised A' and improving B's capability again"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_source_labels_do_not_prove_closed_recursion() {
        let cfg_root=std::env::temp_dir().join(format!("dominion-closure-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{
            port:0,data_dir:cfg_root.clone(),web_dir:cfg_root.join("web"),
            workspace:cfg_root.clone()
        };
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let fake=(0..3).map(|i|json!({
            "id":format!("fake-{i}"),"accepted":true,
            "sourceLineageVerified":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40)
        })).collect::<Vec<_>>();
        assert_eq!(evidence(&state,&fake)["proven"],false);
        assert_eq!(evidence(&state,&[])["proven"],false);
        let _=std::fs::remove_dir_all(cfg_root);
    }
}
