//! Source-grounded closure audit. Stored plans or adopted commits alone cannot
//! prove RSI. Require independent holdout gains AND a causal, compiled-policy
//! behavior difference that changed the next descendant's actual edits.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn meta_source(path:&str)->bool {
    // A source candidate must never earn a META gain by editing the judge.
    if matches!(path,"src/engine/rsi/proof.rs"
        |"src/engine/rsi_system/design.rs"
        |"src/engine/rsi_system/proposal.rs"
        |"src/engine/rsi_system/handoff.rs"
        |"src/engine/rsi_system/peer_verifier.rs") {return false;}
    path=="src/engine/rsi_evolve.rs" ||
    path=="src/engine/rsi_meta_controller.rs" ||
    path=="src/engine/rsi_program_graph.rs" ||
    path=="src/engine/rsi_system.rs" ||
    path.starts_with("src/engine/rsi_system/") ||
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
    let Some(job)=g["jobId"].as_str().and_then(|id|state.store.get("job",id)) else{return false};
    if job["kind"]!="rsi_generation"
        || job["id"]!=g["jobId"]
        || job["input"]["rsiGeneratorCommit"]!=g["generatorCommit"]
        || job["input"]["rsiControllerPlan"]!=g["controllerPlan"]
        || job["rsiCompiledPlanExecuted"]!=g["compiledPlanExecuted"]
        || job["rsiDomain"]!=g["domain"]
        || job["result"]["rsi"]["candidateId"]!=g["candidateId"] {
        return false;
    }
    candidate["status"]=="verified" && candidate["origin"]=="model_driven_detached_worktree"
        // Generation-level metrics are copied from compare_at. Bind each
        // claimed holdout gain to its independently stored candidate report.
        && g["evaluation"]["candidateId"]==candidate["id"]
        && g["evaluation"]["evaluationId"]==candidate["evaluation"]["id"]
        && g["evaluation"]["evidenceSha256"]==candidate["evaluation"]["evidenceSha256"]
        && g["evaluation"]["metrics"]==candidate["evaluation"]["metrics"]
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
fn independent_quality_gain(g:&Value,domain:&str)->bool {
    g["evaluation"]["metrics"]["measurements"].as_array().is_some_and(|cases|
        cases.iter().any(|case|case["domain"]==domain && case["holdout"]==true
            && case["baselinePass"]==false && case["candidatePass"]==true))
}
fn positive_meta(g:&Value)->bool {
    g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0 &&
        improved_domain(g,"improver") && independent_quality_gain(g,"improver")
            && paths(g).iter().any(|p|meta_source(p))
}
/// Revalidate the exact procedure proposed by the previous META generation,
/// executed by this descendant and responsible for an accepted source byte.
/// The execution association is necessary provenance, not proof of isolated
/// improvement: the independent paired task report supplies quality evidence.
fn executed_inherited_method(state:&AppState,parent:&Value,child:&Value)->Option<Value> {
    let job=state.store.get("job",child["jobId"].as_str()?)?;
    let session=job["sessionId"].as_str()?;
    for method in parent["methodProposals"].as_array()?.iter() {
        if method["sourceGenerationJobId"]!=parent["jobId"]
            || !method["synthesis"].is_null()
            || method["verification"]["type"]!="procedure"
            || method["domain"]!=child["domain"] {
            continue;
        }
        for execution in child["methodExecutions"].as_array()?.iter() {
            if execution["methodId"]!=method["id"] || execution["revision"]!=method["revision"]
                || execution["result"]["verified"]!=true || execution["result"]["followed"]!=true
                || !crate::engine::rsi_evolve::method_wrote_accepted_candidate(
                    state,child,execution,session) {continue;}
            let Some(experience)=execution["experienceId"].as_str()
                .and_then(|id|state.store.get("experience",id)) else{continue};
            let valid=experience["executableMethods"].as_array().is_some_and(|methods|
                methods.iter().any(|stored|stored["id"]==method["id"]
                    && stored["revision"]==method["revision"]
                    && stored["verification"]==method["verification"]
                    && stored["applications"].as_array().is_some_and(|apps|apps.iter().any(|app|
                        app["jobId"]==child["jobId"]
                            && app["executionId"]==execution["executionId"]
                            && app["methodWroteAcceptedCandidate"]==true
                            && app["verificationVerdict"]=="passed"))));
            if valid {
                return Some(json!({
                    "methodId":method["id"],"revision":method["revision"],
                    "name":method["name"],"domain":method["domain"],
                    "sourceGenerationJobId":parent["jobId"],
                    "executedGenerationJobId":child["jobId"],
                    "executionId":execution["executionId"],
                    "experienceId":execution["experienceId"],
                    "wroteAdoptedSource":true,"qualityClaim":"paired_downstream_only"
                }));
            }
        }
    }
    None
}

fn revised_method_after_feedback(previous:&Value,revised:&Value,prior_child:&Value,
    meta_revision:&Value)->bool {
    previous["name"].as_str().is_some_and(|name|!name.trim().is_empty())
        && previous["name"]==revised["name"]
        && previous["domain"]==revised["domain"]
        && previous["methodId"]!=revised["methodId"]
        && previous["revision"]!=revised["revision"]
        && meta_revision["improvementDirective"]["sourceGenerationId"]==prior_child["id"]
        && meta_revision["improvementDirective"]["previousAccepted"]==true
        && meta_revision["improvementDirective"]["previousCandidateId"]==prior_child["candidateId"]
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
        if !improved_domain(cap_b,cap_b["domain"].as_str().unwrap_or(""))
            || !independent_quality_gain(cap_b,cap_b["domain"].as_str().unwrap_or(""))
            || !improved_domain(cap_b2,cap_b2["domain"].as_str().unwrap_or(""))
            || !independent_quality_gain(cap_b2,cap_b2["domain"].as_str().unwrap_or("")) {
            continue;
        }
        // A1 produced a method used in B1; B1's measured outcome reached A2;
        // A2 produced a different executable revision and B2 used it to write
        // independently adopted bytes. A change in plan metadata is not enough.
        let Some(first_method)=executed_inherited_method(state,meta_a,cap_b) else {continue};
        let Some(revised_method)=executed_inherited_method(state,meta_a2,cap_b2) else {continue};
        if !revised_method_after_feedback(&first_method,&revised_method,cap_b,meta_a2) {
            continue;
        }
        verified.push(json!({
            "source":"independent_meta_capability_meta_capability_closed_loop_v1",
            "acceptedCommits":rows.iter().map(|g|g["acceptedCommit"].clone()).collect::<Vec<_>>(),
            "compiledBehaviorHashes":rows.iter().skip(1)
                .map(|g|g["behaviorCounterfactual"]["descendantActionSha256"].clone())
                .collect::<Vec<_>>(),
            "evaluationIds":rows.iter().map(|g|g["evaluation"]["id"].clone()).collect::<Vec<_>>(),
            "firstMetaHoldoutGains":meta_a["evaluation"]["metrics"]["metaHoldoutGains"],
            "secondMetaHoldoutGains":meta_a2["evaluation"]["metrics"]["metaHoldoutGains"],
            "capabilityDomain":cap_b["domain"],"descendantCapabilityDomain":cap_b2["domain"],
            "initialInheritedMethod":first_method,
            "revisedInheritedMethod":revised_method,
            "feedbackGenerationId":cap_b["id"],
            "methodRevisionVerified":true
        }));
    }
    let sha=format!("{:x}",Sha256::digest(serde_json::to_vec(&verified).unwrap_or_default()));
    json!({"source":"independent_rsi_closure_v1","closedLoopCount":verified.len(),
        "demonstrated":!verified.is_empty(),"loops":verified,"sha256":sha,
        "requires":"4 contiguous independently adopted generations; 3 matched compiled planner replays; 2 improved descendant source edits using independently verified inherited procedures; second procedure revision grounded in first descendant feedback; 2 meta and 2 capability paired holdout gains",
        "modelWeightUpdates":0})
}
// Regression fixtures are kept outside the production RSI source inventory.
#[cfg(test)]
include!("../../tests/support/engine/rsi_causal_closure.tests.rs");
