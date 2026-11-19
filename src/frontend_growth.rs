//! Original main capabilities, learning and usage views, sourced from real
//! native runs, trial measurements and installed native tools.
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
  .route("/improvements",get(improvements))
  .route("/algorithms",get(algorithms))
  .route("/algorithms/evaluate",get(algorithm_evaluation))
  .route("/algorithms/evaluations",get(algorithm_evaluations))
  .route("/experiment-requests",get(experiment_requests))
  .route("/optimizers",get(optimizers))
  .route("/policies",get(policies))
  .route("/policy-searches",get(policy_searches))
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
async fn model_usage(State(engine):State<Arc<Engine>>)->Json<Value>{
 let mut grouped=HashMap::<String,(u64,u64,u64,u64)>::new();
 let mut calls=0u64;let mut tokens=0u64;let mut completion=0u64;
 for run in engine.store.list().await {
  let mut events=0u64;
  for event in run.events.iter().filter(|event|event.kind=="model"){
   let mut fields=event.message.split(" · ");
   let name=fields.next().unwrap_or("").trim();
   let usage=fields.next().unwrap_or("");
   let mut parts=usage.split_whitespace();
   let input=if parts.next()==Some("in"){parts.next().and_then(|s|s.parse::<u64>().ok())}else{None};
   let output=usage.split("out ").nth(1).and_then(|s|s.split_whitespace().next())
    .and_then(|s|s.parse::<u64>().ok());
   let model=if name.is_empty(){engine.settings.main.name.as_str()}else{name};
   let row=grouped.entry(model.to_string()).or_default();
   row.0+=1;calls+=1;events+=1;
   if let (Some(input),Some(output))=(input,output){
    let total=input.saturating_add(output);
    row.1=row.1.saturating_add(total);
    row.2=row.2.saturating_add(output);
    tokens=tokens.saturating_add(total);
    completion=completion.saturating_add(output);
   }else{
    row.3+=1; // Receipt format did not expose tokens; never invent them.
   }
  }
  // Historical model-only snapshots can contain usage even when older
  // receipts used a different format. Attribute only when unambiguous.
  if events==0 && run.input_tokens.saturating_add(run.output_tokens)>0{
   let row=grouped.entry(engine.settings.main.name.clone()).or_default();
   row.0+=1;calls+=1;
   let total=run.input_tokens.saturating_add(run.output_tokens);
   row.1=row.1.saturating_add(total);row.2=row.2.saturating_add(run.output_tokens);
   tokens=tokens.saturating_add(total);
   completion=completion.saturating_add(run.output_tokens);
  }
 }
 let mut groups=grouped.into_iter().map(|(model,(count,total,output,unknown))|json!({
  "providerId":"native","model":model,"purpose":"native execution",
  "calls":count,"totalTokens":total,"completionTokens":output,
  "estimatedCalls":unknown
 })).collect::<Vec<_>>();
 groups.sort_by(|a,b|b["calls"].as_u64().cmp(&a["calls"].as_u64()));
 Json(json!({"calls":calls,"totalTokens":tokens,
    "completionTokens":completion,"groups":groups}))
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
 Json(json!({"agents":rows,"sharedMethods":[],"algorithmStats":[]}))
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

/// Main's read-only learning tabs project genuine native Run evidence.
/// They do not start an RSI loop, invent optimizer candidates or auto-adopt.
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
 Json(json!({"principles":entries,"rules":[],"usages":[],
  "learningCases":[],"automaticAdoption":false}))
}
async fn improvements(State(engine):State<Arc<Engine>>)->Json<Value>{
 let rows=recorded(&engine).await.into_iter().map(|(_,e)|json!({
  "id":e.source_id,"text":e.principle,"rationale":e.failure_mode,
  "status":"observed","kind":"same_file_repair",
  "evidenceGrade":e.outcome,"rollout":"not_adopted",
  "source":"native_run_journal","lastObservedAt":
   crate::frontend_state::timestamp(e.updated_at_ms),
  "repository":e.repository,"files":e.files
 })).collect::<Vec<_>>();
 Json(json!(rows))
}
async fn algorithms()->Json<Value>{
 Json(json!({"kinds":[],"status":"not_benchmarked",
  "reason":"No measured algorithm variant is registered in this independent native engine"}))
}
async fn algorithm_evaluation()->Json<Value>{
 Json(json!({"dimensions":[],"status":"not_benchmarked",
  "reason":"No independent held-out variant evaluation has run"}))
}
async fn algorithm_evaluations()->Json<Value>{
 Json(json!({"items":[],"status":"not_benchmarked"}))
}
async fn optimizers()->Json<Value>{
 Json(json!({"optimizers":[],"episodes":0,
  "status":"no_optimizer_engine","automaticAdoption":false}))
}
async fn policies()->Json<Value>{Json(json!([]))}
async fn policy_searches(State(engine):State<Arc<Engine>>)->Json<Value>{
 let mut rows=Vec::new();
 for run in engine.store.list().await{
  for event in run.events.iter().filter(|e|e.kind=="strategy_trials_launched"){
   rows.push(json!({"id":format!("{}:{}",run.id,event.seq),
    "runId":run.id,"status":"recorded","source":"native_run_journal",
    "createdAt":crate::frontend_state::timestamp(event.at_ms),
    "candidateCount":event.detail.as_ref()
      .and_then(|d|d["candidates"].as_array()).map_or(0,Vec::len)}));
  }
 }
 rows.truncate(100);
 Json(json!(rows))
}
async fn experiment_requests(State(engine):State<Arc<Engine>>)->Json<Value>{
 let mut rows=Vec::new();
 for run in engine.store.list().await{
  let tested=run.events.iter().any(|e|e.kind=="strategy_trials_evaluated");
  for event in run.events.iter().filter(|e|e.kind=="strategy_trials_launched"){
   rows.push(json!({"id":format!("{}:{}",run.id,event.seq),
    "status":if tested{"observed"}else{"pending_native_settlement"},
    "strategyId":"isolated_native_trial","fixture":run.instruction.chars()
      .take(150).collect::<String>(),
    "createdAt":crate::frontend_state::timestamp(event.at_ms),
    "promotionStatus":"not_adopted",
    "source":"native_run_journal","runId":run.id}));
  }
 }
 rows.truncate(100);
 Json(json!(rows))
}
