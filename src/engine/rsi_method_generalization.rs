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
    let id=rule["id"].as_str().filter(|s|hash_ok(s,64))?;
    let generation=g["generatorCommit"].as_str().filter(|s|hash_ok(s,40))?;
    let plan=&g["improvementAlgebraPlan"];
    if g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=generation
        || plan["source"]!="compiled_model_authored_improvement_algebra_v1"
        || plan["generatorCommit"]!=generation
        || plan["selected"]["ruleId"]!=id
        || plan["selected"]["rule"]["id"]!=id
        || g["controllerPlan"]["improvementAlgebra"]["sha256"]!=plan["sha256"]
        || !plan["sha256"].as_str().is_some_and(|s|hash_ok(s,64)) {
        return None;
    }
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
        let mut gains=0u64;let mut regressions=0u64;let mut neutral=0u64;
        let mut cross_gains=0u64;let mut cross_regressions=0u64;
        let mut adopted=0u64;
        let mut domains=BTreeSet::<String>::new();
        let mut profiles=BTreeSet::<String>::new();
        let mut commits=BTreeSet::<String>::new();
        let mut receipt_ids=BTreeSet::<String>::new();
        for g in history {
            let Some((candidate,metrics,profile))=independent_use(state,g,rule)
                else{continue};
            let Some(candidate_id)=g["candidateId"].as_str()else{continue};
            if !trials.insert(candidate_id.to_owned()) {continue}
            profiles.insert(profile.to_owned());
            if let Some(parent)=g["generatorCommit"].as_str() {
                commits.insert(parent.to_owned());
            }
            if let Some(evidence)=candidate["evaluation"]["evidenceSha256"].as_str() {
                receipt_ids.insert(evidence.to_owned());
            }
            adopted+=u64::from(actual_adoption(g,&candidate,&metrics));
            for m in metrics["measurements"].as_array().into_iter().flatten() {
                if m["holdout"]!=true {continue}
                let (Some(case),Some(domain),Some(effect))=(
                    m["id"].as_str(),m["domain"].as_str(),case_effect(m))
                else{continue};
                // A single case is counted once per independently evaluated
                // candidate+profile+domain, not once per rewritten JSON copy.
                if !cases.insert((candidate_id.to_string(),profile.to_owned(),
                    case.to_owned(),domain.to_owned())) {continue;}
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
        let measured=gains+regressions+neutral;
        let mean=(gains as f64-2.0*regressions as f64)/(measured as f64+8.0);
        let coverage=(domains.len() as f64).min(5.0)/5.0;
        let support=(trials.len() as f64)/(trials.len() as f64+4.0);
        // Shrink strongly with sparse evidence. A positive score is a
        // predictive SEARCH signal, not a declaration of causal improvement.
        let transferable=(mean*(0.7+0.3*coverage)*support).clamp(-1.0,1.0);
        rows.push(json!({
            "ruleId":id,"methodRole":edited_role,
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
            "transferScore":transferable,
            "hasMeasuredTransfer":cross_gains>0 && cross_regressions==0,
            "causalityProven":false,
            "source":"replayed_independent_paired_candidate_receipts"
        }));
    }
    rows.sort_by(|a,b|a["ruleId"].as_str().cmp(&b["ruleId"].as_str()));
    let mut result=json!({
        "source":"compiled_replayed_downstream_method_learning_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "operatorLearning":rows,
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
        let adjusted=(transfer*600.0).round() as i64
            + (if cross>0 && reg==0 {240}else{0});
        let base=row["selectionPriority"].as_i64().unwrap_or(0);
        row["selectionPriority"]=json!(base.saturating_add(adjusted));
        row["generalizationEvidence"]=evidence.clone();
        // When the method was actually used and consistently hurt external
        // heldouts, prefer editing that method rather than repeating the same
        // target. NO reward from rejected self-reported candidate output.
        if trials>=2 && reg>=2 && reg>gain {
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
pub(crate) fn instruction(learning:&Value)->String {
    let productive=learning["operatorLearning"].as_array()
        .into_iter().flatten()
        .filter(|row|row["hasMeasuredTransfer"]==true)
        .take(4).map(|row|format!("method={} cross-domain-gains={} regressions={} profiles={}",
            row["ruleId"],row["crossCapabilityGains"],row["crossCapabilityRegressions"],
            row["frozenProfileDigests"])).collect::<Vec<_>>().join("; ");
    format!("SOURCE-REPLAYED METHOD GENERALIZATION: {}. \
        Successful source adoption alone does not mean the improvement method \
        became better. Improve actual MODEL decision code, MEMORY, LEARNING, \
        TOOLS and other agent roles on independent frozen tasks; use the current \
        source methods to propose the changes. Build new methods to overcome \
        observed limitations and evaluate transfer in the next compiled child. \
        Only independent paired receipt replay can teach this method learner.",
        if productive.is_empty(){"no measured cross-capability transfer yet".to_owned()}
            else{productive})
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
