//! Editable factorial experiment designer for cross-subsystem RSI.
//!
//! The descendant asks which A-only, B-only or A+B source experiment is missing
//! on a COMMON frozen baseline. The supervisor owns holdout evaluation.
//! Matching observations are useful experimental contrasts, NOT randomised
//! causal identification or proof of a new intelligence capability.
use crate::engine::{rsi,rsi_interaction};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn hash(value:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn paired(g:&Value)->bool {
    (g["pairedMeasured"]==true || g["pairedRevalidated"]==true)
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["generatorCommit"].as_str().is_some_and(|s|s.len()==40
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["evaluationProfileSha256"].as_str().is_some_and(|s|s.len()==64
            && s.bytes().all(|c|c.is_ascii_hexdigit()))
        && g["evaluation"]["metrics"]["measurements"].is_array()
}
fn variant(roles:&[String],a:&str,b:&str)->Option<usize> {
    if roles.len()==1 && roles[0]==a {Some(0)}
    else if roles.len()==1 && roles[0]==b {Some(1)}
    else if roles.len()==2 && roles.iter().any(|r|r==a)
        && roles.iter().any(|r|r==b) {Some(2)}
    else {None}
}
fn average(xs:&[f64])->f64 {
    xs.iter().sum::<f64>()/(xs.len().max(1) as f64)
}
type BlockKey=(String,String,String,bool);
type Observations=[Vec<f64>;3];

/// An experiment must compare the SAME generator, frozen evaluator profile,
/// held-out task and baseline outcome. Other histories may influence
/// exploration, but cannot be blended into a synthetic factorial result.
fn evidence(history:&[Value],frontier:&[Value],a:&str,b:&str)->Value {
    let mut blocks=BTreeMap::<BlockKey,Observations>::new();
    let mut counts=[0usize;3];
    let mut generation_ids=[BTreeSet::<String>::new(),BTreeSet::new(),BTreeSet::new()];
    for g in history.iter().filter(|g|paired(g)) {
        let roles=rsi_interaction::actual_roles(g,frontier);
        let Some(index)=variant(&roles,a,b) else {continue};
        let generator=g["generatorCommit"].as_str().unwrap_or("");
        let profile=g["evaluationProfileSha256"].as_str().unwrap_or("");
        let gid=g["id"].as_str().unwrap_or("");
        let mut observed=false;
        for measurement in g["evaluation"]["metrics"]["measurements"]
            .as_array().into_iter().flatten() {
            if measurement["holdout"]!=true {continue;}
            let (Some(case),Some(before),Some(after))=(
                measurement["id"].as_str(),
                measurement["baselinePass"].as_bool(),
                measurement["candidatePass"].as_bool(),
            ) else {continue};
            if case.is_empty() {continue;}
            let key=(generator.to_owned(),profile.to_owned(),case.to_owned(),before);
            let delta=(after as i32-before as i32) as f64;
            blocks.entry(key).or_insert_with(||[Vec::new(),Vec::new(),Vec::new()])[index].push(delta);
            observed=true;
        }
        if observed && !gid.is_empty() && generation_ids[index].insert(gid.to_owned()){
            counts[index]+=1;
        }
    }
    let mut contrasts=Vec::new();
    for ((generator,profile,case,before),variants) in &blocks {
        if variants.iter().any(Vec::is_empty){continue;}
        let a_delta=average(&variants[0]);
        let b_delta=average(&variants[1]);
        let both_delta=average(&variants[2]);
        contrasts.push(json!({
            "generatorCommit":generator,"profileSha256":profile,
            "caseId":case,"baselinePass":before,
            "isolatedLeftDelta":a_delta,"isolatedRightDelta":b_delta,
            "jointDelta":both_delta,
            "interactionContrast":both_delta-a_delta-b_delta,
            "measurementCounts":variants.iter().map(Vec::len).collect::<Vec<_>>(),
            "causalityProven":false
        }));
    }
    let matched=contrasts.len();
    let interaction=if matched>0 {
        Some(contrasts.iter().map(|r|r["interactionContrast"].as_f64().unwrap_or(0.0))
            .sum::<f64>()/matched as f64)
    }else{None};
    let mut out=json!({
        "measuredVariantGenerations":{"leftOnly":counts[0],"rightOnly":counts[1],"joint":counts[2]},
        "matchedFrozenHoldoutCases":matched,
        "meanInteractionContrast":interaction,
        "contrasts":contrasts.into_iter().take(12).collect::<Vec<_>>(),
        "evidenceClass":"matched_observational_factorial_not_causal_proof",
    });
    out["sha256"]=json!(hash(&out));
    out
}
/// Compiled descendant owns the experiment-design curriculum; its source can
/// be rewritten by a verified successor. Each path is rebound to CURRENT bytes.
pub(crate) fn design(history:&[Value],frontier:&[Value],graph:&Value,
    allowed:&[&str],commit:&str)->Value {
    let mut targets=BTreeMap::<String,Value>::new();
    for target in frontier {
        if let (Some(role),Some(path),Some(domain))=(
            target["role"].as_str(),target["path"].as_str(),target["domain"].as_str()) {
            if allowed.contains(&domain) && rsi::safe_path(path)
                && target["sourceSha256"].as_str().is_some_and(|h|h.len()==64) {
                targets.entry(role.to_string()).or_insert_with(||target.clone());
            }
        }
    }
    let mut pairs=BTreeSet::<(String,String)>::new();
    for edge in graph["edges"].as_array().into_iter().flatten() {
        if let (Some(a),Some(b))=(edge["sourceRole"].as_str(),edge["targetRole"].as_str()){
            if a!=b && targets.contains_key(a)&&targets.contains_key(b)
                && edge["isolatedObservations"].as_u64().unwrap_or(0)>0 {
                let (left,right)=if a<b {(a,b)}else{(b,a)};
                pairs.insert((left.to_owned(),right.to_owned()));
            }
        }
    }
    for g in history.iter().filter(|g|paired(g)) {
        let roles=rsi_interaction::actual_roles(g,frontier);
        if roles.len()==2 && roles.iter().all(|role|targets.contains_key(role)){
            pairs.insert((roles[0].clone(),roles[1].clone()));
        }
    }
    let mut ranked=Vec::<(i64,Value)>::new();
    for (a,b) in pairs.into_iter().take(48) {
        let observation=evidence(history,frontier,&a,&b);
        let c=&observation["measuredVariantGenerations"];
        let counts=[
            c["leftOnly"].as_u64().unwrap_or(0),
            c["rightOnly"].as_u64().unwrap_or(0),
            c["joint"].as_u64().unwrap_or(0),
        ];
        if counts.iter().all(|n|*n==0){continue;}
        let completed=counts.iter().filter(|n|**n>0).count();
        let next=counts.iter().position(|n|*n==0);
        let phase=match next {
            Some(0)=>"isolate_left",
            Some(1)=>"isolate_right",
            Some(2)=>"test_joint_interaction",
            _=>"all_variants_observed",
        };
        let left=targets[&a].clone();let right=targets[&b].clone();
        let primary=if next==Some(1){right.clone()}else{left.clone()};
        let row=json!({
            "roles":[a,b],"left":left,"right":right,
            "phase":phase,"target":primary,
            "companion":if next==Some(1){targets[&a].clone()}else{targets[&b].clone()},
            "isolatedEditRequired":next==Some(0)||next==Some(1),
            "observations":observation,"nextVariantIndex":next,
            "qualityProven":false,"causalityProven":false
        });
        let priority=(completed as i64)*100
            +counts.iter().map(|n|(*n).min(3) as i64).sum::<i64>()*5
            +observation["matchedFrozenHoldoutCases"].as_u64().unwrap_or(0).min(10) as i64;
        ranked.push((if next.is_some(){priority}else{-1},row));
    }
    ranked.sort_by(|(sa,a),(sb,b)|sb.cmp(sa)
        .then_with(||a["roles"][0].as_str().cmp(&b["roles"][0].as_str()))
        .then_with(||a["roles"][1].as_str().cmp(&b["roles"][1].as_str())));
    let selected=ranked.iter().find(|(score,_)|*score>=0)
        .map(|(_,row)|row.clone()).unwrap_or(Value::Null);
    let mut result=json!({
        "source":"compiled_factorial_ablation_curriculum_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "interactionGraphSha256":graph["sha256"],
        "selected":selected,
        "pairFrontier":ranked.into_iter().take(8).map(|(_,row)|row).collect::<Vec<_>>(),
        "externalEvaluationOwner":"independent_supervisor",
        "causalityProven":false,"qualityProven":false,
        "requiresFrozenPairedHoldout":true,"editableInDescendant":true
    });
    result["sha256"]=json!(hash(&result));
    result
}
pub(crate) fn valid(plan:&Value,frontier:&[Value],commit:&str,history_count:usize,
    graph:&Value)->bool {
    let mut unsigned=plan.clone();
    let Some(map)=unsigned.as_object_mut() else{return false};
    map.remove("sha256");
    if plan["source"]!="compiled_factorial_ablation_curriculum_v1"
        || plan["sha256"].as_str()!=Some(hash(&unsigned).as_str())
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history_count as u64)
        || plan["interactionGraphSha256"]!=graph["sha256"]
        || plan["externalEvaluationOwner"]!="independent_supervisor"
        || plan["causalityProven"]!=false || plan["qualityProven"]!=false
        || plan["requiresFrozenPairedHoldout"]!=true
        || plan["pairFrontier"].as_array().is_none_or(|pairs|pairs.len()>8) {
        return false;
    }
    let matches=|row:&Value|row["path"].as_str().is_some_and(|p|
        rsi::safe_path(p) && frontier.iter().any(|known|known==row));
    let valid_pair=|pair:&Value|{
        if !pair.is_object() || !matches(&pair["left"])
            || !matches(&pair["right"])||!matches(&pair["target"])
            || !matches(&pair["companion"])
            || pair["left"]["role"]==pair["right"]["role"]
            || pair["qualityProven"]!=false||pair["causalityProven"]!=false{
            return false;
        }
        let expected=if pair["phase"]=="isolate_right"{&pair["right"]}else{&pair["left"]};
        let companion=if pair["phase"]=="isolate_right"{&pair["left"]}else{&pair["right"]};
        let mut observed=pair["observations"].clone();
        let Some(body)=observed.as_object_mut() else{return false};
        body.remove("sha256");
        pair["target"]==*expected && pair["companion"]==*companion
            && pair["observations"]["sha256"].as_str()==Some(hash(&observed).as_str())
            && pair["observations"]["evidenceClass"]=="matched_observational_factorial_not_causal_proof"
            && pair["observations"]["matchedFrozenHoldoutCases"].as_u64()
                .is_some_and(|n|n<=history_count as u64 * 128)
            && matches!(pair["phase"].as_str(),
                Some("isolate_left"|"isolate_right"|"test_joint_interaction"|"all_variants_observed"))
    };
    plan["pairFrontier"].as_array().is_some_and(|pairs|
        pairs.iter().all(|pair|valid_pair(pair))
        && (plan["selected"].is_null()
            || (valid_pair(&plan["selected"])
                && pairs.iter().any(|row|row==&plan["selected"]))))
}
pub(crate) fn instruction(plan:&Value)->String {
    let selection=&plan["selected"];
    if !selection.is_object(){return String::new();}
    let left=&selection["left"];let right=&selection["right"];
    let phase=selection["phase"].as_str().unwrap_or("");
    let instruction=match phase {
        "isolate_left"|"isolate_right"=>
            "Change the selected role alone. Read the companion as CONTEXT only; do not edit companion sources. The independently measured changed-source paths, not your declaration, determine whether the isolated variant was observed.",
        "test_joint_interaction"=>
            "Test a genuinely JOINT redesign of both roles. Record exact changed paths; an improvement in a joint edit must not be credited to either role alone.",
        _=>"Existing matched contrasts are hypotheses; seek a novel mechanism."
    };
    format!(
        "FACTORIZED CROSS-SYSTEM EXPERIMENT {} (plan SHA={}): \
         A role={} path={} sha={}, B role={} path={} sha={}. \
         Matched frozen cases={} observed variants={}. {} \
         The supervisor controls the frozen paired evaluation; do not claim causal proof. \
         A rebuilt descendant may revise this experiment-selection algorithm.",
        phase,plan["sha256"],left["role"],left["path"],left["sourceSha256"],
        right["role"],right["path"],right["sourceSha256"],
        selection["observations"]["matchedFrozenHoldoutCases"],
        selection["observations"]["measuredVariantGenerations"],instruction)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(role:&str,path:&str)->Value {
        json!({"role":role,"path":path,"domain":"improver",
            "sourceSha256":"a".repeat(64)})
    }
    fn generation(id:&str,paths:&[&str],after:bool)->Value {
        json!({"id":id,"generatorCommit":"f".repeat(40),
            "evaluationProfileSha256":"e".repeat(64),
            "candidateId":id,"pairedMeasured":true,"changedSourcePaths":paths,
            "evaluation":{"metrics":{"measurements":[{"id":"case1","holdout":true,
                "baselinePass":false,"candidatePass":after}]}}})
    }
    #[test]
    fn chooses_missing_isolation_and_uses_real_matched_triplets() {
        let frontier=vec![source("memory","src/engine/memory.rs"),
            source("learning","src/engine/learning.rs")];
        let joint=generation("joint",&["src/engine/memory.rs",
            "src/engine/learning.rs"],true);
        let graph=json!({"sha256":"g".repeat(64),"edges":[]});
        let plan=design(&[joint.clone()],&frontier,&graph,&["improver"],&"f".repeat(40));
        assert_eq!(plan["selected"]["phase"],"isolate_left");
        assert!(valid(&plan,&frontier,&"f".repeat(40),1,&graph));
        let a=generation("left",&["src/engine/learning.rs"],true);
        let p2=design(&[joint.clone(),a.clone()],&frontier,&graph,&["improver"],&"f".repeat(40));
        assert_eq!(p2["selected"]["phase"],"isolate_right");
        let b=generation("right",&["src/engine/memory.rs"],false);
        let p3=design(&[joint,a,b],&frontier,&graph,&["improver"],&"f".repeat(40));
        assert!(p3["selected"].is_null());
        assert_eq!(p3["pairFrontier"][0]["observations"]["matchedFrozenHoldoutCases"],1);
        assert_eq!(p3["pairFrontier"][0]["observations"]["meanInteractionContrast"],0.0);
    }
    #[test]
    fn forged_success_claim_and_changed_target_fail_attestation() {
        let frontier=vec![source("memory","src/engine/memory.rs"),
            source("learning","src/engine/learning.rs")];
        let graph=json!({"sha256":"g".repeat(64),"edges":[]});
        let joint=generation("joint",&["src/engine/memory.rs",
            "src/engine/learning.rs"],true);
        let mut plan=design(&[joint],&frontier,&graph,&["improver"],&"f".repeat(40));
        assert!(valid(&plan,&frontier,&"f".repeat(40),1,&graph));
        plan["selected"]["observations"]["matchedFrozenHoldoutCases"]=json!(100);
        // Even after recomputing the outer digest, nested measured facts
        // cannot be changed without breaking their own evidence digest.
        let mut unsigned=plan.clone();
        unsigned.as_object_mut().unwrap().remove("sha256");
        plan["sha256"]=json!(hash(&unsigned));
        assert!(!valid(&plan,&frontier,&"f".repeat(40),1,&graph));
    }
    #[test]
    fn different_generator_or_evaluation_cannot_create_false_interaction(){
        let frontier=vec![source("learning","src/engine/learning.rs"),
            source("memory","src/engine/memory.rs")];
        let graph=json!({"sha256":"g".repeat(64),"edges":[]});
        let joint=generation("joint",&["src/engine/memory.rs","src/engine/learning.rs"],true);
        let left=generation("left",&["src/engine/learning.rs"],true);
        let mut right=generation("right",&["src/engine/memory.rs"],true);
        right["generatorCommit"]=json!("d".repeat(40));
        let result=design(&[joint,left,right],&frontier,&graph,&["improver"],&"f".repeat(40));
        assert_eq!(result["pairFrontier"][0]["observations"]["matchedFrozenHoldoutCases"],0);
    }
}
