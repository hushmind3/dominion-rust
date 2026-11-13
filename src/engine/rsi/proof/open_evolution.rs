//! Independent, topology-agnostic audit for improvement protocols that
//! improve their OWN selection/experimentation/learning code and then transfer
//! to task-solving descendants. This is an additional proof mode; it does not
//! relax the frozen paired evaluator or credit authored plans as execution.
use crate::state::AppState;
use serde_json::{json,Value};
use std::collections::BTreeSet;

fn meta_algorithm(path:&str)->bool {
    matches!(path,
        "src/engine/rsi/meta/evolution.rs"
        |"src/engine/rsi/meta/composition.rs"
        |"src/engine/rsi/meta/policy_dispatch.rs"
        |"src/engine/rsi/meta/controller_selection.rs"
        |"src/engine/rsi/meta/controller_policy.rs"
        |"src/engine/rsi/meta/reflexive_design.rs"
        |"src/engine/rsi_meta_controller.rs"
        |"src/engine/rsi_program_graph.rs"
        |"src/engine/rsi_principle_learning.rs"
        |"src/engine/rsi_edit_genome.rs"
        |"src/engine/rsi_rule_rewrite.rs")
}
fn accepted(state:&AppState,g:&Value)->bool {
    if g["accepted"]!=true || g["systemEvolution"]!=true
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=g["generatorCommit"]
        || g["controllerPlan"]["generatorCommit"]!=g["generatorCommit"]
        || g["pairedRevalidated"]!=true
        || g["evaluation"]["metrics"]["eligible"]!=true
        || g["evaluation"]["metrics"]["regressions"]!=0
        || g["evaluation"]["metrics"]["improvedHoldouts"]
            .as_u64().unwrap_or(0)==0 {return false;}
    let Some(candidate)=g["candidateId"].as_str().and_then(|id|
        state.store.get("rsi_candidate",id)) else{return false;};
    candidate["status"]=="verified"
        && candidate["origin"]=="model_driven_detached_worktree"
        && candidate["baseCommit"]==g["generatorCommit"]
        && candidate["experimentAdoption"]["commit"]==g["acceptedCommit"]
        && g["evaluation"]["evaluationId"]==candidate["evaluation"]["id"]
        && g["evaluation"]["evidenceSha256"]==candidate["evaluation"]["evidenceSha256"]
        && g["evaluation"]["metrics"]==candidate["evaluation"]["metrics"]
        && candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && candidate["paths"]==g["changedSourcePaths"]
        && crate::engine::rsi::verified_pair_receipt(state,&candidate)
}
fn touched(g:&Value,path:&str)->bool {
    g["changedSourcePaths"].as_array().is_some_and(|paths|
        paths.iter().any(|p|p.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|s|
            s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn executed(state:&AppState,history:&[Value],index:usize)->bool {
    let g=&history[index];
    let plan=&g["metaEvolutionProgram"];
    let Some(id)=g["jobId"].as_str() else{return false};
    let Some(job)=state.store.get("job",id) else{return false};
    accepted(state,g)
        && plan["source"]=="compiled_descendant_recursive_evolution_protocol_v1"
        && g["controllerPlan"]["metaEvolutionProgram"]==*plan
        && job["input"]["rsiMetaEvolutionProgram"]==*plan
        && job["input"]["rsiControllerPlan"]==g["controllerPlan"]
        && job["input"]["rsiExperimentContract"]==g["experimentContract"]
        && crate::engine::rsi_evolve::experiment_contract::valid(
            &g["controllerPlan"],g["evaluationProfileSha256"].as_str().unwrap_or(""),
            &g["experimentContract"])
        && g["compiledPlanExecuted"]["evolutionProtocolRevision"]==plan["revision"]
        && job["rsiCompiledPlanExecuted"]==g["compiledPlanExecuted"]
        && crate::engine::rsi_evolve::meta_evolution::valid(
            plan,&history[..index],
            g["controllerPlan"]["architectureFrontier"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]),
            g["generatorCommit"].as_str().unwrap_or(""))
        && plan["target"]["domain"]==g["domain"]
        && plan["target"]["path"].as_str().is_some_and(|path|touched(g,path))
        && g["evaluation"]["metrics"]["improvedHoldoutDomains"]
            .as_array().is_some_and(|domains|
                domains.contains(&g["domain"]))
}
fn accepted_protocol_source(g:&Value)->bool {
    g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0
        && g["changedSourcePaths"].as_array().is_some_and(|paths|paths.iter()
            .filter_map(Value::as_str).any(|path|
                meta_algorithm(path) && touched(g,path)))
        && crate::engine::rsi_evolve::meta_evolution::valid_authored(
            &g["systemDesign"]["evolutionProtocol"])
}

/// Every reported improvement is replayable from independently adopted
/// candidate bytes and actual compiled workers. A new protocol revision must
/// follow measured descendant feedback; the checker does not require any
/// specific A/B/A/B order, domain name or hard-coded source-edit target.
pub(crate) fn audit(state:&AppState,history:&[Value])->Value {
    let mut revisions=BTreeSet::<String>::new();
    let mut executed_nodes=Vec::<Value>::new();
    let mut meta_authors=Vec::<Value>::new();
    let mut feedback_revisions=0usize;
    for (i,g) in history.iter().enumerate() {
        if !accepted(state,g){continue;}
        if accepted_protocol_source(g) {
            let Some(revision)=g["systemDesign"]["evolutionProtocol"]["revision"]
                .as_str() else{continue};
            let previous_feedback=g["improvementDirective"]["sourceGenerationId"]
                .as_str().unwrap_or("");
            let earlier=executed_nodes.iter().find(|row|
                row["generationId"]==previous_feedback);
            let feedback_bound=earlier.is_some_and(|row|
                row["independentTaskQualityGain"]==true)
                && !revisions.contains(revision);
            if feedback_bound {feedback_revisions+=1;}
            revisions.insert(revision.to_string());
            meta_authors.push(json!({
                "generationId":g["id"],"acceptedCommit":g["acceptedCommit"],
                "metaSourceHashes":g["acceptedSourceHashes"],
                "protocolRevision":revision,
                "feedbackGenerationId":if feedback_bound {
                    json!(previous_feedback)
                }else{Value::Null},
                "feedbackRevision":feedback_bound
            }));
        }
        if !executed(state,history,i){continue;}
        let plan=&g["metaEvolutionProgram"];
        let parent=history[..i].iter().rev().find(|row|
            row["accepted"]==true && row["acceptedCommit"]==g["generatorCommit"]);
        if !parent.is_some_and(|p|accepted(state,p)
            && plan["parentGenerationId"]==p["id"]
            && plan["parentAcceptedCommit"]==p["acceptedCommit"]) {continue;}
        executed_nodes.push(json!({
            "generationId":g["id"],"generatorCommit":g["generatorCommit"],
            "acceptedCommit":g["acceptedCommit"],
            "operatorId":plan["operatorId"],"operatorRevision":plan["revision"],
            "programRevision":plan["protocol"]["revision"],
            "originGenerationId":plan["originGenerationId"],
            "experiment":plan["operator"]["experiment"],
            "targetDomain":g["domain"],"targetSource":plan["target"]["path"],
            "evaluationId":g["evaluation"]["evaluationId"],
            "independentTaskQualityGain":true,
            "independentAdoption":true
        }));
    }
    let adopted_versions=meta_authors.len();
    let distinct_protocols=revisions.len();
    let mut traced=Vec::<Value>::new();
    for author in &meta_authors {
        let instances=executed_nodes.iter().filter(|run|
            run["originGenerationId"]==author["generationId"]
                && run["generatorCommit"]==author["acceptedCommit"])
            .collect::<Vec<_>>();
        if !instances.is_empty() {
            traced.push(json!({"metaGenerationId":author["generationId"],
                "protocolRevision":author["protocolRevision"],
                "descendants":instances}));
        }
    }
    let distinct_traced=traced.iter().filter_map(|line|
        line["protocolRevision"].as_str()).collect::<BTreeSet<_>>().len();
    let demonstrated=distinct_protocols>=2 && adopted_versions>=2
        && distinct_traced>=2 && feedback_revisions>=1
        && executed_nodes.len()>=2;
    json!({
        "source":"independent_open_recursive_meta_evolution_v1",
        "demonstrated":demonstrated,
        "authoredRevisions":adopted_versions,
        "distinctProtocolRevisions":distinct_protocols,
        "feedbackDrivenRevisions":feedback_revisions,
        "independentExecutedOperators":executed_nodes.len(),
        "distinctExecutedProgramRevisions":distinct_traced,
        "sourceBoundRevisions":meta_authors,
        "actualExecutedDescendants":executed_nodes,
        "adoptedMethodFamilies":traced,
        "evaluationOwner":"independent_frozen_supervisor",
        "modelWeightUpdates":0,
        "causalIsolationLimit":"independent sequential source adoption and downstream task quality; no randomized causal intervention",
        "required":"two distinct adopted meta-source+protocol revisions, feedback-driven redesign, and two genuinely recompiled independently paired descendants executing adopted source-bound operator programs"
    })
}
