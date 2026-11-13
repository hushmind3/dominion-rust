//! Independent downstream-learning loop for open source-authored RSI methods.
//!
//! A source edit becoming accepted is not yet evidence that its *improvement
//! method* learned to produce better descendants. The archived independent
//! paired receipt, frozen profile and actual edited target are replayed per
//! execution, then aggregated across capability roles and generations.
//! The method-ranking learner lives in ordinary descendant-editable Rust.
//! The frozen oracle and adoption boundary remain external to this controller.
use crate::{engine::rsi, state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn hash_ok(s:&str,size:usize)->bool {
    s.len()==size && s.bytes().all(|byte|byte.is_ascii_hexdigit())
}
fn observed_domain(name:&str)->&str {
    match name {
        "model"|"models"|"model_architecture"=>"models",
        "recall"|"memory"=>"memory",
        "training"|"learning"=>"learning",
        "tool-use"|"tools"=>"tools",
        "context"|"planning"|"harness"=>"harness",
        "improver"|"algorithm"|"search"|"scientific_method"
            |"improvement_methods"=>"improvement_methods",
        "ui"|"frontend"=>"frontend",
        other=>other
    }
}
fn independent_use<'a>(state:&AppState,g:&'a Value,rule:&Value)
    ->Option<(Value,Value,&'a str)> {
    let family=crate::engine::rsi_improvement_algebra::method_family_id(rule)?;
    let generation=g["generatorCommit"].as_str().filter(|s|hash_ok(s,40))?;
    let plan=&g["improvementAlgebraPlan"];
    if g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=generation
        || plan["source"]!="compiled_model_authored_improvement_algebra_v1"
        || plan["generatorCommit"]!=generation
        // A rewrite earns a NEW evidence ID but may preserve the same
        // source-bound semantic method family. Otherwise the learner can
        // never compare one version of an improvement method to another.
        || plan["selected"]["ruleId"]!=plan["selected"]["rule"]["id"]
        || !plan["selected"]["ruleId"].as_str().is_some_and(|s|hash_ok(s,64))
        || crate::engine::rsi_improvement_algebra::method_family_id(
            &plan["selected"]["rule"]).as_deref()!=Some(family.as_str())
        || g["controllerPlan"]["improvementAlgebra"]["sha256"]!=plan["sha256"]
        || !plan["sha256"].as_str().is_some_and(|s|hash_ok(s,64)) {
        return None;
    }
    // Paired gain is not attributable to a rule merely mentioned by a plan.
    if !crate::engine::rsi_operator_provenance::verified(state,g,rule){return None;}
    let changed=g["changedSourcePaths"].as_array()?;
    let target=plan["selected"]["target"]["path"].as_str()
        .filter(|p|rsi::safe_path(p))?;
    if !changed.iter().any(|p|p==target) {return None;}
    let candidate_id=g["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",candidate_id)?;
    let metrics=rsi::replayed_pair_metrics(state,&candidate)?;
    // The receipt replay is the authority. Neither the model's text nor a
    // self-reported evaluation JSON can earn training credit.
    if candidate["baseCommit"]!=generation
        || candidate["evaluation"]["metrics"]!=metrics
        || g["evaluation"]["metrics"]!=metrics
        || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || candidate["paths"].as_array().is_none_or(|xs|
            !xs.iter().any(|path|path==target)) {
        return None;
    }
    let profile=g["evaluationProfileSha256"].as_str()
        .filter(|s|hash_ok(s,64))?;
    Some((candidate,metrics,profile))
}
fn actual_adoption(g:&Value,candidate:&Value,metrics:&Value)->bool {
    g["accepted"]==true && g["pairedRevalidated"]==true
        && metrics["eligible"]==true && metrics["verdict"]=="improved"
        && candidate["status"]=="verified"
        && g["acceptedCommit"].as_str().is_some_and(|s|hash_ok(s,40))
        && candidate["experimentAdoption"]["commit"]==g["acceptedCommit"]
        && candidate["experimentAdoption"]["generatorCommit"]==g["generatorCommit"]
}
fn case_effect(m:&Value)->Option<i8>{
    let before=m["baselinePass"].as_bool()?;
    let after=m["candidatePass"].as_bool()?;
    let mut gain=!before && after;
    let mut loss=before && !after;
    for row in m["metrics"].as_array().into_iter().flatten() {
        gain|=row["improved"]==true;
        loss|=row["regressed"]==true;
    }
    if loss {Some(-1)} else if gain {Some(1)} else {Some(0)}
}
/// Cross-capability failures are active source development opportunities, not
/// a reason to improve only the RSI planner. In particular models, memory,
/// learning and tools use the SAME frozen feedback as any future invented role.
/// The role set is taken from the current source-grounded architecture.
fn capability_pressure(state:&AppState,history:&[Value],frontier:&[Value])
    ->Vec<Value> {
    let mut stats=BTreeMap::<String,(u64,u64,u64,
        BTreeSet<String>,BTreeSet<String>)>::new();
    let known=frontier.iter().filter_map(|row|row["role"].as_str())
        .collect::<BTreeSet<_>>();
    let mut counted=BTreeSet::new();
    for g in history {
        let Some(id)=g["candidateId"].as_str()else{continue};
        let Some(profile)=g["evaluationProfileSha256"].as_str()
            .filter(|s|hash_ok(s,64))else{continue};
        let Some(head)=g["generatorCommit"].as_str()
            .filter(|s|hash_ok(s,40))else{continue};
        let Some(candidate)=state.store.get("rsi_candidate",id)else{continue};
        if candidate["baseCommit"]!=head
            || candidate["evaluation"]["source"]!=
                "server_invoked_local_paired_git_worktrees"
            || candidate["paths"]!=g["changedSourcePaths"] {continue}
        let Some(metrics)=rsi::replayed_pair_metrics(state,&candidate)
            else{continue};
        if metrics!=g["evaluation"]["metrics"]{continue}
        for m in metrics["measurements"].as_array().into_iter().flatten(){
            if m["holdout"]!=true{continue}
            let (Some(domain),Some(case),Some(effect))=(
                m["domain"].as_str(),m["id"].as_str(),case_effect(m))
            else{continue};
            let role=observed_domain(domain);
            if !known.contains(role){continue}
            let key=(id.to_owned(),profile.to_owned(),
                case.to_owned(),role.to_owned());
            if !counted.insert(key){continue}
            let s=stats.entry(role.to_owned()).or_default();
            if effect>0{s.0+=1;}else if effect<0{s.1+=1;}else{s.2+=1;}
            s.3.insert(profile.to_owned());
            s.4.insert(id.to_owned());
        }
    }
    let mut rows=Vec::new();
    for (role,(gains,regressions,neutral,profiles,ids)) in stats{
        let Some(target)=frontier.iter().find(|source|
            source["role"]==role
            && source["sourceSha256"].as_str().is_some_and(|s|hash_ok(s,64)))
            else{continue};
        let pressure=(regressions as i64)*8-(gains as i64)*3;
        rows.push(json!({
            "role":role,"target":target,
            "independentHoldoutGains":gains,
            "independentHoldoutRegressions":regressions,
            "independentHoldoutNeutral":neutral,
            "replayedCandidateCount":ids.len(),
            "frozenProfiles":profiles.len(),
            "priority":pressure,
            "source":"frozen_evaluation_capability_gap_not_model_claim",
            "qualityProven":false
        }));
    }
    rows.sort_by(|a,b|b["priority"].as_i64().cmp(&a["priority"].as_i64())
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str())));
    rows
}
/// The learner aggregates only independently replayed evaluations actually
/// produced by a running compiled descendant selecting a specific operator.
/// Observations may be useful for search WITHOUT being identified causal
/// effects; held-out gains in a different capability matter more than edits.
pub(crate) fn learn(state:&AppState,history:&[Value],frontier:&[Value],
    algebra:&Value,commit:&str)->Value {
    let mut rows=Vec::new();
    let known_roles=frontier.iter().filter_map(|f|f["role"].as_str())
        .collect::<BTreeSet<_>>();
    for rule in algebra["operators"].as_array().into_iter().flatten() {
        let id=rule["id"].as_str().unwrap_or("");
        let edited_role=rule["methodRole"].as_str().unwrap_or("");
        let mut trials=BTreeSet::new();
        let mut cases=BTreeSet::new();
        let mut verified_method_controls=0u64;
        let mut method_control_quality_wins=0u64;
        let mut method_control_quality_losses=0u64;
        let mut control_receipt_ids=BTreeSet::<String>::new();
        let mut gains=0u64;let mut regressions=0u64;let mut neutral=0u64;
        let mut cross_gains=0u64;let mut cross_regressions=0u64;
        let mut adopted=0u64;
        let mut domains=BTreeSet::<String>::new();
        let mut profiles=BTreeSet::<String>::new();
        let mut commits=BTreeSet::<String>::new();
        let mut receipt_ids=BTreeSet::<String>::new();
        // Compare successive METHOD SOURCE REVISIONS on exactly the same
        // frozen profile, holdout case, domain and baseline result. A new
        // method source SHA alone is not evidence of improved methodology.
        let mut method_versions=Vec::<String>::new();
        let mut matched_cases=BTreeMap::<(String,String,String,bool),
            BTreeMap<String,Vec<i8>>>::new();
        for g in history {
            let Some((candidate,metrics,profile))=independent_use(state,g,rule)
                else{continue};
            let Some(candidate_id)=g["candidateId"].as_str()else{continue};
            if !trials.insert(candidate_id.to_owned()) {continue}
            let method_sha=g["improvementAlgebraPlan"]["selected"]["rule"]["method"]
                ["sourceSha256"].as_str().filter(|sha|hash_ok(sha,64));
            if let Some(sha)=method_sha {
                if !method_versions.iter().any(|old|old==sha) {
                    method_versions.push(sha.to_owned());
                }
            }
            profiles.insert(profile.to_owned());
            if let Some(parent)=g["generatorCommit"].as_str() {
                commits.insert(parent.to_owned());
            }
            if let Some(evidence)=candidate["evaluation"]["evidenceSha256"].as_str() {
                receipt_ids.insert(evidence.to_owned());
            }
            adopted+=u64::from(actual_adoption(g,&candidate,&metrics));
            // The original OFF worker and ON worker must have matched,
            // independent candidate receipts. The descendant learner
            // consumes this intervention, not a stored success label.
            let intervention=crate::engine::rsi_evolve::measured_method_ablation(state,g);
            if actual_adoption(g,&candidate,&metrics)
                && intervention["qualityProven"]==true
                && intervention["methodAssociationObserved"]==true
                && g["methodAblation"]==intervention
                && intervention["methodFamilyId"]==
                    crate::engine::rsi_improvement_algebra::method_family_id(rule) {
                verified_method_controls+=1;
                method_control_quality_wins+=
                    intervention["treatmentOnlyWins"].as_u64().unwrap_or(0)
                    +intervention["treatmentQualityWins"].as_u64().unwrap_or(0);
                method_control_quality_losses+=
                    intervention["controlOnlyWins"].as_u64().unwrap_or(0)
                    +intervention["controlQualityWins"].as_u64().unwrap_or(0);
                if let Some(id)=intervention["controlEvidenceSha256"].as_str(){
                    control_receipt_ids.insert(id.to_owned());
                }
            }
            for m in metrics["measurements"].as_array().into_iter().flatten() {
                if m["holdout"]!=true {continue}
                let (Some(case),Some(domain),Some(effect))=(
                    m["id"].as_str(),m["domain"].as_str(),case_effect(m))
                else{continue};
                // A single case is counted once per independently evaluated
                // candidate+profile+domain, not once per rewritten JSON copy.
                if !cases.insert((candidate_id.to_string(),profile.to_owned(),
                    case.to_owned(),domain.to_owned())) {continue;}
                if let (Some(sha),Some(before))=(method_sha,m["baselinePass"].as_bool()) {
                    matched_cases.entry((profile.to_owned(),case.to_owned(),
                        domain.to_owned(),before)).or_default()
                        .entry(sha.to_owned()).or_default().push(effect);
                }
                let capability=observed_domain(domain);
                if effect>0 {gains+=1;domains.insert(capability.to_owned());}
                else if effect<0 {regressions+=1;}
                else {neutral+=1;}
                if capability!=edited_role && known_roles.contains(capability) {
                    if effect>0 {cross_gains+=1;}
                    if effect<0 {cross_regressions+=1;}
                }
            }
        }
        let current_method=rule["method"]["sourceSha256"].as_str().unwrap_or("");
        let predecessor=method_versions.iter().rfind(|sha|
            sha.as_str()!=current_method).map(String::as_str);
        let mut matched_revision_cases=0u64;
        let mut revision_lift=0.0f64;
        if let Some(previous)=predecessor {
            for versions in matched_cases.values(){
                if let (Some(new),Some(old))=(
                    versions.get(current_method),versions.get(previous)) {
                    if !new.is_empty() && !old.is_empty(){
                        let new_mean=new.iter().map(|x|*x as f64).sum::<f64>()
                            /new.len() as f64;
                        let old_mean=old.iter().map(|x|*x as f64).sum::<f64>()
                            /old.len() as f64;
                        revision_lift+=new_mean-old_mean;
                        matched_revision_cases+=1;
                    }
                }
            }
            if matched_revision_cases>0 {
                revision_lift=(revision_lift/(matched_revision_cases as f64))
                    .clamp(-2.0,2.0);
            }
        }
        let measured=gains+regressions+neutral;
        let mean=(gains as f64-2.0*regressions as f64)/(measured as f64+8.0);
        let coverage=(domains.len() as f64).min(5.0)/5.0;
        let support=(trials.len() as f64)/(trials.len() as f64+4.0);
        // Shrink strongly with sparse evidence. A positive score is a
        // predictive SEARCH signal, not a declaration of causal improvement.
        let transferable=(mean*(0.7+0.3*coverage)*support).clamp(-1.0,1.0);
        rows.push(json!({
            "ruleId":id,"methodRole":edited_role,
            "methodFamilyId":crate::engine::rsi_improvement_algebra::
                method_family_id(rule),
            "independentReplayedTrials":trials.len(),
            "adoptedDescendantTrials":adopted,
            "frozenHoldoutCases":measured,
            "holdoutGains":gains,"holdoutRegressions":regressions,
            "holdoutNeutral":neutral,
            "crossCapabilityGains":cross_gains,
            "crossCapabilityRegressions":cross_regressions,
            "improvedCapabilityRoles":domains,
            "frozenProfileDigests":profiles.into_iter().take(12).collect::<Vec<_>>(),
            "generatorCommits":commits.into_iter().take(12).collect::<Vec<_>>(),
            "independentReceiptSha256":receipt_ids.into_iter().take(12).collect::<Vec<_>>(),
            "verifiedMethodControlTrials":verified_method_controls,
            "methodControlQualityWins":method_control_quality_wins,
            "methodControlQualityLosses":method_control_quality_losses,
            "methodControlEvidenceSha256":control_receipt_ids.into_iter().take(12).collect::<Vec<_>>(),
            "transferScore":transferable,
            "currentMethodSourceSha256":current_method,
            "previousMethodSourceSha256":predecessor,
            "matchedFrozenRevisionCases":matched_revision_cases,
            "observedRevisionTransferLift":revision_lift,
            "revisionDifferenceIsCausalProof":false,
            "hasMeasuredTransfer":cross_gains>0 && cross_regressions==0,
            "causalityProven":false,
            "source":"replayed_independent_paired_candidate_receipts"
        }));
    }
    rows.sort_by(|a,b|a["ruleId"].as_str().cmp(&b["ruleId"].as_str()));
    let capability_gaps=capability_pressure(state,history,frontier);
    // An uncovered model/memory/tool failure can drive the NEXT source edit
    // even if nobody has yet authored a successful meta improvement rule.
    let capability_target=capability_gaps.iter()
        .find(|gap|gap["independentHoldoutRegressions"]
            .as_u64().unwrap_or(0)>0 && gap["priority"].as_i64().unwrap_or(0)>0)
        .map(|gap|gap["target"].clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_replayed_downstream_method_learning_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "operatorLearning":rows,
        "capabilityGaps":capability_gaps,
        "suggestedCapabilityTarget":capability_target,
        "evidenceRequirement":"replayed_frozen_paired_receipt_plus_actual_compiled_source_target",
        "methodSuccessNotInferredFromSourceAdoption":true,
        "causalityProven":false,"qualityProven":false
    });
    result["sha256"]=json!(digest(&result));
    result
}
pub(crate) fn valid(feedback:&Value,commit:&str,history_len:usize)->bool {
    let mut unsigned=feedback.clone();
    let Some(map)=unsigned.as_object_mut()else{return false};
    map.remove("sha256");
    feedback["source"]=="compiled_replayed_downstream_method_learning_v1"
        && feedback["generatorCommit"]==commit
        && feedback["historyCount"].as_u64()==Some(history_len as u64)
        && feedback["sha256"].as_str()==Some(digest(&unsigned).as_str())
        && feedback["methodSuccessNotInferredFromSourceAdoption"]==true
        && feedback["causalityProven"]==false
        && feedback["qualityProven"]==false
        && feedback["operatorLearning"].as_array().is_some_and(|rows|
            rows.len()<=128 && rows.iter().all(|row|
                row["ruleId"].as_str().is_some_and(|id|hash_ok(id,64))
                && row["source"]=="replayed_independent_paired_candidate_receipts"
                && row["causalityProven"]==false))
}
/// The actual next descendant controller uses source-replay evidence to
/// re-rank executable operators. The algorithm itself can be rewritten by a
/// successfully adopted future descendant.
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
        let control_weight=if control_trials>0 && control_wins>0 && control_losses==0 {
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
        if control_trials>0 && control_wins>0 && control_losses==0 {
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
    if evidence["verifiedMethodControlTrials"].as_u64().unwrap_or(0)>0
        && selected["intent"]=="evolve_measured_method_after_controlled_downstream_gain" {
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn self_reported_gains_do_not_change_empty_evidence(){
        let method=json!({"source":"compiled_model_authored_improvement_algebra_v1",
            "rankedCandidates":[{"ruleId":"a".repeat(64),"selectionPriority":100,
                "target":{"path":"src/engine/models.rs"},
                "rule":{"method":{"path":"src/engine/tools.rs"}}}]});
        let feedback=json!({"operatorLearning":[{"ruleId":"a".repeat(64),
            "independentReplayedTrials":0,"holdoutRegressions":0,"holdoutGains":0,
            "crossCapabilityGains":0,"transferScore":0.0}]});
        let adapted=adapt(method,&feedback);
        assert_eq!(adapted["selected"]["selectionPriority"],100);
        assert_eq!(adapted["selected"]["target"]["path"],"src/engine/models.rs");
    }
    #[test]
    fn forged_holdout_feedback_without_independent_pair_receipt_is_ignored(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(
            format!("rsi-method-learning-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let rule="a".repeat(64);
        let base="b".repeat(40);
        let path="src/engine/models.rs";
        let fake=json!({
            "id":"candidate-fake",
            "baseCommit":base,
            "paths":[path],
            "status":"verified",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":true,"verdict":"improved",
                    "measurements":[{"id":"model-generalization",
                        "domain":"models","holdout":true,
                        "baselinePass":false,"candidatePass":true}]}}
        });
        state.store.put("rsi_candidate",&fake).unwrap();
        let plan=json!({"source":"compiled_model_authored_improvement_algebra_v1",
            "generatorCommit":base,"sha256":"c".repeat(64),
            "selected":{"ruleId":rule,"rule":{"id":rule},
                "target":{"path":path}}});
        let generation=json!({
            "generatorCommit":base,"candidateId":"candidate-fake",
            "changedSourcePaths":[path],
            "evaluationProfileSha256":"d".repeat(64),
            "accepted":true,"pairedRevalidated":true,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":base},
            "improvementAlgebraPlan":plan,
            "controllerPlan":{"improvementAlgebra":plan},
            "evaluation":fake["evaluation"]
        });
        let algebra=json!({"operators":[{"id":rule,
            "methodRole":"scientific_method"}]});
        let learned=learn(&state,&[generation],&[],&algebra,&base);
        assert_eq!(learned["operatorLearning"][0]["independentReplayedTrials"],0);
        assert_eq!(learned["operatorLearning"][0]["crossCapabilityGains"],0);
        assert!(valid(&learned,&base,1));
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn replayed_feedback_changes_selection_without_faking_success(){
        let strong="a".repeat(64);
        let weak="b".repeat(64);
        let algebra=json!({"source":"compiled_model_authored_improvement_algebra_v1",
            "rankedCandidates":[
                {"ruleId":weak,"selectionPriority":300,
                    "target":{"path":"src/engine/memory.rs"},
                    "rule":{"method":{"path":"src/engine/rsi_improvement_algebra.rs"}}},
                {"ruleId":strong,"selectionPriority":100,
                    "target":{"path":"src/engine/models.rs"},
                    "rule":{"method":{"path":"src/engine/rsi_method_generalization.rs"}}}
            ]});
        let feedback=json!({"sha256":"e".repeat(64),"operatorLearning":[
            {"ruleId":weak,"independentReplayedTrials":3,
                "holdoutRegressions":0,"holdoutGains":0,
                "crossCapabilityGains":0,"transferScore":0.0},
            {"ruleId":strong,"independentReplayedTrials":3,
                "holdoutRegressions":0,"holdoutGains":4,
                "crossCapabilityGains":3,"transferScore":0.75}
        ]});
        let evolved=adapt(algebra,&feedback);
        assert_eq!(evolved["selected"]["ruleId"],strong);
        assert_eq!(evolved["selected"]["target"]["path"],"src/engine/models.rs");
        assert_eq!(evolved["selected"]["qualityProven"],Value::Null);
    }
    #[test]
    fn source_revision_family_matches_without_reusing_a_rule_evidence_id(){
        let a=json!({"id":"a".repeat(64),"name":"tool_repair",
            "premises":["memory"],"products":["tools"],
            "methodRole":"scientific_method",
            "method":{"path":"src/engine/rsi_improvement_algebra.rs",
                "sourceSha256":"a".repeat(64)}});
        let mut b=a.clone();
        b["id"]=json!("b".repeat(64));
        b["method"]["sourceSha256"]=json!("b".repeat(64));
        assert_ne!(a["id"],b["id"]);
        assert_eq!(
            crate::engine::rsi_improvement_algebra::method_family_id(&a),
            crate::engine::rsi_improvement_algebra::method_family_id(&b));
        let mut altered=b.clone();
        altered["products"]=json!(["learning"]);
        assert_ne!(
            crate::engine::rsi_improvement_algebra::method_family_id(&a),
            crate::engine::rsi_improvement_algebra::method_family_id(&altered));
    }
    #[test]
    fn archived_pair_without_executed_operator_does_not_train_a_method(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(
            format!("rsi-method-family-replay-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        // Synthetic paired-runner fixture. This exercises replay logic,
        // NOT real API calls or a live independent model benchmark.
        let commit="a".repeat(40);
        let path="src/engine/models.rs";
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),
            "domain":if i%2==0{"models"}else{"tools"},
            "holdout":i<4,"metrics":[]})).collect::<Vec<_>>();
        let observed=(0..8).map(|i|json!({"id":format!("case-{i}"),
            "baselinePass":i!=0,"candidatePass":true,
            "metrics":{}})).collect::<Vec<_>>();
        let metrics=rsi::evaluate_cases(&cases,&observed,"harness").unwrap();
        assert_eq!(metrics["eligible"],true);
        let evidence="e".repeat(64);
        state.store.put("rsi_candidate",&json!({
            "id":"paired-revision-candidate","status":"verified",
            "baseCommit":commit,"patchSha256":"c".repeat(64),
            "oracleVersion":"rsi-worktree-v2","cases":cases,
            "paths":[path],"target":"harness",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":metrics,"verdict":metrics["verdict"],
                "evidenceSha256":evidence}
        })).unwrap();
        state.store.put("rsi_pair",&json!({
            "id":"rsi-pair-paired-revision-candidate",
            "candidateId":"paired-revision-candidate",
            "runner":"local_gitrepo_not_os_sandbox",
            "evidenceSha256":evidence,
            "metrics":metrics,"verdict":metrics["verdict"],
            "report":{"baseCommit":commit,"patchSha256":"c".repeat(64),
                "oracleVersion":"rsi-worktree-v2",
                "reportSha256":evidence,
                "baselineBuildOk":true,"candidateBuildOk":true,
                "cases":observed}
        })).unwrap();
        let old_rule=json!({"id":"1".repeat(64),"name":"improve_agent",
            "premises":["memory"],"products":["tools"],
            "methodRole":"scientific_method","method":{
                "path":"src/engine/rsi_improvement_algebra.rs",
                "sourceSha256":"3".repeat(64)}});
        let mut current_rule=old_rule.clone();
        current_rule["id"]=json!("2".repeat(64));
        current_rule["method"]["sourceSha256"]=json!("4".repeat(64));
        let plan=json!({"source":"compiled_model_authored_improvement_algebra_v1",
            "generatorCommit":commit,"sha256":"5".repeat(64),
            "selected":{"ruleId":old_rule["id"],"rule":old_rule,
                "target":{"path":path}}});
        let generation=json!({"generatorCommit":commit,
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":commit},
            "improvementAlgebraPlan":plan,
            "controllerPlan":{"improvementAlgebra":plan},
            "changedSourcePaths":[path],"candidateId":"paired-revision-candidate",
            "evaluation":{"metrics":metrics},
            "evaluationProfileSha256":"6".repeat(64)});
        assert_ne!(plan["selected"]["ruleId"],current_rule["id"]);
        assert!(independent_use(&state,&generation,&current_rule).is_none(),
            "a real paired receipt cannot attribute gains to an unexecuted method");
        let mut unrelated=current_rule.clone();
        unrelated["name"]=json!("other_principle");
        assert!(independent_use(&state,&generation,&unrelated).is_none(),
            "unrelated improvement principles cannot inherit a sibling's evidence");
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn independently_measured_model_failure_beats_unmeasured_meta_rule(){
        let algebra=json!({"selected":{"target":{"role":"scientific_method"},
            "generalizationEvidence":{"independentReplayedTrials":0}}});
        let learning=json!({"suggestedCapabilityTarget":{
                "role":"models","path":"src/engine/models.rs"},
            "capabilityGaps":[{"role":"models","priority":16,
                "independentHoldoutRegressions":2,
                "independentHoldoutGains":0}]});
        assert!(prefer_capability_target(&algebra,&learning));
        let strong=json!({"selected":{"target":{"role":"tools"},
            "generalizationEvidence":{"independentReplayedTrials":7,
                "crossCapabilityGains":4,"holdoutRegressions":0,
                "transferScore":0.8}}});
        assert!(!prefer_capability_target(&strong,&learning));
    }
    #[test]
    fn measured_regression_redirects_a_future_method_edit(){
        let method=json!({"rankedCandidates":[{"ruleId":"a".repeat(64),
            "selectionPriority":100,"intent":"improve_composed_intelligence",
            "target":{"path":"src/engine/models.rs"},
            "rule":{"method":{"path":"src/engine/rsi_improvement_algebra.rs"}}}]});
        let feedback=json!({"sha256":"b".repeat(64),"operatorLearning":[{
            "ruleId":"a".repeat(64),"independentReplayedTrials":3,
            "holdoutRegressions":3,"holdoutGains":0,
            "crossCapabilityGains":0,"transferScore":-0.4}]});
        let adapted=adapt(method,&feedback);
        assert_eq!(adapted["selected"]["intent"],
            "rewrite_method_after_replayed_transfer_regression");
        assert_eq!(adapted["selected"]["target"]["path"],
            "src/engine/rsi_improvement_algebra.rs");
    }
}
