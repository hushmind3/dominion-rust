//! Source-grounded closure audit. Stored plans or adopted commits alone cannot
//! prove RSI. Require independent holdout gains AND a causal, compiled-policy
//! behavior difference that changed the next descendant's actual edits.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn meta_source(path:&str)->bool {
    path=="src/engine/rsi_evolve.rs" ||
    path=="src/engine/rsi_meta_controller.rs" ||
    path=="src/engine/rsi_program_graph.rs" ||
    path=="src/engine/rsi_system.rs" ||
    path=="src/engine/algorithms.rs" ||
    path.starts_with("src/engine/rsi/") ||
    path=="src/engine/rsi_reflexive.rs" ||
    path=="src/engine/rsi_factorial.rs" ||
    path=="src/engine/rsi_mutual_program.rs"
}
fn paths(g:&Value)->Vec<&str> {
    g["changedSourcePaths"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect()
}
fn independently_adopted(state:&AppState,g:&Value)->bool {
    if g["accepted"]!=true || g["systemEvolution"]!=true ||
        g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary" ||
        g["compiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"] ||
        g["controllerPlan"]["generatorCommit"]!=g["generatorCommit"] ||
        g["compiledPlanExecuted"]["metaRevision"]!=g["controllerPlan"]["metaPolicy"]["revision"] {
        return false;
    }
    let Some(candidate)=g["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return false};
    candidate["status"]=="verified" && candidate["origin"]=="model_driven_detached_worktree"
        && candidate["baseCommit"]==g["generatorCommit"]
        && candidate["experimentAdoption"]["commit"]==g["acceptedCommit"]
        && candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && candidate["evaluation"]["metrics"]["eligible"]==true
        && candidate["evaluation"]["metrics"]["regressions"]==0
        && candidate["evaluation"]["metrics"]["improvedHoldouts"].as_u64().unwrap_or(0)>0
        && crate::engine::rsi::verified_pair_receipt(state,&candidate)
        && candidate["paths"]==g["changedSourcePaths"]
        && !paths(g).is_empty()
        && paths(g).iter().all(|p|g["acceptedSourceHashes"][*p].as_str()
            .is_some_and(|sha|sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))
}
fn handoff(parent:&Value,child:&Value,require_behavior_change:bool)->bool {
    let proof=&child["behaviorCounterfactual"];
    let behavior_matches=if require_behavior_change {
        proof["status"]=="behavior_changed" &&
            proof["previousActionSha256"]!=proof["descendantActionSha256"]
    } else {
        matches!(proof["status"].as_str(),
            Some("behavior_changed"|"behavior_unchanged"))
    };
    child["generatorCommit"]==parent["acceptedCommit"] && behavior_matches &&
        child["generatorBinarySha256"]!=parent["generatorBinarySha256"] &&
        proof["source"]=="matched_compiled_planner_intervention_v1" &&
        proof["sameSourceRoot"]==true && proof["samePersistedHistory"]==true &&
        proof["onlyBinaryIdentityFieldsRebound"]==true &&
        proof["previousGeneratorCommit"]==parent["generatorCommit"] &&
        proof["previousGeneratorBinarySha256"]==parent["generatorBinarySha256"] &&
        proof["parentAcceptedCommit"]==parent["acceptedCommit"] &&
        proof["descendantGeneratorCommit"]==child["generatorCommit"] &&
        proof["descendantGeneratorBinarySha256"]==child["generatorBinarySha256"] &&
        proof["descendantActionSha256"]==json!(
            crate::engine::rsi_evolve::action_hash(&child["controllerPlan"])) &&
        proof["historyCount"].as_u64()==child["attempt"].as_u64()
            .and_then(|n|n.checked_sub(1))
}
fn actually_changed_target(g:&Value)->bool {
    let earlier=g["behaviorCounterfactual"]["previousAction"]["focusFiles"].as_array();
    let current=g["controllerPlan"]["focusFiles"].as_array();
    let changed=paths(g);
    let (Some(earlier),Some(current))=(earlier,current) else{return false};
    current.iter().filter_map(Value::as_str).any(|selected|
        changed.contains(&selected) &&
            !earlier.iter().any(|old|old.as_str()==Some(selected)))
}
fn improved_domain(g:&Value,domain:&str)->bool {
    g["evaluation"]["metrics"]["improvedHoldoutDomains"].as_array()
        .is_some_and(|rows|rows.iter().any(|row|row.as_str()==Some(domain)))
}
fn positive_meta(g:&Value)->bool {
    g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0 &&
        improved_domain(g,"improver") && paths(g).iter().any(|p|meta_source(p))
}
pub(crate) fn audit(state:&AppState,history:&[Value])->Value {
    let mut verified=Vec::new();
    for rows in history.windows(4) {
        let (meta_a,cap_b,meta_a2,cap_b2)=(&rows[0],&rows[1],&rows[2],&rows[3]);
        if rows.iter().any(|g|!independently_adopted(state,g)) ||
            meta_a["domain"]!="improver" || cap_b["domain"]=="improver" ||
            meta_a2["domain"]!="improver" || cap_b2["domain"]=="improver" ||
            !positive_meta(meta_a) || !positive_meta(meta_a2) {continue;}
        if !handoff(meta_a,cap_b,true) || !handoff(cap_b,meta_a2,false) ||
            !handoff(meta_a2,cap_b2,true) ||
            !actually_changed_target(cap_b) || !actually_changed_target(cap_b2) {
            continue;
        }
        if !improved_domain(cap_b,cap_b["domain"].as_str().unwrap_or("")) ||
            !improved_domain(cap_b2,cap_b2["domain"].as_str().unwrap_or("")) {continue;}
        verified.push(json!({
            "source":"independent_meta_capability_meta_capability_closed_loop_v1",
            "acceptedCommits":rows.iter().map(|g|g["acceptedCommit"].clone()).collect::<Vec<_>>(),
            "compiledBehaviorHashes":rows.iter().skip(1)
                .map(|g|g["behaviorCounterfactual"]["descendantActionSha256"].clone())
                .collect::<Vec<_>>(),
            "evaluationIds":rows.iter().map(|g|g["evaluation"]["id"].clone()).collect::<Vec<_>>(),
            "firstMetaHoldoutGains":meta_a["evaluation"]["metrics"]["metaHoldoutGains"],
            "secondMetaHoldoutGains":meta_a2["evaluation"]["metrics"]["metaHoldoutGains"],
            "capabilityDomain":cap_b["domain"],"descendantCapabilityDomain":cap_b2["domain"]
        }));
    }
    let sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&verified).unwrap_or_default()));
    json!({"source":"independent_rsi_closure_v1","closedLoopCount":verified.len(),
        "demonstrated":!verified.is_empty(),"loops":verified,"sha256":sha,
        "requires":"4 contiguous adopted generations; 3 matched compiled planner replays; 2 effective behavior changes after meta improvements; 2 meta and 2 capability independent holdout gains",
        "modelWeightUpdates":0})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_file_is_not_meta_improvement() {
        assert!(meta_source("src/engine/rsi/planner/descendant.rs"));
        assert!(!meta_source("src/engine/memory.rs"));
        assert!(!meta_source("README.md"));
    }
    #[test]
    fn forged_action_flag_does_not_pass_handoff(){
        let p=json!({"generatorCommit":"old","acceptedCommit":"new","generatorBinarySha256":"a"});
        let c=json!({"generatorCommit":"new","generatorBinarySha256":"b",
            "behaviorCounterfactual":{"status":"behavior_changed"}});
        assert!(!handoff(&p,&c,true));
    }
}
