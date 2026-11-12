//! Self-improving selection of the *model that improves DOMINION*.
//! Distinguishes upstream API weights from the downstream intelligence
//! actually improved by an independently evaluated source-editing descendant.
//!
//! The frozen external evaluator controls adoption. The compiled descendant
//! owns this selection algorithm and may rewrite it. Explicit owner-specified
//! provider/model allowlists bound all exploration (including paid API use).
//! Comparison requires the SAME generator source commit AND frozen evaluation
//! profile; unpaired cross-generation wins never establish model superiority.
use crate::{engine::{gateway,rsi},state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn sha(s:&str,n:usize)->bool{
    s.len()==n && s.bytes().all(|c|c.is_ascii_hexdigit())
}
fn route_key(p:&str,m:&str)->String{format!("{p}\u{001f}{m}")}
fn route(origin:&Value)->Option<(String,String)> {
    let provider=origin["providerId"].as_str()
        .filter(|p|!p.is_empty()&&p.len()<=128)?;
    let model=origin["model"].as_str()
        .filter(|m|!m.is_empty()&&m.len()<=256)?;
    Some((provider.to_owned(),model.to_owned()))
}
fn base_route(input:&Value)->Option<(String,String)>{
    if input["rsiModelOrigin"].is_object(){route(&input["rsiModelOrigin"])}
    else{route(input)}
}
fn eligible_provider(state:&AppState,p:&str,model:&str,local_only:bool)->bool {
    let Some(provider)=state.store.get("provider",p)else{return false};
    if provider["enabled"]==false || !gateway::provider_eligible(&provider) {
        return false;
    }
    let Ok(model_ref)=gateway::route_from_provider(state,&provider,Some(model))
        else{return false};
    !local_only || model_ref.local
}
fn allowed(state:&AppState,input:&Value)->Result<Vec<Value>,String>{
    if input["rsiAllowModelEvolution"]!=true {return Ok(Vec::new());}
    if input["rsiSystemEvolution"]!=true || input["rsiAutonomous"]!=true {
        return Err("모델 진화는 명시적으로 승인된 자율 시스템 캠페인에서만 가능합니다.".into());
    }
    let Some((provider,model))=base_route(input)else{
        return Err("기존 세대 모델의 제공자·모델 출처가 없습니다.".into());
    };
    let rows=input["rsiApprovedModelPool"].as_array()
        .filter(|rows|(2..=8).contains(&rows.len()))
        .ok_or("비교할 모델 제공자·모델 목록 2~8개를 먼저 승인해야 합니다.")?;
    let mut pool=Vec::new();let mut seen=BTreeSet::new();
    let local_only=input["rsiLocalOnly"]==true;
    for row in rows {
        let Some((p,m))=route(row)else {
            return Err("승인 후보의 providerId/model 형식이 유효하지 않습니다.".into());
        };
        if !seen.insert(route_key(&p,&m)) {
            return Err("모델 비교 승인 목록에 중복 항목이 있습니다.".into());
        }
        if !eligible_provider(state,&p,&m,local_only) {
            return Err("승인 모델이 등록되지 않았거나 비활성·로컬 정책에 어긋납니다.".into());
        }
        pool.push(json!({"providerId":p,"model":m}));
    }
    if !seen.contains(&route_key(&provider,&model)){
        return Err("현재 사용자 모델이 승인된 비교 목록에 없습니다.".into());
    }
    Ok(pool)
}
fn independent_task_receipt(state:&AppState,g:&Value,profile:&str,head:&str)
    ->Option<Value>{
    if g["generatorCommit"]!=head || g["evaluationProfileSha256"]!=profile
        || g["compiledPlanExecuted"]["source"]!="running_compiled_descendant_binary"
        || g["compiledPlanExecuted"]["generatorCommit"]!=head
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || g["pairedMeasured"]!=true
        || g["modelSelectionActual"]["providerId"]!=g["generatorProviderId"]
        || g["modelSelectionActual"]["model"]!=g["generatorModel"]
        || g["modelEvolutionPlanExecuted"]["source"]!=
            "running_compiled_descendant_binary"
        || g["modelEvolutionPlanExecuted"]["selectionRevision"]!=
            g["modelEvolutionPlan"]["revision"] {return None;}
    let id=g["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",id)?;
    let replay=rsi::replayed_pair_metrics(state,&candidate)?;
    if candidate["baseCommit"]!=head
        || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
        || candidate["evaluation"]["metrics"]!=replay
        || g["evaluation"]["metrics"]!=replay {return None;}
    Some(replay)
}
/// The selected generator model is not automatically the "best" model.
/// Only independent, frozen, same-source results yield relative evidence.
pub(crate) fn plan(state:&AppState,history:&[Value],input:&Value,commit:&str)
    ->Result<Value,String>{
    let origin=base_route(input);
    let mut body=json!({
        "source":"compiled_descendant_model_lineage_policy_v1",
        "generatorCommit":commit,"historyCount":history.len(),
        "ownerOptIn":input["rsiAllowModelEvolution"]==true,
        "selected":Value::Null,"baseline":Value::Null,
        "approvedPool":[],"evidence":[],
        "matchedBaselineModelComparisons":0,
        "selectionBasis":"no_owner_model_evolution",
        "externalApiModelWeightsChanged":false,
        "independentModelSuperiorityProven":false,
        "qualityProven":false
    });
    if let Some((p,m))=&origin {
        body["baseline"]=json!({"providerId":p,"model":m});
        body["selected"]=body["baseline"].clone();
    }
    if input["rsiAllowModelEvolution"]!=true {
        body["revision"]=json!(digest(&body));return Ok(body);
    }
    let pool=allowed(state,input)?;
    body["approvedPool"]=json!(pool);
    let profile_input=&input["rsiFrozenCases"];
    let profile=if profile_input.is_null(){Value::Null}else{
        json!(format!("{:x}",Sha256::digest(serde_json::to_vec(&json!([
            profile_input,input["rsiEvaluationConfig"],input["rsiMetaCases"]
        ])).unwrap_or_default())))
    };
    body["frozenEvaluationProfileSha256"]=profile.clone();
    // Declared objective cannot win a comparison. Only a replayed candidate
    // from an actual compiled worker with the exact same source/profile counts.
    let mut results=BTreeMap::<String,(usize,usize,usize,usize,u64)>::new();
    let mut historic_trials=BTreeMap::<String,usize>::new();
    let mut used=BTreeSet::<String>::new();
    for g in history {
        let Some(p)=g["generatorProviderId"].as_str()else{continue};
        let Some(m)=g["generatorModel"].as_str()else{continue};
        if !pool.iter().any(|row|row["providerId"]==p&&row["model"]==m){
            continue;
        }
        let Some(profile_name)=profile.as_str()else{continue};
        let Some(generation_commit)=g["generatorCommit"].as_str()
            .filter(|s|sha(s,40))else{continue};
        let Some(metrics)=independent_task_receipt(
            state,g,profile_name,generation_commit)else{continue};
        let Some(id)=g["candidateId"].as_str()else{continue};
        if !used.insert(id.to_owned()){continue;}
        let key=route_key(p,m);
        *historic_trials.entry(key.clone()).or_default()+=1;
        // A changed generator is NOT the same experiment. Historical results
        // may guide exploration, but cannot claim that one API model beat
        // another on identical source.
        if generation_commit!=commit {continue;}
        let entry=results.entry(key).or_default();
        entry.0+=1;
        // Only actual held-out task outcomes are a competence signal.
        for row in metrics["measurements"].as_array().into_iter().flatten(){
            if row["holdout"]!=true{continue;}
            if row["baselinePass"]==false && row["candidatePass"]==true {
                entry.2+=1;
            }else if row["baselinePass"]==true && row["candidatePass"]==false {
                entry.3+=1;
            }
            for measure in row["metrics"].as_array().into_iter().flatten() {
                if measure["improved"]==true {entry.2+=1;}
                if measure["regressed"]==true{entry.3+=1;}
            }
        }
        if metrics["eligible"]==true && metrics["verdict"]=="improved"
            && g["accepted"]==true && g["pairedRevalidated"]==true
            && g["acceptedCommit"].as_str().is_some_and(|c|sha(c,40)){
            entry.1+=1;
        }
        entry.4+=g["tokens"].as_u64().unwrap_or(0);
    }
    let mut evidence=Vec::new();
    for row in &pool {
        let p=row["providerId"].as_str().unwrap_or("");
        let m=row["model"].as_str().unwrap_or("");
        let key=route_key(p,m);
        let historical=historic_trials.get(&key).copied().unwrap_or(0);
        let (trials,wins,gains,losses,tokens)=results
            .get(&key).copied().unwrap_or_default();
        let predictive=(wins as f64+1.0)/(trials as f64+2.0);
        let reward=(gains as f64-2.0*losses as f64)/(gains+losses+8) as f64;
        evidence.push(json!({"providerId":p,"model":m,
            "replayedSameSourceTrials":trials,
            "historicalReplayedTrialsForExplorationOnly":historical,
            "independentlyAdopted":wins,
            "frozenHoldoutGains":gains,"frozenHoldoutRegressions":losses,
            "observedTokens":tokens,
            "predictiveImprovement":predictive,
            "holdoutUtility":reward,
            "source":"same_generator_frozen_profile_replayed_receipts",
            "modelWeightsModified":false}));
    }
    let baseline=&body["baseline"];
    let baseline_trials=evidence.iter().find(|row|
        row["providerId"]==baseline["providerId"]&&row["model"]==baseline["model"])
        .and_then(|row|row["replayedSameSourceTrials"].as_u64())
        .unwrap_or(0);
    // Preserving the first owner-selected generation is essential. Only
    // subsequent attempts may spend on an alternative APPROVED model.
    let mut selected=body["baseline"].clone();
    let mut basis="owner_selected_baseline";
    if baseline_trials==0 {
        // Model choices survive code adoption even though each new source
        // commit resets direct comparison. Prior commits teach WHICH
        // approved model should be tried next, not which was superior.
        let historical_base=evidence.iter().find(|row|
            row["providerId"]==baseline["providerId"]
            && row["model"]==baseline["model"])
            .and_then(|row|row["historicalReplayedTrialsForExplorationOnly"].as_u64())
            .unwrap_or(0);
        if historical_base>0 {
            if let Some(underexplored)=evidence.iter()
                .filter(|row|row["providerId"]!=baseline["providerId"]
                    || row["model"]!=baseline["model"])
                .min_by_key(|row|row["historicalReplayedTrialsForExplorationOnly"]
                    .as_u64().unwrap_or(0)) {
                if underexplored["historicalReplayedTrialsForExplorationOnly"]
                    .as_u64().unwrap_or(0)<historical_base {
                    selected=json!({"providerId":underexplored["providerId"],
                        "model":underexplored["model"]});
                    basis="owner_approved_cross_generation_exploration_not_quality_proof";
                }
            }
        }
    }else {
        if let Some(untried)=evidence.iter().find(|row|
            row["replayedSameSourceTrials"]==0
            && (row["providerId"]!=baseline["providerId"]
                || row["model"]!=baseline["model"])) {
            selected=json!({"providerId":untried["providerId"],
                "model":untried["model"]});
            basis="explicitly_approved_cold_start_model_experiment";
        }else if evidence.iter().filter(|r|
                r["replayedSameSourceTrials"].as_u64().unwrap_or(0)>0)
                .count()>=2 {
            let baseline_quality=evidence.iter().find(|r|
                r["providerId"]==baseline["providerId"]
                && r["model"]==baseline["model"]);
            let quality=|r:&Value| {
                r["predictiveImprovement"].as_f64().unwrap_or(0.0)
                    +0.12*r["holdoutUtility"].as_f64().unwrap_or(0.0)
                    -0.02*(r["frozenHoldoutRegressions"].as_u64()
                        .unwrap_or(0) as f64)
            };
            if let Some(original)=baseline_quality{
                for row in &evidence {
                    // Only an independent measured gain warrants replacing
                    // the baseline API model on future descendant calls.
                    if row["replayedSameSourceTrials"].as_u64().unwrap_or(0)>0
                        && row["frozenHoldoutRegressions"]==0
                        && quality(row)>quality(original)+0.05 {
                        selected=json!({"providerId":row["providerId"],
                            "model":row["model"]});
                        basis="matched_source_measured_model_route_preference";
                    }
                }
            }
        }
    }
    body["selected"]=selected;
    body["evidence"]=json!(evidence);
    let observed_on_same_source=evidence.iter().filter(|row|
        row["replayedSameSourceTrials"].as_u64().unwrap_or(0)>0).count();
    body["observedModelsOnCurrentSource"]=json!(observed_on_same_source);
    body["matchedBaselineModelComparisons"]=json!(
        observed_on_same_source.saturating_sub(1));
    body["selectionBasis"]=json!(basis);
    // A winner here means "best observed generator of independently
    // evaluated code patches", NOT a newly trained API model or proof
    // of better direct conversational inference quality.
    body["revision"]=json!(digest(&body));
    Ok(body)
}
pub(crate) fn valid(policy:&Value,original:&Value,commit:&str,
    history_len:usize)->bool{
    let mut unsigned=policy.clone();
    let Some(map)=unsigned.as_object_mut()else{return false};
    map.remove("revision");
    if policy["source"]!="compiled_descendant_model_lineage_policy_v1"
        || policy["generatorCommit"]!=commit
        || policy["historyCount"].as_u64()!=Some(history_len as u64)
        || policy["revision"].as_str()!=Some(digest(&unsigned).as_str())
        || policy["externalApiModelWeightsChanged"]!=false
        || policy["independentModelSuperiorityProven"]!=false
        || policy["qualityProven"]!=false {return false;}
    let Some((p,m))=base_route(original)else{
        return original["rsiAllowModelEvolution"]!=true
            && policy["ownerOptIn"]==false
            && policy["baseline"].is_null()
            && policy["selected"].is_null();
    };
    if policy["baseline"]["providerId"]!=p
        ||policy["baseline"]["model"]!=m{return false;}
    if original["rsiAllowModelEvolution"]!=true{
        return policy["ownerOptIn"]==false && policy["selected"]==policy["baseline"];
    }
    policy["ownerOptIn"]==true
        && policy["approvedPool"].as_array().is_some_and(|pool|
            pool.len()>=2&&pool.len()<=8 && pool.iter()
                .any(|row|row==&policy["selected"]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_never_changes_owner_model(){
        let c="a".repeat(40);
        let state=make_state();
        let input=json!({"providerId":"approved","model":"api-pro",
            "rsiAllowModelEvolution":false});
        let p=plan(&state.0,&[],&input,&c).unwrap();
        assert_eq!(p["selected"],p["baseline"]);
        assert!(valid(&p,&input,&c,0));
        let mut corrupted=p.clone();corrupted["externalApiModelWeightsChanged"]=json!(true);
        assert!(!valid(&corrupted,&input,&c,0));
        let _=std::fs::remove_dir_all(state.1);
    }
    #[test]
    fn approved_pool_rejects_unapproved_remote_and_never_autospends(){
        let (state,root)=make_state();
        state.store.put("provider",&json!({"id":"approved","kind":"openai",
            "name":"Approved","baseUrl":"https://example.invalid/v1",
            "model":"api-pro","enabled":true})).unwrap();
        let input=json!({"providerId":"approved","model":"api-pro",
            "rsiAllowModelEvolution":true,"rsiAutonomous":true,
            "rsiSystemEvolution":true,
            "rsiApprovedModelPool":[
                {"providerId":"approved","model":"api-pro"},
                {"providerId":"unregistered","model":"api-new"}]});
        assert!(plan(&state,&[],&input,&"a".repeat(40)).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    fn make_state()->(AppState,std::path::PathBuf){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(
            format!("rsi-model-lineage-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        (state,root)
    }
}
