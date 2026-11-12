//! Cross-component self-development programme of the compiled successor.
//! This is a source-editable hypothesis generator: observed continuation,
//! co-design, and an authored next-stage intention are different evidence.
//! An external paired evaluator alone decides whether any change improved.
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};

#[derive(Default,Clone)]
struct EdgeEvidence {
    authored_stages:u64,
    coupled_adoptions:u64,
    direct_successor_attempts:u64,
    direct_successor_adoptions:u64,
}

fn roles_of(g:&Value)->Vec<String>{
    let design=&g["systemDesign"];
    if design["source"]!="model_authored_system_design"
        || design["generationJobId"]!=g["jobId"]
        || design["generatorCommit"]!=g["generatorCommit"] {
        return Vec::new();
    }
    let parts=if design["component"]=="system"{
        design["components"].as_array().cloned().unwrap_or_default()
    }else{vec![design["component"].clone()]};
    parts.iter().filter_map(Value::as_str)
        .filter(|name|!name.is_empty()&&name.len()<=64
            && name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')
            && *name!="system")
        .map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect()
}

fn accepted(g:&Value)->bool{
    if g["accepted"]!=true || g["pairedRevalidated"]!=true
        || g["evaluation"]["metrics"]["eligible"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || g["acceptedCommit"].as_str().is_none_or(|s|s.len()!=40
            || !s.bytes().all(|c|c.is_ascii_hexdigit())){
        return false;
    }
    let paths=g["acceptedSourceHashes"].as_object().map(|m|
        m.keys().map(|path|json!(path)).collect::<Vec<_>>())
        .unwrap_or_default();
    crate::engine::rsi_system::design_implemented(&g["systemDesign"],&paths)
}

/// Portfolio of potential mutual improvements, grounded in source revisions
/// and independent adoption receipts. Edge rank is for exploration only.
/// A predecessor->child observed gain is not claimed to be causal attribution.
pub(crate) fn graph(history:&[Value],frontier:&[Value])->Value{
    let roles=frontier.iter().filter_map(|row|row["role"].as_str())
        .filter(|role|!role.is_empty()&&role.len()<=64)
        .map(str::to_string).collect::<BTreeSet<_>>();
    let recent=history.iter().rev().take(96).collect::<Vec<_>>();
    let mut attempts=BTreeMap::<String,u64>::new();
    let mut proven=BTreeMap::<String,u64>::new();
    let mut evidence=BTreeMap::<(String,String),EdgeEvidence>::new();
    for g in &recent {
        let attributed=roles_of(g);
        if attributed.is_empty(){continue;}
        for role in &attributed{
            if roles.contains(role){
                *attempts.entry(role.clone()).or_default()+=1;
                if accepted(g){*proven.entry(role.clone()).or_default()+=1;}
            }
        }
        if !accepted(g){continue;}
        for from in &attributed{
            for stage in g["systemDesign"]["successorStages"]
                .as_array().into_iter().flatten(){
                let Some(to)=stage["role"].as_str() else{continue};
                if from!=to && roles.contains(from) && roles.contains(to){
                    evidence.entry((from.clone(),to.into())).or_default()
                        .authored_stages+=1;
                }
            }
            for to in &attributed{
                if from!=to && roles.contains(from)&&roles.contains(to){
                    evidence.entry((from.clone(),to.clone())).or_default()
                        .coupled_adoptions+=1;
                }
            }
        }
        let Some(accepted_commit)=g["acceptedCommit"].as_str() else{continue};
        for later in &recent{
            if later["generatorCommit"].as_str()!=Some(accepted_commit){continue;}
            for from in &attributed{
                for to in roles_of(later) {
                    if *from==to || !roles.contains(from)||!roles.contains(&to){continue;}
                    let edge=evidence.entry((from.clone(),to)).or_default();
                    edge.direct_successor_attempts+=1;
                    if accepted(later){edge.direct_successor_adoptions+=1;}
                }
            }
        }
    }
    let mut candidates=Vec::<(i64,Value)>::new();
    for from in &roles{
        for to in &roles{
            if from==to{continue;}
            let count=evidence.get(&(from.clone(),to.clone()))
                .cloned().unwrap_or_default();
            let origin=proven.get(from).copied().unwrap_or(0);
            let destination=proven.get(to).copied().unwrap_or(0);
            let tries=attempts.get(to).copied().unwrap_or(0);
            let failed_followups=count.direct_successor_attempts
                .saturating_sub(count.direct_successor_adoptions);
            // An open target reached from an already improved source is a
            // strong exploration opportunity, not a measured transfer.
            let score=45*i64::from(origin>0)
                +35*i64::from(destination==0)
                +25*(count.direct_successor_adoptions.min(3) as i64)
                +12*(count.authored_stages.min(3) as i64)
                +8*(count.coupled_adoptions.min(3) as i64)
                -8*(failed_followups.min(4) as i64)
                -(tries.min(6) as i64)*3;
            let focus=frontier.iter().find(|row|row["role"]==to);
            candidates.push((score,json!({
                "from":from,"to":to,"targetDomain":focus.map(|r|&r["domain"]),
                "targetPath":focus.map(|r|&r["path"]),
                "targetSourceSha256":focus.map(|r|&r["sourceSha256"]),
                "sourceAcceptedGenerations":origin,
                "targetAcceptedGenerations":destination,
                "targetAttemptedGenerations":tries,
                "authoredSuccessorStages":count.authored_stages,
                "coupledAdoptions":count.coupled_adoptions,
                "observedDirectSuccessorAttempts":count.direct_successor_attempts,
                "observedDirectSuccessorAdoptions":count.direct_successor_adoptions,
                "priority":score,
                "causalAttributionProven":false,
                "qualityProvenForThisDesign":false
            })));
        }
    }
    candidates.sort_by(|(a,left),(b,right)|b.cmp(a)
        .then_with(||left["to"].as_str().cmp(&right["to"].as_str()))
        .then_with(||left["from"].as_str().cmp(&right["from"].as_str())));
    json!({
        "source":"compiled_mutual_intelligence_development_v1",
        "roleCount":roles.len(),
        "observedGenerations":recent.len(),
        "edges":candidates.into_iter().take(96).map(|(_,row)|row)
            .collect::<Vec<_>>(),
        "interpretation":"source-bound cross-role exploration, not causal proof",
        "qualityProven":false
    })
}
