//! Independent lineage checks for reflexive meta/capability experiments.
use super::*;

pub(crate) fn valid(plan:&Value,history:&[Value],frontier:&[Value],
    commit:&str)->bool {
    if plan.is_null(){return true;}
    let mut unsigned=plan.clone();
    let Some(obj)=unsigned.as_object_mut() else{return false};
    obj.remove("sha256");
    let target=&plan["target"];
    let Some(path)=target["path"].as_str() else{return false};
    if plan["source"]!="compiled_reflexive_intelligence_curriculum_v1"
        || plan["sha256"].as_str()!=Some(digest(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["independentEvaluationOwner"]!="supervisor"
        || plan["editableSource"]!="src/engine/rsi_reflexive.rs"
        || plan["qualityProven"]!=false
        || plan["recursiveCapabilityProven"]!=false
        || plan["currentSourceRebound"]!=true
        || !rsi::safe_path(path)
        || !frontier.iter().any(|f|f==target) {
        return false;
    }
    if plan["phase"]=="seed" {
        return meta_source(path)
            && target["role"]=="improvement_methods"
            && plan["inheritedFromVerifiedParent"]==false
            && plan["rootGenerationId"].is_null();
    }
    let Some(parent)=history.iter().rev().find(|g|
        rsi_mutual_program::adopted_source(g) && g["acceptedCommit"]==commit)
        else{return false};
    if plan["parentAcceptedCommit"]!=parent["acceptedCommit"]
        || plan["inheritedFromVerifiedParent"]!=true {return false;}
    match plan["phase"].as_str() {
        Some("transfer")=>{
            let origin=parent["reflexiveProgram"]["target"]["path"].as_str();
            parent["reflexiveProgram"]["phase"]=="seed"
                && stage_executed(parent)
                && origin.is_some_and(|p|meta_source(p)&&actual_source(parent,p))
                && plan["rootGenerationId"]==parent["id"]
                && plan["originMetaPath"]==parent["reflexiveProgram"]["target"]["path"]
                && target["role"]!="improvement_methods"
                && !meta_source(path)
        },
        Some("return")=>{
            let Some(first)=history.iter().find(|g|
                g["id"]==plan["rootGenerationId"]
                && g["acceptedCommit"]==parent["generatorCommit"]
                && rsi_mutual_program::adopted_source(g)
                && g["reflexiveProgram"]["phase"]=="seed"
                && stage_executed(g)
                && g["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|actual_source(g,p)&&meta_source(p)))
                else{return false};
            parent["reflexiveProgram"]["phase"]=="transfer"
                && parent["reflexiveProgram"]["rootGenerationId"]==first["id"]
                && capability_change(parent)
                && parent["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|actual_source(parent,p))
                && plan["transferredRole"]==parent["reflexiveProgram"]["target"]["role"]
                && target["role"]=="improvement_methods"
                && plan["originMetaPath"]==first["reflexiveProgram"]["target"]["path"]
                && target["path"]==plan["originMetaPath"]
        },
        Some("redesign_experiment")=>{
            parent["reflexiveProgram"]["phase"]=="return"
                && stage_executed(parent)
                && parent["reflexiveProgram"]["target"]["path"].as_str()
                    .is_some_and(|p|meta_source(p)&&actual_source(parent,p))
                && completed(history)["loops"].as_array().is_some_and(|loops|
                    loops.iter().any(|row|row["returnGenerationId"]==parent["id"]))
                && plan["rootGenerationId"]
                    ==parent["reflexiveProgram"]["rootGenerationId"]
                && target["role"]=="improvement_methods"
                && path=="src/engine/rsi_factorial.rs"
                && plan["experimentPlanSha256"].is_null()
        },
        Some("execute_revised_experiment")=>{
            parent["reflexiveProgram"]["phase"]=="redesign_experiment"
                && stage_executed(parent)
                && parent["reflexiveProgram"]["target"]["path"]
                    =="src/engine/rsi_factorial.rs"
                && actual_source(parent,"src/engine/rsi_factorial.rs")
                && plan["rootGenerationId"]
                    ==parent["reflexiveProgram"]["rootGenerationId"]
                && target["role"]!="improvement_methods"
                && plan["experimentPlanSha256"].as_str().is_some_and(|h|
                    h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit()))
        },
        _=>false
    }
}
/// Completed source-level feedback loops are distinct from planned stages.
/// Every step must have an independently adopted patch and a different
/// compiled generator. No claim of unlimited recursion or causal benefit.
pub(crate) fn completed(history:&[Value])->Value {
    let mut loops=Vec::new();
    for last in history.iter().filter(|g|
        rsi_mutual_program::adopted_source(g)
        && stage_executed(g)
        && g["reflexiveProgram"]["phase"]=="return"
        && g["reflexiveProgram"]["target"]["path"].as_str()
            .is_some_and(|p|meta_source(p)&&actual_source(g,p))) {
        let Some(first)=history.iter().find(|g|
            g["id"]==last["reflexiveProgram"]["rootGenerationId"]
            && rsi_mutual_program::adopted_source(g)
            && stage_executed(g)
            && g["reflexiveProgram"]["phase"]=="seed"
            && g["reflexiveProgram"]["target"]["path"].as_str()
                .is_some_and(|p|meta_source(p)&&actual_source(g,p)))
        else{continue};
        let Some(middle)=history.iter().find(|g|
            g["generatorCommit"]==first["acceptedCommit"]
            && g["acceptedCommit"]==last["generatorCommit"]
            && g["reflexiveProgram"]["phase"]=="transfer"
            && g["reflexiveProgram"]["rootGenerationId"]==first["id"]
            && g["reflexiveProgram"]["originMetaPath"]==first["reflexiveProgram"]["target"]["path"]
            && capability_change(g)
            && g["reflexiveProgram"]["target"]["path"].as_str()
                .is_some_and(|p|actual_source(g,p))) else{continue};
        if last["reflexiveProgram"]["originMetaPath"]
            !=first["reflexiveProgram"]["target"]["path"]
            || !rsi_mutual_program::adopted_source(middle)
            || last["reflexiveProgram"]["transferredRole"]
                !=middle["reflexiveProgram"]["target"]["role"] {continue;}
        loops.push(json!({
            "seedGenerationId":first["id"],"transferGenerationId":middle["id"],
            "returnGenerationId":last["id"],
            "metaOriginCommit":first["acceptedCommit"],
            "capabilityImprovedCommit":middle["acceptedCommit"],
            "metaReturnCommit":last["acceptedCommit"],
            "executedRecompiledMetaPolicy":true,
            "threeIndependentSourceAdoptions":true,
            "causalBenefitProven":false
        }));
    }
    let mut out=json!({
        "source":"witnessed_reflexive_improvement_lineage_v1",
        "completedLoops":loops.len(),"loops":loops,
        "meaning":"three_independent_compiled_adoptions_not_indefinite_recursion"
    });
    out["sha256"]=json!(digest(&out));
    out
}
/// Five-stage proof: a verified meta->capability->meta loop must
/// independently rewrite the experiment METHOD, then the next compiled
/// descendant must USE the revised experiment to generate a verified
/// source patch on its chosen isolated capability target.
pub(crate) fn completed_method_cycles(history:&[Value])->Value {
    let prior=completed(history);
    let mut cycles=Vec::new();
    let mut seen=BTreeSet::new();
    for outcome in history.iter().filter(|g|
        g["reflexiveProgram"]["phase"]=="execute_revised_experiment"
            && rsi_mutual_program::adopted_source(g) && stage_executed(g)) {
        let Some(outcome_id)=outcome["id"].as_str() else{continue};
        let Some(path)=outcome["reflexiveProgram"]["target"]["path"].as_str() else{continue};
        if !actual_source(outcome,path)
            || outcome["reflexiveProgram"]["target"]["role"]=="improvement_methods" {
            continue;
        }
        let Some(method)=history.iter().find(|g|
            rsi_mutual_program::adopted_source(g) && stage_executed(g)
                && g["acceptedCommit"]==outcome["generatorCommit"]
                && g["reflexiveProgram"]["phase"]=="redesign_experiment"
                && g["reflexiveProgram"]["rootGenerationId"]
                    ==outcome["reflexiveProgram"]["rootGenerationId"]
                && g["reflexiveProgram"]["target"]["path"]
                    =="src/engine/rsi_factorial.rs"
                && actual_source(g,"src/engine/rsi_factorial.rs"))
            else{continue};
        let Some(returned)=history.iter().find(|g|
            rsi_mutual_program::adopted_source(g) && stage_executed(g)
                && g["acceptedCommit"]==method["generatorCommit"]
                && g["reflexiveProgram"]["phase"]=="return"
                && g["reflexiveProgram"]["rootGenerationId"]
                    ==outcome["reflexiveProgram"]["rootGenerationId"])
            else{continue};
        let Some(triple)=prior["loops"].as_array().into_iter().flatten()
            .find(|proof|proof["returnGenerationId"]==returned["id"])
            else{continue};
        if !seen.insert(outcome_id.to_string()){continue;}
        cycles.push(json!({
            "seedGenerationId":triple["seedGenerationId"],
            "capabilityGenerationId":triple["transferGenerationId"],
            "metaReturnGenerationId":returned["id"],
            "methodRedesignGenerationId":method["id"],
            "methodExecutionGenerationId":outcome["id"],
            "originMetaCommit":triple["metaOriginCommit"],
            "capabilityCommit":triple["capabilityImprovedCommit"],
            "returnedMetaCommit":returned["acceptedCommit"],
            "experimentMethodCommit":method["acceptedCommit"],
            "experimentOutcomeCommit":outcome["acceptedCommit"],
            "revisedExperimentPlanSha256":
                outcome["reflexiveProgram"]["experimentPlanSha256"],
            "executedMethodTarget":outcome["reflexiveProgram"]["target"],
            "fiveIndependentSourceAdoptions":true,
            "recompiledMethodApplied":true,
            "causalPerformanceAdvantageProven":false
        }));
    }
    let mut result=json!({
        "source":"independently_witnessed_experiment_method_evolution_v1",
        "completedMethodCycles":cycles.len(),"cycles":cycles,
        "claim":"source_adopted_meta_capability_meta_method_experiment_execution"
    });
    result["sha256"]=json!(digest(&result));
    result
}
