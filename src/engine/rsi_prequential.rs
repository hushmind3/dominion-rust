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
        && g["controllerPlan"]["metaPolicy"]==*policy
        && g["controllerPlan"]["architectureTarget"]==policy["target"]
        && g["controllerPlan"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["metaRevision"]==policy["revision"]
        && g["focusFiles"].as_array().is_some_and(|paths|
            paths.iter().any(|path|path==&policy["target"]["path"]))
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["pairedMeasured"]==true
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && hex(&g["evaluation"]["evidenceSha256"],64)
        && g["evaluation"]["metrics"]["eligible"].is_boolean()
        // An eligible edit not adopted by its owner is a censored choice,
        // not evidence that the proposed prediction failed.
        && !(g["evaluation"]["metrics"]["eligible"]==true
            && g["accepted"]!=true)
        && (g["accepted"]!=true || g["pairedRevalidated"]==true)
}

/// Record a strictly prequential target-specific outcome. Failed builds,
/// missing paired measurements, foreign model actors, mismatched plans and
/// abandoned work are CENSORED, not negative target labels.
pub(crate) fn feedback(policy:&Value,g:&Value)->Value{
    let target_adopted=g["accepted"]==true
        && g["pairedRevalidated"]==true
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|path|path==&policy["target"]["path"]))
        && policy["target"]["path"].as_str().is_some_and(|path|
            g["acceptedSourceHashes"][path].as_str().is_some_and(|sha|
                sha.len()==64 && sha.bytes().all(|b|b.is_ascii_hexdigit())
                    && sha!=policy["target"]["sourceSha256"].as_str().unwrap_or("")));

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
        "attemptedSourceChange":g["changedSourcePaths"].as_array()
            .is_some_and(|paths|paths.iter()
                .any(|path|path==&policy["target"]["path"])),
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

/// Historical summaries are inputs to the *display*, never training labels.
/// Reconstruct a label from the policy and the independently replayed candidate;
/// reject even a partially edited summary (forecast, outcome or source).
fn trusted_feedback(g:&Value)->Option<Value>{
    let fresh=feedback(&g["metaPolicy"],g);
    let row=&g["metaLearning"];
    if fresh["nextPolicyCanTrainFromObservation"]!=true
        || row["source"]!=fresh["source"]
        || row["nextPolicyCanTrainFromObservation"]!=true
        || row["independentlyMeasured"]!=true
        || row["policyRevision"]!=fresh["policyRevision"]
        || row["targetRole"]!=fresh["targetRole"]
        || row["targetPath"]!=fresh["targetPath"]
        || row["targetSourceSha256"]!=fresh["targetSourceSha256"]
        || row["predictedProbability"]!=fresh["predictedProbability"]
        || row["actualTargetAdoption"]!=fresh["actualTargetAdoption"]
        || row["brierError"]!=fresh["brierError"] {
        return None;
    }
    Some(fresh)
}

/// The forecast controller learns only from its own actually measured outcomes.
/// Different revisions of the *same path* transfer with a discount. Edits to
/// a different file, even under the same broad role, cannot silently change
/// this file's adoption probability.
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
        let Some(row)=trusted_feedback(g) else{continue;};
        if row["targetRole"]!=role || row["targetPath"]!=path {continue;}
        let Some(success)=row["actualTargetAdoption"].as_bool() else{continue;};
        let Some(p)=row["predictedProbability"].as_f64()
            .filter(|v|v.is_finite()&&(0.0..=1.0).contains(v))
            else{continue;};
        let exact_source=row["targetSourceSha256"]==source_sha;
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
        "source":"source_bound_prequential_posterior_v2",
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

/// Re-evaluate forecast drift from replayed outcomes, not mutable Brier
/// summaries. A stopped, declined, unmeasured or substituted edit is censored.
pub(crate) fn drift(history:&[Value],window:usize)->Value{
    let mut losses=Vec::new();
    for generation in history.iter().rev(){
        if losses.len()>=window.min(32){break;}
        let Some(row)=trusted_feedback(generation) else{continue;};
        let Some(loss)=row["brierError"].as_f64()
            .filter(|v|v.is_finite()&&(0.0..=1.0).contains(v))
            else{continue;};
        losses.push(loss);
    }
    let mean=(!losses.is_empty()).then(||
        losses.iter().sum::<f64>()/losses.len() as f64);
    json!({"source":"source_replayed_prequential_drift_v2",
        "recentMeasuredPredictions":losses.len(),
        "meanBrierError":mean,
        "requiresSourceBoundReplay":true})
}

/// A single verified *attempt* can cool down predictor repair; an
/// unmeasured focus hint must not indefinitely suppress self-correction.
pub(crate) fn measured_target_attempt(g:&Value,path:&str)->bool{
    g["pairedReceiptReplayed"]==true
        && g["metaPolicy"]["target"]["path"]==path
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|changed|changed==path))
        && trusted_feedback(g).is_some()
}

/// Distinguish the model's repair intention from executable source editing
/// and independent adoption. This record is NOT itself an evaluation receipt.
pub(crate) fn repair_execution_evidence(g:&Value)->Value{
    let policy=&g["metaPolicy"];
    let target="src/engine/rsi_prequential.rs";
    let planned=policy["predictionRuleRepair"]["repairRuleSourceSelected"]==true
        && policy["target"]["path"]==target;
    let compiled=planned
        && g["controllerPlan"]["metaPolicy"]==*policy
        && g["controllerPlan"]["architectureTarget"]==policy["target"]
        && g["controllerPlan"]["generatorCommit"]==g["generatorCommit"]
        && g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["source"]=="running_compiled_descendant_binary"
        && g["metaPolicyExecuted"]["metaRevision"]==policy["revision"]
        && g["focusFiles"].as_array().is_some_and(|paths|
            paths.iter().any(|path|path==target));
    let changed=compiled && g["pairedMeasured"]==true
        && g["changedSourcePaths"].as_array().is_some_and(|paths|
            paths.iter().any(|path|path==target));
    let adopted=changed && g["pairedReceiptReplayed"]==true
        && trusted_feedback(g)
            .is_some_and(|row|row["actualTargetAdoption"]==true);
    json!({"source":"replayed_prediction_repair_execution_v1",
        "planned":planned,
        "compiledControllerExecuted":compiled,
        "evaluatedCandidateEditedSource":changed,
        "sourceIndependentlyAdopted":adopted,
        "causalAdvantageProven":false})
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
        json!({"metaPolicy":policy,"controllerPlan":{
                "metaPolicy":policy,"architectureTarget":policy["target"],
                "generatorCommit":"c".repeat(40)},
            "generatorCommit":"c".repeat(40),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "metaPolicyExecuted":{"source":"running_compiled_descendant_binary","metaRevision":policy["revision"]},
            "focusFiles":[policy["target"]["path"]],
            "changedSourcePaths":[policy["target"]["path"]],
            "acceptedSourceHashes":{"src/engine/memory.rs":"f".repeat(64)},
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
        g["controllerPlan"]["architectureTarget"]["path"]=json!("other.rs");
        g["metaLearning"]=feedback(&p,&g);
        assert_eq!(g["metaLearning"]["independentlyMeasured"],false);
        let result=calibrate(&[g],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5);
        assert_eq!(result["observations"],0);
        assert_eq!(result["probability"],0.5);
    }
    #[test]
    fn target_specific_feedback_censors_unverified_adoptions(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,true);
        g["pairedRevalidated"]=json!(false);
        assert_eq!(feedback(&p,&g)["brierError"],Value::Null);
        g["pairedRevalidated"]=json!(true);
        g["changedSourcePaths"]=json!(["src/engine/other.rs"]);
        assert_eq!(feedback(&p,&g)["failureKind"],"off_target_adoption");
        assert_eq!(feedback(&p,&g)["actualTargetAdoption"],false);
        g["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        assert_eq!(feedback(&p,&g)["actualTargetAdoption"],true);
    }
    #[test]
    fn matched_source_outcomes_affect_next_choice_more_than_other_files(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,true);
        g["metaLearning"]=feedback(&p,&g);
        let a=calibrate(&[g.clone()],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5);
        let b=calibrate(&[g.clone()],"memory","src/engine/new_memory.rs",
            "e".repeat(64).as_str(),0.5);
        let c=calibrate(&[g],"memory","src/engine/memory.rs",
            "e".repeat(64).as_str(),0.5);
        assert!(a["probability"].as_f64().unwrap()>
            c["probability"].as_f64().unwrap());
        assert!(c["probability"].as_f64().unwrap()>
            b["probability"].as_f64().unwrap());
        assert_eq!(a["exactSourceTrials"],1);
        assert_eq!(b["observations"],0);
        assert_eq!(c["discountedTransferTrials"],1);
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
    #[test]
    fn owner_declined_eligible_patch_is_censored(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,false);
        g["evaluation"]["metrics"]["eligible"]=json!(true);
        g["metaLearning"]=feedback(&p,&g);
        assert_eq!(g["metaLearning"]["independentlyMeasured"],false);
        assert_eq!(drift(&[g.clone()],6)["recentMeasuredPredictions"],0);
        assert_eq!(calibrate(&[g],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.4)["observations"],0);
    }
    #[test]
    fn forged_prediction_and_outcome_cannot_retrain_controller(){
        let p=policy("src/engine/memory.rs");
        let mut g=observed(&p,true);
        g["metaLearning"]=feedback(&p,&g);
        assert_eq!(drift(&[g.clone()],6)["recentMeasuredPredictions"],1);
        g["metaLearning"]["actualTargetAdoption"]=json!(false);
        g["metaLearning"]["predictedProbability"]=json!(0.99);
        g["metaLearning"]["brierError"]=json!(0.0);
        assert_eq!(drift(&[g.clone()],6)["recentMeasuredPredictions"],0);
        assert_eq!(calibrate(&[g],"memory","src/engine/memory.rs",
            "b".repeat(64).as_str(),0.5)["observations"],0);
    }
    #[test]
    fn repair_intent_is_not_an_adopted_repaired_predictor(){
        let mut p=policy("src/engine/rsi_prequential.rs");
        p["predictionRuleRepair"]=json!({"repairRuleSourceSelected":true});
        let mut g=observed(&p,false);
        g["changedSourcePaths"]=json!([]);
        assert_eq!(repair_execution_evidence(&g)["planned"],true);
        assert_eq!(repair_execution_evidence(&g)["compiledControllerExecuted"],true);
        assert_eq!(repair_execution_evidence(&g)["evaluatedCandidateEditedSource"],false);
        assert_eq!(repair_execution_evidence(&g)["sourceIndependentlyAdopted"],false);
        g["changedSourcePaths"]=json!(["src/engine/rsi_prequential.rs"]);
        assert_eq!(repair_execution_evidence(&g)["sourceIndependentlyAdopted"],false);
    }

}
