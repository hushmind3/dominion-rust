//! Independent, matched NEXT-GENERATION source productivity for RSI edit methods.
//!
//! A policy that creates an accepted patch is NOT yet a better self-improver.
//! Compare the *first real compiled successor* from competing policies started
//! at the same source, provider/model, objective, domain, branch and frozen
//! oracle profile. Never rank by a child's prompt or a candidate without its
//! original paired receipt. Each base commit is ONE independent comparison.
//! Non-randomized matched cohorts are evidence for a measured preference,
//! not a scientific assertion that the method caused a general intelligence gain.
use crate::{engine::rsi,state::AppState};
use serde_json::{json,Value};
use std::collections::BTreeMap;

fn identity(v:&Value,names:&[&str])->Option<String>{
    let mut parts=Vec::new();
    for name in names{
        parts.push(v[*name].as_str().filter(|s|!s.is_empty())?);
    }
    Some(json!(parts).to_string())
}
fn scope(g:&Value)->Option<String>{
    identity(g,&["generatorCommit","sourceBranch","generatorProviderId",
        "generatorModel","objectiveId","knowledgePartition",
        "comparisonProfileSha256","domain"])
}
fn real_model_call(g:&Value)->bool {
    g["tokenUsage"]["calls"].as_u64().is_some_and(|n|n>0)
        && g["generatorProviderId"].as_str().is_some_and(|s|!s.is_empty()
            && s!="dominion-native")
        && g["generatorModel"].as_str().is_some_and(|s|!s.is_empty())
        && g["nativeMutation"].is_null()
        && (!g["modelRoute"].is_object()
            || (g["modelRouteExecuted"]==true
                && g["modelRoute"]["selected"]["providerId"]==g["generatorProviderId"]
                && g["modelRoute"]["selected"]["model"]==g["generatorModel"]))
}
fn shape(g:&Value)->bool {
    g["systemEvolution"]==true
        && g["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && g["generatorBinarySha256"].as_str().is_some_and(|s|s.len()==64)
        && g["generatorCommit"].as_str().is_some_and(|s|s.len()==40)
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40)
        && g["editPolicy"]["revision"].as_str().is_some_and(|s|!s.is_empty())
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["accepted"]==true
        && g["pairedRevalidated"]==true
        && g["evidenceVerified"]==true
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["editMethodExecutionProof"]["matchedAdoptedSourceWrite"]==true
        && g["acceptedSourceHashes"].as_object()
            .is_some_and(|m|!m.is_empty())
        && real_model_call(g)
}
fn eligible_child<'a>(g:&Value,child:&'a Value)->bool{
    child["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && child["jobId"]!=g["jobId"]
        && child["systemEvolution"]==true
        && child["generatorCommit"]==g["acceptedCommit"]
        && child["generatorBinarySha256"].as_str().is_some_and(|s|s.len()==64)
        && child["generatorBinarySha256"]!=g["generatorBinarySha256"]
        && child["sourceBranch"]==g["sourceBranch"]
        && child["generatorProviderId"]==g["generatorProviderId"]
        && child["generatorModel"]==g["generatorModel"]
        && child["objectiveId"]==g["objectiveId"]
        && child["knowledgePartition"]==g["knowledgePartition"]
        && child["comparisonProfileSha256"]==g["comparisonProfileSha256"]
        && child["domain"]==g["domain"]
        && real_model_call(child)
}
fn first_successor<'a>(g:&Value,history:&'a [Value])->Option<&'a Value>{
    history.iter().filter(|child|eligible_child(g,child))
        .min_by(|a,b|a["createdAt"].as_str().cmp(&b["createdAt"].as_str())
            .then_with(||a["jobId"].as_str().cmp(&b["jobId"].as_str())))
}
#[derive(Clone)]
struct Outcome {
    parent:String, child:String, accepted_commit:String,
    child_adopted:bool,
}
fn verified_generation(state:&AppState,g:&Value)->bool {
    let Some(id)=g["candidateId"].as_str() else{return false};
    let Some(c)=state.store.get("rsi_candidate",id) else{return false};
    g["evaluation"]["evidenceSha256"]==c["evaluation"]["evidenceSha256"]
        && g["generatorCommit"]==c["baseCommit"]
        && g["sourceBranch"]==c["sourceBranch"]
        && g["acceptedCommit"]==c["experimentAdoption"]["commit"]
        && g["candidateOrigin"]==c["origin"]
        && rsi::verified_pair_receipt(state,&c)
}
/// A proof can be recomputed by a NEW child using immutable candidate-bound
/// paired reports. The caller supplies this verification function in tests,
/// while live execution replays the original server receipt from the store.
fn compare_by(
    history:&[Value],profile:&str,domain:&str,
    verified:&dyn Fn(&Value)->bool,
)->Value{
    let mut cohorts=BTreeMap::<String,BTreeMap<String,Outcome>>::new();
    let mut ambiguous=BTreeMap::<String,bool>::new();
    for parent in history {
        if parent["comparisonProfileSha256"]!=profile || parent["domain"]!=domain
            || !shape(parent) || !verified(parent) {continue;}
        let Some(child)=first_successor(parent,history) else{continue};
        let Some(context)=scope(parent) else{continue};
        let Some(revision)=parent["editPolicy"]["revision"].as_str() else{continue};
        let row=Outcome {
            parent:parent["jobId"].as_str().unwrap_or("").to_owned(),
            child:child["jobId"].as_str().unwrap_or("").to_owned(),
            accepted_commit:parent["acceptedCommit"].as_str().unwrap_or("").to_owned(),
            child_adopted:shape(child) && verified(child),
        };
        let entry=cohorts.entry(context.clone()).or_default();
        if entry.insert(revision.to_owned(),row).is_some() {
            // More than one trial of a method at an identical base creates
            // a cherry-pick opportunity. Exclude the entire cohort.
            ambiguous.insert(context,true);
        }
    }
    let mut trials=BTreeMap::<(String,String),(u64,u64,u64,Vec<Value>)>::new();
    let mut compared=0_u64;
    for (key,policies) in &cohorts{
        if ambiguous.get(key)==Some(&true) || policies.len()<2{continue;}
        let alternatives=policies.iter().collect::<Vec<_>>();
        for (i,(left,lo)) in alternatives.iter().enumerate(){
            for (right,ro) in alternatives.iter().skip(i+1){
                if lo.accepted_commit==ro.accepted_commit
                    || lo.child==ro.child || lo.parent==ro.parent {continue;}
                let stats=trials.entry((left.to_string(),right.to_string()))
                    .or_insert((0,0,0,Vec::new()));
                if lo.child_adopted && !ro.child_adopted {stats.0+=1;}
                else if ro.child_adopted && !lo.child_adopted {stats.1+=1;}
                else{stats.2+=1;}
                stats.3.push(json!({
                    "sourceScopeSha256":crate::engine::memory::stable_id(&[key.as_str()]),
                    "leftParentJobId":lo.parent,"rightParentJobId":ro.parent,
                    "leftSuccessorJobId":lo.child,"rightSuccessorJobId":ro.child,
                    "leftRebuiltSuccessorAdopted":lo.child_adopted,
                    "rightRebuiltSuccessorAdopted":ro.child_adopted
                }));
                compared+=1;
            }
        }
    }
    let mut evidence=Vec::new();
    let mut best=None::<(String,String,u64)>;
    for ((a,b),(win_a,win_b,tie,pairs)) in trials{
        // Two different ORIGINAL source commits, independently rebuilt
        // successor sessions, and no contradictory matched context.
        let candidate=if win_a>=2 && win_b==0 {
            Some((a.clone(),b.clone(),win_a))
        }else if win_b>=2 && win_a==0 {
            Some((b.clone(),a.clone(),win_b))
        }else{None};
        evidence.push(json!({
            "aRevision":a,"bRevision":b,
            "aWins":win_a,"bWins":win_b,"ties":tie,
            "independentMatchedSourceContexts":win_a+win_b+tie,
            "firstSuccessors":pairs
        }));
        if let Some(winner)=candidate {
            if best.as_ref().is_none_or(|b|winner.2>b.2
                || (winner.2==b.2 && winner.0<b.0)){
                best=Some(winner);
            }
        }
    }
    match best{
        Some((winner,control,wins))=>json!({
            "status":"matched_rebuilt_successor_productivity_preference",
            "selectionEligible":true,
            "winnerRevision":winner,"controlRevision":control,
            "matchedWins":wins,"comparedContexts":compared,
            "comparisons":evidence,
            "causalSuperiorityProven":false,
            "generalIntelligenceImprovementProven":false,
            "source":"first_rebuilt_successor_independently_adopted_pair_v1"
        }),
        None=>json!({
            "status":"insufficient_independent_matched_successors",
            "selectionEligible":false,"comparedContexts":compared,
            "comparisons":evidence,
            "causalSuperiorityProven":false,
            "generalIntelligenceImprovementProven":false,
            "source":"first_rebuilt_successor_independently_adopted_pair_v1"
        })
    }
}
pub(crate) fn compare(
    state:&AppState,history:&[Value],profile:&str,domain:&str,
)->Value{
    compare_by(history,profile,domain,&|g|verified_generation(state,g))
}
/// Compiled descendant selects a measured controller instead of relying on
/// current patch success alone. The original candidate remains source bound;
/// this never changes the frozen independent oracle or source adoption.
pub(crate) fn choose(
    state:&AppState,history:&[Value],mut proposed:Value,
    profile:&str,domain:&str,
)->Value{
    let report=compare(state,history,profile,domain);
    if report["selectionEligible"]==true {
        if let Some(revision)=report["winnerRevision"].as_str(){
            if let Some(winner)=history.iter().find(|g|
                shape(g) && g["comparisonProfileSha256"]==profile
                    && g["domain"]==domain
                    && g["editPolicy"]["revision"]==revision
                    && verified_generation(state,g)) {
                let mut adopted=winner["editPolicy"].clone();
                adopted["productivitySelection"]=report;
                adopted["selectedFromGenerationId"]=winner["jobId"].clone();
                adopted["improvementProven"]=json!(false);
                return adopted;
            }
        }
    }
    proposed["productivitySelection"]=report;
    proposed
}

#[cfg(test)]
mod tests{
    use super::*;
    fn parent(id:&str,base:&str,head:&str,rev:&str,when:&str)->Value{
        json!({
            "jobId":id,"createdAt":when,"systemEvolution":true,
            "generatorCommit":base,"acceptedCommit":head,
            "generatorBinarySha256":"a".repeat(64),
            "sourceBranch":"dominion-sub2",
            "generatorProviderId":"provider","generatorModel":"model",
            "objectiveId":"objective","knowledgePartition":"partition",
            "comparisonProfileSha256":"oracle-a","domain":"learning",
            "tokenUsage":{"calls":1},"candidateId":id,
            "candidateOrigin":"model_driven_detached_worktree",
            "nativeMutation":null,"accepted":true,"pairedRevalidated":true,
            "evidenceVerified":true,
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":true}},
            "acceptedSourceHashes":{"src/engine/learning.rs":"b".repeat(64)},
            "editMethodExecutionProof":{"matchedAdoptedSourceWrite":true},
            "editPolicy":{"revision":rev,"method":"executable-edit"}
        })
    }
    fn successor(id:&str,head:&str,when:&str,adopted:bool)->Value {
        let mut child=parent(id,head,&"f".repeat(40),"any",when);
        child["generatorBinarySha256"]=json!("c".repeat(64));
        child["accepted"]=json!(adopted);
        child["evidenceVerified"]=json!(adopted);
        child
    }
    fn fixture()->Vec<Value>{
        let p1=parent("parent-a1",&"1".repeat(40),&"2".repeat(40),"control","2026-11-01");
        let p2=parent("parent-b1",&"1".repeat(40),&"3".repeat(40),"rewrite","2026-11-02");
        let p3=parent("parent-a2",&"4".repeat(40),&"5".repeat(40),"control","2026-11-03");
        let p4=parent("parent-b2",&"4".repeat(40),&"6".repeat(40),"rewrite","2026-11-04");
        let c1=successor("child-a1",&"2".repeat(40),"2026-11-05",false);
        let c2=successor("child-b1",&"3".repeat(40),"2026-11-06",true);
        let c3=successor("child-a2",&"5".repeat(40),"2026-11-07",false);
        let c4=successor("child-b2",&"6".repeat(40),"2026-11-08",true);
        vec![p1,p2,p3,p4,c1,c2,c3,c4]
    }
    fn verified(g:&Value)->bool{g["accepted"]==true && g["evidenceVerified"]==true}
    #[test]
    fn two_independent_matched_source_contexts_promote_only_rebuilt_successors(){
        let h=fixture();
        let proof=compare_by(&h,"oracle-a","learning",&verified);
        assert_eq!(proof["selectionEligible"],true);
        assert_eq!(proof["winnerRevision"],"rewrite");
        assert_eq!(proof["matchedWins"],2);
        assert_eq!(proof["causalSuperiorityProven"],false);
    }
    #[test]
    fn no_promotion_without_second_independent_source_context(){
        let h=fixture().into_iter().filter(|g|
            g["generatorCommit"]!="4444444444444444444444444444444444444444"
                && g["generatorCommit"]!="5555555555555555555555555555555555555555"
                && g["generatorCommit"]!="6666666666666666666666666666666666666666")
            .collect::<Vec<_>>();
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
    }
    #[test]
    fn mixed_model_unverified_child_or_no_real_calls_never_earns_preference(){
        let mut h=fixture();
        h[7]["generatorModel"]=json!("other-model");
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
        h=fixture();
        h[7]["tokenUsage"]["calls"]=json!(0);
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
        h=fixture();
        h[7]["pairedRevalidated"]=json!(false);
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
        h=fixture();
        h[7]["generatorBinarySha256"]=json!("a".repeat(64));
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
    }
    #[test]
    fn conflicting_matched_source_context_blocks_promotion(){
        let mut h=fixture();
        h[4]["accepted"]=json!(true);
        h[4]["evidenceVerified"]=json!(true);
        h[5]["accepted"]=json!(false);
        h[5]["evidenceVerified"]=json!(false);
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
    }
    #[test]
    fn never_choose_a_lucky_second_child_after_first_real_failure(){
        let mut h=fixture();
        h.push(successor("later-lucky",&"2".repeat(40),"2026-11-10",true));
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["winnerRevision"],"rewrite");
    }
    #[test]
    fn identity_and_scope_mismatch_do_not_form_false_pairs(){
        let mut h=fixture();
        h[1]["comparisonProfileSha256"]=json!("oracle-b");
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
        h=fixture();
        h[1]["generatorProviderId"]=json!("other-provider");
        assert_eq!(compare_by(&h,"oracle-a","learning",&verified)["selectionEligible"],false);
    }
}