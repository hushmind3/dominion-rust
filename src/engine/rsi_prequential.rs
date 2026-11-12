//! Source-bound prequential learning for the compiled RSI metacontroller.
//!
//! Prediction is made before a patch. A label is learned only from the
//! original independently replayed paired receipt, bound to the exact policy,
//! source target and actual descendant plan. A success elsewhere in the
//! multi-file patch is NOT a success of the selected target.
//!
//! This module lives in the editable descendant and may itself evolve. It
//! never promotes patches or overrides the frozen evaluator.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

fn hex(value:&Value,length:usize)->bool{
    value.as_str().is_some_and(|s|
        s.len()==length&&s.bytes().all(|b|b.is_ascii_hexdigit()))
}

fn measured(policy:&Value,g:&Value)->bool{
    hex(&policy["revision"],64)
        && hex(&policy["target"]["sourceSha256"],64)
        && policy["target"]["role"].as_str().is_some_and(|s|!s.is_empty())
        && policy["target"]["path"].as_str().is_some_and(|s|
            crate::engine::rsi::safe_path(s))
        && g["metaPolicy"]==*policy
        && g["descendantPlan"]["metaPolicy"]==*policy
        && g["descendantPlan"]["architectureTarget"]==policy["target"]
        && g["descendantPlan"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_meta_controller"
        && g["metaPolicyExecuted"]["metaRevision"]==policy["revision"]
        // A valid plan is not an executed experiment. The actual worker
        // must have been directed to inspect and edit the forecast target.
        && g["focusFiles"].as_array().is_some_and(|paths|
            paths.iter().any(|p|p==&policy["target"]["path"]))
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["pairedMeasured"]==true
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && hex(&g["evaluation"]["evidenceSha256"],64)
        && g["evaluation"]["metrics"]["eligible"].is_boolean()
        // A measured eligible candidate can be declined by its owner.
        // This must not become a fabricated failure of the source method.
        && !(g["evaluation"]["metrics"]["eligible"]==true
            && g["accepted"]!=true)
        && (g["accepted"]!=true || g["pairedRevalidated"]==true)
}

/// Record a strictly prequential target-specific outcome. Failed builds,
/// missing paired measurements, foreign model actors, mismatched plans and
/// abandoned work are CENSORED, not negative target labels.
pub(crate) fn feedback(policy:&Value,g:&Value,target_adopted:bool)->Value{
    if policy["source"]!="compiled_editable_meta_controller_v1"{
        return Value::Null;
    }
    let prior=policy["forecast"]["acceptanceProbability"].as_f64()
        .filter(|p|p.is_finite()&&(0.0..=1.0).contains(p));
    let assessed=prior.is_some()&&measured(policy,g);
    let observed=assessed.then_some(target_adopted);
    let brier=match(prior,observed){
        (Some(p),Some(success))=>Some((p-if success{1.0}else{0.0}).powi(2)),
        _=>None
    };
    json!({
        "source":"source_bound_prequential_feedback_v1",
        "policyRevision":policy["revision"],
        "targetRole":policy["target"]["role"],
        "targetPath":policy["target"]["path"],
        "targetSourceSha256":policy["target"]["sourceSha256"],
        "predictedProbability":prior,
        "independentlyMeasured":assessed,
        "actualTargetAdoption":observed,
        "actualModelAuthoredAdoption":observed,
        "brierError":brier,
        "attemptedSourceChange":g["candidateId"].is_string(),
        "nextPolicyCanTrainFromObservation":assessed,
        "qualityProven":observed==Some(true),
        "observationScope":"selected_source_not_whole_patch",
        "failureKind":if !assessed{"unmeasured_or_unbound"}
            else if observed==Some(true){"target_adopted"}
            else if g["accepted"]==true{"off_target_adoption"}
            else if g["evaluation"]["metrics"]["regressions"].as_u64()
                .unwrap_or(0)>0{"paired_regression"}
            else{"measured_target_not_adopted"}
    })
}

/// Use recent, source-matched observed outcomes to adjust a PRIOR from
/// causal-credit accounting. Cross-revision observations remain hypotheses
/// with reduced weight, rather than being treated as an unchanged source.
/// This is a bounded, editable Bayesian updater, not an evaluation oracle.
pub(crate) fn calibrate(
    history:&[Value],role:&str,path:&str,source_sha:&str,prior:f64
)->Value{
    let prior=if prior.is_finite(){prior.clamp(0.0,1.0)}else{0.5};
    let mut successes=0.0f64;
    let mut weight_sum=0.0f64;
    let mut predicted_losses=0.0f64;
    let mut exact=0u64;
    let mut transferred=0u64;
    let mut observed_count=0u64;
    for g in history.iter().rev(){
        if observed_count>=32{break;}
        let row=&g["metaLearning"];
        if row["source"]!="source_bound_prequential_feedback_v1"
            || row["nextPolicyCanTrainFromObservation"]!=true
            || row["targetRole"]!=role
            || row["policyRevision"]!=g["metaPolicy"]["revision"]
            || row["targetRole"]!=g["metaPolicy"]["target"]["role"]
            || row["targetPath"]!=g["metaPolicy"]["target"]["path"]
            || row["targetSourceSha256"]!=g["metaPolicy"]["target"]["sourceSha256"]
            || !measured(&g["metaPolicy"],g) {continue;}
        let Some(success)=row["actualTargetAdoption"].as_bool() else{continue;};
        let Some(p)=row["predictedProbability"].as_f64()
            .filter(|v|v.is_finite()&&(0.0..=1.0).contains(v))
            else{continue;};
        let exact_source=row["targetPath"]==path
            && row["targetSourceSha256"]==source_sha;
        let similarity=if exact_source{1.0}else{0.25};
        let weight=similarity*0.96f64.powi(observed_count as i32);
        observed_count+=1;
        if exact_source{exact+=1;}else{transferred+=1;}
        weight_sum+=weight;
        successes+=weight*if success{1.0}else{0.0};
        predicted_losses+=weight*(p-if success{1.0}else{0.0}).powi(2);
    }
    // Two virtual prior trials keep tiny or changed lineages from
    // overreacting to one lucky accepted patch.
    let estimate=(2.0*prior+successes)/(2.0+weight_sum);
    let uncertainty=(estimate*(1.0-estimate)/(3.0+weight_sum)).sqrt();
    json!({
        "source":"source_bound_prequential_posterior_v1",
        "probability":estimate,
        "uncertainty":uncertainty,
        "unadjustedPrior":prior,
        "effectiveObservedTrials":weight_sum,
        "exactSourceTrials":exact,
        "discountedTransferTrials":transferred,
        "observations":observed_count,
        "observedWeightedBrier":if weight_sum>0.0 {
            Some(predicted_losses/weight_sum)
        }else{None},
        "sourceLocalLearning":exact>0,
        "crossSourceOutcomeIsNotAdoption":true
    })
}


/// An inherited prediction-repair decision must survive composition with
/// principle learning, recurrent rewrites and the factorial scheduler. The
/// compiled successor can change this arbiter in a future generation.
pub(crate) fn arbitrate_repair(
    mut meta:Value,frontier:&[Value],allowed:&[&str],
    automatic:bool,stage_active:bool
)->Value{
    let repair=meta["predictionRuleRepair"].clone();
    if repair["source"]!="independently_observed_forecast_drift_v1"
        || repair["repairRuleSourceSelected"]!=true {return meta;}
    let allowed_now=automatic && !stage_active && allowed.contains(&"improver");
    let path="src/engine/rsi_prequential.rs";
    let selected=frontier.iter().find(|target|
        target["path"]==path && target["role"]=="improvement_methods"
        && target["domain"]=="improver" && hex(&target["sourceSha256"],64)
        && meta["alternatives"].as_array().is_some_and(|alternatives|
            alternatives.iter().any(|entry|entry.get("target")==Some(*target))));
    if !allowed_now || selected.is_none(){
        meta["predictionRuleRepair"]["active"]=json!(false);
        meta["predictionRuleRepair"]["deferredBy"]=json!(
            if stage_active{"verified_successor_stage"}
            else if !automatic{"manual_source_selection"}
            else{"owner_domain_or_source_not_available"});
        return meta;
    }
    let target=(*selected.expect("checked")).clone();
    let winner=meta["alternatives"].as_array().into_iter().flatten()
        .find(|entry|entry["target"]==target).cloned()
        .expect("frontier selection already required matching alternative");
    let old_revision=meta["revision"].clone();
    if meta["principleLearning"]["active"]==true{
        meta["principleLearning"]["active"]=json!(false);
        meta["principleLearning"]["nextTrialRequired"]=json!(false);
        meta["principleLearning"]["deferredBy"]=
            json!("source_bound_prediction_repair");
    }
    if meta["recursiveProgram"]["active"]==true {
        meta["recursiveProgram"]["active"]=json!(false);
        meta["recursiveProgram"]["deferredBy"]=
            json!("source_bound_prediction_repair");
    }
    meta["target"]=target.clone();
    meta["selectionEvidence"]=winner.clone();
    meta["forecast"]["acceptanceProbability"]=
        winner["predictedAdoptionProbability"].clone();
    meta["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    meta["mode"]=json!("source_bound_prediction_repair_execution");
    meta["predictionRuleRepair"]["active"]=json!(true);
    meta["predictionRuleRepair"]["executionTarget"]=target.clone();
    meta["predictionRuleRepair"]["scheduledBy"]=json!("compiled_descendant_source_rewrite_arbiter");
    meta["predictionRuleRepair"]["requiresIndependentPairedReceipt"]=json!(true);
    meta["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([old_revision,target,winner,
            meta["predictionRuleRepair"],meta["principleLearning"],
            meta["recursiveProgram"]])).unwrap_or_default())));
    meta
}

/// Persist a distinction between scheduled meta repair, actual scoped edits,
/// and independently measured/adopted source code. Neither an instruction nor
/// a compiler check counts as empirical method improvement.
pub(crate) fn repair_execution_evidence(g:&Value)->Value{
    let program=&g["metaPolicy"]["predictionRuleRepair"];
    let active=program["active"]==true;
    let target=&program["executionTarget"];
    let valid=active
        && target==&g["metaPolicy"]["target"]
        && target==&g["descendantPlan"]["architectureTarget"]
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["metaRevision"]==g["metaPolicy"]["revision"]
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_meta_controller";
    let path=target["path"].as_str().filter(|s|crate::engine::rsi::safe_path(s));
    let focused=valid && path.is_some_and(|p|g["focusFiles"]
        .as_array().is_some_and(|rows|rows.iter().any(|row|row.as_str()==Some(p))));
    let edited=focused && path.is_some_and(|p|g["changedSourcePaths"]
        .as_array().is_some_and(|rows|rows.iter().any(|row|row.as_str()==Some(p))));
    let measured=edited && g["pairedMeasured"]==true;
    let adopted=measured && g["accepted"]==true
        && g["pairedRevalidated"]==true
        && path.is_some_and(|p|hex(&g["acceptedSourceHashes"][p],64));
    json!({
        "source":"compiled_descendant_source_prediction_repair_evidence_v1",
        "planned":active,"compiledPlanExecuted":valid,
        "actualWorkerFocusedOnSource":focused,
        "evaluatedCandidateEditedSource":edited,
        "originalPairedReceiptReplayed":measured,
        "sourceIndependentlyAdopted":adopted,
        "metaMethodPerformanceGainProven":false,
        "repairProgramRevision":g["metaPolicy"]["revision"],
        "sourcePath":target["path"],
        "deferredBy":program["deferredBy"]
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    fn policy(path:&str)->Value{
        json!({"source":"compiled_editable_meta_controller_v1",
            "revision":"a".repeat(64),
            "target":{"role":"memory","domain":"memory",
                "path":path,"sourceSha256":"b".repeat(64)},
            "forecast":{"acceptanceProbability":0.6}})
    }
    fn observed(policy:&Value,adopted:bool)->Value{
        json!({"metaPolicy":policy,"descendantPlan":{
                "metaPolicy":policy,"architectureTarget":policy["target"],
                "generatorCommit":"c".repeat(40)},
            "generatorCommit":"c".repeat(40),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "metaPolicyExecuted":{"source":"running_compiled_descendant_meta_controller",
                "metaRevision":policy["revision"]},
            "focusFiles":[policy["target"]["path"]],
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":"candidate-1","pairedMeasured":true,
            "accepted":adopted,"pairedRevalidated":adopted,
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"d".repeat(64),
                "metrics":{"eligible":adopted,"regressions":0}}})
    }
    #[test]
    fn unbound_and_unmeasured_records_cannot_calibrate(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,false);
        g["descendantPlan"]["architectureTarget"]["path"]=json!("other.rs");
        g["metaLearning"]=feedback(&p,&g,true);
        assert_eq!(g["metaLearning"]["independentlyMeasured"],false);
        let result=calibrate(&[g],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5);
        assert_eq!(result["observations"],0);
        assert_eq!(result["probability"],0.5);
    }
    #[test]
    fn owner_declined_but_eligible_patch_does_not_count_as_failure(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,false);
        g["evaluation"]["metrics"]["eligible"]=json!(true);
        assert_eq!(feedback(&p,&g,false)["independentlyMeasured"],false);
        assert_eq!(feedback(&p,&g,false)["brierError"],Value::Null);
    }

    #[test]
    fn target_specific_feedback_censors_unverified_adoptions(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,true);
        g["pairedRevalidated"]=json!(false);
        assert_eq!(feedback(&p,&g,true)["brierError"],Value::Null);
        g["pairedRevalidated"]=json!(true);
        assert_eq!(feedback(&p,&g,false)["failureKind"],"off_target_adoption");
        assert_eq!(feedback(&p,&g,false)["actualTargetAdoption"],false);
        assert_eq!(feedback(&p,&g,true)["actualTargetAdoption"],true);
    }
    #[test]
    fn matched_source_outcomes_affect_next_choice_more_than_other_files(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,true);
        g["metaLearning"]=feedback(&p,&g,true);
        let a=calibrate(&[g.clone()],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5);
        let b=calibrate(&[g],"memory","src/engine/new_memory.rs",
            "e".repeat(64).as_str(),0.5);
        assert!(a["probability"].as_f64().unwrap()>
            b["probability"].as_f64().unwrap());
        assert_eq!(a["exactSourceTrials"],1);
        assert_eq!(b["discountedTransferTrials"],1);
    }

    #[test]
    fn true_execution_requires_actual_source_focus_and_compiled_meta_revision(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,false);
        assert_eq!(feedback(&p,&g,false)["independentlyMeasured"],true);
        g["focusFiles"]=json!(["src/engine/learning.rs"]);
        assert_eq!(feedback(&p,&g,false)["independentlyMeasured"],false);
        g["focusFiles"]=json!(["src/engine/memory.rs"]);
        g["metaPolicyExecuted"]["metaRevision"]=json!("c".repeat(64));
        assert_eq!(feedback(&p,&g,false)["independentlyMeasured"],false);
    }

    #[test]
    fn compiled_arbiter_preserves_repair_and_does_not_forge_execution(){
        let p=policy("src/engine/memory.rs");
        let repair_target=json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_prequential.rs",
            "sourceSha256":"e".repeat(64)});
        let winner=json!({"target":repair_target,
            "predictedAdoptionProbability":0.35,"uncertainty":0.2});
        let mut meta=p.clone();
        meta["target"]=json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64)});
        meta["alternatives"]=json!([winner]);
        meta["principleLearning"]=json!({"active":true,"nextTrialRequired":true});
        meta["recursiveProgram"]=json!({"active":true});
        meta["predictionRuleRepair"]=json!({
            "source":"independently_observed_forecast_drift_v1",
            "repairRuleSourceSelected":true});
        let chosen=arbitrate_repair(meta.clone(),&[repair_target.clone()],
            &["memory","improver"],true,false);
        assert_eq!(chosen["target"],repair_target);
        assert_eq!(chosen["mode"],"source_bound_prediction_repair_execution");
        assert_eq!(chosen["principleLearning"]["active"],false);
        assert_eq!(chosen["recursiveProgram"]["active"],false);
        assert_eq!(chosen["predictionRuleRepair"]["active"],true);
        let mut generation=observed(&p,false);
        generation["metaPolicy"]=chosen.clone();
        generation["descendantPlan"]["metaPolicy"]=chosen.clone();
        generation["descendantPlan"]["architectureTarget"]=chosen["target"].clone();
        let planned=repair_execution_evidence(&generation);
        assert_eq!(planned["planned"],true);
        assert_eq!(planned["sourceIndependentlyAdopted"],false);
        assert_eq!(planned["evaluatedCandidateEditedSource"],false);
        let stage=arbitrate_repair(meta,&[repair_target],&["improver"],true,true);
        assert_eq!(stage["predictionRuleRepair"]["active"],false);
        assert_eq!(stage["predictionRuleRepair"]["deferredBy"],
            "verified_successor_stage");
    }
    #[test]
    fn unsigned_past_feedback_is_not_self_training_evidence(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,false);
        g["pairedMeasured"]=json!(false);
        g["metaLearning"]=json!({
            "source":"source_bound_prequential_feedback_v1",
            "nextPolicyCanTrainFromObservation":true,
            "policyRevision":p["revision"],"targetRole":"memory",
            "targetPath":p["target"]["path"],
            "targetSourceSha256":p["target"]["sourceSha256"],
            "predictedProbability":1.0,"actualTargetAdoption":true});
        assert_eq!(calibrate(&[g],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5)["observations"],0);
    }
}
