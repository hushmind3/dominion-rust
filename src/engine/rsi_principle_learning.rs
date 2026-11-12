//! Source-backed and outcome-conditioned synthesis of improvement principles.
//!
//! An adopted redesign is a hypothesis. A stronger improver is one that
//! subsequently chooses edits that generalize across independently measured
//! task domains. The recompiled child learns this prequentially from accepted
//! AND rejected measured attempts. Original frozen holdout cases never enter
//! editable policy code or become training examples.
//!
//! Unlike a fixed two-step scheduler, the source-written principle vocabulary,
//! dependencies, targets, and executable implementation can all be redesigned
//! by later descendants. This module cannot itself authorize adoption.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(value:&Value)->String{
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn sha(s:&str,n:usize)->bool{
    s.len()==n && s.bytes().all(|b|b.is_ascii_hexdigit())
}
fn adopted(g:&Value)->bool{
    g["accepted"]==true && g["pairedRevalidated"]==true
        && g["candidateOrigin"]=="model_driven_detached_worktree"
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["systemDesign"]["source"]=="model_authored_system_design"
        && g["systemDesign"]["generatorCommit"]==g["generatorCommit"]
        && g["systemDesign"]["generationJobId"]==g["jobId"]
        && g["acceptedCommit"].as_str().is_some_and(|s|sha(s,40))
        && g["generatorCommit"].as_str().is_some_and(|s|sha(s,40))
}
fn actual_edited(g:&Value,path:&str)->bool{
    crate::engine::rsi::safe_path(path)
        && g["changedSourcePaths"].as_array().is_some_and(|rows|
            rows.iter().any(|v|v.as_str()==Some(path)))
        && g["systemDesign"]["targetFiles"].as_array().is_some_and(|rows|
            rows.iter().any(|v|v.as_str()==Some(path)))
        && g["acceptedSourceHashes"][path].as_str().is_some_and(|s|sha(s,64))
}
fn authorship(g:&Value,spec:&Value)->bool {
    adopted(g)
        && spec["source"]=="model_authored_executable_principle_v1"
        && spec["id"].as_str().is_some_and(|id|
            (4..=64).contains(&id.len()) && id.bytes().all(|c|
                c.is_ascii_alphanumeric()||c==b'_'||c==b'-'))
        && spec["mechanism"].as_str().is_some_and(|s|s.len()>=25)
        && spec["falsifier"].as_str().is_some_and(|s|s.len()>=20)
        && spec["targetRoles"].as_array().is_some_and(|rows|!rows.is_empty())
        && spec["methodPaths"].as_array().is_some_and(|paths|
            !paths.is_empty() && paths.iter().all(|p|p.as_str()
                .is_some_and(|s|actual_edited(g,s))))
        && spec["methodPaths"].as_array().is_some_and(|paths|
            paths.iter().any(|p|p.as_str().is_some_and(|s|
                s.starts_with("src/engine/") && s.ends_with(".rs"))))
}
fn reward(g:&Value)->Option<(f64,bool,String)> {
    if g["pairedMeasured"]!=true || g["candidateId"].is_null()
        || g["evaluation"]["metrics"]["measurements"].as_array().is_none() {
        return None;
    }
    let metrics=&g["evaluation"]["metrics"];
    let gains=metrics["improvedHoldouts"].as_u64().unwrap_or(0).min(12) as f64;
    let meta=metrics["metaHoldoutGains"].as_u64().unwrap_or(0).min(12) as f64;
    let regress=metrics["regressions"].as_u64().unwrap_or(0).min(12) as f64;
    let successful=adopted(g);
    let value=(gains+0.5*meta-2.0*regress).clamp(-24.0,18.0);
    let domain=g["domain"].as_str().unwrap_or("unknown").to_string();
    Some((value,successful,domain))
}

/// Weight learning is not the same as source adoption. It is a second,
/// independent learning signal emitted by a separate parent-side oracle;
/// no private example, expected answer or training record is ever copied here.
fn paired_model_reward(g:&Value)->Option<(f64,bool)>{
    let observation=&g["modelAdapterIndependentEvaluation"];
    if observation["source"]!="independent_parent_model_adapter_pair_v1"
        || !matches!(observation["status"].as_str(),Some("promoted"|"rejected"))
        || observation["generationJobId"]!=g["jobId"]
        || observation["generatorCommit"]!=g["generatorCommit"] {
        return None;
    }
    let metrics=&observation["measurements"];
    if metrics["source"]!="separate_parent_paired_lora_oracle_v1"
        || metrics["measuredExamples"].as_u64().unwrap_or(0)<8
        || metrics["distinctDomains"].as_u64().unwrap_or(0)<2 {
        return None;
    }
    let before=metrics["baselineMeanLoss"].as_f64()
        .filter(|s|s.is_finite()&&*s>=0.0)?;
    let after=metrics["candidateMeanLoss"].as_f64()
        .filter(|s|s.is_finite()&&*s>=0.0)?;
    let successful=observation["status"]=="promoted"
        && observation["pairedModelImprovementMeasured"]==true;
    Some(((before-after).clamp(-3.0,3.0),successful))
}
/// Follow the real adopted commit ancestry from the birth of the principle.
fn outcomes(history:&[Value],birth_position:usize,commit:&str,id:&str,birth_domain:&str)->Value{
    let mut cursor=commit.to_string();
    let mut attempted=0u64;
    let mut accepted_trials=0u64;
    let mut transfer_successes=0u64;
    let mut regressions=0u64;
    let mut model_trials=0u64;
    let mut model_improvements=0u64;
    let mut model_loss_delta_sum=0.0f64;
    let mut total_reward=0.0f64;
    let mut distinct=BTreeSet::<String>::new();
    let mut records=Vec::new();
    for g in history.iter().skip(birth_position+1){
        if g["generatorCommit"]!=cursor{continue;}
        let run=&g["metaPolicy"]["principleLearning"];
        if g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
            && run["active"]==true && run["selectedPrincipleId"]==id {
            if let Some((weight_delta,improved))=paired_model_reward(g){
                model_trials+=1;
                if improved{model_improvements+=1;}
                model_loss_delta_sum+=weight_delta;
                if records.len()<12{records.push(json!({
                    "generationJobId":g["jobId"],
                    "domain":"model_architecture",
                    "pairedWeightLossDelta":weight_delta,
                    "independentlyPromoted":improved,
                    "sourceAdoptionClaimed":false
                }));}
            }
            if let Some((value,accepted,domain))=reward(g){
                attempted+=1;
                if accepted{accepted_trials+=1;}
                if g["evaluation"]["metrics"]["regressions"].as_u64().unwrap_or(0)>0{
                    regressions+=1;
                }
                if accepted && domain!=birth_domain {
                    distinct.insert(domain.clone());
                    transfer_successes+=1;
                }
                total_reward+=value;
                if records.len()<12{records.push(json!({
                    "generationJobId":g["jobId"],
                    "candidateId":g["candidateId"],
                    "generatorCommit":g["generatorCommit"],
                    "domain":domain,"accepted":accepted,
                    "independentHoldoutReward":value
                }));}
            }
        }
        if adopted(g){
            if let Some(next)=g["acceptedCommit"].as_str(){cursor=next.to_string();}
        }
    }
    json!({
        "measuredTrials":attempted,
        "measuredAdoptions":accepted_trials,
        "measuredRegressions":regressions,
        "measuredModelAdaptationTrials":model_trials,
        "independentModelImprovements":model_improvements,
        "sumPairedWeightLossReduction":model_loss_delta_sum,
        "observedTransferAdoptions":transfer_successes,
        "distinctTransferDomains":distinct,
        "sumIndependentHoldoutReward":total_reward,
        "meanIndependentHoldoutReward":if attempted==0{Value::Null}
            else{json!(total_reward/attempted as f64)},
        "outcomes":records,
        "causalSuperiorityProven":false
    })
}
/// A principle survives as a source-attributed hypothesis, not as a proof.
/// Multiple source-backed principles are accumulated, not only the latest.
pub(crate) fn archive(history:&[Value])->Vec<Value>{
    let mut found=BTreeMap::<String,Value>::new();
    for (i,g) in history.iter().enumerate(){
        if !adopted(g){continue;}
        for spec in g["systemDesign"]["improvementPrinciples"].as_array().into_iter().flatten(){
            if !authorship(g,spec){continue;}
            let Some(id)=spec["id"].as_str() else{continue};
            // A composite mechanism must cite genuinely adopted precursor
            // methods in this compiled lineage, not arbitrary invented IDs.
            if let Some(parents)=spec["composedFrom"].as_array(){
                let mut distinct=BTreeSet::<String>::new();
                if parents.iter().any(|parent|parent.as_str().is_none_or(|p|
                    !distinct.insert(p.to_string())||!found.contains_key(p))){
                    continue;
                }
            }
            // Same ID in a newly adopted program intentionally supersedes
            // earlier text; concrete evidence is pinned to its new commit.
            let birth=g["acceptedCommit"].as_str().unwrap_or("");
            let birth_domain=g["domain"].as_str().unwrap_or("unknown");
            let evaluation=outcomes(history,i,birth,id,birth_domain);
            found.insert(id.to_string(),json!({
                "source":"independently_adopted_executable_principle_v1",
                "id":id,"spec":spec,
                "birthCommit":birth,"birthGenerationJobId":g["jobId"],
                "birthDomain":birth_domain,
                "validatedMethodPaths":spec["methodPaths"],
                "descendantEvidence":evaluation,
                "verifiedAttribution":true,
                "causalityProven":false
            }));
        }
    }
    found.into_values().take(32).collect()
}
fn ranked_rule(entry:&Value)->f64{
    let e=&entry["descendantEvidence"];
    let trials=e["measuredTrials"].as_u64().unwrap_or(0) as f64;
    let wins=e["measuredAdoptions"].as_u64().unwrap_or(0) as f64;
    let transfers=e["observedTransferAdoptions"].as_u64().unwrap_or(0) as f64;
    let avg=e["meanIndependentHoldoutReward"].as_f64().unwrap_or(0.0);
    let model_trials=e["measuredModelAdaptationTrials"].as_u64().unwrap_or(0) as f64;
    let weight_wins=e["independentModelImprovements"].as_u64().unwrap_or(0) as f64;
    // Weight gains inform method search, but NEVER turn an unmeasured
    // source-code change into a successful source adoption.
    let measured_weight_bonus=if model_trials==0.0{0.0}
        else{0.12*(weight_wins+1.0)/(model_trials+2.0)};
    // Uncertainty is an exploration incentive, not measured superiority.
    (wins+1.0)/(trials+2.0)+measured_weight_bonus+0.09*avg.clamp(-3.0,3.0)
        +0.13*transfers/(trials+1.0)+0.2/(trials+1.0).sqrt()
}
fn allowed_roles(spec:&Value)->BTreeSet<String>{
    spec["targetRoles"].as_array().into_iter().flatten().filter_map(Value::as_str)
        .filter(|role|crate::engine::rsi_open_topology::valid_role(role))
        .map(str::to_string).collect()
}
fn score_target(row:&Value,entry:&Value)->Option<f64>{
    let role=row["role"].as_str()?;
    if !allowed_roles(&entry["spec"]).contains(role){return None;}
    let domain=row["domain"].as_str()?;
    let e=&entry["descendantEvidence"];
    let target_evidence=e["outcomes"].as_array().into_iter().flatten()
        .filter(|x|x["domain"]==domain).count() as f64;
    let transfer=e["distinctTransferDomains"].as_array().into_iter().flatten()
        .any(|d|d.as_str()==Some(domain));
    Some(ranked_rule(entry)+if transfer{0.12}else{0.0}
        +0.17/(target_evidence+1.0))
}
/// Find a synthesis opportunity using TWO independently adopted mechanisms
/// that do not already share exactly the same target-role portfolio.
fn composition(archive:&[Value])->Value{
    let mut ordered=archive.iter().filter(|entry|
        entry["descendantEvidence"]["measuredAdoptions"].as_u64().unwrap_or(0)>0
            || entry["descendantEvidence"]["independentModelImprovements"]
                .as_u64().unwrap_or(0)>0
    ).collect::<Vec<_>>();
    ordered.sort_by(|a,b|
        ranked_rule(b).total_cmp(&ranked_rule(a))
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str())));
    for (i,a) in ordered.iter().enumerate(){
        for b in ordered.iter().skip(i+1){
            if allowed_roles(&a["spec"])==allowed_roles(&b["spec"]){continue;}
            // Existing combined principle may already exist. A new one
            // needs a source edit and its own subsequent downstream trial.
            let already=archive.iter().any(|entry|{
                let deps=entry["spec"]["composedFrom"].as_array();
                deps.is_some_and(|rows|rows.iter().any(|p|p==&a["id"])
                    && rows.iter().any(|p|p==&b["id"]))
            });
            if already{continue;}
            return json!({"parents":[a["id"],b["id"]],
                "parentSourceCommits":[a["birthCommit"],b["birthCommit"]],
                "parentMechanisms":[a["spec"]["mechanism"],b["spec"]["mechanism"]],
                "transferRoles":allowed_roles(&a["spec"]).union(
                    &allowed_roles(&b["spec"])).cloned().collect::<Vec<_>>(),
                "hypothesis":"Invent and implement a NEW source-level operator that addresses the failure modes of both parents. Do not concatenate descriptions.",
                "qualityProven":false});
        }
    }
    Value::Null
}
/// Called by the compiled successor after its existing meta policy.
/// This policy can itself be replaced by an adopted source revision.
pub(crate) fn recompose(
    history:&[Value],frontier:&[Value],mut meta:Value,
    allowed_domains:&[&str],automatic:bool,stage_active:bool
)->Value{
    let lessons=archive(history);
    let synthesis=composition(&lessons);
    let mut candidates=Vec::<(Value,f64)>::new();
    for entry in &lessons {
        for row in frontier {
            if !row["domain"].as_str().is_some_and(|d|allowed_domains.contains(&d)){continue;}
            let Some(score)=score_target(row,entry) else{continue};
            if !meta["alternatives"].as_array().is_some_and(|alts|
                alts.iter().any(|alt|alt["target"]==*row)){continue;}
            candidates.push((json!({
                "target":row,"id":entry["id"],"principleBirth":entry["birthCommit"],
                "mechanism":entry["spec"]["mechanism"],
                "falsifier":entry["spec"]["falsifier"],
                "predictionBeforeEdit":entry["spec"]["prediction"],
                "outcomeEvidence":entry["descendantEvidence"],
                "crossDomainTrial":true,"priority":score
            }),score));
        }
    }
    // Synthesis chooses the actual editable improvement algorithm, not an
    // imaginary task-role patch; the model then writes a new source operator.
    if synthesis.is_object(){
        for row in frontier.iter().filter(|f|
            f["role"]=="improvement_methods" && f["domain"]=="improver"
            && allowed_domains.contains(&"improver")
            && f["path"].as_str().is_some_and(|p|
                p.ends_with("rsi_principle_learning.rs")
                || p.ends_with("rsi_meta_controller.rs"))){
            if meta["alternatives"].as_array().is_some_and(|alts|
                alts.iter().any(|alt|alt["target"]==*row)){
                candidates.push((json!({"target":row,"id":"compose",
                    "parentMechanisms":synthesis,
                    "crossDomainTrial":false,"priority":2.4}),
                    2.4));
            }
        }
    }
    candidates.sort_by(|(a,ap),(b,bp)|bp.total_cmp(ap)
        .then_with(||a["target"]["path"].as_str()
            .cmp(&b["target"]["path"].as_str())));
    let chosen=candidates.first().map(|r|r.0.clone()).unwrap_or(Value::Null);
    let active=automatic && !stage_active && chosen.is_object();
    if active {
        let old=meta["revision"].clone();
        let selected=chosen["target"].clone();
        let mut alternatives=meta["alternatives"].as_array().cloned().unwrap_or_default();
        for row in &mut alternatives{
            if row["target"]==selected {
                row["score"]=json!(row["score"].as_f64().unwrap_or(0.0)+
                    chosen["priority"].as_f64().unwrap_or(0.0));
                row["principleLearningScore"]=chosen["priority"].clone();
            }
        }
        // The ranking change must actually select the chosen experiment;
        // otherwise a recorded "active principle trial" would receive
        // outcomes from an unrelated target and corrupt meta-learning.
        let other_max=alternatives.iter().filter(|r|r["target"]!=selected)
            .filter_map(|r|r["score"].as_f64())
            .fold(0.0f64,f64::max);
        if let Some(row)=alternatives.iter_mut().find(|r|r["target"]==selected){
            row["score"]=json!(other_max+chosen["priority"].as_f64().unwrap_or(0.0)+1.0);
        }
        alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0)));
        if let Some(winner)=alternatives.first(){
            meta["target"]=winner["target"].clone();
            meta["selectionEvidence"]=winner.clone();
            meta["forecast"]["acceptanceProbability"]=
                winner["predictedAdoptionProbability"].clone();
            meta["forecast"]["uncertainty"]=winner["uncertainty"].clone();
            meta["mode"]=json!("empirically_learned_improvement_principle");
        }
        meta["alternatives"]=json!(alternatives);
        meta["revision"]=json!(digest(&json!([old,meta["target"],chosen,
            meta["alternatives"]])));
    }
    meta["principleLearning"]=json!({
        "source":"compiled_source_outcome_principle_learner_v1",
        "active":active,
        "selectedPrincipleId":if active {chosen["id"].clone()}else{Value::Null},
        "selectedEvidence":chosen,
        "learnedPrinciples":lessons,
        "synthesisAgenda":synthesis,
        "nextTrialRequired":active,
        "measuredTransferNotAssumed":true,
        "frozenOracleUnchanged":true,
        "causalAdvantageProven":false
    });
    meta["revision"]=json!(digest(&json!([meta["revision"],meta["principleLearning"]])));
    meta
}
pub(crate) fn instruction(meta:&Value)->String {
    let learning=&meta["principleLearning"];
    let archive=learning["learnedPrinciples"].as_array().into_iter().flatten()
        .take(5).map(|entry|format!(
            "principle={} inventedAt={} mechanism={} measured={} adopted={} transfers={} regressions={} modelEvaluations={} modelGains={}",
            entry["id"],entry["birthCommit"],entry["spec"]["mechanism"],
            entry["descendantEvidence"]["measuredTrials"],
            entry["descendantEvidence"]["measuredAdoptions"],
            entry["descendantEvidence"]["observedTransferAdoptions"],
            entry["descendantEvidence"]["measuredRegressions"],
            entry["descendantEvidence"]["measuredModelAdaptationTrials"],
            entry["descendantEvidence"]["independentModelImprovements"]
        )).collect::<Vec<_>>().join("\n");
    let chosen=&learning["selectedEvidence"];
    format!("[LEARNED IMPROVEMENT PRINCIPLE LAB] Source-grounded principles and later independent observations:\n{archive}\n        Chosen experiment: id={} target role={} source={} falsifier={}.\n        Synthesis evidence: {}. Form new mechanisms from contrasting successes and failures.        When evidence justifies it, call rsi_system_plan with improvementPrinciples         (id, mechanism, falsifier, prediction, methodPaths, targetRoles, transferDomains, composedFrom)         and edit the REAL implementation source. Changing a Rust program, not a prose recipe,         is required. Prior accepted source is NOT evidence that a later transfer succeeds.         Model weights, memory, retrieval, learning and tools are first-class redesign         targets; a new training operator must also modify executable model and         training-harness code. The evaluator and private held-out tasks stay frozen.",
        chosen["id"],chosen["target"]["role"],chosen["target"]["path"],
        chosen["falsifier"],learning["synthesisAgenda"])
}
/// A source-authored forecasting claim becomes training data ONLY after the
/// independently measured next generation is actually attempted.
pub(crate) fn feedback(meta:&Value,g:&Value)->Value{
    let l=&meta["principleLearning"];
    let observed=if l["active"]==true{reward(g)}else{None};
    json!({"selectedPrincipleId":l["selectedPrincipleId"],
        "observed":observed.as_ref().map(|v|json!({
            "holdoutReward":v.0,"independentAdoption":v.1,"domain":v.2})),
        "independentlyMeasured":observed.is_some(),
        "readyForNextGenerationLearning":observed.is_some(),
        "causalEffectProven":false
    })
}

#[cfg(test)]
mod tests{
    use super::*;
    fn born(id:&str,birth:&str)->Value{
        let path="src/engine/rsi_principle_learning.rs";
        json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "generatorCommit":"a".repeat(40),
            "acceptedCommit":birth,"jobId":"birth-worker",
            "changedSourcePaths":[path],
            "acceptedSourceHashes":{"src/engine/rsi_principle_learning.rs":"c".repeat(64)},
            "domain":"improver",
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),
                "generationJobId":"birth-worker",
                "targetFiles":[path],
                "improvementPrinciples":[{
                    "source":"model_authored_executable_principle_v1",
                    "id":id,"mechanism":"An executable search operator analyzes independent descendant outcomes",
                    "falsifier":"Measured holdout outcomes fail to transfer",
                    "prediction":"cross-domain positive",
                    "methodPaths":[path],"targetRoles":["memory","tools"],
                    "transferDomains":["memory","tools"],"composedFrom":[]}]}})
    }
    fn child(commit:&str,accepted:bool)->Value{
        let mut g=json!({"generatorCommit":commit,
            "accepted":accepted,"pairedRevalidated":accepted,
            "candidateOrigin":"model_driven_detached_worktree",
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "candidateId":"measured",
            "pairedMeasured":true,
            "evaluation":{"metrics":{"eligible":accepted,
                "measurements":[{"id":"heldout"}],
                "improvedHoldouts":if accepted{1}else{0},
                "regressions":0}},
            "metaPolicy":{"principleLearning":{"active":true,
                "selectedPrincipleId":"p_a"}},"domain":"memory"});
        if accepted {
            g["acceptedCommit"]=json!("f".repeat(40));
            g["systemDesign"]=json!({"source":"model_authored_system_design",
                "generatorCommit":commit,"generationJobId":"later",
                "targetFiles":["src/engine/memory.rs"]});
            g["jobId"]=json!("later");
        }
        g
    }
    #[test]
    fn never_credit_unedited_or_unmeasured_method_claims(){
        let mut g=born("p_a",&"b".repeat(40));
        assert_eq!(archive(&[g.clone()]).len(),1);
        g["acceptedSourceHashes"]=json!({});
        assert!(archive(&[g]).is_empty());
    }
    #[test]
    fn measured_downstream_success_and_failure_train_a_principle(){
        let g=born("p_a",&"b".repeat(40));
        let ok=child(&"b".repeat(40),true);
        let later=child(&"f".repeat(40),false);
        let a=archive(&[g,ok,later]);
        assert_eq!(a[0]["descendantEvidence"]["measuredTrials"],2);
        assert_eq!(a[0]["descendantEvidence"]["measuredAdoptions"],1);
        assert_eq!(a[0]["descendantEvidence"]["observedTransferAdoptions"],1);
    }
    #[test]
    fn unconnected_branch_cannot_train_an_inherited_principle(){
        let g=born("p_a",&"b".repeat(40));
        let other=child(&"d".repeat(40),true);
        let a=archive(&[g,other]);
        assert_eq!(a[0]["descendantEvidence"]["measuredTrials"],0);
    }
    #[test]
    fn empirical_target_reaches_model_memory_tools_when_supported(){
        let g=born("p_a",&"b".repeat(40));
        let frontier=vec![json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"a".repeat(64)})];
        let policy=json!({"revision":"a".repeat(64),
            "target":frontier[0],"alternatives":[{
                "target":frontier[0],"role":"memory",
                "score":0.2,"predictedAdoptionProbability":0.4,
                "uncertainty":0.2}],"forecast":{}});
        let updated=recompose(&[g],&frontier,policy,&["memory"],true,false);
        assert_eq!(updated["principleLearning"]["active"],true);
        assert_eq!(updated["target"]["role"],"memory");
    }
    #[test]
    fn independently_evaluated_lora_informs_principle_without_source_credit(){
        let original=born("p_a",&"b".repeat(40));
        let mut descendant=child(&"b".repeat(40),false);
        descendant["jobId"]=json!("model-only-child");
        descendant["pairedMeasured"]=json!(false);
        descendant["candidateId"]=Value::Null;
        descendant["modelAdapterIndependentEvaluation"]=json!({
            "source":"independent_parent_model_adapter_pair_v1",
            "generationJobId":descendant["jobId"],
            "generatorCommit":descendant["generatorCommit"],
            "status":"promoted","pairedModelImprovementMeasured":true,
            "measurements":{"source":"separate_parent_paired_lora_oracle_v1",
                "measuredExamples":12,"distinctDomains":3,
                "baselineMeanLoss":1.2,"candidateMeanLoss":0.9}
        });
        let entries=archive(&[original,descendant]);
        let evidence=&entries[0]["descendantEvidence"];
        assert_eq!(evidence["measuredTrials"],0);
        assert_eq!(evidence["measuredAdoptions"],0);
        assert_eq!(evidence["measuredModelAdaptationTrials"],1);
        assert_eq!(evidence["independentModelImprovements"],1);
    }
    #[test]
    fn explicit_owner_goal_prevents_automatic_override(){
        let g=born("p_a",&"b".repeat(40));
        let original=json!({"revision":"a".repeat(64),"alternatives":[]});
        let result=recompose(&[g],&[],original,&["memory"],false,false);
        assert_eq!(result["principleLearning"]["active"],false);
    }
}
