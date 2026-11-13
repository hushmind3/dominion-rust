//! Model-agnostic self-improvement actor ecology.
//!
//! External APIs are immutable teachers/actors: we evolve the agent around
//! them and select real inference routes by subsequent source-adoption
//! outcomes. Selected providers retain their fixed model parameters.
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
    routes.truncate(12);
    routes
}
/// Attribute only actual source-authoring turns in the exact generation
/// session. Helper calls and model failover may consume tokens without being
/// the sole actor that produced the source patch.
fn session_chat_calls(rows:&[Value],session:&str,route:&Value)->(u64,u64,BTreeSet<String>){
    let mut selected=0u64;
    let mut mixed=0u64;
    let mut actual=BTreeSet::new();
    if session.is_empty(){return (selected,mixed,actual);}
    for row in rows {
        if row["event"]!="model.usage"
            ||row["detail"]["sessionId"]!=session
            ||row["detail"]["purpose"]!="chat"
            ||row["detail"]["totalTokens"].as_u64().unwrap_or(0)==0{
            continue;
        }
        let detail=&row["detail"];
        if let (Some(provider),Some(model))=(
            detail["providerId"].as_str(),detail["model"].as_str()){
            actual.insert(format!("{provider}/{model}"));
        }else{
            // A real token-bearing call with missing route metadata is
            // unidentifiable, not evidence that the selected model was alone.
            actual.insert("unknown_model_actor".into());
        }
        if same_route(detail,route){selected+=1;}else{mixed+=1;}
    }
    (selected,mixed,actual)
}

fn verified_attempt(g:&Value)->bool{
    g["modelEcologyExecuted"]["auditComplete"]==true
        &&g["modelEcologyExecuted"]["auditWindowStartRowid"]
            .as_i64().zip(g["modelEcologyExecuted"]["auditWindowEndRowid"].as_i64())
            .is_some_and(|(start,end)|start>=0 &&end>=start)
        &&g["modelEcologyExecuted"]["routeMatchedWorkerInput"]==true
        &&g["modelEcologyExecuted"]["sessionOwnedByGeneration"]==true
        &&g["modelEcologyExecuted"]["mixedChatModelCalls"]==0
        &&g["modelEcologyExecuted"]["actuallyUsedInGeneration"]==true
        &&g["modelEcologyExecuted"]["actualModelCalls"].as_u64().unwrap_or(0)>0
        &&g["modelEcologyExecuted"]["source"]=="real_descendant_model_usage_v1"
        &&g["modelEcologyExecuted"]["generationJobId"]==g["jobId"]
        &&g["modelEcologyExecuted"]["generatorCommit"]==g["generatorCommit"]
        &&g["modelEcologyExecuted"]["selectedBy"]==g["modelEcologyPolicy"]["revision"]
        &&g["modelEcologyExecuted"]["providerId"]
            ==g["modelEcologyPolicy"]["selected"]["providerId"]
        &&g["modelEcologyExecuted"]["model"]
            ==g["modelEcologyPolicy"]["selected"]["model"]
        &&g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        &&g["candidateOrigin"]=="model_driven_detached_worktree"
        &&sha(&g["generatorCommit"],40)
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

fn same_source_profile_trial(
    g:&Value,revision:&str,profile:&str,objective:&str,domain:&str
)->bool{
    g["generatorCommit"]==revision
        &&g["comparisonProfileSha256"]==profile
        &&g["objectiveId"]==objective
        &&g["domain"]==domain
        &&g["compiledPlanExecuted"]["source"]=="running_compiled_descendant_binary"
        &&verified_attempt(g)
        &&outcome(g).is_some()
}
fn matched_cohort(
    history:&[Value],route:&Value,revision:&str,
    profile:&str,objective:&str,domain:&str
)->Value{
    let mut trials=0u64;
    let mut wins=0u64;
    let mut gain=0.0f64;
    let mut regressions=0u64;
    let mut distinct=BTreeSet::<String>::new();
    for g in history{
        if !same_route(&g["modelEcologyExecuted"],route)
            ||!same_source_profile_trial(g,revision,profile,objective,domain){
            continue;
        }
        let key=g["candidateId"].as_str()
            .or_else(||g["jobId"].as_str()).unwrap_or("");
        if key.is_empty()||!distinct.insert(key.to_string()){continue;}
        trials+=1;
        gain+=outcome(g).unwrap_or(0.0);
        if g["accepted"]==true &&g["pairedRevalidated"]==true {wins+=1;}
        regressions+=g["evaluation"]["metrics"]["regressions"]
            .as_u64().unwrap_or(0).min(12);
    }
    json!({
        "sameGeneratorCommit":revision,
        "sameIndependentProfile":profile,
        "sameObjectiveId":objective,
        "sameDomain":domain,
        "comparedGenerations":trials,
        "adoptedSourceTrials":wins,
        "measuredRegressions":regressions,
        "meanHeldoutSourceImprovement":if trials==0{Value::Null}
            else{json!(gain/trials as f64)},
        "independentlyReplayedSourceOutcome":true,
        "samePromptOrModelWeightClaim":false,
        "modelCausalityProven":false
    })
}
/// Historical model comparisons are allowed to survive a campaign restart,
/// but only under the same immutable source commit, objective and frozen
/// evaluator. Reconcile a generation with its original job, actual actor
/// receipt, source candidate and replayed paired result. A claimed success
/// in a mutable generation row is not itself evidence.
fn independently_bound_comparison(
    state:&AppState,g:&Value,revision:&str,profile:&str,objective:&str,domain:&str
)->Option<Value>{
    if !same_source_profile_trial(g,revision,profile,objective,domain){
        return None;
    }
    let job_id=g["jobId"].as_str()?;
    let job=state.store.get("job",job_id)?;
    let candidate_id=g["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",candidate_id)?;
    if job["kind"]!="rsi_generation"
        ||job["id"]!=job_id
        ||job["input"]["rsiGeneratorCommit"]!=revision
        ||job["input"]["rsiModelEcologyPolicy"]!=g["modelEcologyPolicy"]
        ||job["rsiModelEcologyExecuted"]!=g["modelEcologyExecuted"]
        // The original raw audit is independently replayed over the frozen
        // rowid window; matching two editable summaries is not enough.
        ||executed(state,&job)!=g["modelEcologyExecuted"]
        ||job["rsiCompiledPlanExecuted"]!=g["compiledPlanExecuted"]
        ||job["input"]["providerId"]!=g["modelEcologyExecuted"]["providerId"]
        ||job["input"]["model"]!=g["modelEcologyExecuted"]["model"]
        ||crate::engine::rsi_lineage_transfer::profile(&job["input"]).as_deref()
            !=Some(profile)
        ||candidate["baseCommit"]!=revision
        ||candidate["origin"]!="model_driven_detached_worktree"
        ||g["evaluation"]["evidenceSha256"]!=candidate["evaluation"]["evidenceSha256"] {
        return None;
    }
    let receipt=crate::engine::rsi::replayed_pair_metrics(state,&candidate)?;
    if g["evaluation"]["metrics"]!=receipt
        ||candidate["evaluation"]["metrics"]!=receipt
        ||g["evaluation"]["source"]!="server_invoked_local_paired_git_worktrees"{
        return None;
    }
    let audited=crate::engine::rsi_system::revalidated_history(state,&[g.clone()]);
    let mut verified=audited.into_iter().next()?;
    if g["accepted"]==true &&verified["pairedRevalidated"]!=true{
        return None;
    }
    // Do not trust a mutable stale flag from a now-invalid adoption.
    verified["pairedMeasured"]=json!(true);
    Some(verified)
}

/// This is NOT pooled superiority across source revisions. A historical
/// campaign contributes only if it independently ran the exact same frozen
/// comparison while using the same source bytes and objective. Failed
/// campaigns can contribute neutral/negative paired evidence as well.
fn historical_same_source_trials(
    state:&AppState,history:&[Value],revision:&str,
    profile:&str,objective:&str,domain:&str
)->Vec<Value>{
    let mut already=history.iter().filter_map(|g|
        g["jobId"].as_str().map(str::to_string)).collect::<BTreeSet<_>>();
    let mut records=state.store.all("rsi_generation").into_iter()
        .filter(|g|g["generatorCommit"]==revision
            &&g["comparisonProfileSha256"]==profile
            &&g["objectiveId"]==objective
            &&g["domain"]==domain)
        .collect::<Vec<_>>();
    records.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let mut verified=Vec::new();
    for record in records.into_iter().take(256){
        let Some(job)=record["jobId"].as_str() else{continue};
        if !already.insert(job.to_string()){continue;}
        if let Some(row)=independently_bound_comparison(
            state,&record,revision,profile,objective,domain){
            verified.push(row);
        }
    }
    verified
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
            if matched.len()<5 {
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
    let profile=crate::engine::rsi_lineage_transfer::profile(input)
        .unwrap_or_default();
    // The same objective key used in the actual generation receipt.
    let task=if input["rsiAutonomous"]==true{domain}
        else{input["rsiObjective"].as_str().unwrap_or("")};
    let objective=crate::engine::memory::stable_id(&[task]);
    let recovered=if enabled &&!profile.is_empty(){
        historical_same_source_trials(
            state,history,revision,&profile,&objective,domain)
    }else{Vec::new()};
    let mut comparable=history.to_vec();
    comparable.extend(recovered.iter().cloned());
    let scores=routes.iter().map(|route|{
        let mut evidence=statistics(&comparable,route,domain);
        evidence["sameSourceCohort"]=matched_cohort(
            &comparable,route,revision,&profile,&objective,domain);
        (route.clone(),evidence)
    }).collect::<Vec<_>>();
    let incumbent_matched=scores.first()
        .and_then(|r|r.1["sameSourceCohort"]["comparedGenerations"].as_u64())
        .unwrap_or(0);
    let incumbent_score=scores.first()
        .and_then(|r|r.1["sameSourceCohort"]["meanHeldoutSourceImprovement"].as_f64())
        .unwrap_or(0.0);
    let paired_preference=if incumbent_matched>=2{
        scores.iter().enumerate().skip(1).filter_map(|(i,(_,stats))|{
            let matched=&stats["sameSourceCohort"];
            if matched["comparedGenerations"].as_u64().unwrap_or(0)<2
                ||matched["adoptedSourceTrials"].as_u64().unwrap_or(0)==0
                ||matched["measuredRegressions"].as_u64().unwrap_or(u64::MAX)>0 {
                return None;
            }
            let value=matched["meanHeldoutSourceImprovement"].as_f64()?;
            if value<=incumbent_score+0.20 {None}else{Some((i,value))}
        }).max_by(|a,b|a.1.total_cmp(&b.1))
    }else{None};
    // When no controlled same-source/profile cohort exists, cumulative
    // outcomes are exploration hints only, NOT an API-model win.
    let mut explore=scores.iter().enumerate()
        .filter(|(_,(_,stats))|stats["attempts"].as_u64().unwrap_or(0)<2)
        .map(|(i,_)|i).collect::<Vec<_>>();
    explore.sort_by(|a,b|scores[*a].1["attempts"].as_u64()
        .cmp(&scores[*b].1["attempts"].as_u64())
        .then(a.cmp(b)));
    let index=if let Some((i,_))=paired_preference{Some(i)}
        else if let Some(&first)=explore.first(){Some(first)}
        else{scores.iter().enumerate().max_by(|(_,a),(_,b)|
            a.1["rankScore"].as_f64().unwrap_or(0.0)
                .total_cmp(&b.1["rankScore"].as_f64().unwrap_or(0.0)))
            .map(|(i,_)|i)};
    let selected=index.map(|i|scores[i].0.clone())
        .unwrap_or_else(||current.clone());
    let choice_type=if paired_preference.is_some(){
        "same_source_profile_heldout_preference"
    }else if !explore.is_empty(){
        "unmatched_actor_exploration"
    }else{"cross_commit_transfer_hypothesis_not_model_proof"};
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
        "selectedBy":choice_type,
        "matchedSourceComparisonAvailable":paired_preference.is_some(),
        "independentlyRevalidatedPriorCampaignTrials":recovered.len(),
        "historicalEvidenceSameSourceOnly":true,
        "comparisonProfileSha256":profile,
        "comparisonObjectiveId":objective,
        "sourceRevisionsNeverPooledAsPairedModelProof":true,
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
    let mut sealed=policy.clone();
    if let Some(map)=sealed.as_object_mut(){map.remove("revision");}
    let expected=json!(format!("{:x}",Sha256::digest(
        serde_json::to_vec(&sealed).unwrap_or_default())));
    if !sha(&policy["revision"],64) ||policy["revision"]!=expected
        ||policy["modelWeightChanged"]!=false
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
    let session_owned=!id.is_empty() &&!session.is_empty()
        &&state.store.get("session",session).is_some_and(|row|
            row["rsiRun"]==true
                &&row["rsiGenerationJobId"]==id);
    // The child records this durable rowid immediately before its real
    // source-authoring turn. Reused sessions cannot import older inference.
    let cursor=job["rsiAuditStartRowid"].as_i64().filter(|v|*v>=0);
    let end=job["rsiAuditEndRowid"].as_i64().filter(|v|*v>=0);
    let audit=cursor.zip(end).map(|(start,end)|
        state.store.model_usage_for_session_in_window(session,start,end));
    let audit_complete=audit.as_ref().is_some_and(Result::is_ok);
    let (calls,mixed,actual)=audit.as_ref().and_then(|result|result.as_ref().ok())
        .map(|rows|session_chat_calls(rows,session,route))
        .unwrap_or((0,0,BTreeSet::new()));
    let route_matches_input=input["providerId"]==provider
        &&input["model"]==model;
    let exclusive=audit_complete &&session_owned &&route_matches_input
        &&calls>0 &&mixed==0;
    json!({
        "source":"real_descendant_model_usage_v1",
        "generationJobId":id,
        "generatorCommit":input["rsiGeneratorCommit"],
        "providerId":provider,"model":model,
        "actualModelCalls":calls,
        "mixedChatModelCalls":mixed,
        "auditWindowStartRowid":cursor,
        "auditWindowEndRowid":end,
        "auditComplete":audit_complete,
        "observedChatActors":actual,
        "sessionOwnedByGeneration":session_owned,
        "routeMatchedWorkerInput":route_matches_input,
        "actuallyUsedInGeneration":exclusive,
        "excludedFromSingleActorCredit":!exclusive,
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
        Never substitute provider reputation for measured code improvements.",
        selected["providerId"],selected["model"])
}
#[cfg(test)]
#[path = "../../../../tests/unit/engine_rsi_model_ecology_tests.rs"]
mod tests;
