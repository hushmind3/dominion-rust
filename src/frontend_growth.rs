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
  .route("/transfer",get(transfer))
  .route("/policies",get(policies))
  .route("/policy-searches",get(policy_searches))
  .route("/experiment-requests",get(experiments))
  .route("/experience",get(experience))
  .route("/improvements",get(improvements))
  .route("/optimizers",get(optimizers))
  .route("/algorithms/evaluations",get(evaluations))
  .route("/algorithms/evaluate",get(evaluations))
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
async fn evidence(engine:&Engine)->Vec<strategy::StrategyEvidence>{
 let mut output=Vec::new();
 for trial in engine.improvements.list().await{
  if let Some(row)=strategy::from_improvement(&trial){output.push(row);}
 }
 for run in engine.store.list().await{
  if let Some(row)=strategy::from_run(&run){output.push(row);}
 }
 output.sort_by_key(|v|std::cmp::Reverse(v.updated_at_ms));
 output
}
async fn transfer(State(engine):State<Arc<Engine>>)->Json<Value>{
 let verified=evidence(&engine).await;
 let principles=verified.iter().filter(|e|e.outcome=="adopted").map(|e|json!({
   "id":e.source_id,"kind":"verified_rsi","status":"active",
   "principle":e.principle,"confidence":Value::Null,
   "supportingCases":[e.source_id],"supportingModels":[],
   "supportingAgents":[],"counterexamples":[],
   "provenance":{"source":e.source,"repository":e.repository,
      "baselineScore":e.baseline_score,"candidateScore":e.resulting_score}
 })).collect::<Vec<_>>();
 let usages=verified.iter().filter(|e|e.outcome=="adopted")
  .map(|e|json!({"id":e.source_id,"principle":e.principle,
    "outcome":"verified","model":Value::Null,
    "createdAt":crate::frontend_state::timestamp(e.updated_at_ms)}))
  .collect::<Vec<_>>();
 Json(json!({"principles":principles,"rules":[],"usages":usages,"learningCases":[]}))
}
async fn policies(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!(evidence(&engine).await.into_iter().filter(|e|e.outcome=="adopted").map(|e|json!({
  "id":e.source_id,"slot":"rsi","name":e.principle,
  "status":"active","evidenceGrade":"measured",
  "lastExperiment":{"baseline":e.baseline_score,"candidate":e.resulting_score}
 })).collect::<Vec<_>>()))
}
async fn policy_searches()->Json<Value>{Json(json!([]))}
async fn experiments(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!(engine.improvements.list().await.iter().map(|trial|json!({
  "id":trial.id,"status":trial.status,
  "fixture":trial.repository,"strategyId":trial.parent,
  "prompt":trial.instruction,"error":trial.error,
  "baseline":trial.baseline,"candidate":trial.candidate
 })).collect::<Vec<_>>()))
}
async fn experience(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!(evidence(&engine).await.iter().map(|e|json!({
  "id":e.source_id,"lesson":e.principle,"request":e.goal,
  "status":e.outcome,"scope":e.repository,
  "lastSeenAt":crate::frontend_state::timestamp(e.updated_at_ms),
  "occurrences":e.attempts,"evidenceMessageIds":[]
 })).collect::<Vec<_>>()))
}
async fn improvements(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!(engine.improvements.list().await))
}
async fn optimizers(State(engine):State<Arc<Engine>>)->Json<Value>{
 Json(json!(evidence(&engine).await))
}
async fn evaluations(State(engine):State<Arc<Engine>>)->Json<Value>{
 let measured=engine.improvements.list().await.into_iter().filter(|trial|
  trial.baseline.is_some() && trial.candidate.is_some()).map(|trial|json!({
   "id":trial.id,"status":trial.status,"generatedAt":crate::frontend_state::timestamp(trial.updated_at_ms),
   "baseline":trial.baseline,"candidate":trial.candidate,
   "dimensions":[],"evidenceSource":"frozen_native_judge"
 })).collect::<Vec<_>>();
 Json(json!({"items":measured}))
}
