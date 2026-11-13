//! Model-authored successor programs and measured cross-generation handoff.
use super::*;

/// Choose the next *model-authored* child development stage only from an
/// independently adopted implementation. The successor rebinds every stage
/// to the CURRENT architecture frontier, and advances on recorded execution
/// rather than the parent's declarative intention.
pub(crate) fn inherited_stage(
    state:&crate::state::AppState,history:&[Value],frontier:&[Value]
)->Value {
    for parent in history.iter().rev(){
        if parent["accepted"]!=true || parent["pairedRevalidated"]!=true
            || parent["candidateOrigin"]!="model_driven_detached_worktree"
            || parent["evaluation"]["metrics"]["eligible"]!=true
            || parent["systemDesign"]["source"]!="model_authored_system_design"
            || parent["systemDesign"]["generationJobId"]!=parent["jobId"]
            || parent["systemDesign"]["generatorCommit"]!=parent["generatorCommit"] {
            continue;
        }
        let Some(stages)=parent["systemDesign"]["successorStages"].as_array()
            .filter(|steps|!steps.is_empty() && steps.len()<=8) else{continue};
        let Some(id)=parent["candidateId"].as_str() else{continue};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{continue};
        if candidate["status"]!="verified"
            || candidate["baseCommit"]!=parent["generatorCommit"]
            || candidate["experimentAdoption"]["commit"]!=parent["acceptedCommit"]
            || candidate["evaluation"]["metrics"]["eligible"]!=true
            || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {
            continue;
        }
        let accepted_files=candidate["paths"].as_array().cloned().unwrap_or_default();
        if !design_implemented(&parent["systemDesign"],&accepted_files){continue;}
        let design_id=&parent["systemDesign"]["id"];
        let mut next=0usize;
        for executed in history.iter().filter(|child|
            child["successorStage"]["designId"]==*design_id
                && child["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"){
            if executed["successorStage"]["stageIndex"].as_u64()!=Some(next as u64){
                continue;
            }
            if executed["accepted"]==true && executed["pairedRevalidated"]==true
                || executed["successorStage"]["learningPolicy"]!="retry_failed"{
                next+=1;
            }
        }
        let Some(stage)=stages.get(next) else {continue};
        let Some(path)=stage["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p))
            else{continue};
        let Some(role)=stage["role"].as_str().filter(|r|valid_role(r)) else{continue};
        let Some(target)=frontier.iter().find(|row|
            row["path"]==path && row["role"]==role
                && row["domain"]==stage["domain"]
                && row["sourceSha256"].as_str().is_some_and(|sha|
                    sha.len()==64 && sha.bytes().all(|c|c.is_ascii_hexdigit())))
            else{continue};
        let mut bound=stage.clone();
        bound["source"]=json!("rebound_verified_model_successor_program_v1");
        bound["designId"]=design_id.clone();
        bound["stageIndex"]=json!(next);
        bound["acceptedParentCommit"]=parent["acceptedCommit"].clone();
        bound["sourceSha256"]=target["sourceSha256"].clone();
        bound["qualityProven"]=json!(false);
        return bound;
    }
    Value::Null
}

/// Reuse the model's target only when its *source change*, not its
/// explanation, was independently evaluated and adopted.
pub(crate) fn inherited_domain(history:&[Value])->Option<String>{
    let previous=history.last()?;
    let design=&previous["systemDesign"];
    if previous["accepted"]!=true || previous["pairedRevalidated"]!=true
        || previous["evaluation"]["metrics"]["eligible"]!=true
        || previous["candidateOrigin"]!="model_driven_detached_worktree"
        || !previous["acceptedCommit"].is_string()
        || design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=previous["generatorCommit"]
        || design["generationJobId"]!=previous["jobId"]
    {return None;}
    // A good score for an unrelated patch cannot establish that the
    // model's proposed system design was actually implemented.
    let paths=design["targetFiles"].as_array()?;
    if paths.is_empty() || !paths.iter().all(|entry|entry.as_str().is_some_and(|path|
        previous["acceptedSourceHashes"][path].as_str().is_some_and(|hash|
            hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())))){
        return None;
    }
    let domain=design["nextDomain"].as_str()?;
    crate::engine::rsi_evolve::DOMAINS.contains(&domain)
        .then(||domain.to_string())
}

/// This is a strictly narrower claim than general autonomous intelligence:
/// the *model* produced a cross-component design, the accepted patch touched
/// that design, and the rebuilt child executed its own subsequent plan.
/// The caller separately replays independent paired receipts and the exact
/// source lineage; JSON hints alone cannot establish capability.
pub(crate) fn verified_handoff(history:&[Value])->bool {
    let mut chain:Vec<&Value>=Vec::new();
    for generation in history {
        let design=&generation["systemDesign"];
        let edits=generation["acceptedSourceHashes"].as_object();
        let target=design["targetFiles"].as_array();
        let valid=generation["accepted"]==true
            && generation["systemEvolution"]==true
            && generation["pairedRevalidated"]==true
            && generation["candidateOrigin"]=="model_driven_detached_worktree"
            && generation["evaluation"]["metrics"]["eligible"]==true
            && generation["evaluation"]["metrics"]["regressions"]==0
            && generation["evaluation"]["metrics"]["improvedHoldouts"]
                .as_u64().unwrap_or(0)>0
            && generation["generatorBinarySha256"].as_str()
                .is_some_and(|hash|hash.len()==64)
            && generation["systemBlueprint"]["source"]=="compiled_descendant_system_blueprint_v1"
            && generation["systemBlueprint"]["generatorCommit"]==generation["generatorCommit"]
            && generation["descendantPlan"]["generatorCommit"]==generation["generatorCommit"]
            && generation["descendantPlan"]["systemBlueprint"]==
                generation["systemBlueprint"]
            && design["source"]=="model_authored_system_design"
            && design["generatorCommit"]==generation["generatorCommit"]
            && design["generationJobId"]==generation["jobId"]
            && design["component"].as_str().is_some_and(valid_role)
            && target.is_some_and(|files|!files.is_empty() && files.iter()
                .all(|entry|entry.as_str().is_some_and(|path|edits.is_some_and(|all|
                    all.get(path).and_then(Value::as_str).is_some_and(|hash|
                        hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit()))))));
        if !valid {
            chain.clear();
            continue;
        }
        if chain.last().is_some_and(|last|
            last["acceptedCommit"]!=generation["generatorCommit"]) {
            chain.clear();
        }
        chain.push(generation);
        if chain.len()<3 {continue;}
        let distinct=chain.iter().filter_map(|g|
            g["generatorBinarySha256"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if distinct.len()!=chain.len(){continue;}
        let roles=chain.iter().flat_map(|g|design_roles(&g["systemDesign"]))
            .collect::<std::collections::BTreeSet<_>>();
        if roles.len()<3 || !roles.contains("improvement_methods"){continue;}
        let meta_transferred=chain.iter().enumerate().any(|(i,g)|
            g["domain"]=="improver"
            && g["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)>0
            && chain[i+1..].iter().any(|successor|
                successor["domain"]!="improver"));
        if meta_transferred {return true;}
    }
    false
}

