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
#[path = "rsi_method_generalization/learning.rs"]
mod learning;
pub(crate) use learning::learn;

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
#[path = "rsi_method_generalization/ranking.rs"]
mod ranking;
pub(crate) use ranking::{adapt,prefer_capability_target,instruction};

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
    fn measured_control_routes_a2_method_rewrite_then_b2_use(){
        let method=json!({"rankedCandidates":[{"ruleId":"a".repeat(64),
            "selectionPriority":100,"intent":"improve_composed_intelligence",
            "target":{"path":"src/engine/memory.rs"},
            "productTarget":{"path":"src/engine/memory.rs"},
            "rule":{"method":{"path":"src/engine/rsi_meta_controller.rs",
                "sourceSha256":"b".repeat(64)}}}]});
        let measured=json!({"sha256":"c".repeat(64),"operatorLearning":[{
            "ruleId":"a".repeat(64),"independentReplayedTrials":1,
            "verifiedMethodControlTrials":1,"methodControlQualityWins":2,
            "methodControlQualityLosses":0,"transferScore":0.3,
            "methodControlEvidenceSha256":["d".repeat(64)]}]});
        let after_b1=adapt(method.clone(),&measured);
        assert_eq!(after_b1["selected"]["intent"],
            "evolve_measured_method_after_controlled_downstream_gain");
        assert_eq!(after_b1["selected"]["target"]["path"],
            "src/engine/rsi_meta_controller.rs");
        let mut revised=measured;
        revised["operatorLearning"][0]["revisedMethodAwaitingDownstreamControl"]=json!(true);
        let after_a2=adapt(method.clone(),&revised);
        assert_eq!(after_a2["selected"]["intent"],
            "run_revised_method_in_controlled_descendant");
        assert_eq!(after_a2["selected"]["target"]["path"],
            "src/engine/memory.rs");
        let no_controls=adapt(method,&json!({"sha256":"e".repeat(64),
            "operatorLearning":[{"ruleId":"a".repeat(64),
                "independentReplayedTrials":1,"transferScore":0.3}]}));
        assert_ne!(no_controls["selected"]["target"]["path"],
            "src/engine/rsi_meta_controller.rs",
            "ordinary paired gain must not trigger controlled A2 rewrite");
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
