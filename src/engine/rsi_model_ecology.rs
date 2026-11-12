//! Model-agnostic self-improvement actor ecology.
//!
//! External APIs are immutable teachers/actors: we evolve the agent around
//! them and select real inference routes by subsequent source-adoption
//! outcomes. Local learned weights are a *distinct* line, and cannot
//! inherit API superiority from a LoRA-only token-loss comparison.
//!
//! Ranking is an exploration policy, not a causal superiority claim. An
//! accepted edit remains judged only by the frozen source-code evaluator.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn sha(v:&Value,n:usize)->bool{
    v.as_str().is_some_and(|s|s.len()==n&&s.bytes().all(|b|b.is_ascii_hexdigit()))
}
fn same_route(a:&Value,b:&Value)->bool{
    a["providerId"].as_str()==b["providerId"].as_str()
        &&a["model"].as_str()==b["model"].as_str()
        &&a["providerId"].is_string()
        &&a["model"].is_string()
}
fn identity(provider:&str,model:&str)->Value{
    json!({"providerId":provider,"model":model})
}
fn valid_route(state:&AppState,route:&Value)->bool{
    let (Some(provider),Some(model))=(
        route["providerId"].as_str(),route["model"].as_str())
        else{return false};
    if provider.is_empty()||model.is_empty()||provider.starts_with("rsi-adapter-"){
        return false;
    }
    let Some(config)=state.store.get("provider",provider) else{return false};
    if !crate::engine::gateway::provider_eligible(&config)
        ||config["capabilities"]["tools"]==false{
        return false;
    }
    crate::engine::gateway::route_from_provider(state,&config,Some(model)).is_ok()
}
/// Owner-selected base route remains a candidate even if the backend
/// provider exposes further registered model IDs.
fn candidates(state:&AppState,input:&Value)->Vec<Value>{
    let owner=identity(input["providerId"].as_str().unwrap_or(""),
        input["model"].as_str().unwrap_or(""));
    let allowed=input["rsiModelProviderAllowlist"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).collect::<BTreeSet<_>>();
    let mut routes=Vec::<Value>::new();
    if valid_route(state,&owner){routes.push(owner);}
    for provider in state.store.all("provider"){
        if provider["enabled"]==false || provider["capabilities"]["tools"]==false{
            continue;
        }
        let Some(id)=provider["id"].as_str() else{continue};
        if !allowed.is_empty() && !allowed.contains(id) {continue;}
        let mut choices=Vec::new();
        if let Some(model)=provider["model"].as_str(){choices.push(model.to_string());}
        for row in provider["models"].as_array().into_iter().flatten().take(20){
            if let Some(model)=row.as_str()
                .or_else(||row["id"].as_str()){
                choices.push(model.to_string());
            }
        }
        for name in choices.into_iter().take(12){
            let route=identity(id,&name);
            if valid_route(state,&route)&&!routes.iter().any(|v|same_route(v,&route)){
                routes.push(route);
            }
        }
    }
    routes.truncate(20);
    routes
}
fn verified_attempt(g:&Value)->bool{
    g["modelEcologyExecuted"]["actuallyUsedInGeneration"]==true
        &&g["modelEcologyExecuted"]["actualModelCalls"].as_u64().unwrap_or(0)>0
        &&g["modelEcologyExecuted"]["source"]=="real_descendant_model_usage_v1"
        &&g["candidateOrigin"]=="model_driven_detached_worktree"
        &&g["generatorCommit"].as_str().is_some_and(|s|s.len()==40)
}
fn outcome(g:&Value)->Option<f64>{
    if !verified_attempt(g) ||g["pairedMeasured"]!=true
        ||g["evaluation"]["metrics"]["measurements"].as_array().is_none(){
        return None;
    }
    let accepted=g["accepted"]==true &&g["pairedRevalidated"]==true
        &&sha(&g["acceptedCommit"],40)
        &&g["evaluation"]["metrics"]["eligible"]==true;
    let metrics=&g["evaluation"]["metrics"];
    let regress=metrics["regressions"].as_u64().unwrap_or(0).min(10) as f64;
    let gains=metrics["improvedHoldouts"].as_u64().unwrap_or(0).min(12) as f64;
    // A successful edit, not model prose or token-loss, is the dominant
    // reward. Regressions lower measured task performance across domains.
    Some((if accepted{1.0+0.05*gains}else{0.0})-0.08*regress)
}
fn statistics(history:&[Value],route:&Value,domain:&str)->Value{
    let mut attempts=0u64;
    let mut measured=0u64;
    let mut adoptions=0u64;
    let mut global=0.0f64;
    let mut same_domain=0.0f64;
    let mut local_n=0u64;
    let mut domains=BTreeMap::<String,u64>::new();
    let mut matched=Vec::new();
    for g in history{
        if !verified_attempt(g)||!same_route(&g["modelEcologyExecuted"],route){
            continue;
        }
        attempts+=1;
        if let Some(reward)=outcome(g){
            measured+=1;
            global+=reward;
            let d=g["domain"].as_str().unwrap_or("unknown").to_string();
            *domains.entry(d.clone()).or_insert(0)+=1;
            if g["accepted"]==true &&g["pairedRevalidated"]==true {adoptions+=1;}
            if d==domain {local_n+=1;same_domain+=reward;}
            if matched.len()<12 {
                matched.push(json!({
                    "generationJobId":g["jobId"],
                    "parentCommit":g["generatorCommit"],
                    "adoptedCommit":if g["accepted"]==true
                        &&g["pairedRevalidated"]==true {g["acceptedCommit"].clone()}
                        else{Value::Null},
                    "domain":d,"measured":true,
                    "independentOutcome":reward
                }));
            }
        }
    }
    let global_mean=if measured==0{0.0}else{global/measured as f64};
    let local_mean=if local_n==0{global_mean}else{same_domain/local_n as f64};
    // Sparse, domain-conditioned Bayesian smoothing. No success is
    // awarded for an unmeasured or unexecuted model route.
    let posterior=(adoptions as f64+1.0)/(measured as f64+2.0);
    let score=0.5*posterior+0.30*local_mean.clamp(-1.0,1.5)
        +0.15*global_mean.clamp(-1.0,1.5)
        +0.32/(attempts as f64+1.0).sqrt();
    json!({"attempts":attempts,"independentlyMeasuredTrials":measured,
        "sourceAdoptions":adoptions,"domainTrials":local_n,
        "globalObservedMeanReward":global_mean,
        "domainObservedMeanReward":local_mean,
        "domainEvidence":domains,"rankScore":score,
        "matchedOutcomeReceipts":matched,
        "modelWeightsModified":false,
        "apiProviderImprovementClaimed":false,
        "crossModelCausalAdvantageProven":false})
}
pub(crate) fn plan(
    state:&AppState,history:&[Value],input:&Value,domain:&str,revision:&str
)->Value{
    let owner=identity(input["providerId"].as_str().unwrap_or(""),
        input["model"].as_str().unwrap_or(""));
    let enabled=input["rsiModelEvolution"]==true
        &&input["selectionPolicy"]=="automatic"
        &&input["rsiSystemEvolution"]==true;
    let routes=if enabled {candidates(state,input)} else {Vec::new()};
    let current=routes.first().cloned().unwrap_or_else(||owner.clone());
    let mut scores=routes.iter().map(|route|{
        let evidence=statistics(history,route,domain);
        (route.clone(),evidence)
    }).collect::<Vec<_>>();
    // New model choices get actual independent trials, rather than a
    // simulated supremacy declaration based on parameter count.
    let mut explore=scores.iter().enumerate()
        .filter(|(_,(_,stats))|stats["attempts"].as_u64().unwrap_or(0)<2)
        .map(|(i,_)|i).collect::<Vec<_>>();
    explore.sort_by(|a,b|scores[*a].1["attempts"].as_u64()
        .cmp(&scores[*b].1["attempts"].as_u64())
        .then(a.cmp(b)));
    let index=if let Some(&first)=explore.first(){Some(first)}
    else{scores.iter().enumerate().max_by(|(_,a),(_,b)|
        a.1["rankScore"].as_f64().unwrap_or(0.0)
            .total_cmp(&b.1["rankScore"].as_f64().unwrap_or(0.0)))
        .map(|(i,_)|i)};
    let selected=index.map(|i|scores[i].0.clone())
        .unwrap_or_else(||current.clone());
    let source=if enabled &&!scores.is_empty(){
        "compiled_descendant_model_actor_ecology_v1"
    }else{"owner_fixed_model_no_route_change"};
    let evidence=json!({
        "source":source,
        "generatorCommit":revision,
        "targetDomain":domain,
        "active":enabled&&!scores.is_empty(),
        "ownerBaseline":owner,
        "selected":selected,
        "selectedBy":"real_child_outcomes_exploration_and_transfer",
        "alternatives":scores.iter().map(|(route,stat)|json!({
            "route":route,"evidence":stat
        })).collect::<Vec<_>>(),
        "candidateCount":scores.len(),
        "realChildInferenceRequired":true,
        "independentSourceAdoptionRequiredForCredit":true,
        "modelWeightChanged":false,
        "apiWeightsModified":false,
        "sourcePolicyRevision":revision,
        "conclusion":"A comparison policy, not proof any model is globally superior."
    });
    let mut policy=evidence.clone();
    policy["revision"]=json!(format!("{:x}",
        Sha256::digest(serde_json::to_vec(&evidence).unwrap_or_default())));
    policy
}
pub(crate) fn validate(
    state:&AppState,policy:&Value,input:&Value,domain:&str,revision:&str
)->Result<(),String>{
    if policy["sourcePolicyRevision"]!=revision
        ||policy["generatorCommit"]!=revision
        ||policy["targetDomain"]!=domain{
        return Err("모델 선택 정책이 이 후손의 실제 작업·소스 revision과 다릅니다.".into());
    }
    if !sha(&policy["revision"],64) ||policy["modelWeightChanged"]!=false
        ||policy["apiWeightsModified"]!=false{
        return Err("외부 모델의 가중치 수정 또는 선택 근거가 허위입니다.".into());
    }
    let selected=&policy["selected"];
    if policy["active"]==true {
        if input["rsiModelEvolution"]!=true
            ||input["selectionPolicy"]!="automatic"
            ||policy["source"]!="compiled_descendant_model_actor_ecology_v1"
            ||!valid_route(state,selected) {
            return Err("자동 모델 변경 권한이나 등록된 API 제공자 조건 불일치".into());
        }
        let available=candidates(state,input);
        if !available.iter().any(|v|same_route(v,selected)){
            return Err("검증된 모델 후보 목록 밖의 제공자입니다.".into());
        }
    }else if !same_route(selected,&identity(
        input["providerId"].as_str().unwrap_or(""),
        input["model"].as_str().unwrap_or(""))){
        return Err("사용자 지정 고정 모델이 임의로 변경됐습니다.".into());
    }
    Ok(())
}
pub(crate) fn executed(state:&AppState,job:&Value)->Value{
    let input=&job["input"];
    let policy=&input["rsiModelEcologyPolicy"];
    if policy["active"]!=true{return Value::Null;}
    let id=job["id"].as_str().unwrap_or("");
    let session=job["sessionId"].as_str().unwrap_or("");
    let route=&policy["selected"];
    let provider=route["providerId"].as_str().unwrap_or("");
    let model=route["model"].as_str().unwrap_or("");
    let calls=state.store.recent_audit(800).into_iter().filter(|row|
        row["event"]=="model.usage"
            &&row["detail"]["providerId"]==provider
            &&row["detail"]["model"]==model
            &&row["detail"]["sessionId"]==session
            &&row["detail"]["totalTokens"].as_u64().unwrap_or(0)>0
        ).count();
    json!({
        "source":"real_descendant_model_usage_v1",
        "generationJobId":id,
        "generatorCommit":input["rsiGeneratorCommit"],
        "providerId":provider,"model":model,
        "actualModelCalls":calls,
        "actuallyUsedInGeneration":calls>0,
        "selectedBy":policy["revision"],
        "apiProviderWeightsChanged":false,
        "independentSourcePerformanceNotYetKnown":true
    })
}
pub(crate) fn instruction(policy:&Value)->String{
    let selected=&policy["selected"];
    let summary=policy["alternatives"].as_array().into_iter().flatten()
        .take(10).map(|v|format!("{} / {}: outcomes={} adopted={} domains={}",
            v["route"]["providerId"],v["route"]["model"],
            v["evidence"]["independentlyMeasuredTrials"],
            v["evidence"]["sourceAdoptions"],
            v["evidence"]["domainEvidence"]
        )).collect::<Vec<_>>().join("\n");
    format!("[MODEL-AGNOSTIC RSI ACTOR ECOLOGY]\n\
        Actual selected external/local provider={} model={}.\
        This model is your tool for improving the DOMINION agent, not \
        a claim that its API weights were modified. Verified source-edit \
        observations by real model actor:\n{summary}\n\
        Evolve executable memory, learning, tools, inference routing, and \
        the improvement program as a single system. Record failed experiments \
        and transfer the causal hypotheses to the next compiled child. \
        Never infer whole-agent ability from LoRA perplexity alone.",
        selected["providerId"],selected["model"])
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn execution_requires_real_model_traffic(){
        let g=json!({"modelEcologyExecuted":{"source":"real_descendant_model_usage_v1",
            "providerId":"api-1","model":"model-a",
            "actuallyUsedInGeneration":true,"actualModelCalls":0},
            "generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"evaluation":{"metrics":{"measurements":[]}}});
        assert!(outcome(&g).is_none());
    }
    #[test] fn independent_source_receipt_not_token_loss_is_reward(){
        let mut g=json!({"modelEcologyExecuted":{"source":"real_descendant_model_usage_v1",
            "providerId":"api-1","model":"model-a",
            "actuallyUsedInGeneration":true,"actualModelCalls":1},
            "generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"accepted":true,"pairedRevalidated":true,
            "acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"eligible":true,
                "measurements":[{"id":"holdout"}],
                "improvedHoldouts":2}}});
        assert!(outcome(&g).unwrap()>1.0);
        g["pairedMeasured"]=json!(false);
        assert!(outcome(&g).is_none());
    }
    #[test] fn selection_does_not_credit_unmeasured_models(){
        let route=identity("api-1","model-a");
        let history=vec![json!({"modelEcologyExecuted":{"source":
            "real_descendant_model_usage_v1","providerId":"api-1",
            "model":"model-a","actuallyUsedInGeneration":false,
            "actualModelCalls":0}})];
        let stats=statistics(&history,&route,"memory");
        assert_eq!(stats["independentlyMeasuredTrials"],0);
    }
    #[test] fn records_models_as_strata_not_parameter_updates(){
        assert!(!same_route(&identity("p","m1"),&identity("p","m2")));
        assert!(!same_route(&identity("p1","m"),&identity("p2","m")));
    }
    #[test] fn a_model_route_is_not_parameter_update_evidence(){
        let route=identity("api-provider","gpt-external");
        assert_eq!(route["providerId"],"api-provider");
        assert_eq!(route["model"],"gpt-external");
        assert!(route.get("weightsUpdated").is_none());
    }
}
