//! Main RoomsPage contract over Dominion's persisted native Groups engine.
//! Main frontend stays unmodified. Every emitted message comes from real group
//! history; unsupported tool/model overrides are rejected before charging.
use crate::{
  domain::now_ms,
  engine::Engine,
  frontend_state::{timestamp,UiMessage,UiSession},
  group::{FrontendRoom,NewRoom,Participant,Room,RoomStatus},
};
use axum::{
  extract::{Path,Query,State},http::StatusCode,
  routing::{get,post},Json,Router,
};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::HashSet,sync::Arc};
use uuid::Uuid;
mod room_native;

pub(crate) type Fault=(StatusCode,Json<Value>);
fn bad(message:impl Into<String>)->Fault{
    (StatusCode::BAD_REQUEST,Json(json!({"error":message.into()})))
}
fn missing()->Fault{(StatusCode::NOT_FOUND,Json(json!({"error":"room not found"})))}
fn unsupported(reason:&str)->Fault{(StatusCode::NOT_IMPLEMENTED,Json(json!({"error":reason})))}
fn failed(message:String)->Fault{(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":message})))}

pub fn routes()->Router<Arc<Engine>>{
  Router::new()
    .route("/rooms",get(list).post(save))
    .route("/rooms/{id}",get(one).post(update).delete(remove))
    .route("/rooms/{id}/messages",get(messages).post(send))
    .route("/rooms/{id}/messages/background",post(send_background))
    .route("/rooms/{id}/state",get(state))
    .route("/rooms/{id}/reset",post(reset))
    .route("/rooms/{id}/stop",post(stop))
    .route("/rooms/{id}/run",post(run))
    .route("/rooms/{id}/knowledge",get(knowledge))
    .route("/rooms/{id}/knowledge/learn",post(learn))
    .route("/rooms/{id}/system-prompt",get(system_prompt))
}
fn room_session(id:Uuid)->String{format!("room-{id}")}
fn config_of(room:&Room)->FrontendRoom{
  room.frontend.clone().unwrap_or_else(||FrontendRoom{
    name:room.topic.clone(),topic:room.topic.clone(),owner_name:"나".into(),
    agent_ids:room.participants.iter().map(|p|p.name.clone()).collect(),
    routing_mode:"discussion".into(),rounds:1,max_turns:0,
    lifetime_max_responses:room.max_rounds,
    token_budget:0,context_budget_tokens:2000,tools_enabled:false,
    auto_learning:false,auto_models:true,models:vec![],
    reasoning_effort:String::new(),learned_context:String::new(),
  })
}
fn room_status(room:&Room)->&'static str{
    if room.status==RoomStatus::Open{
        return if room.last_provider_failure.is_some(){"interrupted"}else{"idle"};
    }
    match room.finish_reason.as_str(){
        "stopped"=>"stopped",
        "max_turns_reached"|"room_lifetime_limit_reached"=>"interrupted",
        "cost_budget_exceeded"|"token_budget_exceeded"=>"failed",
        _=>"completed"
    }
}
/// A discussion response is a full set of distinct registered speakers,
/// not one model step. Non-discussion routing retains its explicit target.
fn speakers_for_owner_turn(room:&Room)->Vec<String>{
    // A durable, indexed work board—not the last model's free-form prose—
    // decides which original speaker tickets still need a response.
    super::coordination::RoomWorkBoard::project(room).pending_agents()
}
fn owner_turn_progress(room:&Room,index:usize,requested:&[String])->(usize,usize){
    let config=config_of(room);
    let wanted=if config.routing_mode=="round_robin"{1}
    else if !requested.is_empty(){requested.iter().collect::<HashSet<_>>().len()}
    else if config.routing_mode=="discussion"{room.participants.len()}
    else{
        // Mention-only rounds are intentionally scoped to named targets.
        room.messages.get(index).map(|m|m.content.to_lowercase()).map_or(0,|content|
            room.participants.iter().filter(|p|
                content.contains(&format!("@{}",p.name.to_lowercase()))).count())
            .max(room.recipients.len())
    };
    let produced=room.messages.iter().skip(index+1)
        .take_while(|m|m.speaker!="user")
        .filter(|m|m.speaker!="user").count();
    (wanted,produced)
}
pub(crate) fn room_usage(room:&Room)->Value{
    let prompt=room.usage_receipts.iter().map(|m|m.input_tokens).sum::<u64>();
    let output=room.usage_receipts.iter().map(|m|m.output_tokens).sum::<u64>();
    let cached=room.usage_receipts.iter().map(|m|m.cached_input_tokens).sum::<u64>();
    let writes=room.usage_receipts.iter().map(|m|m.cache_write_tokens).sum::<u64>();
    let unmetered=room.usage_receipts.iter().filter(|m|
        m.provider_usage_reported!=Some(true)).count();
    let unknown_cache=room.usage_receipts.iter().filter(|m|
        m.cache_usage_reported!=Some(true)).count();
    let delivered=room.usage_receipts.iter().filter(|m|m.accepted).count();
    let delta_calls=room.usage_receipts.iter()
        .filter(|m|m.reused_server_continuation).count();
    let expired_rebases=room.usage_receipts.iter()
        .filter(|m|m.remote_history_rebased).count();
    let total_wire_chars=room.usage_receipts.iter()
        .map(|m|m.wire_prompt_chars).sum::<usize>();
    let by_agent=room.participants.iter().map(|p|{
        let receipts=room.usage_receipts.iter().filter(|m|m.participant==p.name)
            .collect::<Vec<_>>();
        let not_reported=receipts.iter().filter(|r|
            r.provider_usage_reported!=Some(true)).count();
        let unknown_cache=receipts.iter().filter(|r|
            r.cache_usage_reported!=Some(true)).count();
        json!({"agentId":p.name,"calls":receipts.len(),
            "unmeteredCalls":not_reported,
            "deliveredCalls":receipts.iter().filter(|r|r.accepted).count(),
            "billedUndeliveredCalls":receipts.iter().filter(|r|!r.accepted).count(),
            "serverContinuationCalls":receipts.iter()
                .filter(|r|r.reused_server_continuation).count(),
            "expiredContinuationRebases":receipts.iter()
                .filter(|r|r.remote_history_rebased).count(),
            "wirePromptChars":receipts.iter().map(|r|r.wire_prompt_chars).sum::<usize>(),
            "providerUsageReported":!receipts.is_empty() && not_reported==0,
            "cacheUsageReported":!receipts.is_empty() && unknown_cache==0,
            "promptTokens":receipts.iter().map(|r|r.input_tokens).sum::<u64>(),
            "completionTokens":receipts.iter().map(|r|r.output_tokens).sum::<u64>(),
            "cachedPromptTokens":receipts.iter().map(|r|r.cached_input_tokens).sum::<u64>(),
            "uncachedPromptTokens":receipts.iter().map(|r|
                r.input_tokens.saturating_sub(r.cached_input_tokens)).sum::<u64>(),
            "cacheWriteTokens":receipts.iter().map(|r|r.cache_write_tokens).sum::<u64>(),
            "costKrw":receipts.iter().map(|r|r.cost_krw).sum::<f64>(),
            "contextSourceChars":receipts.iter().map(|r|r.source_context_chars).sum::<usize>(),
            "contextSelectedChars":receipts.iter().map(|r|r.projected_context_chars).sum::<usize>(),
            "omittedContextMessages":receipts.iter().map(|r|r.omitted_context_messages).sum::<usize>()})
    }).collect::<Vec<_>>();
    json!({"totalTokens":room.used_tokens,
        "promptTokens":prompt,"completionTokens":output,
        "cachedPromptTokens":cached,"cacheWriteTokens":writes,
        "uncachedPromptTokens":prompt.saturating_sub(cached),
        "cacheHitRate":if prompt>0{cached as f64/prompt as f64}else{0.0},
        "calls":room.usage_receipts.len(),"estimatedCalls":0,
        "deliveredCalls":delivered,
        "billedUndeliveredCalls":room.usage_receipts.len().saturating_sub(delivered),
        "serverContinuationCalls":delta_calls,
        "expiredContinuationRebases":expired_rebases,
        "fullPromptCalls":room.usage_receipts.len().saturating_sub(delta_calls),
        "wirePromptChars":total_wire_chars,
        "wireUnits":"characters_transmitted_not_provider_billing_tokens",
        "unmeteredCalls":unmetered,
        "cacheUnreportedCalls":unknown_cache,
        "cacheUsageReported":!room.usage_receipts.is_empty()&&unknown_cache==0,
        "providerUsageReported":!room.usage_receipts.is_empty()&&unmetered==0,
        "usageStatus":if room.usage_receipts.is_empty(){"not_started"}
            else if unmetered>0{"partial_or_unreported"}else{"reported"},
        "costKrw":room.cost_krw,
        "contextSourceChars":room.usage_receipts.iter()
            .map(|r|r.source_context_chars).sum::<usize>(),
        "contextSelectedChars":room.usage_receipts.iter()
            .map(|r|r.projected_context_chars).sum::<usize>(),
        "omittedContextMessages":room.usage_receipts.iter()
            .map(|r|r.omitted_context_messages).sum::<usize>(),
        "contextUnits":"characters_not_provider_tokens",
        "byAgent":by_agent})
}
pub fn room_view(room:&Room)->Value{
  let conf=config_of(room);
  json!({
    "id":room.id.to_string(),"name":conf.name,"topic":conf.topic,
    "ownerName":conf.owner_name,"agentIds":conf.agent_ids,
    "routingMode":conf.routing_mode,"rounds":conf.rounds,
    "maxTurns":conf.max_turns,
    "lifetimeMaxResponses":conf.lifetime_max_responses,
    "tokenBudget":conf.token_budget,
    "contextBudgetTokens":conf.context_budget_tokens,
    "toolsEnabled":conf.tools_enabled,"autoLearning":conf.auto_learning,
    "autoModels":conf.auto_models,"models":conf.models,
    "reasoningEffort":conf.reasoning_effort,"sessionId":room_session(room.id),
    "status":room_status(room),"finishReason":room.finish_reason,
    "lastProviderFailure":room.last_provider_failure,
    "updatedAt":timestamp(room.updated_at_ms)
  })
}
pub fn message_rows(room:&Room)->Vec<UiMessage>{
  let session_id=room_session(room.id);
  let receipts=room.usage_receipts.iter().filter(|r|r.accepted)
    .map(|r|(r.message_index,r)).collect::<std::collections::HashMap<_,_>>();
  room.messages.iter().enumerate().map(|(i,m)|{
    let user=m.speaker=="user";
    let usage=receipts.get(&i).map(|receipt|json!({
        "promptTokens":receipt.input_tokens,
        "completionTokens":receipt.output_tokens,
        "cachedPromptTokens":receipt.cached_input_tokens,
        "cacheWriteTokens":receipt.cache_write_tokens,
        "providerUsageReported":receipt.provider_usage_reported,
        "cacheUsageReported":receipt.cache_usage_reported,
        "wirePromptChars":receipt.wire_prompt_chars,
        "serverContinuation":receipt.reused_server_continuation,
        "expiredContinuationRebase":receipt.remote_history_rebased,
        "totalTokens":receipt.input_tokens.saturating_add(receipt.output_tokens),
        "costKrw":receipt.cost_krw,
        "contextComposition":{
            "sourceChars":receipt.source_context_chars,
            "selectedChars":receipt.projected_context_chars,
            "omittedMessages":receipt.omitted_context_messages,
            "commonPrefixChars":receipt.common_prefix_chars,
            "personaChars":receipt.persona_chars,
            "stableContextChars":receipt.stable_context_chars,
            "rollingContextChars":receipt.rolling_context_chars,
            "checkpointedMessages":receipt.checkpoint_end,
            "selfRecallChars":receipt.self_recall_chars,
            "mutableMemoryChars":receipt.mutable_memory_chars
        }
    }));
    UiMessage{
      id:format!("{}:{i}",room.id),
      session_id:session_id.clone(),
      role:if user{"user"}else{"assistant"}.into(),
      content:m.content.clone(),
      created_at:timestamp(m.at_ms),
      agent_id:if user{None}else{Some(m.speaker.clone())},
      metadata:Some(json!({
        "roomId":room.id.to_string(),
        "authorKind":if user{"user"}else{"agent"},
        "model":m.model,"usage":usage
      }))
    }
  }).collect()
}
async fn list(State(engine):State<Arc<Engine>>)->Json<Value>{
  Json(Value::Array(engine.groups.list().await.iter().map(room_view).collect()))
}
async fn one(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  engine.groups.get(id).await.map(|r|Json(room_view(&r))).ok_or_else(missing)
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct RoomDraft {
  id:Option<String>,name:String,topic:String,owner_name:String,
  agent_ids:Vec<String>,routing_mode:String,rounds:usize,max_turns:usize,
  token_budget:u64,context_budget_tokens:usize,
  #[serde(default)]lifetime_max_responses:usize,
  auto_models:bool,tools_enabled:bool,auto_learning:bool,
  #[serde(default)]models:Vec<Value>,#[serde(default)]reasoning_effort:String,
}
async fn checked(engine:&Engine,input:&RoomDraft)->Result<(FrontendRoom,Vec<Participant>),Fault>{
  if input.name.trim().is_empty()||input.name.len()>120||input.topic.len()>10_000
    ||input.owner_name.len()>120||!(1..=32).contains(&input.agent_ids.len())
    ||!(1..=12).contains(&input.rounds)
    ||input.max_turns>10_000||input.lifetime_max_responses>10_000
    ||input.context_budget_tokens>8_000{
    return Err(bad("invalid room title, participant count, rounds or limit"));
  }
  if !matches!(input.routing_mode.as_str(),"discussion"|"round_robin"|"mentioned"){
    return Err(bad("unknown room routing mode"));
  }
  // A tool-enabled Room routes speakers through native, read-only
  // Engine Runs with the same journal, approval and cancellation controls.
  if !input.reasoning_effort.is_empty(){
    return Err(unsupported("Per-room reasoning effort requires native provider controls."));
  }
  if !input.auto_models&&input.models.is_empty(){
    return Err(bad("manual room model routing requires at least one selected model"));
  }
  for model in &input.models{
    let provider=model.get("providerId").and_then(Value::as_str)
        .ok_or_else(||bad("room model provider missing"))?;
    let chosen=model.get("model").and_then(Value::as_str)
        .ok_or_else(||bad("room model name missing"))?;
    if model.get("reasoningEffort").and_then(Value::as_str).is_some_and(|s|!s.is_empty()){
        return Err(unsupported("Room model-specific reasoning effort is not implemented by the selected native provider"));
    }
    engine.providers.resolve(provider,chosen,&engine.settings.main).map_err(bad)?;
  }
  let agents=engine.frontend.agents().await;
  let mut members=Vec::new();
  let mut seen=HashSet::new();
  for id in &input.agent_ids{
    if !seen.insert(id){return Err(bad("duplicate room agent id"));}
    let agent=agents.iter().find(|a|&a.id==id)
      .ok_or_else(||bad(format!("room agent not found: {id}")))?;
    engine.providers.resolve(&agent.provider_id,&agent.model,&engine.settings.main)
        .map_err(bad)?;
    members.push(Participant{
      name:agent.id.clone(),
      role:if agent.system_prompt.trim().is_empty(){agent.name.clone()}
        else{agent.system_prompt.clone()},
      economy:false
    });
  }
  let view=FrontendRoom{
    name:input.name.trim().into(),topic:input.topic.clone(),
    owner_name:input.owner_name.clone(),agent_ids:input.agent_ids.clone(),
    routing_mode:input.routing_mode.clone(),rounds:input.rounds,
    max_turns:input.max_turns,
    lifetime_max_responses:input.lifetime_max_responses,
    token_budget:input.token_budget,
    context_budget_tokens:input.context_budget_tokens.clamp(200,8_000),
    tools_enabled:input.tools_enabled,auto_learning:input.auto_learning,
    auto_models:input.auto_models,models:input.models.clone(),
    reasoning_effort:String::new(),learned_context:String::new()
  };
  Ok((view,members))
}
async fn save(State(engine):State<Arc<Engine>>,Json(input):Json<RoomDraft>)
  ->Result<Json<Value>,Fault>{
  let (view,participants)=checked(&engine,&input).await?;
  if let Some(id)=input.id.as_deref().filter(|id|!id.is_empty()){
    return update_existing(&engine,Uuid::parse_str(id).map_err(|_|bad("invalid room id"))?,view,participants).await;
  }
  let new=engine.groups.create(NewRoom{
    topic:if view.topic.trim().is_empty(){view.name.clone()}else{view.topic.clone()},
    participants:participants.clone(),max_rounds:Some(
      if view.lifetime_max_responses==0{10000}else{view.lifetime_max_responses}),
    budget_krw:None,
  }).await.map_err(bad)?;
  let room=match engine.groups.configure(new.id,view.clone(),participants).await{
    Ok(room)=>room,
    Err(err)=>{
      let _=engine.groups.remove(new.id).await;
      return Err(bad(err));
    }
  };
  let session_id=room_session(room.id);
  let now=timestamp(now_ms());
  let session=UiSession{
    id:session_id.clone(),title:view.name,agent_id:"general".into(),
    provider_id:"dominion-native".into(),model:engine.settings.main.name.clone(),
    created_at:now.clone(),updated_at:now,room_id:Some(room.id.to_string()),
    pending_run:None,messages:vec![],
  };
  if let Err(err)=engine.frontend.mutate(|state|{
    if state.sessions.contains_key(&session_id){return Err("room session already exists".into());}
    state.sessions.insert(session_id,session);
    Ok(())
  }).await{
    let _=engine.groups.remove(room.id).await;
    return Err(failed(err));
  }
  Ok(Json(room_view(&room)))
}
async fn update_existing(engine:&Arc<Engine>,id:Uuid,view:FrontendRoom,participants:Vec<Participant>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.configure(id,view.clone(),participants).await.map_err(bad)?;
  let session_id=room_session(id);
  if let Err(err)=engine.frontend.mutate(|data|{
    if let Some(session)=data.sessions.get_mut(&session_id){
      session.title=view.name.clone();
      session.updated_at=timestamp(now_ms());
    }
    Ok(())
  }).await{tracing::warn!(%id,%err,"room linked session title not updated");}
  Ok(Json(room_view(&room)))
}
async fn update(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
  Json(input):Json<RoomDraft>)->Result<Json<Value>,Fault>{
  if let Some(ref source_id)=input.id{
    if source_id!=&id.to_string(){return Err(bad("room id/path mismatch"));}
  }
  let (view,members)=checked(&engine,&input).await?;
  update_existing(&engine,id,view,members).await
}
async fn remove(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  engine.groups.remove(id).await.map_err(bad)?;
  let session_id=room_session(id);
  if let Err(err)=engine.frontend.mutate(|state|{
    state.sessions.remove(&session_id);
    Ok(())
  }).await{tracing::warn!(%id,%err,"orphan room session cleanup failed");}
  Ok(Json(json!({"ok":true})))
}
async fn messages(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Vec<UiMessage>>,Fault>{
  room_native::reconcile_ready(&engine,id).await.map_err(bad)?;
  engine.groups.get(id).await.map(|room|Json(message_rows(&room)))
    .ok_or_else(missing)
}
fn completed_job(room:&Room,turns:usize,expected:usize,receipt:&str)->Value{
  let complete=expected>0&&turns==expected;
  json!({
    "id":receipt,
    "kind":"room_turn","status":if complete{"completed"}else if expected==0{"skipped"}else{"interrupted"},
    "createdAt":timestamp(room.updated_at_ms),
    "input":{"roomId":room.id.to_string(),"sessionId":room_session(room.id)},
    "result":{"rounds":turns,"expectedResponses":expected,
        "actualResponses":turns,"missingResponses":expected.saturating_sub(turns),
        "totalTokens":room.used_tokens,"usage":room_usage(room)}
  })
}
async fn state(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  room_native::reconcile_ready(&engine,id).await.map_err(bad)?;
  let room=engine.groups.get(id).await.ok_or_else(missing)?;
  let active=room_native::running(&engine,&room).await;
  let approval=room_native::approvals(&engine,&room).await;
  let running=engine.groups.frontend_turn_active(id)||active;
  let pending=if running{speakers_for_owner_turn(&room)}else{Vec::new()};
  let owner=room.messages.iter().rposition(|m|m.speaker=="user");
  let (wanted,actual)=owner.map_or((0,0),|index|
    owner_turn_progress(&room,index,&room.recipients));
  let last_message_id=owner.and_then(|index|
    room.client_messages.iter().find(|(_,position)|**position==index)
        .map(|(key,_)|key.clone())).unwrap_or_default();
  let jobs=if running{vec![json!({
    "id":last_message_id,"kind":"room_turn","status":"running",
    "stepIndex":actual,"stepTotal":wanted,
    "input":{"roomId":id.to_string(),"sessionId":room_session(id)}
  })]}else{vec![]};
  Ok(Json(json!({
    "room":room_view(&room),"sessionId":room_session(id),
    "jobs":jobs,"executions":[],"learningJobs":[],"operations":[],"approvals":approval,
    "status":if running{"running"}else{room_status(&room)},
    "activeAgentId":pending.first(),"queueDepth":pending.len(),
    "lastProviderFailure":room.last_provider_failure,
    "usage":room_usage(&room)
  })))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct SayInput{
  input:String,
  #[serde(default)]agent_ids:Vec<String>,
  client_message_id:String,
  #[serde(default)]reasoning_language:Option<String>,
}
/// Shared owner-message admission for synchronous and background fanout.
async fn admit_owner_message(
    engine:&Arc<Engine>,id:Uuid,input:&SayInput
)->Result<(Room,bool,usize,Vec<String>),Fault>{
    if input.input.trim().is_empty(){return Err(bad("room message cannot be empty"));}
    if input.reasoning_language.as_deref().is_some_and(|value|
        !matches!(value,"ko"|"default"|"original")){
        return Err(bad("unsupported reasoning language"));
    }
    let requested=input.agent_ids.clone();
    let mut recipients=requested.clone();
    if recipients.is_empty(){
        let room=engine.groups.get(id).await.ok_or_else(missing)?;
        let content=input.input.to_lowercase();
        let members=room.participants.iter()
            .map(|participant|participant.name.as_str()).collect::<HashSet<_>>();
        for agent in engine.frontend.agents().await{
            if members.contains(agent.id.as_str())&&!agent.name.is_empty()
                &&content.contains(&format!("@{}",agent.name.to_lowercase())){
                recipients.push(agent.id);
            }
        }
    }
    let (room,inserted,index)=engine.groups.say_once(
        id,input.input.trim().to_owned(),&input.client_message_id,recipients
    ).await.map_err(bad)?;
    Ok((room,inserted,index,requested))
}
pub(crate) async fn generate(engine:&Arc<Engine>,id:Uuid)->Result<(Room,usize,usize),Fault>{
  let before=engine.groups.get(id).await.ok_or_else(missing)?;
  let config=config_of(&before);
  let scheduled=speakers_for_owner_turn(&before);
  let expected=scheduled.len();
  let mut room=before;
  let mut turns=0;
  // Snapshot participant metadata once. Resolve credentials at each dispatch.
  let agent_configs=if !config.auto_models&&!config.models.is_empty(){
      None
  }else{Some(engine.frontend.agents().await)};
  // One client message gets at most maxTurns responses. Retries operate on
  // the remaining allowance and never reset it or spend on a duplicate turn.
  let owner=room.messages.iter().rposition(|m|m.speaker=="user");
  let already_generated=owner.map_or(0,|index|
    room.messages[index+1..].iter().take_while(|m|m.speaker!="user").count());
  let allowance=if config.max_turns==0{usize::MAX}
    else{config.max_turns.saturating_sub(already_generated)};
  for agent_id in scheduled.into_iter().take(allowance){
    if room.status==RoomStatus::Finished{break;}
    // Refresh scoped evidence when auto-learning may change it mid-turn.
    if turns==0||config.auto_learning{
        let notes=engine.knowledge.list(&format!("room:{id}")).await;
        let excerpts=notes.iter().take(4).map(|n|format!("{}: {}",n.title,n.content))
            .collect::<Vec<_>>().join("\n");
        let excerpt=if excerpts.chars().count()>1250{
            excerpts.chars().take(1250).collect::<String>()
        }else{excerpts};
        engine.groups.set_knowledge_context(id,excerpt).await.map_err(bad)?;
    }
    let (provider,model)=if !config.auto_models && !config.models.is_empty(){
        let selected=&config.models[room.rounds%config.models.len()];
        (selected.get("providerId").and_then(Value::as_str).unwrap_or("").to_owned(),
         selected.get("model").and_then(Value::as_str).unwrap_or("").to_owned())
    }else{
        let agent=agent_configs.as_ref()
            .and_then(|agents|agents.iter().find(|agent|agent.id==agent_id))
            .ok_or_else(||bad("selected room agent was deleted"))?;
        (agent.provider_id.clone(),agent.model.clone())
    };
    room=match if config.tools_enabled{
        room_native::execute(engine,id,&agent_id,&provider,&model).await
    }else{
        let selected_model=engine.model_for_selection(&provider,&model).map_err(bad)?
            .with_session_affinity(&format!("dominion-room-{id}"));
        engine.providers.throttle(&provider).await;
        engine.groups.next_for(&selected_model,id,Some(&agent_id)).await
    }{
      Ok(next)=>next,
      Err(error)=>{
        let snapshot=engine.groups.get(id).await.ok_or_else(missing)?;
        if let Some(ref failure)=snapshot.last_provider_failure{
          let index=snapshot.messages.iter().rposition(|m|m.speaker=="user")
            .unwrap_or(0);
          let (wanted,actual)=owner_turn_progress(&snapshot,index,&snapshot.recipients);
          // Upstream HTTP 400 is not an invalid owner message. Report a
          // 502 with precise, privacy-safe provider context and the durable
          // partial speaker transcript for idempotent same-ID resume.
          tracing::warn!(room=%id,participant=%failure.participant,
            status=?failure.http_status,upstream_code=?failure.upstream_code,
            context_chars=failure.context_chars,
            completed=actual,expected=wanted,
            "room model call failed; partial room retained for retry");
          return Err((StatusCode::BAD_GATEWAY,Json(json!({
            "error":"Room model request failed; retry the same clientMessageId to continue from the missing speaker",
            "errorCode":failure.category,
            "providerStatus":failure.http_status,
            "providerCode":failure.upstream_code,
            "failedSpeaker":failure.participant,
            "model":failure.model,
            "contextChars":failure.context_chars,
            "systemChars":failure.system_chars,
            "actualResponses":actual,
            "expectedResponses":wanted,
            "room":room_view(&snapshot),
            "usage":room_usage(&snapshot)
          }))));
        }
        return Err(bad(error));
      }
    };
    // Native Run may pause for approval, or a provider call may still be
    // executing. Neither is an accepted speaker reply yet.
    if config.tools_enabled && room.messages.last().is_none_or(|m|m.speaker!=agent_id){
        break;
    }
    turns+=1;
    if config.auto_learning{
      if let Some(last)=room.messages.last().filter(|m|m.speaker!="user"){
        let excerpt=last.content.chars().take(3900).collect::<String>();
        if let Err(err)=engine.knowledge.remember(
          &format!("room:{id}"),&format!("Recorded turn {}",room.rounds),&excerpt
        ).await{
          tracing::warn!(room=%id,error=%err,"unable to record room knowledge");
        }
      }
    }
  }
  crate::addons::dispatch_room(engine,&room).await;
  Ok((room,turns,expected))
}
async fn send(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
  Json(input):Json<SayInput>)->Result<Json<Value>,Fault>{
  // One full owner turn must have one execution owner, not a separate
  // lock for each speaker. A dropped HTTP future releases this lease.
  let _lease=engine.groups.try_frontend_turn(id).ok_or_else(||(
    StatusCode::CONFLICT,Json(json!({"error":
      "room turn already running; inspect /state before resending"}))))?;
  let (before,inserted,index,requested_initial)=
      admit_owner_message(&engine,id,&input).await?;
  // Retry only the original owner turn; stale clientMessageIds may never
  // schedule replies for a newer user message.
  let newest=before.messages.iter().rposition(|m|m.speaker=="user")==Some(index);
  let should_generate=newest&&before.status==RoomStatus::Open
      &&!speakers_for_owner_turn(&before).is_empty();
  let (room,_new_turns,_new_expected)=if should_generate{
      generate(&engine,id).await?
  }else{(before,0,0)};
  let (expected,turns)=owner_turn_progress(&room,index,
      if requested_initial.is_empty(){&room.recipients}else{&requested_initial});
  // generate() has already dispatched completed replies; avoid a second
  // outbound-channel scan when handling a synchronous owner turn.
  if inserted&&!should_generate{
      crate::addons::dispatch_room(&engine,&room).await;
  }
  let message=message_rows(&room).get(index).cloned().ok_or_else(||failed("stored message disappeared".into()))?;
  Ok(Json(json!({
    "room":room_view(&room),"message":message,
    "job":completed_job(&room,turns,expected,&input.client_message_id),"sessionId":room_session(id)
  })))
}
/// Accept a nine-speaker owner turn without holding the browser/edge HTTP
/// request open for nine sequential paid model calls. The durable Room is
/// updated after each speaker. A process restart releases the in-memory lease
/// and the original clientMessageId can resume remaining speakers.
async fn send_background(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
  Json(input):Json<SayInput>)->Result<(StatusCode,Json<Value>),Fault>{
  // Repeating the same durable clientMessageId while work is active is
  // an idempotent status read, not another nine-model fan-out.
  let lease=if let Some(lease)=engine.groups.try_frontend_turn(id){lease}else{
    let room=engine.groups.get(id).await.ok_or_else(missing)?;
    let index=room.client_messages.get(&input.client_message_id).copied();
    let same=index.and_then(|index|room.messages.get(index).map(|message|
      (index,message.content.as_str()==input.input.trim())));
    if let Some((index,true))=same{
      let message=message_rows(&room).get(index).cloned()
        .ok_or_else(||failed("stored message disappeared".into()))?;
      let (wanted,actual)=owner_turn_progress(&room,index,&room.recipients);
      return Ok((StatusCode::ACCEPTED,Json(json!({
        "room":room_view(&room),"message":message,
        "job":{"id":input.client_message_id,"kind":"room_turn",
            "status":"running","stepIndex":actual,"stepTotal":wanted},
        "sessionId":room_session(id)
      }))));
    }
    return Err((StatusCode::CONFLICT,Json(json!({
      "error":"another room owner turn is running; inspect /state before resending"
    }))));
  };
  let (room,inserted,index,requested_initial)=
      admit_owner_message(&engine,id,&input).await?;
  let newest=room.messages.iter().rposition(|m|m.speaker=="user")==Some(index);
  let started=newest&&room.status==RoomStatus::Open
    &&!speakers_for_owner_turn(&room).is_empty();
  let (expected,actual)=owner_turn_progress(&room,index,
    if requested_initial.is_empty(){&room.recipients}else{&requested_initial});
  if started {
    let worker=Arc::clone(&engine);
    tokio::spawn(async move {
      let _lease=lease;
      if let Err((status,_))=generate(&worker,id).await{
        tracing::warn!(room=%id,status=?status,
          "background room fanout interrupted; durable partial replies retained");
      }
    });
  }else{drop(lease);}
  if inserted{crate::addons::dispatch_room(&engine,&room).await;}
  let message=message_rows(&room).get(index).cloned()
    .ok_or_else(||failed("stored message disappeared".into()))?;
  let job=if started{json!({
    "id":input.client_message_id,"kind":"room_turn","status":"running",
    "createdAt":timestamp(room.updated_at_ms),
    "stepIndex":actual,"stepTotal":expected,
    "input":{"roomId":id.to_string(),"sessionId":room_session(id)}
  })}else{completed_job(&room,actual,expected,&input.client_message_id)};
  Ok((if started{StatusCode::ACCEPTED}else{StatusCode::OK},
    Json(json!({
      "room":room_view(&room),"message":message,"job":job,
      "sessionId":room_session(id)
    }))))
}
async fn run(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let _lease=engine.groups.try_frontend_turn(id).ok_or_else(||(
    StatusCode::CONFLICT,Json(json!({"error":"room turn already running"}))))?;
  let (room,turns,expected)=generate(&engine,id).await?;
  Ok(Json(json!({"room":room_view(&room),"job":completed_job(&room,turns,expected,&format!("manual-{}-{}",id,room.rounds)),
    "sessionId":room_session(id)})))
}
async fn reset(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.reset(id).await.map_err(bad)?;
  Ok(Json(json!({"ok":true,"room":room_view(&room),"sessionId":room_session(id)})))
}
async fn stop(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.stop(id).await.map_err(bad)?;
  Ok(Json(json!({"ok":true,"room":room_view(&room)})))
}
async fn knowledge(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.get(id).await.ok_or_else(missing)?;
  let all=engine.knowledge.list(&format!("room:{id}")).await;
  Ok(Json(json!(all.iter().map(|entry|json!({
    "id":entry.id,"type":"room_note","text":entry.content,
    "status":"active","verification":"unverified",
    "sources":[{"roomId":room.id,"type":"recorded_transcript"}]
  })).collect::<Vec<_>>())))
}
async fn learn(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.get(id).await.ok_or_else(missing)?;
  let text=room.messages.iter().rev().filter(|m|m.speaker!="user")
    .take(6).map(|m|format!("{}: {}",m.speaker,m.content))
    .collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n\n");
  if text.is_empty(){return Err(bad("no agent conversation to learn from"));}
  let content=if text.chars().count()>3900{text.chars().rev().take(3900).collect::<String>().chars().rev().collect::<String>()}else{text};
  let saved=engine.knowledge.remember(&format!("room:{id}"),
    &format!("Room transcript summary at turn {}",room.rounds),&content).await.map_err(bad)?;
  Ok(Json(json!({"ok":true,"id":saved.id,"text":saved.content})))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct PromptQuery{agent_id:Option<String>}
async fn system_prompt(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
  Query(q):Query<PromptQuery>)->Result<Json<Value>,Fault>{
  let room=engine.groups.get(id).await.ok_or_else(missing)?;
  let participant=room.participants.iter()
    .find(|p|Some(p.name.as_str())==q.agent_id.as_deref())
    .or_else(||room.participants.first()).ok_or_else(||bad("room has no agents"))?;
  let common=crate::group::Groups::shared_prefix(&room);
  let persona=crate::group::Groups::persona_prompt(participant);
  let prompt=format!("{common}\n\n[Final operator-selected participant metadata follows shared room evidence]\n{persona}");
  let chars=prompt.chars().count();
  Ok(Json(json!({
    "common":common,"persona":persona,"prompt":prompt,
    "chars":chars,"tokens":chars.div_ceil(3),
    "commonChars":common.chars().count(),"commonTokens":common.chars().count().div_ceil(3),
    "personaChars":persona.chars().count(),"personaTokens":persona.chars().count().div_ceil(3),
    "cacheLayout":"shared_system_and_room_evidence_before_participant_identity",
    "estimated":true
  })))
}

#[cfg(test)]
mod fanout_tests;
