//! Evolvable interaction model for *mutual* intelligence improvement.
//!
//! A change to the harness may improve memory, tools or meta-search;
//! the next compiled descendant should see these cross-subsystem effects,
//! not merely a success label attached to one source file.
//! Only independently paired outcomes enter the graph. Joint changes
//! remain joint observations, never fabricated single-component causality.
//! This source belongs to the descendant and may itself be rewritten.
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn actual_roles(g:&Value,frontier:&[Value])->Vec<String>{
    // Measure the modules that REALLY changed, not the roles written in a
    // proposed design or a broad requested focus list. This distinction is
    // essential for both negative trials and multi-module interventions.
    let changes=g["changedSourcePaths"].as_array().cloned().unwrap_or_default();
    let changed=changes.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>();
    if changed.is_empty(){return Vec::new();}
    let design=&g["systemDesign"];
    let authentic=design["source"]=="model_authored_system_design"
        && design["generatorCommit"]==g["generatorCommit"]
        && design["generationJobId"]==g["jobId"];
    let mut roles=BTreeSet::new();
    if authentic {
        let declared=design["components"].as_array().cloned()
            .unwrap_or_else(||vec![design["component"].clone()]);
        for role in declared.iter().filter_map(Value::as_str)
            .filter(|role|*role!="system"&&!role.is_empty()&&role.len()<=64){
            let assigned=design["componentFiles"][role].as_array()
                .or_else(||(declared.len()==1)
                    .then(||design["targetFiles"].as_array()).flatten());
            if assigned.is_some_and(|rows|rows.iter().any(|row|
                row.as_str().is_some_and(|p|changed.contains(p)))){
                roles.insert(role.to_owned());
            }
        }
    }
    // Native transformations or plans without a classified design still
    // deserve evidence. Attribute them by ACTUAL changed files only.
    for path in changed {
        for entry in frontier.iter().filter(|entry|entry["path"]==path){
            if let Some(role)=entry["role"].as_str(){roles.insert(role.to_owned());}
        }
    }
    roles.into_iter().take(16).collect()
}
fn measured_role(domain:&str)->&str{
    match domain{
        "memory"|"recall"=>"memory",
        "context"|"planning"|"plan"=>"harness",
        "learning"|"training"=>"learning",
        "tools"|"tool-use"=>"tools",
        "improver"|"algorithm"|"search"=>"improvement_methods",
        "models"|"model"=>"models",
        "ui"|"frontend"=>"frontend",
        other=>other
    }
}
fn paired_observation(g:&Value)->bool{
    // Do not label a model's self-reported feedback as cross-role influence.
    (g["pairedMeasured"]==true || g["pairedRevalidated"]==true)
        && g["candidateId"].as_str().is_some()
        && g["evaluation"]["metrics"]["measurements"].is_array()
        && g["evaluation"]["metrics"]["verdict"].as_str().is_some()
        && g["generatorCommit"].as_str().is_some_and(|s|s.len()==40)
}
fn case_effect(m:&Value)->Option<i8>{
    let before=m["baselinePass"].as_bool()?;
    let after=m["candidatePass"].as_bool()?;
    if before&&!after{return Some(-1);}
    let mut gain=!before&&after;
    let mut loss=false;
    for item in m["metrics"].as_array().into_iter().flatten(){
        gain|=item["improved"]==true;
        loss|=item["regressed"]==true;
    }
    if loss{Some(-1)}
    else if gain{Some(1)}
    else{Some(0)}
}
/// Evidence-backed directional coupling: source-role -> measured target-role.
/// Observations from multi-role edits are explicitly confounded and never
/// counted as isolated effects. This is causal *hypothesis* selection, not
/// an ablation proof.
pub(crate) fn influence_graph(history:&[Value],frontier:&[Value])->Value{
    let mut edges=BTreeMap::<(String,String),(usize,usize,usize,usize,usize,usize,BTreeSet<String>)>::new();
    let mut paired=0usize;
    for g in history{
        if !paired_observation(g){continue;}
        let roles=actual_roles(g,frontier);
        if roles.is_empty(){continue;}
        paired+=1;
        let joint=roles.len()>1;
        let generation=g["id"].as_str().unwrap_or("").to_string();
        for measurement in g["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten().filter(|case|case["holdout"]==true){
            let Some(domain)=measurement["domain"].as_str() else{continue};
            let target=measured_role(domain);
            let Some(effect)=case_effect(measurement) else{continue};
            for role in &roles {
                let key=(role.clone(),target.to_string());
                let row=edges.entry(key).or_default();
                row.0+=usize::from(effect>0);
                row.1+=usize::from(effect<0);
                row.2+=usize::from(effect==0);
                row.3+=usize::from(joint);
                if !joint {
                    row.4+=usize::from(effect>0);
                    row.5+=usize::from(effect<0);
                }
                if !generation.is_empty() && row.6.len()<12 {
                    row.6.insert(generation.clone());
                }
            }
        }
    }
    let mut rows=edges.into_iter().map(|((source,target),(gain,loss,neutral,joint,isolated_gain,isolated_loss,ids))|{
        let total=gain+loss+neutral;
        let isolated=total.saturating_sub(joint);
        // No gains from a confounded multi-module edit should become proof
        // that its individual source component caused the observed gain.
        let strength=if isolated>0{
            (isolated_gain as f64-isolated_loss as f64)/(isolated.max(1) as f64)
        }else{0.0};
        json!({"sourceRole":source,"targetRole":target,
            "pairedGainObservations":gain,"pairedRegressionObservations":loss,
            "pairedNeutralObservations":neutral,
            "jointInterventionObservations":joint,"isolatedObservations":isolated,
            "isolatedGains":isolated_gain,"isolatedRegressions":isolated_loss,
            "directionalScore":strength,
            "evidenceClass":if isolated>0 {"single_component_paired_observation"}
                else{"joint_confounding_no_single_role_credit"},
            "generationIds":ids.into_iter().collect::<Vec<_>>(),
            "causalityProven":false})
    }).collect::<Vec<_>>();
    rows.sort_by(|a,b|b["directionalScore"].as_f64()
        .partial_cmp(&a["directionalScore"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)
        .then_with(||a["sourceRole"].as_str().cmp(&b["sourceRole"].as_str())));
    rows.truncate(96);
    let mut graph=json!({"source":"compiled_cross_system_influence_graph_v1",
        "observedPairedGenerations":paired,"edges":rows,
        "claim":"directional_independent_paired_observations_not_isolated_causal_proof",
        "rewardSource":"independent_frozen_holdout_only"});
    let hash=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&graph).unwrap_or_default()));
    graph["sha256"]=json!(hash);
    graph
}
/// Select a coupled redesign portfolio for the current improvement plan.
/// Cross-role positive evidence is a preference, not a compulsory fixed loop.
/// With no evidence, explore a distinct subsystem instead of self-confirming
/// the same file repeatedly. All targets must be in the descendant's current
/// SHA-anchored frontier and are independently verified before promotion.
pub(crate) fn coupled_portfolio(
    selected:&Value,frontier:&[Value],graph:&Value,allowed_domains:&[&str],
)->Value{
    let Some(primary_path)=selected["path"].as_str() else {
        return json!({"source":"descendant_coupled_redesign_v1","focus":[],"influenceEdges":[],
            "hypothesis":"no_source_selected","verified":false});
    };
    let Some(primary_role)=selected["role"].as_str() else {return Value::Null};
    let mut options=Vec::<(f64,Value,Value)>::new();
    for row in frontier {
        let (Some(role),Some(path),Some(domain))=(
            row["role"].as_str(),row["path"].as_str(),row["domain"].as_str()
        )else{continue};
        if path==primary_path || role==primary_role
            || !allowed_domains.contains(&domain)
            || !crate::engine::rsi::safe_path(path){continue;}
        let source_affinity=graph["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["sourceRole"]==primary_role && edge["targetRole"]==role
                && edge["isolatedObservations"].as_u64().unwrap_or(0)>0)
            .max_by(|a,b|a["directionalScore"].as_f64()
                .partial_cmp(&b["directionalScore"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal));
        let reverse_affinity=graph["edges"].as_array().into_iter().flatten()
            .filter(|edge|edge["sourceRole"]==role && edge["targetRole"]==primary_role
                && edge["isolatedObservations"].as_u64().unwrap_or(0)>0)
            .max_by(|a,b|a["directionalScore"].as_f64()
                .partial_cmp(&b["directionalScore"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal));
        let strongest=[source_affinity,reverse_affinity].into_iter().flatten()
            .max_by(|a,b|a["directionalScore"].as_f64()
                .partial_cmp(&b["directionalScore"].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal));
        let score=strongest.and_then(|e|e["directionalScore"].as_f64())
            .unwrap_or(0.0)*2.0
            +(if strongest.is_none(){0.20}else{0.0})
            -(row["sourceAttempts"].as_u64().unwrap_or(0) as f64*0.02)
            -((row["sourceBytes"].as_u64().unwrap_or(0) as f64)/100_000.0).min(0.10);
        options.push((score,row.clone(),strongest.cloned().unwrap_or(Value::Null)));
    }
    options.sort_by(|(score_a,a,_),(score_b,b,_)|
        score_b.partial_cmp(score_a).unwrap_or(std::cmp::Ordering::Equal)
        .then(a["role"].as_str().cmp(&b["role"].as_str()))
        .then(a["path"].as_str().cmp(&b["path"].as_str())));
    let mut focus=vec![selected.clone()];
    let mut edges=Vec::new();
    let mut roles=BTreeSet::new();
    roles.insert(primary_role.to_string());
    for (_,option,evidence) in options{
        let Some(role)=option["role"].as_str() else{continue};
        if !roles.insert(role.to_owned()){continue;}
        focus.push(option);
        if evidence.is_object(){edges.push(evidence);}
        if focus.len()>=3{break;}
    }
    let core=json!({
        "source":"descendant_coupled_redesign_v1",
        "focus":focus,"influenceEdges":edges,
        "sourceGraphSha256":graph["sha256"],
        "hypothesis":if graph["observedPairedGenerations"].as_u64().unwrap_or(0)>0 {
            "cross_role_measured_transfer_and_counterfactual_redesign"
        }else{"exploratory_cross_role_coupled_redesign"},
        "verified":false,"requiresIndependentPairedEvaluation":true,
        "allowsNovelArchitecture":true
    });
    let mut out=core;
    out["sha256"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&out).unwrap_or_default())));
    out
}
pub(crate) fn valid_graph(graph:&Value)->bool{
    let mut copy=graph.clone();
    let Some(map)=copy.as_object_mut() else{return false};
    map.remove("sha256");
    let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(&copy).unwrap_or_default()));
    graph["source"]=="compiled_cross_system_influence_graph_v1"
        && graph["sha256"].as_str()==Some(digest.as_str())
        && graph["rewardSource"]=="independent_frozen_holdout_only"
        && graph["edges"].as_array().is_some_and(|rows|
            rows.len()<=96 && rows.iter().all(|row|
                row["causalityProven"]==false
                && row["sourceRole"].as_str().is_some_and(|r|!r.is_empty()&&r.len()<=64)
                && row["targetRole"].as_str().is_some_and(|r|!r.is_empty()&&r.len()<=64)
                && row["directionalScore"].as_f64().is_some_and(|v|
                    v.is_finite()&&(-1.0..=1.0).contains(&v))))
}
pub(crate) fn valid_portfolio(portfolio:&Value,graph:&Value,frontier:&[Value])->bool{
    let mut copy=portfolio.clone();
    let Some(obj)=copy.as_object_mut() else{return false};
    obj.remove("sha256");
    let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(&copy).unwrap_or_default()));
    let Some(rows)=portfolio["focus"].as_array() else{return false};
    let mut paths=BTreeSet::new();
    portfolio["source"]=="descendant_coupled_redesign_v1"
        && portfolio["sha256"].as_str()==Some(digest.as_str())
        && portfolio["sourceGraphSha256"]==graph["sha256"]
        && portfolio["verified"]==false
        && portfolio["requiresIndependentPairedEvaluation"]==true
        && rows.len()<=3
        && rows.iter().all(|row|{
            let Some(p)=row["path"].as_str() else{return false};
            paths.insert(p.to_string())
                && frontier.iter().any(|known|known==row)
        })
}
/// Feed independently measured cross-role effects back into the *editable*
/// target-selection algorithm. Only isolated source-role interventions have
/// directional credit; jointly edited subsystems may suggest a follow-up
/// experiment but cannot be credited as individually productive.
///
/// The prior is intentionally conservative: one positive case is a hint,
/// not an automatic ranking victory. This affects search ONLY; the frozen
/// paired evaluator is still the sole source of successful adoption.
pub(crate) fn reinforce_meta_with_holdout(mut policy:Value,graph:&Value)->Value{
    if policy["source"]!="compiled_editable_meta_controller_v1"
        || !valid_graph(graph){return policy;}
    let Some(mut alternatives)=policy["alternatives"].as_array().cloned()
        else{return policy};
    for alternative in &mut alternatives{
        let Some(role)=alternative["role"].as_str() else{continue};
        let mut gains=0u64;
        let mut regressions=0u64;
        let mut observations=0u64;
        let mut confounded=0u64;
        for edge in graph["edges"].as_array().into_iter().flatten(){
            if edge["sourceRole"]!=role || edge["targetRole"]==role {continue;}
            gains+=edge["isolatedGains"].as_u64().unwrap_or(0);
            regressions+=edge["isolatedRegressions"].as_u64().unwrap_or(0);
            observations+=edge["isolatedObservations"].as_u64().unwrap_or(0);
            confounded+=edge["jointInterventionObservations"].as_u64().unwrap_or(0);
        }
        let signed=gains as f64-regressions as f64;
        let adjustment=(0.24*signed/(observations as f64+8.0)).clamp(-0.16,0.16);
        alternative["crossRoleHoldoutAdjustment"]=json!(adjustment);
        alternative["isolatedCrossRoleGains"]=json!(gains);
        alternative["isolatedCrossRoleRegressions"]=json!(regressions);
        alternative["isolatedCrossRoleObservations"]=json!(observations);
        alternative["confoundedJointObservations"]=json!(confounded);
        alternative["score"]=json!(
            alternative["score"].as_f64().unwrap_or(0.0)+adjustment);
    }
    alternatives.sort_by(|a,b|b["score"].as_f64().unwrap_or(0.0)
        .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
        .then_with(||a["role"].as_str().cmp(&b["role"].as_str())));
    let Some(winner)=alternatives.first() else{return policy};
    let old=policy["revision"].clone();
    policy["target"]=winner["target"].clone();
    policy["selectionEvidence"]=winner.clone();
    policy["forecast"]["acceptanceProbability"]=
        winner["predictedAdoptionProbability"].clone();
    policy["forecast"]["uncertainty"]=winner["uncertainty"].clone();
    policy["alternatives"]=json!(alternatives);
    policy["sourceAttributionHoldoutGraphSha256"]=graph["sha256"].clone();
    policy["holdoutTransferPolicy"]="conservative_isolated_observation_only".into();
    policy["revision"]=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&json!([old,graph["sha256"],policy["alternatives"]]))
            .unwrap_or_default())));
    policy
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cross_role_observations_are_not_automatic_single_component_causality(){
        let paired=json!({"id":"adopted","generatorCommit":"a".repeat(40),
            "pairedMeasured":true,"pairedRevalidated":true,"candidateId":"c","accepted":true,
            "jobId":"job1","changedSourcePaths":["src/engine/jobs.rs"],
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),"generationJobId":"job1",
                "component":"harness","targetFiles":["src/engine/jobs.rs"]},
            "evaluation":{"metrics":{"verdict":"improved","measurements":[
                {"domain":"memory","holdout":true,"baselinePass":false,"candidatePass":true,"metrics":[]},
                {"domain":"tools","holdout":true,"baselinePass":true,"candidatePass":true,
                    "metrics":[{"key":"quality","improved":true,"regressed":false}]}
            ]}}});
        let graph=influence_graph(&[paired.clone()],&[]);
        let mut no_actual_patch=paired.clone();
        no_actual_patch["changedSourcePaths"]=json!([]);
        assert!(influence_graph(&[no_actual_patch],&[])["edges"]
            .as_array().unwrap().is_empty(),
            "declared intent without a changed source is never causal evidence");
        assert!(valid_graph(&graph));
        assert_eq!(graph["edges"].as_array().unwrap().len(),2);
        assert!(graph["edges"].as_array().unwrap().iter().any(|row|
            row["sourceRole"]=="harness" && row["targetRole"]=="memory"
            && row["pairedGainObservations"]==1));
        let mut joint=paired;
        joint["systemDesign"]["components"]=json!(["harness","learning"]);
        joint["systemDesign"]["component"]=Value::Null;
        joint["changedSourcePaths"]=json!(["src/engine/jobs.rs","src/engine/learning.rs"]);
        joint["systemDesign"]["componentFiles"]=json!({
            "harness":["src/engine/jobs.rs"],"learning":["src/engine/learning.rs"]});
        let graph=influence_graph(&[joint],&[]);
        assert!(graph["edges"].as_array().unwrap().iter().all(|row|
            row["causalityProven"]==false && row["isolatedObservations"]==0));
    }
    #[test]
    fn only_isolated_verified_holdout_effects_reinforce_meta_targeting(){
        let policy=json!({"source":"compiled_editable_meta_controller_v1",
            "revision":"original",
            "alternatives":[
                {"role":"harness","target":{"role":"harness"},"score":1.0,
                    "predictedAdoptionProbability":0.5,"uncertainty":0.3},
                {"role":"memory","target":{"role":"memory"},"score":1.0,
                    "predictedAdoptionProbability":0.5,"uncertainty":0.3}
            ],"forecast":{"qualityProven":false}});
        let adopted=json!({"id":"adopted","generatorCommit":"a".repeat(40),
            "pairedMeasured":true,"candidateId":"c","jobId":"job1",
            "changedSourcePaths":["src/engine/jobs.rs"],
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),"generationJobId":"job1",
                "component":"harness","targetFiles":["src/engine/jobs.rs"]},
            "evaluation":{"metrics":{"verdict":"improved","measurements":[
                {"domain":"memory","holdout":true,
                 "baselinePass":false,"candidatePass":true,"metrics":[]}
            ]}}});
        let graph=influence_graph(&[adopted.clone()],&[]);
        let updated=reinforce_meta_with_holdout(policy.clone(),&graph);
        assert_eq!(updated["target"]["role"],"harness");
        assert!(updated["selectionEvidence"]["crossRoleHoldoutAdjustment"]
            .as_f64().unwrap()>0.0);
        assert_eq!(updated["forecast"]["qualityProven"],false);
        let mut joint=adopted;
        joint["systemDesign"]["component"]="system".into();
        joint["systemDesign"]["components"]=json!(["harness","memory"]);
        joint["systemDesign"]["componentFiles"]=json!({
            "harness":["src/engine/jobs.rs"],"memory":["src/engine/memory.rs"]});
        joint["changedSourcePaths"]=json!([
            "src/engine/jobs.rs","src/engine/memory.rs"]);
        let confounded=influence_graph(&[joint],&[]);
        let updated=reinforce_meta_with_holdout(policy,&confounded);
        assert!(updated["alternatives"].as_array().unwrap().iter().all(|row|
            row["crossRoleHoldoutAdjustment"]==0.0));
    }

    #[test]
    fn coupled_plan_follows_current_source_sha_and_distinct_components(){
        let frontier=vec![
            json!({"role":"harness","domain":"context","path":"src/engine/jobs.rs","sourceSha256":"a"}),
            json!({"role":"memory","domain":"memory","path":"src/engine/memory.rs","sourceSha256":"b"}),
            json!({"role":"improvement_methods","domain":"improver","path":"src/engine/rsi_meta_controller.rs","sourceSha256":"c"})
        ];
        let graph=influence_graph(&[],&frontier);
        let policy=coupled_portfolio(&frontier[0],&frontier,&graph,
            &["context","memory","improver"]);
        assert_eq!(policy["focus"].as_array().unwrap().len(),3);
        assert!(valid_portfolio(&policy,&graph,&frontier));
        let mut tampered=policy.clone();
        tampered["focus"][1]["sourceSha256"]=json!("wrong");
        assert!(!valid_portfolio(&tampered,&graph,&frontier));
    }
}
