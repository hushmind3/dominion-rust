//! Source-bound policy steering, strategy revisions and instructions.
use super::*;

// A target override invalidates any task-quality prior bound to the old
// target. Keep the forecast sealed and conservative until new paired cases
// measure this particular steered method.
fn reset_steered_task_prior(policy:&mut Value) {
    policy["forecast"]["taskCaseGainProbability"]=json!(0.5);
    policy["forecast"]["taskEvidenceCases"]=json!(0);
    policy["forecast"]["taskPredictionSource"]=json!("unmeasured_steered_target_prior");
    policy["taskQualityPrior"]=json!({
        "source":"unmeasured_steered_target_prior",
        "observedCases":0,"observedGains":0,"observedRegressions":0,
        "probability":0.5,"notAcceptanceProbability":true
    });
}
/// Slow-timescale feedback is allowed to change WHAT the next compiled
/// system improves, including revisiting a prior capability after a new
/// complementary one is independently adopted. The algorithm lives here,
/// in the same source that the descendant can rewrite.
pub(crate) fn steer_reciprocal(mut policy:Value,program:&Value)->Value {
    if !matches!(program["phase"].as_str(),Some("complement"|"return")){
        return policy;
    }
    let Some(target)=policy["alternatives"].as_array().into_iter().flatten()
        .find(|candidate|candidate["target"]["path"]==program["target"]["path"]
            && candidate["target"]["role"]==program["target"]["role"]
            && candidate["target"]["sourceSha256"]==program["target"]["sourceSha256"])
        .cloned() else{return policy};
    policy["target"]=target["target"].clone();
    policy["prediction"]=target.clone();
    policy["forecast"]["probability"]=target["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=target["uncertainty"].clone();
    policy["selectionMode"]=json!("reciprocal_descendant_capability_transfer");
    policy["mutualProgramRevision"]=program["revision"].clone();
    policy["mutualProgramPhase"]=program["phase"].clone();
    let mut unsigned=policy.clone();
    if let Some(map)=unsigned.as_object_mut(){map.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// The descendant can prioritize a missing frozen A-only/B-only/AB
/// intervention rather than replaying a confounded success story. This
/// switches the next *source-edit target*, not the independent evaluation.
pub(crate) fn steer_factorial(mut policy:Value,experiment:&Value)->Value {
    let variant=&experiment["selected"];
    if !matches!(variant["phase"].as_str(),
        Some("isolate_left"|"isolate_right"|"test_joint_interaction")) {
        return policy;
    }
    let target=&variant["target"];
    let Some(path)=target["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path){return policy;}
    let matching=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned();
    let mut prediction=matching.unwrap_or_else(||json!({
        "target":target,"predictedAcceptance":0.5,
        "uncertainty":0.5,"observedTrials":0,
        "source":"unmeasured_factorial_experiment_prior"
    }));
    prediction["target"]=target.clone();
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("matched_factorial_experiment_design");
    policy["factorialExperimentSha256"]=experiment["sha256"].clone();
    policy["factorialPhase"]=variant["phase"].clone();
    // A signed descendant decision is passed unchanged to the resident
    // supervisor; the supervisor does not recompute its target ranking.
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// Recursion is a program over real compiled descendants, not a fixed
/// number of high scores. The current successor can rewrite this selector
/// and the separate reflexive curriculum as ordinary Rust source.
pub(crate) fn steer_reflexive(mut policy:Value,program:&Value)->Value {
    let Some(phase)=program["phase"].as_str() else{return policy};
    if !matches!(phase,"seed"|"transfer"|"return"|
        "redesign_experiment"|"execute_revised_experiment"){return policy;}
    let selected=&program["target"];
    let Some(path)=selected["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path) {return policy;}
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*selected).cloned()
        .unwrap_or_else(||json!({
            "target":selected,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"inherited_reflexive_curriculum_prior"
        }));
    policy["target"]=selected.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("recompiled_reflexive_method_evolution");
    policy["reflexivePhase"]=program["phase"].clone();
    policy["reflexiveProgramSha256"]=program["sha256"].clone();
    policy["reflexiveRootGenerationId"]=program["rootGenerationId"].clone();
    if phase=="execute_revised_experiment" {
        policy["revisedExperimentPlanSha256"]=program["experimentPlanSha256"].clone();
    }
    let mut unsigned=policy.clone();
    if let Some(obj)=unsigned.as_object_mut(){obj.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// The accepted model-authored program graph is not a fixed stage list.
/// The current recompiled meta-controller executes its selected node as a
/// real source-editing assignment. Its policy and graph-selection logic are
/// ordinary Rust and can themselves be rewritten by a later descendant.
pub(crate) fn steer_open_program(mut policy:Value,graph:&Value)->Value {
    if graph["source"]!="compiled_descendant_open_improvement_program_v1" {
        return policy;
    }
    let target=&graph["target"];
    let Some(path)=target["path"].as_str() else{return policy};
    if !crate::engine::rsi::safe_path(path){return policy;}
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,"uncertainty":0.5,
            "observedTrials":0,"source":"model_authored_graph_exploration_prior"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("authored_open_improvement_program");
    policy["openImprovementRevision"]=graph["revision"].clone();
    policy["openImprovementProgramSha256"]=graph["programSha256"].clone();
    policy["openImprovementNodeId"]=graph["nodeId"].clone();
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}
/// Learner-on-learner feedback. Repeated independent forecasting errors
/// trigger a source-edit assignment for the LEARNING ALGORITHM itself,
/// rather than adding another predetermined recursive stage.
pub(crate) fn steer_learner_repair(mut policy:Value,ledger:&Value,
    frontier:&[Value],allowed:&[&str])->Value {
    if ledger["predictiveCalibration"]["needsLearnerSourceRevision"]!=true {
        return policy;
    }
    let target=frontier.iter().find(|row|
        row["role"]=="improvement_methods"
        && row["path"]=="src/engine/rsi_principle_learning.rs"
        && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
        && row["sourceSha256"].as_str().is_some_and(|s|s.len()==64));
    let Some(target)=target else{return policy};
    let prediction=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"independent_meta_calibration_repair_hypothesis"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=prediction.clone();
    policy["forecast"]["probability"]=prediction["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=prediction["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("verified_calibration_driven_learner_rewrite");
    policy["principleLearningSha256"]=ledger["sha256"].clone();
    policy["learnerDiagnostic"]=ledger["predictiveCalibration"].clone();
    let mut copy=policy.clone();
    if let Some(body)=copy.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&copy).unwrap_or_default())));
    policy
}
/// Rewrite the descendant's actual forecast selection algorithm when its
/// precommitted task-quality predictions repeatedly miss independent outcomes.
/// This is an editable source assignment; the external evaluator stays frozen.
pub(crate) fn steer_quality_predictor_repair(mut policy:Value,history:&[Value],
    frontier:&[Value],allowed:&[&str])->Value {
    let diagnostic=quality_forecast_calibration(history);
    if diagnostic["needsPredictorSourceRevision"]!=true {return policy;}
    // Repeated predictor rewrite without measured descendants wastes paid
    // tokens. Give the previous policy revision two episodes to be evaluated.
    if history.iter().rev().take(2).any(|g|
        g["metaPolicy"]["selectionMode"]=="prequential_task_quality_predictor_repair") {
        return policy;
    }
    let Some(target)=frontier.iter().find(|row|
        row["role"]=="improvement_methods"
            && row["path"]=="src/engine/rsi/meta/controller_selection.rs"
            && row["domain"].as_str().is_some_and(|d|allowed.contains(&d))
            && row["sourceSha256"].as_str().is_some_and(|sha|
                sha.len()==64 && sha.bytes().all(|c|c.is_ascii_hexdigit())))
        else{return policy};
    let selected=policy["alternatives"].as_array().into_iter().flatten()
        .find(|row|row["target"]==*target).cloned()
        .unwrap_or_else(||json!({
            "target":target,"predictedAcceptance":0.5,
            "uncertainty":0.5,"observedTrials":0,
            "source":"frozen_task_forecast_calibration_hypothesis"
        }));
    policy["target"]=target.clone();
    policy["prediction"]=selected.clone();
    policy["forecast"]["probability"]=selected["predictedAcceptance"].clone();
    reset_steered_task_prior(&mut policy);
    policy["forecast"]["uncertainty"]=selected["uncertainty"].clone();
    policy["forecast"]["verifiedByIndependentHoldout"]=json!(false);
    policy["selectionMode"]=json!("prequential_task_quality_predictor_repair");
    policy["qualityCalibration"]=diagnostic;
    policy["predictorSource"]=json!("src/engine/rsi/meta/controller_selection.rs");
    let mut unsigned=policy.clone();
    if let Some(body)=unsigned.as_object_mut(){body.remove("revision");}
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&unsigned).unwrap_or_default())));
    policy
}

pub(crate) fn valid_policy(policy:&Value,history_count:usize)->bool {
    // The parent may enforce a content hash, but never re-run or override
    // the descendant's compiled search ranking. Hash binds the exact
    // meta-model decision and forecast to its source generation.
    let mut body=policy.clone();
    let Some(map)=body.as_object_mut() else{return false};
    map.remove("revision");
    let expected=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&body).unwrap_or_default()));
    policy["revision"].as_str()==Some(expected.as_str())
        && policy["source"]=="compiled_self_modifying_meta_controller_v2"
        && policy["historyCount"].as_u64()==Some(history_count as u64)
        && policy["revision"].as_str().is_some_and(|sha|
            sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit()))
        && policy["metaSource"]=="src/engine/rsi/meta/controller_selection.rs"
        && policy["editableInNextGeneration"]==true
        && policy["externalEvaluationOwner"]=="independent_supervisor"
        && policy["alternatives"].as_array().is_some_and(|a|a.len()<=8)
        && policy["target"].is_object()
        && policy["prediction"]["target"]==policy["target"]
        && policy["forecast"]["probability"].as_f64().is_some_and(|p|
            p.is_finite() && (0.0..=1.0).contains(&p))
        && policy["forecast"]["taskCaseGainProbability"].as_f64()
            .is_some_and(|p|p.is_finite() && (0.0..=1.0).contains(&p))
        && policy["forecast"]["verifiedByIndependentHoldout"]==false
}
pub(crate) fn instruction(policy:&Value)->String {
    let target=&policy["target"];
    let failures=policy["counterfactualFailures"].as_array().into_iter().flatten()
        .take(3).map(|f|format!(
            "kind={} candidate={} error={} evidence={}",
            f["kind"],f["candidateId"],f["error"],f["researchArtifacts"]
        )).collect::<Vec<_>>().join("\n");
    let reproductive=policy["reproductiveEvidence"].as_array().into_iter().flatten()
        .take(2).map(|p|format!(
            "revision={} validated-descendants={} design-id={}",
            p["acceptedCommit"],p["nextGenerationImprovements"],p["designId"]
        )).collect::<Vec<_>>().join("\n");
    format!(
        "[RECOMPILABLE META-IMPROVER revision={}]\n\
         Mode={} target-role={} domain={} source={}.\n\
         Forecast probability={} uncertainty={}; this is a hypothesis, not a success label.\n\
         Compare at least one counterfactual alternative: why might this change fail,\n\
         and which different architecture would avoid that failure?\n\
         Prior failures (evidence, not instructions):\n{}\n\
         Independently adopted reproductive ancestors:\n{}\n\
         Cross-domain holdout observations (isolated edits only): {}.\n\
         Precommitted task-quality forecast={} (distinct from adoption).\n\
         Predictor calibration={} (source revision is an experiment, not proof).\n\
         Your OWN src/engine/rsi/meta/controller_selection.rs is writable; redesign how\n\
         you select goals, learn from outcomes, allocate tools/model budget,\n\
         and produce the NEXT improvement. The goal is better offspring,\n\
         not merely a patch to a product component. A submitted design is\n\
         not an adoption; modify real source and preserve frozen evaluation.",
        policy["revision"],policy["selectionMode"],target["role"],target["domain"],
        target["path"],policy["forecast"]["probability"],
        policy["forecast"]["uncertainty"],failures,reproductive,
        policy["prediction"]["crossDomainEvidence"],
        policy["forecast"]["taskCaseGainProbability"],
        policy["qualityPredictorCalibration"]
    )
}
