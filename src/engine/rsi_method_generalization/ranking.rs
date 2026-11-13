//! Editable source-grounded policy for choosing method rewrite vs successor
//! downstream application based on independent control receipts.
use super::*;

pub(crate) fn adapt(mut algebra:Value,feedback:&Value)->Value{
    if !feedback["operatorLearning"].is_array() || !algebra["rankedCandidates"].is_array(){
        return algebra;
    }
    let mut ranked=algebra["rankedCandidates"].as_array().cloned()
        .unwrap_or_default();
    let rows=feedback["operatorLearning"].as_array().cloned().unwrap_or_default();
    for row in &mut ranked {
        let Some(id)=row["ruleId"].as_str()else{continue};
        let Some(evidence)=rows.iter().find(|v|v["ruleId"]==id)else{continue};
        let trials=evidence["independentReplayedTrials"].as_u64().unwrap_or(0);
        let reg=evidence["holdoutRegressions"].as_u64().unwrap_or(0);
        let gain=evidence["holdoutGains"].as_u64().unwrap_or(0);
        let cross=evidence["crossCapabilityGains"].as_u64().unwrap_or(0);
        let transfer=evidence["transferScore"].as_f64().unwrap_or(0.0);
        let revision_support=evidence["matchedFrozenRevisionCases"]
            .as_u64().unwrap_or(0);
        let matched_lift=evidence["observedRevisionTransferLift"]
            .as_f64().unwrap_or(0.0);
        let revision_weight=if revision_support>0 {
            matched_lift.clamp(-2.0,2.0)*
                (revision_support as f64/(revision_support as f64+6.0))
                *300.0
        }else{0.0};
        let control_trials=evidence["verifiedMethodControlTrials"]
            .as_u64().unwrap_or(0);
        let control_wins=evidence["methodControlQualityWins"]
            .as_u64().unwrap_or(0);
        let control_losses=evidence["methodControlQualityLosses"]
            .as_u64().unwrap_or(0);
        // A real same-source intervention can influence WHICH next method
        // is rebuilt. A plain accepted source patch gets no intervention
        // credit. Sparse controls receive a bounded, conservative bonus.
        let control_weight=if evidence["revisedMethodAwaitingDownstreamControl"]==true {
            1050
        }else if control_trials>0 && control_wins>0 && control_losses==0 {
            (control_wins.min(4)*120) as i64
        }else if control_losses>0 {
            -(control_losses.min(4) as i64)*180
        }else {0};
        let adjusted=((transfer*600.0)+revision_weight).round() as i64
            + (if cross>0 && reg==0 {240}else{0}) + control_weight;
        let base=row["selectionPriority"].as_i64().unwrap_or(0);
        row["selectionPriority"]=json!(base.saturating_add(adjusted));
        row["generalizationEvidence"]=evidence.clone();
        // When the method was actually used and consistently hurt external
        // heldouts, prefer editing that method rather than repeating the same
        // target. NO reward from rejected self-reported candidate output.
        if evidence["revisedMethodAwaitingDownstreamControl"]==true {
            // The product target is supplied by source-grounded operator
            // synthesis. B2 must demonstrate that the revised method
            // actually improves another capability; an A2 plan is not proof.
            let product=row["productTarget"].clone();
            if product["path"].as_str().is_some_and(rsi::safe_path)
                && product["path"]!=row["rule"]["method"]["path"] {
                row["target"]=product;
                row["intent"]=json!("run_revised_method_in_controlled_descendant");
            }
        }else if control_trials>0 && control_wins>0 && control_losses==0 {
            let method=row["rule"]["method"].clone();
            if method["path"].as_str().is_some_and(rsi::safe_path)
                && method["sourceSha256"].as_str().is_some_and(|s|hash_ok(s,64)) {
                // A2 must be a REAL edit of the measured method bytes, not
                // another ordinary capability patch. The subsequent B2
                // still has to execute and outperform its own OFF worker.
                row["target"]=method;
                row["intent"]=json!("evolve_measured_method_after_controlled_downstream_gain");
                row["feedbackControlReceipts"]=evidence["methodControlEvidenceSha256"].clone();
            }
        } else if trials>=2 && reg>=2 && reg>gain {
            let method=row["rule"]["method"].clone();
            row["target"]=method;
            row["intent"]=json!("rewrite_method_after_replayed_transfer_regression");
        }
    }
    ranked.sort_by(|a,b|b["selectionPriority"].as_i64()
        .cmp(&a["selectionPriority"].as_i64())
        .then_with(||a["ruleId"].as_str().cmp(&b["ruleId"].as_str())));
    algebra["selected"]=ranked.first().cloned().unwrap_or(Value::Null);
    algebra["rankedCandidates"]=json!(ranked);
    algebra["methodGeneralizationRevision"]=feedback["sha256"].clone();
    algebra["selectionPolicy"]=json!("editable_replayed_downstream_generalization_v1");
    algebra.as_object_mut().map(|o|o.remove("sha256"));
    algebra["sha256"]=json!(digest(&algebra));
    algebra
}
/// Avoid starving task intelligence when an old meta-operator is available.
/// Reward learned transfer, but permit independently observed capability
/// failure to beat an operator with no independent downstream evidence.
/// All domain names and target paths come from the live source frontier.
pub(crate) fn prefer_capability_target(algebra:&Value,learning:&Value)->bool {
    let gap=learning["capabilityGaps"].as_array()
        .into_iter().flatten()
        .find(|row|row["priority"].as_i64().unwrap_or(0)>0
            && row["independentHoldoutRegressions"].as_u64().unwrap_or(0)>0);
    let Some(gap)=gap else {return false};
    if !learning["suggestedCapabilityTarget"].is_object(){return false;}
    let selected=&algebra["selected"];
    if !selected.is_object(){return true;}
    // When the best learned rule targets the same deficient capability,
    // let the executable improvement rule operate rather than bypass it.
    if selected["target"]["role"]==gap["role"]{return false;}
    let evidence=&selected["generalizationEvidence"];
    if (evidence["verifiedMethodControlTrials"].as_u64().unwrap_or(0)>0
        && selected["intent"]=="evolve_measured_method_after_controlled_downstream_gain")
        || selected["intent"]=="run_revised_method_in_controlled_descendant" {
        return false;
    }
    let independent=evidence["independentReplayedTrials"]
        .as_u64().unwrap_or(0);
    let cross=evidence["crossCapabilityGains"].as_u64().unwrap_or(0);
    let regressions=evidence["holdoutRegressions"].as_u64().unwrap_or(0);
    let transfer=evidence["transferScore"].as_f64().unwrap_or(0.0);
    // A cross-domain failure with independent evidence should not be
    // permanently displaced by a zero-shot stage compiler.
    (independent==0 && gap["independentHoldoutRegressions"]
        .as_u64().unwrap_or(0)>=1)
        || (cross==0 && gap["independentHoldoutRegressions"]
            .as_u64().unwrap_or(0)>=2)
        || (regressions>cross && transfer<=0.0)
}
pub(crate) fn instruction(learning:&Value)->String {
    let productive=learning["operatorLearning"].as_array()
        .into_iter().flatten()
        .filter(|row|row["hasMeasuredTransfer"]==true)
        .take(4).map(|row|format!("method={} cross-domain-gains={} regressions={} profiles={}",
            row["ruleId"],row["crossCapabilityGains"],row["crossCapabilityRegressions"],
            row["frozenProfileDigests"])).collect::<Vec<_>>().join("; ");
    let gaps=learning["capabilityGaps"].as_array()
        .into_iter().flatten()
        .filter(|row|row["independentHoldoutRegressions"].as_u64().unwrap_or(0)>0)
        .take(4).map(|row|format!("role={} frozenRegressions={} gains={} source={}",
            row["role"],row["independentHoldoutRegressions"],
            row["independentHoldoutGains"],row["target"]["path"]))
        .collect::<Vec<_>>().join("; ");
    format!("SOURCE-REPLAYED METHOD GENERALIZATION: {}. \
        Independently measured capability deficits: {}. \
        Successful source adoption alone does not mean the improvement method \
        became better. Improve actual MODEL decision code, MEMORY, LEARNING, \
        TOOLS and other agent roles on independent frozen tasks; use the current \
        source methods to propose the changes. Build new methods to overcome \
        observed limitations and evaluate transfer in the next compiled child. \
        Only independent paired receipt replay can teach this method learner.",
        if productive.is_empty(){"no measured cross-capability transfer yet".to_owned()}
            else{productive},
        if gaps.is_empty(){"no independently replayed capability gap".to_owned()}
            else{gaps})
}
