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
    routing_mode:"discussion".into(),rounds:1,max_turns:room.max_rounds,
    token_budget:0,context_budget_tokens:2000,tools_enabled:false,
    auto_learning:false,auto_models:true,models:vec![],
    reasoning_effort:String::new(),learned_context:String::new(),
  })
}
pub fn room_view(room:&Room)->Value{
  let conf=config_of(room);
  json!({
    "id":room.id.to_string(),"name":conf.name,"topic":conf.topic,
    "ownerName":conf.owner_name,"agentIds":conf.agent_ids,
    "routingMode":conf.routing_mode,"rounds":conf.rounds,
    "maxTurns":conf.max_turns,"tokenBudget":conf.token_budget,
    "contextBudgetTokens":conf.context_budget_tokens,
    "toolsEnabled":conf.tools_enabled,"autoLearning":conf.auto_learning,
    "autoModels":conf.auto_models,"models":conf.models,
    "reasoningEffort":conf.reasoning_effort,"sessionId":room_session(room.id),
    "status":if room.status==RoomStatus::Finished{"completed"}else{"idle"},
    "updatedAt":timestamp(room.updated_at_ms)
  })
}
pub fn message_rows(room:&Room)->Vec<UiMessage>{
  let session_id=room_session(room.id);
  room.messages.iter().enumerate().map(|(i,m)|{
    let user=m.speaker=="user";
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
        "model":m.model
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
  auto_models:bool,tools_enabled:bool,auto_learning:bool,
  #[serde(default)]models:Vec<Value>,#[serde(default)]reasoning_effort:String,
}
async fn checked(engine:&Engine,input:&RoomDraft)->Result<(FrontendRoom,Vec<Participant>),Fault>{
  if input.name.trim().is_empty()||input.name.len()>120||input.topic.len()>10_000
    ||input.owner_name.len()>120||!(1..=32).contains(&input.agent_ids.len())
    ||!(1..=12).contains(&input.rounds)
    ||input.max_turns>10_000||input.context_budget_tokens>8_000{
    return Err(bad("invalid room title, participant count, rounds or limit"));
  }
  if !matches!(input.routing_mode.as_str(),"discussion"|"round_robin"|"mentioned"){
    return Err(bad("unknown room routing mode"));
  }
  if input.tools_enabled{
    return Err(unsupported("Room-scoped tool execution is not supported by this native group runtime; no tools will execute without an explicit real implementation."));
  }
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
    max_turns:input.max_turns,token_budget:input.token_budget,
    context_budget_tokens:input.context_budget_tokens.clamp(200,8_000),
    tools_enabled:false,auto_learning:input.auto_learning,
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
    participants:participants.clone(),max_rounds:Some(if view.max_turns==0{10000}else{view.max_turns}),
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
  engine.groups.get(id).await.map(|room|Json(message_rows(&room)))
    .ok_or_else(missing)
}
fn completed_job(room:&Room,turns:usize,receipt:&str)->Value{
  json!({
    "id":receipt,
    "kind":"room_turn","status":if turns>0{"completed"}else{"skipped"},
    "createdAt":timestamp(room.updated_at_ms),
    "input":{"roomId":room.id.to_string(),"sessionId":room_session(room.id)},
    "result":{"rounds":turns,"totalTokens":room.used_tokens}
  })
}
async fn state(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let room=engine.groups.get(id).await.ok_or_else(missing)?;
  Ok(Json(json!({
    "room":room_view(&room),"sessionId":room_session(id),
    "jobs":[],"executions":[],"learningJobs":[],"operations":[],"approvals":[],
    "status":if room.status==RoomStatus::Finished{"completed"}else{"idle"},
    "queueDepth":0,
    "usage":{"totalTokens":room.used_tokens,"estimatedCalls":0,"calls":room.rounds}
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
pub(crate) async fn generate(engine:&Arc<Engine>,id:Uuid)->Result<(Room,usize),Fault>{
  let before=engine.groups.get(id).await.ok_or_else(missing)?;
  let config=config_of(&before);
  let mut room=before;
  let mut turns=0;
  for _ in 0..config.rounds{
    if room.status==RoomStatus::Finished{break;}
    // Read exact-scope durable notes. They influence subsequent turns as
    // explicitly lower-trust task context, never as system instructions.
    let notes=engine.knowledge.list(&format!("room:{id}")).await;
    let excerpts=notes.iter().take(4).map(|n|format!("{}: {}",n.title,n.content))
        .collect::<Vec<_>>().join("\n");
    let excerpt=if excerpts.chars().count()>1250{
        excerpts.chars().take(1250).collect::<String>()
    }else{excerpts};
    engine.groups.set_knowledge_context(id,excerpt).await.map_err(bad)?;
    let index=crate::group::Groups::select(&room);
    let agent_id=room.participants.get(index)
        .ok_or_else(||bad("room speaker missing"))?.name.as_str();
    let (provider,model)=if !config.auto_models && !config.models.is_empty(){
        let selected=&config.models[room.rounds%config.models.len()];
        (selected.get("providerId").and_then(Value::as_str).unwrap_or("").to_owned(),
         selected.get("model").and_then(Value::as_str).unwrap_or("").to_owned())
    }else{
        let agent=engine.frontend.agents().await.into_iter()
            .find(|a|a.id==agent_id)
            .ok_or_else(||bad("selected room agent was deleted"))?;
        (agent.provider_id,agent.model)
    };
    let selected_model=engine.model_for_selection(&provider,&model).map_err(bad)?;
    engine.providers.throttle(&provider).await;
    room=engine.groups.next(&selected_model,id).await.map_err(bad)?;
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
  crate::frontend_channels::dispatch_room(engine,&room).await;
  Ok((room,turns))
}
async fn send(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
  Json(input):Json<SayInput>)->Result<Json<Value>,Fault>{
  if input.input.trim().is_empty(){return Err(bad("room message cannot be empty"));}
  if input.reasoning_language.as_deref().is_some_and(|v|!matches!(v,"ko"|"default"|"original")){
    return Err(bad("unsupported reasoning language"));
  }
  let mut recipients=input.agent_ids;
  if recipients.is_empty() {
    // Main RoomsPage accepts plain @DisplayName mentions as well as
    // explicit selected agent IDs. Resolve them against real room members.
    let room=engine.groups.get(id).await.ok_or_else(missing)?;
    let text=input.input.to_lowercase();
    for agent in engine.frontend.agents().await {
      if room.participants.iter().any(|p|p.name==agent.id) &&
          !agent.name.is_empty() &&
          text.contains(&format!("@{}",agent.name.to_lowercase())){
        recipients.push(agent.id);
      }
    }
  }
  let (before,inserted,index)=engine.groups.say_once(
    id,input.input.trim().to_owned(),&input.client_message_id,recipients
  ).await.map_err(bad)?;
  let (room,turns)=if inserted{
    let mode=config_of(&before).routing_mode;
    // Mention-only rooms don't create an unsolicited model answer.
    let addressed=before.recipients.len()>0 ||
      before.participants.iter().any(|p|input.input.contains(&format!("@{}",p.name)));
    if mode=="mentioned"&&!addressed{(before,0)}
    else{generate(&engine,id).await?}
  }else{(before,0)};
  if inserted{crate::frontend_channels::dispatch_room(&engine,&room).await;}
  let message=message_rows(&room).get(index).cloned().ok_or_else(||failed("stored message disappeared".into()))?;
  Ok(Json(json!({
    "room":room_view(&room),"message":message,
    "job":completed_job(&room,turns,&input.client_message_id),"sessionId":room_session(id)
  })))
}
async fn run(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
  ->Result<Json<Value>,Fault>{
  let (room,turns)=generate(&engine,id).await?;
  Ok(Json(json!({"room":room_view(&room),"job":completed_job(&room,turns,&format!("manual-{}-{}",id,room.rounds)),
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
  let common=format!("Room topic: {}. Participant: {}.",room.topic,participant.name);
  let persona=participant.role.clone();
  let prompt=crate::group::Groups::system_prompt(&room,participant);
  let chars=prompt.chars().count();
  Ok(Json(json!({
    "common":common,"persona":persona,"prompt":prompt,
    "chars":chars,"tokens":chars.div_ceil(3),
    "commonChars":common.chars().count(),"commonTokens":common.chars().count().div_ceil(3),
    "personaChars":persona.chars().count(),"personaTokens":persona.chars().count().div_ceil(3),
    "estimated":true
  })))
}
