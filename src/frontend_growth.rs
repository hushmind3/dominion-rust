//! Original main capabilities, learning and usage views, sourced from real
//! native runs and installed native tools.
use crate::{engine::Engine,domain::RunStatus,strategy};
use axum::{extract::State,routing::get,Json,Router};
use serde_json::{json,Value};
use std::{collections::HashMap,sync::Arc};

pub fn routes()->Router<Arc<Engine>>{
 Router::new()
  .route("/capabilities",get(capabilities))
  .route("/model-usage",get(model_usage))
  .route("/synergy",get(synergy))
  .route("/experience",get(experience))
  .route("/transfer",get(transfer))
}
async fn capabilities(State(engine):State<Arc<Engine>>)->Json<Value>{
 let native=crate::sdk::Dominion::from_engine(Arc::clone(&engine))
  .tool_catalog().await;
 let mut seen=std::collections::HashSet::new();
 let mut items=Vec::new();
 for tool in native{
  let name=tool.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
  if name.is_empty() || !seen.insert(name.to_string()){continue;}
  items.push(json!({
   "id":name,"name":name,
   "description":tool.pointer("/function/description").and_then(Value::as_str).unwrap_or(""),
   "namespace":"builtin","available":true
  }));
 }
 for skill in engine.extensions.skill_list().await{
  if skill.enabled && seen.insert(format!("skill:{}",skill.id)){
   items.push(json!({"id":format!("skill:{}",skill.id),"name":skill.name,
    "description":"User-managed SKILL.md loaded in the Rust service",
    "namespace":"builtin","available":true}));
  }
 }
 Json(json!({"items":items}))
}
/// Provider-reported model-token ledger. A reduced wire payload is NOT a
/// cached token, and a missing cache counter is not a measured zero hit.
/// Legacy Run events with no JSON detail retain the old message-text parser.
async fn model_usage(State(engine):State<Arc<Engine>>)->Json<Value>{
 // calls,total,prompt,output,cached,writes,unmetered,cache_unknown
 type Counts=(u64,u64,u64,u64,u64,u64,u64,u64);
 let mut grouped=HashMap::<String,Counts>::new();
 let mut totals:Counts=(0,0,0,0,0,0,0,0);
 for run in engine.store.list().await{
  let mut events=0u64;
  for event in run.events.iter().filter(|event|event.kind=="model"){
   let detail=event.detail.as_ref();
   let mut fields=event.message.split(" · ");
   let name=fields.next().unwrap_or("").trim();
   let raw=fields.next().unwrap_or("");
   let mut parts=raw.split_whitespace();
   let old_input=if parts.next()==Some("in"){
    parts.next().and_then(|v|v.parse::<u64>().ok())
   }else{None};
   let old_output=raw.split("out ").nth(1)
    .and_then(|v|v.split_whitespace().next())
    .and_then(|v|v.parse::<u64>().ok());
   let input=detail.and_then(|d|d["input_tokens"].as_u64()).or(old_input);
   let output=detail.and_then(|d|d["output_tokens"].as_u64()).or(old_output);
   let model=detail.and_then(|d|d["model"].as_str())
    .filter(|v|!v.is_empty()).unwrap_or_else(||if name.is_empty(){
     if run.model_name.is_empty(){engine.settings.main.name.as_str()}
     else{run.model_name.as_str()}
    }else{name});
   let cached=detail.and_then(|d|d["cached_input_tokens"].as_u64()).unwrap_or(0);
   let written=detail.and_then(|d|d["cache_write_tokens"].as_u64()).unwrap_or(0);
   let unmetered=if input.is_none()||output.is_none()
    ||detail.is_some_and(|d|d["usage_reported"]!=true){1}else{0};
   let cache_unknown=if detail.is_none_or(|d|d["cache_usage_reported"]!=true){1}else{0};
   let (in_tokens,out_tokens)= (input.unwrap_or(0),output.unwrap_or(0));
   let combined=in_tokens.saturating_add(out_tokens);
   let row=grouped.entry(model.to_owned()).or_default();
   for x in [&mut totals,row]{
    x.0+=1;x.1=x.1.saturating_add(combined);
    x.2=x.2.saturating_add(in_tokens);
    x.3=x.3.saturating_add(out_tokens);
    x.4=x.4.saturating_add(cached);
    x.5=x.5.saturating_add(written);
    x.6=x.6.saturating_add(unmetered);
    x.7=x.7.saturating_add(cache_unknown);
   }
   events+=1;
  }
  // Pre-upgrade model-only snapshots can have terminal aggregate usage but
  // no individual event: attribute only to the original selected model.
  if events==0&&run.input_tokens.saturating_add(run.output_tokens)>0{
   let model=if run.model_name.is_empty(){engine.settings.main.name.clone()}
    else{run.model_name.clone()};
   let row=grouped.entry(model).or_default();
   for x in [&mut totals,row]{
    x.0+=1;x.1=x.1.saturating_add(run.input_tokens.saturating_add(run.output_tokens));
    x.2=x.2.saturating_add(run.input_tokens);
    x.3=x.3.saturating_add(run.output_tokens);
    x.4=x.4.saturating_add(run.cached_input_tokens);
    x.5=x.5.saturating_add(run.cache_write_tokens);
    x.7+=1;
   }
  }
 }
 let mut groups=grouped.into_iter().map(|(model,x)|json!({
  "providerId":"native","model":model,"purpose":"native execution",
  "calls":x.0,"totalTokens":x.1,"promptTokens":x.2,
  "completionTokens":x.3,"cachedPromptTokens":x.4,
  "uncachedPromptTokens":x.2.saturating_sub(x.4),
  "cacheWriteTokens":x.5,"cacheUsageReported":x.0>0&&x.7==0,
  "unmeteredCalls":x.6,"cacheUnreportedCalls":x.7,
  "estimatedCalls":x.6
 })).collect::<Vec<_>>();
 groups.sort_by(|a,b|b["calls"].as_u64().cmp(&a["calls"].as_u64()));
 Json(json!({"calls":totals.0,"totalTokens":totals.1,
    "promptTokens":totals.2,"completionTokens":totals.3,
    "cachedPromptTokens":totals.4,
    "uncachedPromptTokens":totals.2.saturating_sub(totals.4),
    "cacheWriteTokens":totals.5,
    "cacheUsageReported":totals.0>0&&totals.7==0,
    "unmeteredCalls":totals.6,"cacheUnreportedCalls":totals.7,
    "groups":groups}))
}

async fn synergy(State(engine):State<Arc<Engine>>)->Json<Value>{
 let agents=engine.frontend.agents().await;
 let sessions=engine.frontend.sessions().await;
 let runs=engine.store.list().await;
 let indexed=runs.iter().map(|run|(run.id,run)).collect::<HashMap<_,_>>();
 let mut rows=Vec::new();
 for agent in agents{
  let mut completed=0u64;let mut failed=0u64;let mut observed=0u64;
  for session in sessions.iter().filter(|session|session.agent_id==agent.id){
   for message in &session.messages{
    let Some(id)=message.metadata.as_ref().and_then(|v|v["executionId"].as_str())
     .and_then(|v|uuid::Uuid::parse_str(v).ok()) else{continue;};
    let Some(run)=indexed.get(&id)else{continue;};
    observed+=1;
    if run.status==RunStatus::Completed{completed+=1;}
    if run.status==RunStatus::Failed{failed+=1;}
   }
  }
  rows.push(json!({"id":agent.id,"name":agent.name,"growth":{
    "runs":observed,"ratedRuns":completed+failed,
    "succeeded":completed,"failed":failed,
    "successRate":if completed+failed>0{completed as f64/(completed+failed) as f64}else{0.0},
    "failureClasses":{},"toolFailures":{},"guidance":[]
  }}));
 }
 Json(json!({"agents":rows}))
}
async fn experience(State(engine):State<Arc<Engine>>)->Json<Value>{
 let mut recorded=engine.store.list().await.into_iter()
  .filter_map(|run|strategy::from_run(&run)).collect::<Vec<_>>();
 recorded.sort_by_key(|entry|std::cmp::Reverse(entry.updated_at_ms));
 Json(json!(recorded.into_iter().take(100).map(|e|json!({
  "id":e.source_id,"lesson":e.principle,"request":e.goal,
  "status":e.outcome,"scope":e.repository,
  "lastSeenAt":crate::frontend_state::timestamp(e.updated_at_ms),
  "occurrences":e.attempts,"evidenceMessageIds":[]
 })).collect::<Vec<_>>()))
}

/// The learning view reads only verified native repair receipts.
async fn recorded(engine:&Engine)->Vec<(crate::domain::Run,strategy::StrategyEvidence)>{
 let mut runs=engine.store.list().await.into_iter()
  .filter_map(|run|strategy::from_run(&run).map(|e|(run,e)))
  .collect::<Vec<_>>();
 runs.sort_by_key(|(_,e)|std::cmp::Reverse(e.updated_at_ms));
 runs.truncate(100);
 runs
}
async fn transfer(State(engine):State<Arc<Engine>>)->Json<Value>{
 let mut entries=recorded(&engine).await.into_iter().map(|(run,e)|json!({
  "id":e.source_id,"kind":"verified_repair_trace","status":"observed",
  "principle":e.principle,"evidenceGrade":e.outcome,
  "confidence":Value::Null,"repository":e.repository,
  "supportingCases":[e.source_id],"supportingModels":[run.model_name],
  "supportingAgents":[],"counterexamples":[],
  "source":"native_run_journal","updatedAt":crate::frontend_state::timestamp(e.updated_at_ms),
  "failureMode":e.failure_mode,"files":e.files
 })).collect::<Vec<_>>();
 entries.truncate(50);
 Json(json!({"principles":entries}))
}
