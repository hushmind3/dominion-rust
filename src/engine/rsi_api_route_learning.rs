//! Compiled-descendant learning of authorized EXTERNAL API execution routes.
//!
//! DeepSeek/OpenAI/etc. weights are owned by the provider and NEVER trained
//! here. The editable intelligence is DOMINION's model-selection algorithm,
//! its system prompts/tools, memory and source editing methods. Empirical
//! quality is based on sealed task cases and REAL API usage, not a patch count.
//! Source-edit outcomes are observational proxies for coder performance,
//! NOT a controlled claim that one provider's underlying weights improved.
use crate::{engine::gateway,state::AppState};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::collections::{BTreeMap,BTreeSet};

fn seal(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
fn name(v:&Value)->Option<&str> {
    v.as_str().filter(|s|!s.is_empty() && s.len()<=200)
}
fn pair(route:&Value)->Option<(&str,&str)> {
    Some((name(&route["providerId"])?,name(&route["model"])?))
}
/// This is a SOURCE CODE discovery policy, never a model or pricing oracle.
/// Only pre-registered providers and their owner-advertised model IDs may
/// appear in a multi-provider campaign. The original owner selection is
/// always permitted and never silently replaced without opt-in.
pub(crate) fn authorized(state:&AppState,origin:&Value,
    proposals:&Value,local_only:bool)->Result<Value,String> {
    let (original_provider,original_model)=pair(origin)
        .ok_or("모델 선택의 원본 제공자와 모델이 없습니다.")?;
    let mut accepted=vec![json!({
        "providerId":original_provider,"model":original_model})];
    if proposals.is_null(){return Ok(json!(accepted));}
    let choices=proposals.as_array()
        .filter(|v|v.len()<=8)
        .ok_or("승인된 API 모델 후보는 최대 8개입니다.")?;
    let mut seen=BTreeSet::new();
    seen.insert((original_provider.to_owned(),original_model.to_owned()));
    for entry in choices {
        let (provider_id,model)=pair(entry)
            .ok_or("승인할 API 후보의 제공자와 모델 ID가 잘못됐습니다.")?;
        let config=state.store.get("provider",provider_id)
            .ok_or("등록되지 않은 API 모델 제공자입니다.")?;
        if !gateway::provider_eligible(&config)
            || (local_only && !gateway::is_local_provider(&config)) {
            return Err("비활성화되었거나 허용되지 않은 모델 제공자입니다.".into());
        }
        // The owner must have registered the exact model ID already;
        // learned policies cannot hallucinate vendor models or API access.
        let exact=config["model"].as_str()==Some(model)
            || config["rsiApprovedModels"].as_array().is_some_and(|models|
                models.iter().any(|m|m.as_str()==Some(model)));
        if !exact {return Err("제공자에 등록되지 않은 모델 ID는 진화 후보가 아닙니다.".into());}
        gateway::route_from_provider(state,&config,Some(model))?;
        if seen.insert((provider_id.to_string(),model.to_string())) {
            accepted.push(json!({"providerId":provider_id,"model":model}));
        }
    }
    Ok(json!(accepted))
}
fn attested(g:&Value)->bool {
    // An agent/model must have run: local native source mutations provide
    // zero evidence that any requested external API route was productive.
    g["apiRouteExecutionVerified"]==true
        && g["generatorProviderId"]==g["requestedProviderId"]
        && g["generatorModel"]==g["requestedModel"]
        && name(&g["requestedProviderId"]).is_some()
        && name(&g["requestedModel"]).is_some()
        && g["apiRouteSessionId"].as_str().is_some_and(|s|!s.is_empty())
        && g["jobId"].as_str().is_some_and(|s|!s.is_empty())
        && g["apiRouteSessionExclusive"]==true
        && g["apiRouteUsage"]["matchedExactProviderModelSession"]==true
        && g["apiRouteUsage"]["calls"].as_u64().unwrap_or(0)>0
        && g["apiRouteUsage"]["totalTokens"].as_u64().unwrap_or(0)>0
        && g["candidateOrigin"]!="native_inductive_source_mutation"
}
fn measured(g:&Value)->bool {
    attested(g)
        && (g["pairedMeasured"]==true||g["pairedRevalidated"]==true)
        && g["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && g["evaluation"]["metrics"]["measurements"].is_array()
        && g["candidateId"].as_str().is_some_and(|s|!s.is_empty())
        && g["evaluationProfileSha256"].as_str().is_some_and(|s|
            s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit()))
}
/// Replay original worker identity, provider/model audit and candidate-scoped
/// frozen paired receipt before an editable descendant rewards API work.
/// Generation text alone is never authoritative.
pub(crate) fn verify_replay(state:&AppState,history:&[Value])->bool {
    for g in history.iter().filter(|g|attested(g)) {
        let (Some(job_id),Some(session),Some(provider),Some(model))=(
            g["jobId"].as_str(),g["apiRouteSessionId"].as_str(),
            g["requestedProviderId"].as_str(),g["requestedModel"].as_str())
            else{return false};
        let Some(job)=state.store.get("job",job_id) else{return false};
        if job["sessionId"]!=session
            || job["input"]["providerId"]!=provider
            || job["input"]["model"]!=model
            || job["input"]["rsiAdaptiveApiRouting"]!=true
            || job["input"]["rsiApiRoutePolicy"]["chosen"]["providerId"]!=provider
            || job["input"]["rsiApiRoutePolicy"]["chosen"]["model"]!=model
            || state.store.api_route_usage(session,provider,model)!=g["apiRouteUsage"] {
            return false;
        }
        // A tool/research side call through another model makes route credit
        // ambiguous; one worker session must have exactly one executed route.
        let Ok(used_routes)=state.store.session_model_routes(session) else{return false};
        if used_routes.len()!=1
            || used_routes[0]["providerId"]!=provider
            || used_routes[0]["model"]!=model {return false;}
        if !measured(g){continue;}
        let Some(id)=g["candidateId"].as_str() else{return false};
        let Some(candidate)=state.store.get("rsi_candidate",id) else{return false};
        let Some(metrics)=crate::engine::rsi::replayed_pair_metrics(state,&candidate)
            else{return false};
        if candidate["id"]!=id
            || candidate["baseCommit"]!=g["generatorCommit"]
            || candidate["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"
            || candidate["evaluation"]["metrics"]!=metrics
            || g["evaluation"]["metrics"]!=metrics {return false;}
        if g["accepted"]==true
            && (g["pairedRevalidated"]!=true
                || !crate::engine::rsi::verified_pair_receipt(state,&candidate)
                || candidate["status"]!="verified"
                || candidate["experimentAdoption"]["commit"]!=g["acceptedCommit"]
                || candidate["experimentAdoption"]["generatorCommit"]!=g["generatorCommit"]) {
            return false;
        }
    }
    true
}
fn outcome(case:&Value)->Option<i8>{
    if case["holdout"]!=true{return None;}
    for metric in case["metrics"].as_array().into_iter().flatten(){
        if metric["key"]!="quality" {continue;}
        if metric["improved"]==true{return Some(1)}
        if metric["regressed"]==true{return Some(-1)}
        if metric["improved"]==false && metric["regressed"]==false {
            return Some(0);
        }
        let before=metric["baseline"].as_f64()?;
        let after=metric["candidate"].as_f64()?;
        return Some(if after>before{1}else if after<before{-1}else{0});
    }
    match (case["baselinePass"].as_bool(),case["candidatePass"].as_bool()){
        (Some(false),Some(true))=>Some(1),
        (Some(true),Some(false))=>Some(-1),
        (Some(_),Some(_))=>Some(0),
        _=>None
    }
}
#[derive(Default)]
struct Evidence {
    trials:u64,holdouts:u64,gains:u64,regressions:u64,neutral:u64,
    adopted:u64,tokens:u64,domains:BTreeSet<String>,
    cases:BTreeSet<String>,sessions:BTreeSet<String>,bad_predictions:Vec<f64>,
}
fn key(route:&Value)->Option<String>{
    let (id,model)=pair(route)?;
    Some(format!("{id}\u{001f}{model}"))
}
/// Same compiled source + sealed evaluator + holdout case + baseline only.
/// Different model-authored edits remain confounded, so these are observational
/// matched comparisons, never intrinsic model-causality claims.
fn matched_source_cases(history:&[Value],routes:&Value)->Value {
    type CaseKey=(String,String,String,String,bool);
    type Observations=BTreeMap<String,Vec<(String,i8)>>;
    let approved=routes.as_array().cloned().unwrap_or_default();
    let owner=approved.first().and_then(key).unwrap_or_default();
    let route_keys=approved.iter().filter_map(key).collect::<BTreeSet<_>>();
    let mut cases=BTreeMap::<CaseKey,Observations>::new();
    let mut seen=BTreeSet::<(String,String,String,String)>::new();
    for g in history.iter().filter(|g|measured(g)) {
        let run=json!({"providerId":g["requestedProviderId"],"model":g["requestedModel"]});
        let Some(route)=key(&run) else{continue};
        if !route_keys.contains(&route){continue;}
        let (Some(commit),Some(profile),Some(candidate))=(
            g["generatorCommit"].as_str().filter(|s|s.len()==40
                && s.bytes().all(|b|b.is_ascii_hexdigit())),
            g["evaluationProfileSha256"].as_str(),
            g["candidateId"].as_str()) else{continue};
        for m in g["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten() {
            let (Some(id),Some(domain),Some(baseline),Some(delta))=(
                m["id"].as_str().filter(|s|!s.is_empty()),
                m["domain"].as_str().filter(|s|!s.is_empty()),
                m["baselinePass"].as_bool(),outcome(m)) else{continue};
            if !seen.insert((profile.to_owned(),candidate.to_owned(),
                domain.to_owned(),id.to_owned())){continue;}
            cases.entry((commit.to_owned(),profile.to_owned(),
                domain.to_owned(),id.to_owned(),baseline)).or_default()
                .entry(route.clone()).or_default().push((candidate.to_owned(),delta));
        }
    }
    let mut result=Vec::new();
    for route in approved.iter().skip(1) {
        let Some(route_id)=key(route) else{continue};
        let mut deltas=Vec::<f64>::new();
        let mut owner_trials=BTreeSet::new();
        let mut route_trials=BTreeSet::new();
        let mut domains=BTreeSet::new();
        for ((_,_,domain,_,_),observed) in &cases {
            let (Some(left),Some(right))=
                (observed.get(&owner),observed.get(&route_id)) else{continue};
            let avg=|rows:&Vec<(String,i8)>|rows.iter()
                .map(|(_,v)|*v as f64).sum::<f64>()/rows.len() as f64;
            deltas.push(avg(right)-avg(left));
            domains.insert(domain.clone());
            owner_trials.extend(left.iter().map(|(id,_)|id.clone()));
            route_trials.extend(right.iter().map(|(id,_)|id.clone()));
        }
        result.push(json!({
            "providerId":route["providerId"],"model":route["model"],
            "matchedFrozenSourceCases":deltas.len(),
            "independentOwnerCandidates":owner_trials.len(),
            "independentRouteCandidates":route_trials.len(),
            "matchedDomains":domains.into_iter().collect::<Vec<_>>(),
            "meanRelativeHoldoutDelta":if deltas.is_empty(){Value::Null}
                else{json!(deltas.iter().sum::<f64>()/deltas.len() as f64)},
            "evidenceClass":"same_source_frozen_case_observational_not_causal_AB",
            "modelQualityCausalityProven":false
        }));
    }
    json!(result)
}
/// Evaluation provenance is verified by the resident coordinator before
/// this descendant sees history. Evidence is grouped by frozen profile,
/// independent candidate, and case ID; repeats cannot multiply gains.
pub(crate) fn learn(history:&[Value],routes:&Value)->Value{
    let approved=routes.as_array().cloned().unwrap_or_default();
    let mut data=BTreeMap::<String,Evidence>::new();
    for route in &approved {
        if let Some(k)=key(route){data.entry(k).or_default();}
    }
    for g in history {
        let run=json!({"providerId":g["requestedProviderId"],"model":g["requestedModel"]});
        let Some(k)=key(&run) else{continue};
        let Some(row)=data.get_mut(&k) else{continue};
        if !attested(g){continue;}
        let session=g["apiRouteSessionId"].as_str().unwrap_or("");
        if !row.sessions.insert(session.to_owned()){continue;}
        row.trials+=1;
        row.tokens+=g["apiRouteUsage"]["totalTokens"].as_u64().unwrap_or(0);
        if !measured(g){continue;}
        let profile=g["evaluationProfileSha256"].as_str().unwrap_or("");
        let candidate=g["candidateId"].as_str().unwrap_or("");
        let mut measured_this=false;
        for case in g["evaluation"]["metrics"]["measurements"].as_array()
            .into_iter().flatten() {
            let Some(id)=case["id"].as_str() else{continue};
            let domain=case["domain"].as_str().unwrap_or("unknown");
            let Some(change)=outcome(case) else{continue};
            let receipt=format!("{profile}:{candidate}:{domain}:{id}");
            if !row.cases.insert(receipt){continue;}
            measured_this=true;
            row.holdouts+=1;
            row.domains.insert(domain.to_owned());
            match change{1=>row.gains+=1,-1=>row.regressions+=1,_=>row.neutral+=1}
        }
        if measured_this && g["accepted"]==true
            && g["pairedRevalidated"]==true
            && g["evaluation"]["metrics"]["eligible"]==true {
            row.adopted+=1;
        }
        if let Some(p)=g["apiModelPolicy"]["forecastCaseGain"].as_f64()
            .filter(|x|x.is_finite()&&(0.0..=1.0).contains(x)) {
            for case in g["evaluation"]["metrics"]["measurements"].as_array()
                .into_iter().flatten() {
                if let Some(delta)=outcome(case){
                    row.bad_predictions.push((p-if delta>0{1.0}else{0.0}).powi(2));
                }
            }
        }
    }
    let rows=approved.iter().filter_map(|route|{
        let k=key(route)?;
        let stat=data.get(&k)?;
        let posterior=(1.0+stat.gains as f64)/(2.0+stat.holdouts as f64);
        let brier=if stat.bad_predictions.is_empty(){Value::Null}
            else{json!(stat.bad_predictions.iter().sum::<f64>()
                /stat.bad_predictions.len() as f64)};
        Some(json!({
            "providerId":route["providerId"],"model":route["model"],
            "verifiedApiWorkerCalls":stat.trials,
            "independentlyMeasuredCases":stat.holdouts,
            "improvedCases":stat.gains,"regressedCases":stat.regressions,
            "neutralCases":stat.neutral,"adoptedSourceTrials":stat.adopted,
            "measuredTokens":stat.tokens,
            "observedDomains":stat.domains.iter().cloned().collect::<Vec<_>>(),
            "posteriorCaseGain":posterior,
            "meanPredictionBrier":brier,
            "proxyCannotIsolateModelFromSourceChange":true
        }))
    }).collect::<Vec<_>>();
    let mut result=json!({
        "source":"compiled_descendant_authorized_api_execution_learning_v1",
        "routes":rows,"observations":history.len(),
        "matchedSourceCaseContrasts":matched_source_cases(history,routes),
        "weightsWereTrained":false,"modelQualityCausalityProven":false,
        "evidence":"real_provider_session_usage_plus_independent_source_task_holdouts",
        "editableAlgorithmSource":"src/engine/rsi_api_route_learning.rs"
    });
    result["sha256"]=json!(seal(&result));
    result
}
/// Compiled child uses the observed task outcome to choose which *approved*
/// API coder route will write the next successor. Original user route is the
/// safe fallback if no comparison is available. All search coefficients are
/// editable Rust for the subsequent descendant to reinvent.
pub(crate) fn decide(history:&[Value],routes:&Value,origin:&Value,
    commit:&str)->Value {
    let ledger=learn(history,routes);
    let original=routes.as_array().into_iter().flatten().find(|r|
        r["providerId"]==origin["providerId"]&&r["model"]==origin["model"]);
    let Some(base)=original else{return Value::Null};
    let mut choices=ledger["routes"].as_array().cloned().unwrap_or_default();
    choices.sort_by(|a,b|{
        let score=|r:&Value|{
            let observed=r["independentlyMeasuredCases"].as_u64().unwrap_or(0);
            let gain=r["posteriorCaseGain"].as_f64().unwrap_or(0.5);
            let uncertainty=0.12/((observed+1) as f64).sqrt();
            let regression=r["regressedCases"].as_u64().unwrap_or(0) as f64
                /((observed+1) as f64);
            gain+uncertainty-0.20*regression
        };
        score(b).total_cmp(&score(a))
            .then_with(||a["providerId"].as_str().cmp(&b["providerId"].as_str()))
            .then_with(||a["model"].as_str().cmp(&b["model"].as_str()))
    });
    let baseline_samples=choices.iter().find(|r|
        r["providerId"]==base["providerId"]&&r["model"]==base["model"])
        .and_then(|r|r["independentlyMeasuredCases"].as_u64()).unwrap_or(0);
    // A controlled exploration trial becomes eligible only after at least
    // two independently measured baseline tasks. No random API spending:
    // all routes were registered and explicitly authorized by the owner.
    let contrasts=ledger["matchedSourceCaseContrasts"].as_array()
        .cloned().unwrap_or_default();
    let mature=|r:&Value|r["matchedFrozenSourceCases"].as_u64().unwrap_or(0)>=2
        && r["independentOwnerCandidates"].as_u64().unwrap_or(0)>=2
        && r["independentRouteCandidates"].as_u64().unwrap_or(0)>=2;
    let mut favorable=contrasts.iter().filter(|r|mature(r))
        .filter(|r|r["meanRelativeHoldoutDelta"].as_f64().unwrap_or(0.0)>0.0)
        .collect::<Vec<_>>();
    favorable.sort_by(|a,b|b["meanRelativeHoldoutDelta"].as_f64()
        .unwrap_or(0.0).total_cmp(&a["meanRelativeHoldoutDelta"].as_f64().unwrap_or(0.0)));
    let matched_best=favorable.first().and_then(|r|choices.iter().find(|c|
        c["providerId"]==r["providerId"]&&c["model"]==r["model"]));
    let trial=if baseline_samples>=2 && matched_best.is_none(){
        choices.iter().find(|r|r["independentlyMeasuredCases"]==0)
    }else{None};
    let disfavored=|r:&Value|contrasts.iter().any(|c|
        mature(c)&&c["providerId"]==r["providerId"]&&c["model"]==r["model"]
            && c["meanRelativeHoldoutDelta"].as_f64().unwrap_or(0.0)<0.0);
    let choice=matched_best.or(trial).or_else(||choices.iter().find(|r|
        r["independentlyMeasuredCases"].as_u64().unwrap_or(0)>0
            && !disfavored(r))).unwrap_or(base);
    let mut result=json!({
        "source":"source_editable_descendant_api_route_policy_v1",
        "generatorCommit":commit,"authorizedRoutes":routes,
        "ownerOriginal":origin,"chosen":{"providerId":choice["providerId"],
            "model":choice["model"]},
        "forecastCaseGain":choice["posteriorCaseGain"],
        "ledgerSha256":ledger["sha256"],
        "observedTaskCases":choice["independentlyMeasuredCases"],
        "matchedEvidence":contrasts.iter().find(|r|
            r["providerId"]==choice["providerId"]&&r["model"]==choice["model"])
            .cloned().unwrap_or(Value::Null),
        "explanation":if matched_best.is_some(){"matched_source_case_observational_advantage"}
            else if trial.is_some(){"owner_approved_bounded_exploration_trial"}
            else if baseline_samples>0{"observed_source_task_quality_not_causal_model_AB"}
            else{"owner_selected_model_until_measured_api_route_evidence"},
        "modelWeightsChanged":false
    });
    result["revision"]=json!(seal(&result));
    result
}
pub(crate) fn valid(plan:&Value,history:&[Value],routes:&Value,origin:&Value,
    commit:&str)->bool{
    plan==&decide(history,routes,origin,commit)
}
pub(crate) fn approved_choice(plan:&Value,routes:&Value)->bool{
    let chosen=&plan["chosen"];
    if pair(chosen).is_none()||plan["authorizedRoutes"]!=*routes{return false;}
    let mut copy=plan.clone();
    if let Some(body)=copy.as_object_mut(){body.remove("revision");}
    plan["revision"]==seal(&copy) && routes.as_array().is_some_and(|rows|
        rows.iter().any(|row|row==chosen))
}
pub(crate) fn instruction(policy:&Value)->String {
    if policy.is_null(){return String::new();}
    format!("API-EXECUTED INTELLIGENCE: selected authorized provider={} model={}, \
        based on {} measured source-task cases (forecast {}). \
        Same-source frozen-case comparisons: {}, relative case delta: {}. \
        These results are observational and NOT intrinsic model A/B. \
        The API model's WEIGHTS are NOT trainable or modified. \
        Improve your own provider selection, tool invocation, search, memory, \
        context management and learning policies as EDITABLE Rust source. \
        Independent source-task results are only observational evidence about \
        the coder route; they do not prove intrinsic superiority between API models. \
        Revise src/engine/rsi_api_route_learning.rs when this policy is poorly \
        calibrated, and let the rebuilt descendant make a new source-grounded choice.",
        policy["chosen"]["providerId"],policy["chosen"]["model"],
        policy["observedTaskCases"],policy["forecastCaseGain"],
        policy["matchedEvidence"]["matchedFrozenSourceCases"],
        policy["matchedEvidence"]["meanRelativeHoldoutDelta"])
}
#[cfg(test)]
mod tests {
    use super::*;
    fn routes()->Value {json!([
        {"providerId":"deepseek","model":"api-a"},
        {"providerId":"deepseek","model":"api-b"}])}
    fn episode(id:&str,model:&str,before:bool,after:bool)->Value{
        json!({"id":id,"candidateId":format!("case-{id}"),
            "generatorProviderId":"deepseek","requestedProviderId":"deepseek",
            "generatorModel":model,"requestedModel":model,
            "apiRouteExecutionVerified":true,
            "apiRouteUsage":{"matchedExactProviderModelSession":true,
                "calls":1,"totalTokens":450},
            "tokenUsage":{"calls":1,"totalTokens":450},
            "generatorCommit":"a".repeat(40),
            "jobId":format!("worker-{id}"),
            "apiRouteSessionId":format!("session-{id}"),
            "apiRouteSessionExclusive":true,
            "evaluationProfileSha256":"e".repeat(64),
            "pairedMeasured":true,"pairedRevalidated":true,"accepted":true,
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":true,"measurements":[
                    {"id":"holdout-1","domain":"memory","holdout":true,
                        "baselinePass":before,"candidatePass":after}]}}})
    }
    #[test]
    fn only_pre_registered_owner_approved_api_models_can_be_selected(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-api-rsi-{}",
            uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("provider",&json!({
            "id":"deepseek","baseUrl":"https://api.deepseek.com/v1",
            "name":"DeepSeek","kind":"openai","enabled":true,
            "model":"deepseek-flash",
            "rsiApprovedModels":["deepseek-flash","deepseek-v4-pro"]
        })).unwrap();
        let original=json!({"providerId":"deepseek","model":"deepseek-flash"});
        let approved=authorized(&state,&original,
            &json!([{"providerId":"deepseek","model":"deepseek-v4-pro"}]),
            false).unwrap();
        assert_eq!(approved.as_array().unwrap().len(),2);
        assert!(authorized(&state,&original,
            &json!([{"providerId":"deepseek","model":"invented-unregistered"}]),
            false).is_err());
        assert!(authorized(&state,&original,
            &json!([{"providerId":"unapproved","model":"deepseek-v4-pro"}]),
            false).is_err());
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn new_api_model_may_be_explored_only_after_independent_baseline_evidence(){
        let original=json!({"providerId":"deepseek","model":"api-a"});
        let single=vec![episode("a","api-a",false,true)];
        assert_eq!(decide(&single,&routes(),&original,&"a".repeat(40))
            ["chosen"]["model"],"api-a");
        let two=vec![episode("a","api-a",false,true),
            episode("b","api-a",false,true)];
        let choice=decide(&two,&routes(),&original,&"a".repeat(40));
        assert_eq!(choice["chosen"]["model"],"api-b");
        assert_eq!(choice["explanation"],"owner_approved_bounded_exploration_trial");
        assert!(approved_choice(&choice,&routes()));
        let mut tampered=choice;
        tampered["chosen"]["model"]=json!("made-up-model");
        assert!(!approved_choice(&tampered,&routes()));
    }
    #[test]
    fn never_change_owner_api_without_evidence(){
        let baseline=json!({"providerId":"deepseek","model":"api-a"});
        let p=decide(&[],&routes(),&baseline,&"a".repeat(40));
        assert_eq!(p["chosen"],baseline);
        assert!(valid(&p,&[],&routes(),&baseline,&"a".repeat(40)));
    }
    #[test]
    fn actual_verified_task_feedback_changes_later_compiled_model_route(){
        let baseline=json!({"providerId":"deepseek","model":"api-a"});
        let history=vec![episode("a","api-a",true,false),
            episode("b","api-b",false,true)];
        let p=decide(&history,&routes(),&baseline,&"a".repeat(40));
        assert_eq!(p["chosen"]["model"],"api-b");
        assert!(valid(&p,&history,&routes(),&baseline,&"a".repeat(40)));
        let mut fake=history;
        fake[1]["apiRouteExecutionVerified"]=json!(false);
        let p=decide(&fake,&routes(),&baseline,&"a".repeat(40));
        assert_eq!(p["chosen"]["model"],"api-a");
    }

    #[test]
    fn same_source_holdouts_influence_route_without_model_causal_claim() {
        let owner=json!({"providerId":"deepseek","model":"api-a"});
        let mut a=episode("a1","api-a",false,false);
        let mut b=episode("b1","api-b",false,true);
        let mut c=episode("a2","api-a",false,false);
        let mut d=episode("b2","api-b",false,true);
        a["evaluation"]["metrics"]["measurements"][0]["id"]=json!("case-1");
        b["evaluation"]["metrics"]["measurements"][0]["id"]=json!("case-1");
        c["evaluation"]["metrics"]["measurements"][0]["id"]=json!("case-2");
        d["evaluation"]["metrics"]["measurements"][0]["id"]=json!("case-2");
        let history=vec![a,b,c,d];
        let result=decide(&history,&routes(),&owner,&"a".repeat(40));
        assert_eq!(result["chosen"]["model"],"api-b");
        assert_eq!(result["explanation"],"matched_source_case_observational_advantage");
        assert_eq!(result["matchedEvidence"]["matchedFrozenSourceCases"],2);
        assert_eq!(result["matchedEvidence"]["modelQualityCausalityProven"],false);
        assert!(valid(&result,&history,&routes(),&owner,&"a".repeat(40)));
    }
    #[test]
    fn source_mismatch_and_duplicated_session_never_create_extra_comparisons() {
        let mut a=episode("a","api-a",false,false);
        let mut b=episode("b","api-b",false,true);
        b["generatorCommit"]=json!("b".repeat(40));
        assert_eq!(learn(&[a.clone(),b],&routes())
            ["matchedSourceCaseContrasts"][0]["matchedFrozenSourceCases"],0);
        a["id"]=json!("duplicate-summary");
        let result=learn(&[a.clone(),a],&routes());
        assert_eq!(result["routes"][0]["verifiedApiWorkerCalls"],1);
        assert_eq!(result["routes"][0]["independentlyMeasuredCases"],1);
    }
    #[test]
    fn mixed_route_session_cannot_earn_model_credit() {
        let mut g=episode("mixed","api-b",false,true);
        g["apiRouteSessionExclusive"]=json!(false);
        assert_eq!(learn(&[g],&routes())["routes"][1]["independentlyMeasuredCases"],0);
    }
    #[test]
    fn unauditable_session_cannot_earn_api_route_rewards() {
        let mut fake=episode("fake","api-b",false,true);
        fake["apiRouteSessionId"]=Value::Null;
        assert_eq!(learn(&[fake],&routes())["routes"][1]["independentlyMeasuredCases"],0);
    }
    #[test]
    fn no_positive_reward_from_model_prose_or_native_mutation(){
        let mut g=episode("fake","api-b",false,true);
        g["apiRouteExecutionVerified"]=json!(false);
        g["candidateOrigin"]=json!("native_inductive_source_mutation");
        let report=learn(&[g],&routes());
        assert_eq!(report["routes"][1]["independentlyMeasuredCases"],0);
    }
}