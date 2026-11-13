//! Editable metacognitive controller compiled into each accepted descendant.
//! Inspired by HyperAgents, predictive improvement dynamics, and comparative
//! cross-trajectory evolution. It proposes search experiments; frozen independent
//! evaluation is the sole authority for adoption. Every forecast is calibrated
//! against the *next* observed, separately verified outcome.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn model_design_roles(g:&Value)->Vec<String>{
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
fn independent_adoption(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["acceptedCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && !model_design_roles(g).is_empty()
        && crate::engine::rsi_system::design_implemented(&g["systemDesign"],
            &g["acceptedSourceHashes"].as_object().map(|object|
                object.keys().map(|path|json!(path)).collect::<Vec<_>>())
                .unwrap_or_default())
        && g["systemDesign"]["targetFiles"].as_array()
            .is_some_and(|files|!files.is_empty() && files.iter().all(|path|
                path.as_str().is_some_and(|p|
                    g["acceptedSourceHashes"][p].as_str().is_some_and(|sha|
                        sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())))))
}
fn role_touched(g:&Value,role:&str)->bool{
    // Failed turns can teach execution feasibility without creating a
    // misleading claim of a model-authored, adopted improvement.
    model_design_roles(g).iter().any(|name|name==role)
        || ((g["metaPolicyExecuted"]==true
            && g["metaPolicy"]["source"]=="compiled_editable_meta_controller_v1"
            || g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary")
            && g["metaPolicy"]["target"]["role"]==role)
}
fn failure_kind(g:&Value)->&'static str{
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
fn next_generation_productivity(history:&[Value],generation:&Value)->usize{
    if !independent_adoption(generation){return 0;}
    let Some(child_commit)=generation["acceptedCommit"].as_str() else{return 0};
    history.iter().filter(|child|
        child["generatorCommit"]==child_commit
            && (child["metaPolicyExecuted"]==true
                || child["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary")
            && independent_adoption(child)
    ).count()
}
fn confidence_adjustment(history:&[Value])->f64{
    let errors=history.iter().filter_map(|row|
        row["metaLearning"]["publicCaseBrierError"].as_f64()
            .or_else(||row["metaLearning"]["proposalBrierError"].as_f64())
            .or_else(||row["metaLearning"]["brierError"].as_f64())
            .filter(|value|value.is_finite() && *value>=0.0 && *value<=1.0))
        .rev().take(12).collect::<Vec<_>>();
    if errors.is_empty(){return 1.0;}
    let mean=errors.iter().sum::<f64>()/errors.len() as f64;
    (1.0-0.6*mean).clamp(0.4,1.0)
}

/// Compare independent outcomes across generations, rather than choosing
/// an arbitrary first unmodified module. Success counts only when a
/// model-authored system design changed its declared source and the original
/// paired evaluator adopted that change. Future-generation productivity is a
/// distinct observation from immediate benchmark gain.
pub(crate) fn synthesize(
    history:&[Value],frontier:&[Value],allowed_domains:&[&str],
    inherited_domain:Option<&str>
)->Value{
    let calibration=confidence_adjustment(history);
    // The editable meta-controller consumes observed cross-role continuity.
    // This changes the next target, not merely the successor's commentary.
    let mutual=crate::engine::rsi_mutual_evolution::graph(history,frontier);
    let mut ranked=Vec::<(Value,f64)>::new();
    let trials=history.len() as f64;
    for target in frontier.iter().filter(|row|
        row["domain"].as_str().is_some_and(|domain|allowed_domains.contains(&domain))) {
        let Some(role)=target["role"].as_str() else{continue};
        let relevant=history.iter().filter(|g|role_touched(g,role)).collect::<Vec<_>>();
        let attempts=relevant.len();
        let adopted=relevant.iter().filter(|g|independent_adoption(g)).count();
        let regressions=relevant.iter().filter(|g|
            g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0).count();
        let fruitless=relevant.iter().filter(|g|
            g["candidateId"].is_null() && g["accepted"]!=true).count();
        let productivity=relevant.iter()
            .map(|g|next_generation_productivity(history,g)).sum::<usize>();
        // Predict observable task outcomes separately. Unevaluated candidates
        // are never mislabeled as independent evidence of quality.
        let assessed=relevant.iter().filter(|g|
            g["evaluation"]["verdict"].is_string()
                && g["evaluation"]["metrics"]["eligible"].is_boolean())
            .copied().collect::<Vec<_>>();
        let measured=assessed.len();
        let captured=relevant.iter().filter(|g|g["candidateId"].is_string()).count();
        let meta_gains=assessed.iter().filter(|g|
            g["evaluation"]["metrics"]["metaHoldoutGains"].as_u64().unwrap_or(0)>0).count();
        let probability=((adopted as f64+1.0)/(measured as f64+2.0)).clamp(0.0,1.0);
        let epistemic=(probability*(1.0-probability)/(measured as f64+3.0)).sqrt();
        // Learn case-specific candidate behavior ONLY from public training
        // observations, never frozen holdouts. The compiled meta-controller
        // may predict held-out aggregate gains, but cannot see held-out IDs
        // or outcomes as training examples.
        let mut cases=BTreeMap::<String,(u64,u64)>::new();
        for generation in &relevant{
            for row in generation["evaluation"]["metrics"]["measurements"]
                .as_array().into_iter().flatten(){
                if row["holdout"]!=false{continue;}
                let Some(id)=row["id"].as_str().filter(|id|!id.is_empty()
                    && id.len()<=128) else{continue;};
                let Some(passed)=row["candidatePass"].as_bool() else{continue;};
                let counts=cases.entry(id.to_string()).or_insert((0,0));
                counts.0+=1;
                counts.1+=u64::from(passed);
            }
        }
        let public_cases=cases.into_iter().take(16)
            .map(|(id,(attempts,passes))|json!({
                "caseId":id,"candidatePassProbability":
                    (passes as f64+1.0)/(attempts as f64+2.0),
                "observedTrials":attempts,
                "holdout":false,"qualityProven":false
            })).collect::<Vec<_>>();
        let public_case_mean=if public_cases.is_empty(){0.5}else{
            public_cases.iter().map(|row|row["candidatePassProbability"]
                .as_f64().unwrap_or(0.5)).sum::<f64>()/public_cases.len() as f64
        };
        let outcomes=json!({
            "candidateCaptured":(captured as f64+1.0)/(attempts as f64+2.0),
            "adoption":probability,
            "metaImprovement":(meta_gains as f64+1.0)/(measured as f64+2.0),
            "regression":(regressions as f64+1.0)/(measured as f64+2.0),
            "publicCaseForecasts":public_cases,
            "assessedGenerations":measured,
            "predictionProven":false
        });
        let coverage=if target["verifiedRoleImproved"]==true{0.0}else{0.22};
        let inherited_bonus=if inherited_domain==target["domain"].as_str(){0.12}else{0.0};
        let size=target["sourceBytes"].as_u64().unwrap_or(0) as f64;
        let cost=(size/12_000.0).min(1.0);
        let penalty=regressions as f64/(attempts.max(1) as f64);
        let stall=fruitless as f64/(attempts.max(1) as f64);
        let clade=(productivity as f64/(adopted.max(1) as f64)).min(3.0);
        // UCB-style uncertainty, Bayesian shrinkage, and descendant
        // productivity provide a bounded, auditable choice. A failed edit is
        // evidence about execution feasibility, not proof the design is bad.
        let incoming=mutual["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["to"]==role)
            .filter_map(|edge|edge["priority"].as_i64()).max().unwrap_or(0);
        let transfer_bonus=0.20*((incoming as f64)/100.0).clamp(-0.5,1.5);
        let causal=crate::engine::rsi_causal_curriculum::role_interaction_feedback(
            history,role);
        let score=causal["selectionBonus"].as_f64().unwrap_or(0.0)
            +transfer_bonus+0.55*probability+0.48*epistemic*calibration
            +0.14*((trials+2.0).ln()/(attempts as f64+1.0)).sqrt()
            +coverage+inherited_bonus+0.14*clade
            -0.12*penalty-0.07*stall-0.035*cost
            +0.08*outcomes["metaImprovement"].as_f64().unwrap_or(0.5)
            -0.12*outcomes["regression"].as_f64().unwrap_or(0.5)
            +0.07*public_case_mean
            +crate::engine::rsi_open_topology::exploration_bonus(history,role);
        ranked.push((json!({
            "target":target,"role":role,"domain":target["domain"],
            "sourceSha256":target["sourceSha256"],
            "score":score,"mutualTransferPriority":incoming,
            "factorialInteractionEvidence":causal,
            "openTopologyBonus":crate::engine::rsi_open_topology::exploration_bonus(history,role),
            "predictedAdoptionProbability":probability,
            "uncertainty":epistemic*calibration,"attempts":attempts,
            "verifiedModelAdoptions":adopted,"regressions":regressions,
            "editsWithoutCandidate":fruitless,
            "outcomeForecast":outcomes,
            "productiveDescendantAdoptions":productivity,
            "newComponent":target["verifiedRoleImproved"]!=true
        }),score));
    }
    ranked.sort_by(|(left,a),(right,b)|
        b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(||left["role"].as_str().cmp(&right["role"].as_str())));
    let mut winner=ranked.first().map(|row|row.0.clone()).unwrap_or(Value::Null);
    // Repeated forecast misses can trigger improvement of the forecast rule
    // itself, not just another patch to the task agent. Bound this choice so
    // it cannot become an endless metacognitive self-edit loop.
    let recent_errors=history.iter().rev().filter_map(|g|
        g["metaLearning"]["publicCaseBrierError"].as_f64()
            .or_else(||g["metaLearning"]["proposalBrierError"].as_f64())
            .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p)))
        .take(6).collect::<Vec<_>>();
    let meta_recent=history.iter().rev().take(3).filter(|g|
        g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|file|
                file=="src/engine/rsi_meta_controller.rs"))).count();
    let forecast_drift=recent_errors.len()>=3
        && recent_errors.iter().sum::<f64>()/recent_errors.len() as f64>0.30;
    let repair_predictor=forecast_drift && meta_recent<2;
    if repair_predictor{
        if let Some(repair)=ranked.iter().find(|(row,_)|
            row["target"]["path"]=="src/engine/rsi_meta_controller.rs") {
            winner=repair.0.clone();
        }
    }
    let target=winner["target"].clone();
    let mutual_next=mutual["edges"].as_array().into_iter().flatten()
        .find(|edge|edge["to"]==target["role"]).cloned().unwrap_or(Value::Null);
    let reference=history.iter().rev().filter(|g|independent_adoption(g))
        .take(2).map(|g|json!({
            "generationJobId":g["jobId"],"acceptedCommit":g["acceptedCommit"],
            "roles":model_design_roles(g),
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(400).collect::<String>(),
            "design":g["systemDesign"]["design"].as_str().unwrap_or("")
                .chars().take(700).collect::<String>(),
            "targetFiles":g["systemDesign"]["targetFiles"],
            "metaGain":g["evaluation"]["metrics"]["metaHoldoutGains"],
            "productiveDescendants":next_generation_productivity(history,g)
        })).collect::<Vec<_>>();
    let counterexamples=history.iter().rev().filter(|g|
        g["accepted"]!=true && !model_design_roles(g).is_empty())
        .take(3).map(|g|json!({
            "generationJobId":g["jobId"],"roles":model_design_roles(g),
            "failureKind":failure_kind(g),
            "sourceTarget":g["systemDesign"]["targetFiles"],
            "hypothesis":g["systemDesign"]["hypothesis"].as_str().unwrap_or("")
                .chars().take(250).collect::<String>(),
            "error":g["error"].as_str().unwrap_or("")
                .chars().take(500).collect::<String>()
        })).collect::<Vec<_>>();
    let mode=if repair_predictor
        && target["path"]=="src/engine/rsi_meta_controller.rs"{
            "repair_own_prediction_mechanism"
        }else if counterexamples.iter().any(|row|
        row["failureKind"]=="compiler_failure"){"causal_repair_and_new_design"}
        else if !reference.is_empty(){"comparative_hybridization"}
        else{"open_ended_mechanism_discovery"};
    let mut alternatives=ranked.iter().take(6)
        .map(|(row,_)|row.clone()).collect::<Vec<_>>();
    // Guarantee that the method-learning source itself is reachable when
    // enough independent transfer counterexamples require a new policy.
    // This is still a source-indexed frontier option, not a forced win.
    if !alternatives.iter().any(|row|
        row["target"]["path"]=="src/engine/rsi_meta_controller.rs") {
        if let Some((method,_))=ranked.iter().find(|(row,_)|
            row["target"]["path"]=="src/engine/rsi_meta_controller.rs"){
            alternatives.push(method.clone());
        }
    }
    // This native successor policy changes source focus, execution mode,
    // and tool admission. An accepted model-authored successorStage overrides it.
    let path=target["path"].as_str().unwrap_or("");
    let repeated_create_failure=history.iter().rev().take(12).any(|g|
        g["focusFiles"].as_array().is_some_and(|files|
            files.iter().any(|item|item.as_str()==Some(path)))
        && g["error"].as_str().is_some_and(|error|
            error.contains("workspace_write") || error.contains("새 파일 생성 전용")));
    let learning_mode=if winner["editsWithoutCandidate"].as_u64().unwrap_or(0)>0
        && winner["verifiedModelAdoptions"].as_u64().unwrap_or(0)>0{
        "retry_failed"
    }else{"avoid_failed"};
    let edit_mode=if target["sourceSha256"].is_string()
        && target["sourceBytes"].as_u64().unwrap_or(u64::MAX)<=24_000
        && (winner["attempts"].as_u64().unwrap_or(0)>0 || !reference.is_empty()){
        "grounded_edit"
    }else{"explore_then_edit"};
    let tool_mode=if repeated_create_failure{"patch_only"}else{"allow_new_modules"};
    let control=json!({
        "source":"editable_compiled_meta_control_v2",
        "role":target["role"],"domain":target["domain"],
        "path":target["path"],"sourceSha256":target["sourceSha256"],
        "learningPolicy":learning_mode,
        "toolPolicy":tool_mode,
        "executionPolicy":edit_mode,
        "qualityProven":false
    });
    let serialized=json!([target,alternatives,reference,counterexamples,mode,
        inherited_domain,history.len(),control,&mutual, &mutual_next]);
    let revision=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&serialized).unwrap_or_default()));
    json!({
        "source":"compiled_editable_meta_controller_v1",
        "revision":revision,"historyCount":history.len(),
        "mode":mode,"target":target,"selectionEvidence":winner,
        "alternatives":alternatives,
        "controlPolicy":control,
        "mutualDevelopmentGraph":mutual,
        "nextMutualRedesign":mutual_next,
        "taskOutcomeForecast":winner["outcomeForecast"],
        "forecastDrift":forecast_drift,
        "metaRepairChosen":repair_predictor
            && target["path"]=="src/engine/rsi_meta_controller.rs",
        "recentPredictionErrors":recent_errors,
        "referenceDesigns":reference,"counterexamples":counterexamples,
        "forecast":{"acceptanceProbability":winner["predictedAdoptionProbability"],
            "uncertainty":winner["uncertainty"],
            "calibrationFactor":calibration,
            "qualityProven":false},
        "metaMechanismSource":"src/engine/rsi_meta_controller.rs",
        "successorCanRewriteMetaMechanism":true,
        "frozenEvaluatorUnchanged":true
    })
}


// The scheduler remains in this editable, compiled parent; donor portability
// and independent feedback have separate ownership and source-edit targets.
pub(crate) const TRANSFER_METHOD_SOURCE: &str = "src/engine/rsi_meta_controller/peer_transfer.rs";
mod peer_transfer;
mod guidance;
mod observations;
pub(crate) use peer_transfer::{
    comparison_profile, evolve_from_peer_methods, peer_lineages, validate_peer_lineages,
};
pub(crate) use guidance::{instruction,peer_instruction};
pub(crate) use observations::feedback;
#[cfg(test)]
mod tests;
