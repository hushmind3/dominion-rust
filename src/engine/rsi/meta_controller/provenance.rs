//! provenance stage of the compiled, revisable meta-improvement controller.
//! These observations propose experiments; only frozen independent evaluation may adopt them.
use super::*;

pub(super) fn model_design_roles(g:&Value)->Vec<String>{
    let design=&g["systemDesign"];
    if design["source"]!="model_authored_system_design"
        || design["generatorCommit"]!=g["generatorCommit"]
        || design["generationJobId"]!=g["jobId"] {return Vec::new();}
    let names=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    names.iter().filter_map(Value::as_str)
        .filter(|name|!name.is_empty() && name.len()<=64
            && name.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-')
            && *name!="system")
        .map(str::to_string).collect()
}
pub(super) fn independent_adoption(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && !model_design_roles(g).is_empty()
        && g["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|files|!files.is_empty() && files.iter().all(|path|
                path.as_str().is_some_and(|p|
                    g["acceptedSourceHashes"][p].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
}

/// Direct evidence that the accepted child rewrote the implementation of
/// improvement itself, not just a task module or a claimed meta design.
pub(crate) fn meta_source_changed(g:&Value)->bool{
    if !independent_adoption(g){return false;}
    let Some(files)=g["acceptedSourceHashes"].as_object() else{return false};
    files.keys().any(|path|crate::engine::rsi::safe_path(path)
        && !path.ends_with("/tests.rs")
        && (matches!(path.as_str(),
        "src/engine/rsi_meta_controller.rs"|"src/engine/rsi_system.rs"
        |"src/engine/rsi_evolve.rs"|"src/engine/rsi_mutation.rs"
        |"src/engine/procedure.rs"|"src/engine/learning.rs"
        |"src/engine/rsi/meta_controller/provenance.rs"
        |"src/engine/rsi/meta_controller/reciprocal.rs"
        |"src/engine/rsi/meta_controller/selection.rs"
        |"src/engine/rsi/meta_controller/transfer.rs"
        |"src/engine/rsi/meta_controller/feedback.rs"
        |"src/engine/rsi/system/inventory.rs"
        |"src/engine/rsi/system/blueprint.rs"
        |"src/engine/rsi/system/discovery.rs"
        |"src/engine/rsi/system/frontier.rs"
        |"src/engine/rsi/system/agenda.rs"
        |"src/engine/rsi/system/design.rs")
        || matches!(path.as_str(),
            "src/engine/rsi/descendant_plan.rs"
            |"src/engine/rsi/method_feedback.rs"
            |"src/engine/rsi/selection.rs"
            |"src/engine/rsi/source_control.rs"
            |"src/engine/rsi/focus.rs"
            |"src/engine/rsi/adoption.rs"
            |"src/engine/rsi/programs/action.rs"
            |"src/engine/rsi/programs/selection.rs"
            |"src/engine/rsi/programs/transfer.rs")))
}

pub(super) fn role_touched(g:&Value,role:&str)->bool{
    model_design_roles(g).iter().any(|name|name==role)
        || g["selfControllerVerified"]==true
            && g["systemAgenda"]["target"]["role"]==role
}
pub(super) fn failure_kind(g:&Value)->&'static str{
    if g["accepted"]==true{return "adopted";}
    if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|error|
            error.contains("compile")||error.contains("compiler")) {
        return "compiler_failure";
    }
    if g["candidateId"].is_null() {
        return "no_verified_code_candidate";
    }
    if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0{
        return "behavior_regression";
    }
    if g["evaluation"]["verdict"].is_null(){return "evaluation_unavailable";}
    "no_independent_gain"
}
pub(super) fn next_generation_productivity(history:&[Value],generation:&Value)->usize{
    if !independent_adoption(generation){return 0;}
    let Some(child_commit)=generation["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|child|
        child["generatorCommit"]==child_commit
            && child["selfControllerVerified"]==true
            && independent_adoption(child)
    ).count()
}

/// Learn an explicit categorical distribution over causal failure modes.
/// A scalar benchmark score cannot tell an agent whether to improve its
/// hypothesis, compiler repair procedure, code writer or predictor.
/// Dirichlet smoothing avoids certainty from one sparse experiment.
pub(crate) const OUTCOME_CLASSES:[&str;5]=[
    "adopted","compiler_failure","no_verified_code_candidate",
    "behavior_regression","no_independent_gain"
];
pub(super) fn observed_class(g:&Value)->Option<&'static str>{
    if independent_adoption(g){return Some("adopted");}
    if g["evaluation"]["verdict"]=="build_failed"
        || g["error"].as_str().is_some_and(|e|
            e.contains("compiler")||e.contains("compile")){
        return Some("compiler_failure");
    }
    if g["candidateId"].is_null(){
        if g["result"].is_object()||g["attemptedCandidateId"].is_string()
            || g["error"].is_string(){return Some("no_verified_code_candidate");}
        return None;
    }
    if g["evaluation"]["metrics"]["regressions"]
        .as_u64().is_some_and(|count|count>0){
        return Some("behavior_regression");
    }
    if g["evaluation"]["verdict"].as_str().is_some(){
        return Some("no_independent_gain");
    }
    None
}
pub(super) fn failure_distribution(rows:&[&Value])->Value{
    let mut counts=[0u64;5];
    for row in rows{
        if let Some(class)=observed_class(row){
            if let Some(index)=OUTCOME_CLASSES.iter().position(|item|*item==class){
                counts[index]+=1;
            }
        }
    }
    let total=counts.iter().sum::<u64>();
    let mut posterior=serde_json::Map::new();
    // One pseudocount per category: no zero-probability failure mode.
    for (index,class) in OUTCOME_CLASSES.iter().enumerate(){
        posterior.insert((*class).into(),
            json!((counts[index] as f64+1.0)/(total as f64+5.0)));
    }
    Value::Object(posterior)
}

pub(super) fn confidence_adjustment(history:&[Value])->f64{
    let errors=history.iter().filter_map(|row|
        row["metaLearning"]["categoricalBrier"].as_f64()
            .or_else(||row["metaLearning"]["brierError"].as_f64())
            .filter(|value|value.is_finite() && *value>=0.0 && *value<=1.0))
        .rev().take(12).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.6*mean).clamp(0.4,1.0)
}


/// Frozen paired holdouts provide *observed* direction-of-effect data.
/// A multi-module patch cannot establish which module caused an improvement.
/// The next compiled descendant owns this editable causal-hypothesis model.
pub(super) fn measured_interaction_edges(history:&[Value])
    ->BTreeMap<(String,String),(u64,u64,u64,u64)>{
    let mut edges=BTreeMap::<(String,String),(u64,u64,u64,u64)>::new();
    for generation in history.iter().filter(|g|independent_adoption(g)){
        let roles=model_design_roles(generation);
        let targets=&generation["systemDesign"]["targetFiles"];
        let implemented=roles.into_iter().filter(|role|{
            let mapped=generation["systemDesign"]["componentFiles"][role.as_str()]
                .as_array().filter(|rows|!rows.is_empty())
                .or_else(||(generation["systemDesign"]["component"]==role.as_str()
                    && generation["systemDesign"]["componentFiles"].is_null())
                    .then(||targets.as_array()).flatten());
            mapped.is_some_and(|files|files.iter().all(|file|{
                let Some(path)=file.as_str() else{return false};
                targets.as_array().is_some_and(|all|all.iter().any(|r|r==path))
                && generation["acceptedSourceHashes"][path].as_str()
                    .is_some_and(|sha|sha.len()==64
                        && sha.bytes().all(|b|b.is_ascii_hexdigit()))
            }))
        }).collect::<Vec<_>>();
        if implemented.is_empty(){continue;}
        for case in generation["evaluation"]["metrics"]["holdoutEvidence"]
            .as_array().into_iter().flatten(){
            let Some(domain)=case["domain"].as_str() else{continue};
            let target_role=match domain{
                "context"|"planning"=>"harness",
                "improver"|"search"=>"improvement_methods",
                "ui"=>"frontend",
                "model"=>"models",
                "memory"|"tools"|"learning"=>domain,
                _=>continue
            };
            let (Some(before),Some(after))=(case["baselinePass"].as_bool(),
                case["candidatePass"].as_bool()) else{continue};
            let loss=before&&!after
                ||case["metricRegressions"].as_u64().unwrap_or(0)>0;
            let gain=!before&&after
                ||case["metricGains"].as_u64().unwrap_or(0)>0;
            for source in &implemented{
                if source==target_role || !gain&&!loss {continue;}
                let evidence=edges.entry((source.clone(),target_role.to_string()))
                    .or_default();
                let joint=implemented.len()>1;
                if loss {
                    if joint{evidence.3+=1}else{evidence.1+=1;}
                } else if joint {evidence.2+=1;} else {evidence.0+=1;}
            }
        }
    }
    edges
}

/// A persistent, inspectable source-role -> measured-role interaction map.
/// Every next compiled binary can rewrite both the edge induction algorithm
/// and its use in choosing the next architecture. The immutable frozen
/// evaluator remains outside this self-model.
pub(super) fn role_influence_model(history:&[Value])->Value {
    let edges=measured_interaction_edges(history);
    let mut rows=edges.into_iter().map(|((source,target),(gains,losses,joint_gains,joint_losses))|
        json!({"sourceRole":source,"targetRole":target,
            "isolatedPairedGains":gains,"isolatedPairedRegressions":losses,
            "jointPairedGains":joint_gains,"jointPairedRegressions":joint_losses,
            "directionalObservationScore":if gains+losses>0 {
                (gains as f64-losses as f64)/(gains+losses) as f64
            }else{0.0},
            "attribution":if gains+losses>0 {
                "single_source_role_observation_not_ablation_proof"
            }else{"joint_redesign_confounding"},
            "causalityProven":false})
    ).collect::<Vec<_>>();
    rows.sort_by(|a,b|{
        let informative=|r:&Value|r["isolatedPairedGains"].as_u64().unwrap_or(0)
            +r["isolatedPairedRegressions"].as_u64().unwrap_or(0);
        informative(b).cmp(&informative(a))
            .then_with(||a["sourceRole"].as_str().cmp(&b["sourceRole"].as_str()))
            .then_with(||a["targetRole"].as_str().cmp(&b["targetRole"].as_str()))
    });
    rows.truncate(96);
    let mut model=json!({
        "source":"descendant_editable_interaction_dynamics_v1",
        "edges":rows,
        "counterfactualStatus":"untested",
        "rewardOrigin":"independently_frozen_paired_holdouts",
        "nextDescendantCanRedesignInfluenceLearning":true,
        "causalLoopProven":false
    });
    model["sha256"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&model).unwrap_or_default())));
    model
}

