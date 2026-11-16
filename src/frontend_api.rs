//! Compatibility for the **unchanged** latest-main React frontend.
//! Every implemented endpoint reads/writes the existing Dominion Rust services.
//! Unsupported operations reject explicitly; no fake success responses.
use crate::{
    domain::{CreateRun,ReconcileTool,Run,RunStatus,now_ms},
    engine::Engine,
    frontend_state::{UiAgent,UiMessage,UiSession,timestamp},
    sdk::Dominion,
};
use axum::{
    extract::{Path,State,Query},http::StatusCode,
    response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},
    routing::{get,post},Json,Router,
};
use futures_core::Stream;
use serde::Deserialize;
use serde_json::{json,Value};
use std::{convert::Infallible,sync::Arc,time::Duration};
use uuid::Uuid;
use base64::Engine as _;
type Fault=(StatusCode,Json<Value>);
fn bad(msg:impl Into<String>)->Fault{(StatusCode::BAD_REQUEST,Json(json!({"error":msg.into()})))}
fn missing(msg:impl Into<String>)->Fault{(StatusCode::NOT_FOUND,Json(json!({"error":msg.into()})))}
fn unsupported(msg:&str)->Fault{(StatusCode::NOT_IMPLEMENTED,Json(json!({"error":msg})))}
fn broken(msg:String)->Fault{(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":msg})))}
fn busy(msg:impl Into<String>)->Fault{(StatusCode::CONFLICT,Json(json!({"error":msg.into()})))}

pub fn router()->Router<Arc<Engine>>{
    Router::new()
        .route("/health",get(health))
        .route("/status",get(status))
        .route("/events",get(global_events))
        .route("/settings/owner-profile",get(owner_profile).post(save_owner_profile))
        .route("/agents",get(agents).post(save_agent))
        .route("/agents/bulk-model",post(bulk_agent_model))
        .route("/agents/{id}",axum::routing::delete(delete_agent))
        .route("/sessions",get(sessions).post(create_session))
        .route("/sessions/{id}",get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages",get(session_messages))
        .route("/sessions/{id}/usage",get(session_usage))
        .route("/sessions/{id}/rename",post(rename_session))
        .route("/sessions/{id}/clear",post(clear_session))
        .route("/sessions/{id}/fork",post(fork_session))
        .route("/sessions/{id}/cancel",post(cancel_session))
        .route("/chat",post(chat))
        .route("/approvals",get(approvals).post(resolve_approval))
        .route("/policy",get(policy).post(save_policy))
        .route("/jobs",get(jobs).post(create_job))
        .route("/jobs/{id}",get(job))
        .route("/jobs/{id}/cancel",post(cancel_job))
        .route("/jobs/{id}/resume",post(resume_job))
        .route("/tools",get(tools_catalog))
        .route("/executions",get(executions))
        .route("/executions/{id}/operations",get(execution_operations))
        .route("/executions/{id}/cancel",post(cancel_job))
        // Local Ollama runtime routes are owned by frontend_local_models.
        // Real external channel routes are owned by frontend_channels.
        .route("/memories",get(memories).post(save_memory))
        .route("/memories/{id}",axum::routing::delete(delete_memory))
        .route("/sdk/runs",get(sdk_runs).post(sdk_start))
        .route("/sdk/runs/{id}",get(sdk_run))
        .route("/sdk/runs/{id}/approve",post(sdk_approve))
        .route("/sdk/runs/{id}/resume",post(sdk_resume))
        .route("/sdk/runs/{id}/cancel",post(sdk_cancel))
        .route("/sdk/runs/{id}/reconcile",post(sdk_reconcile))
        // Unimplemented main features return a precise JSON error instead of
        // silently serving index.html (which the UI could mistake for success).
        .merge(crate::frontend_video::routes())
        .merge(crate::frontend_video_production::routes())
        .merge(crate::frontend_video_models::routes())
        .merge(crate::frontend_rooms::routes())
        .merge(crate::frontend_workflows::routes())
        .merge(crate::frontend_providers::routes())
        .merge(crate::frontend_rsi::routes())
        .merge(crate::frontend_extensions::routes())
        .merge(crate::frontend_automations::routes())
        .merge(crate::frontend_developer::routes())
        .merge(crate::frontend_growth::routes())
        .merge(crate::frontend_personas::routes())
        .merge(crate::frontend_system::routes())
        .merge(crate::frontend_local_models::routes())
        .merge(crate::frontend_channels::routes())
        .merge(crate::frontend_skill_import::routes())
        .merge(crate::frontend_oauth::routes())
        .merge(crate::frontend_youtube::routes())
        .fallback(unsupported_endpoint)
}
async fn unsupported_endpoint()->Fault{
    unsupported("This main frontend action has no native Rust implementation yet; the UI is preserved without fabricated functionality.")
}
async fn health()->Json<Value>{
    Json(json!({
        "ok":true,"service":"dominion","engine":"rust","runtime":"rust",
        "version":env!("CARGO_PKG_VERSION"),
        "branch":option_env!("DOMINION_BUILD_BRANCH").unwrap_or("unknown"),
        "commit":option_env!("DOMINION_BUILD_COMMIT").unwrap_or("unknown")
    }))
}
fn is_configured(engine:&Engine)->bool{
    !engine.settings.main.api_key.is_empty()
        || engine.settings.main.base_url.contains("localhost")
        || engine.settings.main.base_url.contains("127.0.0.1")
}
async fn status(State(engine):State<Arc<Engine>>)->Json<Value>{
    let runs=engine.store.list().await;
    let agents=engine.frontend.agents().await;
    let sessions=engine.frontend.sessions().await;
    let active=runs.iter().filter(|run|matches!(run.status,RunStatus::Queued|RunStatus::Running)).count();
    Json(json!({
        "engine":"rust","runtime":"rust",
        "providers":usize::from(is_configured(&engine))+engine.providers.list().iter().filter(|p|p.enabled).count(),
        "agents":agents.len(),"sessions":sessions.len(),"jobs":active,
        "executions":runs.len(),"mcp":engine.mcp.server_count()
    }))
}
async fn owner_profile(State(engine):State<Arc<Engine>>)->Json<Value>{
    let record=engine.owner_profile.get().await;
    Json(json!({"name":record.name,"avatar":record.avatar,"updatedAt":timestamp(record.updated_at_ms)}))
}
async fn save_owner_profile(State(engine):State<Arc<Engine>>,
    Json(value):Json<crate::profile::NewOwnerProfile>)->Result<Json<Value>,Fault>{
    let record=engine.owner_profile.save(value).await.map_err(bad)?;
    Ok(Json(json!({"name":record.name,"avatar":record.avatar,"updatedAt":timestamp(record.updated_at_ms)})))
}
async fn model_runtime()->Json<Value>{
    Json(json!({"status":"idle","engine":null,"model":null,"managedBy":"external"}))
}
async fn model_installed()->Json<Value>{Json(json!([]))}
async fn channels()->Json<Value>{Json(json!([]))}
async fn agents(State(engine):State<Arc<Engine>>)->Json<Vec<UiAgent>>{
    Json(engine.frontend.agents().await)
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct AgentDraft{
    id:Option<String>,
    name:String,
    description:Option<String>,
    system_prompt:Option<String>,
    provider_id:Option<String>,
    model:Option<String>,
    color:Option<String>,role:Option<String>,aliases:Option<Vec<String>>,
    avatar_url:Option<String>,avatar:Option<String>,
    reasoning_effort:Option<String>,thinking:Option<bool>,
}
fn agent_model(engine:&Engine,provider:Option<&str>,model:Option<&str>)->Result<(),Fault>{
    let provider=provider.filter(|v|!v.is_empty()).unwrap_or("dominion-native");
    let model=model.filter(|v|!v.is_empty()).unwrap_or(if provider=="dominion-native"{
        engine.settings.main.name.as_str()
    }else{""});
    if provider=="dominion-native"
        && crate::frontend_local_models::native_override(&engine.settings,model).is_some() {
        return Ok(());
    }
    engine.providers.resolve(provider,model,&engine.settings.main)
        .map(|_|()).map_err(bad)
}
fn chosen(engine:&Engine,provider:Option<&str>,model:Option<&str>)
    ->Result<(String,String),Fault>{
    let id=provider.filter(|v|!v.is_empty()).unwrap_or("dominion-native");
    let selected=model.filter(|v|!v.is_empty()).map(str::to_owned)
        .or_else(||{
            if id=="dominion-native"{Some(engine.settings.main.name.clone())}
            else{engine.providers.get(id).map(|p|p.model)}
        }).ok_or_else(||bad("selected provider has no configured model"))?;
    agent_model(engine,Some(id),Some(&selected))?;
    Ok((id.to_owned(),selected))
}
async fn save_agent(State(engine):State<Arc<Engine>>,Json(draft):Json<AgentDraft>)
    ->Result<Json<UiAgent>,Fault>{
    if draft.name.trim().is_empty()||draft.name.len()>120{
        return Err(bad("agent name required (up to 120 bytes)"));
    }
    if draft.system_prompt.as_ref().is_some_and(|x|x.len()>12000){
        return Err(bad("agent instructions exceed 12000 bytes"));
    }
    let (selected_provider,selected_model)=chosen(
        &engine,draft.provider_id.as_deref(),draft.model.as_deref())?;
    if draft.thinking==Some(false)||draft.reasoning_effort.as_deref().is_some_and(|v|!v.is_empty()){
        return Err(unsupported("Agent-specific thinking and reasoning-effort settings are not yet implemented by the native model adapter."));
    }
    let avatar=draft.avatar.unwrap_or_default();
    if !avatar.is_empty(){
        let (mime,encoded)=avatar.split_once(";base64,")
            .ok_or_else(||bad("avatar requires PNG/JPEG/WebP data URL"))?;
        if !matches!(mime,"data:image/png"|"data:image/jpeg"|"data:image/webp") || encoded.len()>1_400_000{
            return Err(bad("avatar requires PNG/JPEG/WebP and a maximum 1 MiB file"));
        }
        let bytes=base64::engine::general_purpose::STANDARD.decode(encoded)
            .map_err(|_|bad("avatar has invalid base64"))?;
        if bytes.len()>1_048_576{return Err(bad("avatar exceeds 1 MiB"));}
        let valid=match mime{
            "data:image/png"=>bytes.starts_with(&[0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a]),
            "data:image/jpeg"=>bytes.starts_with(&[0xff,0xd8,0xff]),
            "data:image/webp"=>bytes.len()>=12&&&bytes[..4]==b"RIFF"&&&bytes[8..12]==b"WEBP",
            _=>false
        };
        if !valid{return Err(bad("avatar file signature does not match image type"));}
    }
    let id=draft.id.as_deref().filter(|x|!x.trim().is_empty())
        .map(str::to_owned).unwrap_or_else(||Uuid::new_v4().to_string());
    if id.len()>120{return Err(bad("invalid agent id"));}
    let record=UiAgent{
        id:id.clone(),name:draft.name.trim().to_owned(),
        description:draft.description.unwrap_or_default(),
        system_prompt:draft.system_prompt.unwrap_or_default(),
        provider_id:selected_provider,model:selected_model,
        color:draft.color.unwrap_or_default(),role:draft.role.unwrap_or_default(),
        reasoning_effort:draft.reasoning_effort,thinking:draft.thinking,
        aliases:draft.aliases.unwrap_or_default(),
        avatar_url:draft.avatar_url.unwrap_or_default(),avatar,
    };
    engine.frontend.mutate(|state|{
        if draft.id.is_some()&&!state.agents.contains_key(&id){return Err("agent to update not found".into());}
        if state.agents.len()>=128&&!state.agents.contains_key(&id){return Err("maximum 128 agents".into());}
        state.agents.insert(id,record.clone());
        Ok(record.clone())
    }).await.map(Json).map_err(bad)
}
async fn delete_agent(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    if id=="general"{return Err(bad("the built-in general agent cannot be deleted"));}
    engine.frontend.mutate(|state|{
        if state.sessions.values().any(|s|s.agent_id==id){return Err("agent is bound to existing chat sessions".into());}
        if state.agents.remove(&id).is_none(){return Err("agent not found".into());}
        Ok(())
    }).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct BulkAgentModel{agent_ids:Vec<String>,provider_id:String,model:String,reasoning_effort:Option<String>}
async fn bulk_agent_model(State(engine):State<Arc<Engine>>,Json(body):Json<BulkAgentModel>)
    ->Result<Json<Value>,Fault>{
    if body.agent_ids.is_empty()||body.agent_ids.len()>100{return Err(bad("select 1-100 agents"));}
    agent_model(&engine,Some(&body.provider_id),Some(&body.model))?;
    if body.reasoning_effort.as_deref().is_some_and(|v|!v.is_empty()){
        return Err(unsupported("Per-agent reasoning effort is not implemented by the native model adapter."));
    }
    let (count,records)=engine.frontend.mutate(|state|{
        if body.agent_ids.iter().any(|id|!state.agents.contains_key(id)){
            return Err("one or more agents are missing".into());
        }
        for id in &body.agent_ids{
            let agent=state.agents.get_mut(id).ok_or("agent vanished")?;
            agent.provider_id=body.provider_id.clone();
            agent.model=body.model.clone();
            agent.reasoning_effort=body.reasoning_effort.clone();
        }
        let results=body.agent_ids.iter().filter_map(|id|state.agents.get(id).cloned()).collect::<Vec<_>>();
        Ok((results.len(),results))
    }).await.map_err(bad)?;
    Ok(Json(json!({"count":count,"agents":records})))
}


fn public_session(session:&UiSession)->Value{
    json!({
        "id":session.id,"title":session.title,"agentId":session.agent_id,
        "providerId":session.provider_id,"model":session.model,
        "createdAt":session.created_at,"updatedAt":session.updated_at,
        "roomId":session.room_id,"archived":false
    })
}
async fn sync_session(engine:&Arc<Engine>,id:&str)->Result<(),String>{
    let Some(session)=engine.frontend.session(id).await else{return Ok(());};
    let Some(run_id)=session.pending_run else{return Ok(());};
    let Some(run)=engine.store.get(run_id).await else{return Ok(());};
    if !run.terminal(){return Ok(());}
    let answer=if run.status==RunStatus::Completed{
        run.answer.clone().filter(|text|!text.trim().is_empty())
    }else{None};
    let message=answer.map(|text|UiMessage{
        id:Uuid::new_v4().to_string(),session_id:id.into(),
        role:"assistant".into(),content:text,created_at:timestamp(now_ms()),
        agent_id:Some(session.agent_id.clone()),
        metadata:Some(json!({
            "executionId":run.id,
            "model":engine.settings.main.name,
            "status":run.status,
            "costKrw":run.cost_krw,
            "usage":{
                "promptTokens":run.input_tokens,"cachedPromptTokens":run.cached_input_tokens,
                "completionTokens":run.output_tokens,
                "totalTokens":run.input_tokens+run.output_tokens,
                "model":engine.settings.main.name
            }
        })),
    });
    // No assistant message is fabricated on failure/cancellation.
    engine.frontend.finish_turn(id,run_id,message).await?;
    Ok(())
}
async fn sessions(State(engine):State<Arc<Engine>>)->Json<Value>{
    let before=engine.frontend.sessions().await;
    for session in &before{
        if session.pending_run.is_some(){
            if let Err(e)=sync_session(&engine,&session.id).await{
                tracing::warn!(session=%session.id,error=%e,"unable to reconcile frontend session");
            }
        }
    }
    Json(Value::Array(engine.frontend.sessions().await.iter().map(public_session).collect()))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct CreateSession{
    title:Option<String>,agent_id:Option<String>,provider_id:Option<String>,model:Option<String>
}
async fn create_session(State(engine):State<Arc<Engine>>,Json(body):Json<CreateSession>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    let agent_id=body.agent_id.as_deref().filter(|x|!x.is_empty()).unwrap_or("general");
    let agents=engine.frontend.agents().await;
    if !agents.iter().any(|a|a.id==agent_id){return Err(bad("selected agent is not registered"));}
    let agent=agents.iter().find(|a|a.id==agent_id).ok_or_else(||bad("agent not found"))?;
    let (selected_provider,selected_model)=chosen(&engine,
        body.provider_id.as_deref().or(Some(agent.provider_id.as_str())),
        body.model.as_deref().or(Some(agent.model.as_str())))?;
    let title=body.title.as_deref().unwrap_or("새 대화").trim();
    let title=if title.is_empty(){"새 대화"}else{title};
    let session=UiSession{
        id:Uuid::new_v4().to_string(),title:title.chars().take(200).collect(),
        agent_id:agent_id.into(),provider_id:selected_provider,
        model:selected_model,
        created_at:timestamp(now_ms()),updated_at:timestamp(now_ms()),
        room_id:None,pending_run:None,messages:Vec::new()
    };
    engine.frontend.mutate(|store|{
        if store.sessions.len()>=256{return Err("maximum 256 chat sessions reached".into());}
        store.sessions.insert(session.id.clone(),session.clone());
        Ok(())
    }).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(public_session(&session))))
}
async fn get_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    let record=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    Ok(Json(public_session(&record)))
}
async fn delete_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    engine.frontend.mutate(|store|{
        if store.sessions.get(&id).is_some_and(|s|s.room_id.is_some()){
            return Err("room-linked sessions must be removed from the group management page".into());
        }
        if store.sessions.get(&id).is_some_and(|s|s.pending_run.is_some()){
            return Err("stop the active chat run before deleting the session".into());
        }
        if store.sessions.remove(&id).is_none(){return Err("session not found".into());}
        Ok(())
    }).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn session_messages(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Vec<UiMessage>>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    let record=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    if let Some(room_id)=record.room_id{
        let rid=Uuid::parse_str(&room_id).map_err(|_|missing("room binding invalid"))?;
        let room=engine.groups.get(rid).await.ok_or_else(||missing("bound room not found"))?;
        return Ok(Json(crate::frontend_rooms::message_rows(&room)));
    }
    Ok(Json(record.messages))
}
async fn session_usage(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    let record=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    if let Some(room_id)=record.room_id{
        let rid=Uuid::parse_str(&room_id).map_err(|_|missing("room binding invalid"))?;
        let room=engine.groups.get(rid).await.ok_or_else(||missing("bound room not found"))?;
        return Ok(Json(json!({"totalTokens":room.used_tokens})));
    }
    let total=record.messages.iter().map(|m|m.metadata.as_ref()
        .and_then(|v|v.pointer("/usage/totalTokens")).and_then(Value::as_u64).unwrap_or(0)).sum::<u64>();
    Ok(Json(json!({"totalTokens":total})))
}
#[derive(Deserialize)]
struct Rename{title:String}
async fn rename_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>,Json(input):Json<Rename>)
    ->Result<Json<Value>,Fault>{
    let title=input.title.trim();
    if title.is_empty()||title.len()>200{return Err(bad("title must be 1-200 bytes"));}
    engine.frontend.mutate(|store|{
        let record=store.sessions.get_mut(&id).ok_or("session not found")?;
        record.title=title.into();record.updated_at=timestamp(now_ms());
        Ok(public_session(record))
    }).await.map(Json).map_err(bad)
}
async fn clear_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    engine.frontend.mutate(|store|{
        let session=store.sessions.get_mut(&id).ok_or("session not found")?;
        if session.room_id.is_some(){return Err("reset room-linked transcripts through the room UI".into());}
        if session.pending_run.is_some(){return Err("cannot clear an active conversation".into());}
        session.messages.clear();
        session.updated_at=timestamp(now_ms());
        Ok(())
    }).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn fork_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    let original=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    let source_messages=if let Some(room_id)=&original.room_id{
        let rid=Uuid::parse_str(room_id).map_err(|_|bad("bound room invalid"))?;
        let room=engine.groups.get(rid).await.ok_or_else(||missing("bound room not found"))?;
        crate::frontend_rooms::message_rows(&room)
    }else{original.messages.clone()};
    let fork=engine.frontend.mutate(|store|{
        let original=store.sessions.get(&id).ok_or("session not found")?.clone();
        if original.pending_run.is_some(){return Err("cannot fork an active conversation".into());}
        if store.sessions.len()>=256{return Err("maximum 256 chat sessions reached".into());}
        let mut next=original;
        next.room_id=None;
        next.messages=source_messages;
        next.id=Uuid::new_v4().to_string();
        next.title=format!("{} (fork)",next.title);
        next.created_at=timestamp(now_ms());next.updated_at=next.created_at.clone();
        next.messages=next.messages.iter().map(|msg|{
            let mut copy=msg.clone();
            copy.id=Uuid::new_v4().to_string();
            copy.session_id=next.id.clone();
            copy
        }).collect();
        store.sessions.insert(next.id.clone(),next.clone());
        Ok(next)
    }).await.map_err(bad)?;
    Ok(Json(public_session(&fork)))
}
async fn cancel_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    let record=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    if let Some(room_id)=record.room_id{
        let rid=Uuid::parse_str(&room_id).map_err(|_|bad("bound room invalid"))?;
        engine.groups.stop(rid).await.map_err(bad)?;
        return Ok(Json(json!({"cancelled":true})));
    }
    let Some(run_id)=record.pending_run else{return Ok(Json(json!({"cancelled":false})));};
    let run=engine.store.get(run_id).await.ok_or_else(||missing("bound native run missing"))?;
    if run.terminal(){
        sync_session(&engine,&id).await.map_err(broken)?;
        return Ok(Json(json!({"cancelled":false})));
    }
    engine.cancel(run_id).await.map_err(bad)?;
    Ok(Json(json!({"cancelled":true})))
}

#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct ChatInput {
    session_id:String,agent_id:String,input:String,
    provider_id:Option<String>,model:Option<String>,
    #[serde(default="yes")]tools_enabled:bool,
    #[serde(default="yes")]tools_execution_enabled:bool,
    #[serde(default="yes")]auto_learning:bool,
    reasoning_effort:Option<String>,
    reasoning_language:Option<String>,
    thinking:Option<bool>
}
fn yes()->bool{true}
async fn chat(State(engine):State<Arc<Engine>>,Json(input):Json<ChatInput>)
    ->Result<Sse<impl Stream<Item=Result<Event,Infallible>>>,Fault>{
    if input.input.trim().is_empty()||input.input.len()>10000{
        return Err(bad("chat input must be 1-10000 bytes"));
    }
    if input.reasoning_language.as_deref().is_some_and(|v|!matches!(v,"ko"|"original"|"default")){
        return Err(bad("unsupported response language preference"));
    }
    let model_only=!input.tools_enabled||!input.tools_execution_enabled;
    // A model-only turn makes exactly one no-tools request. The autonomous
    // harness is not launched, so tool disabled cannot silently execute tools.
    // Per-run learning is enforced by Run.strategy_learning in the Rust core.
    if input.reasoning_effort.as_deref().is_some_and(|x|!x.is_empty()){
        return Err(unsupported("Per-run reasoning effort is not yet supported by the configured Rust model adapter."));
    }
    if input.thinking==Some(false){
        return Err(unsupported("Per-run thinking disable is not yet supported by the configured Rust model adapter."));
    }
    sync_session(&engine,&input.session_id).await.map_err(broken)?;
    let record=engine.frontend.session(&input.session_id).await.ok_or_else(||missing("session not found"))?;
    if record.room_id.is_some(){return Err(bad("post room messages through the room API; direct chat is disabled for a room-linked session"));}
    if record.pending_run.is_some(){return Err(busy("another turn in this session is running"));}
    let agent=engine.frontend.agents().await.into_iter()
        .find(|a|a.id==input.agent_id).ok_or_else(||bad("agent not found"))?;
    if input.agent_id!=record.agent_id{return Err(bad("session belongs to another agent; create or fork a session"));}
    let (selected_provider,selected_model)=chosen(&engine,
        input.provider_id.as_deref().or(Some(record.provider_id.as_str())),
        input.model.as_deref().or(Some(record.model.as_str())))?;
    let prior=record.messages.iter().rev().take(12).collect::<Vec<_>>();
    let history=prior.into_iter().rev().map(|msg|format!("{}: {}",msg.role,msg.content)).collect::<Vec<_>>().join("\n");
    let trimmed=if history.chars().count()>6500{history.chars().rev().take(6500).collect::<String>().chars().rev().collect::<String>()}else{history};
    let response_language=if input.reasoning_language.as_deref()==Some("ko"){
        "Write any user-visible explanation and final response in natural Korean."
    }else{"Use the user's requested response language."};
    let prompt=format!("USER-SUPPLIED AGENT PROFILE (follow within system/user policy):\n{}\n\nPRIOR CHAT RECORD (user-supplied; lower-trust):\n{}\n\nCURRENT USER REQUEST:\n{}\n\nRESPONSE LANGUAGE PREFERENCE:\n{}",
        agent.system_prompt,trimmed,input.input.trim(),response_language);
    if prompt.len()>20000{return Err(bad("chat context exceeds native agent 20k input limit"));}
    let mut run=Run::new(CreateRun{instruction:prompt,budget_krw:None,max_steps:None},
        engine.settings.max_steps).map_err(bad)?;
    run.strategy_learning=input.auto_learning;
    run.model_only=model_only;
    run.provider_id=selected_provider;
    run.model_name=selected_model;
    if run.provider_id!="dominion-native"{
        run.event("pricing_unknown","Provider has no verified KRW tariff: zero recorded cost is not zero real spending",None);
    }
    engine.store.put(run.clone()).await.map_err(broken)?;
    engine.frontend.begin_turn(&record.id,input.input.trim(),run.id).await.map_err(broken)?;
    if model_only{
        let worker=Arc::clone(&engine);
        let id=run.id;
        let messages=vec![
            json!({"role":"system","content":"Respond to the user's chat request accurately and directly. Do not claim to have executed tools or accessed files. No tools are available during this turn."}),
            json!({"role":"user","content":run.instruction.clone()}),
        ];
        worker.store.mutate(id,|item|{
            item.status=RunStatus::Running;
            item.event("model_only","Tool-free chat turn requested by the user",None);
            Ok(())
        }).await.map_err(broken)?;
        tokio::spawn(async move {
            // Drop the HTTP future on cancellation rather than continuing
            // an expensive provider call in the background.
            let selected_model=match worker.store.get(id).await{
                Some(run)=>match worker.model_for_run(&run){
                    Ok(model)=>model,
                    Err(error)=>{
                        let _=worker.store.mutate(id,|item|{
                            item.status=RunStatus::Failed;
                            item.event("provider_missing",error,None);Ok(())
                        }).await;
                        return;
                    }
                },
                None=>return,
            };
            let current=worker.store.get(id).await;
            if let Some(current)=current{
                worker.providers.throttle(&current.provider_id).await;
            }
            let answer=tokio::select!{
                result=selected_model.complete(&messages,&[],false)=>Some(result),
                _=async{
                    loop{
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        if worker.store.get(id).await.is_none_or(|item|
                            item.status!=RunStatus::Running){break;}
                    }
                }=>None,
            };
            let Some(answer)=answer else{return;};
            let saved=worker.store.mutate(id,|item|{
                // Cancellation can win while the provider is responding.
                // Never resurrect a cancelled run or publish its answer.
                if item.terminal() || item.status!=RunStatus::Running{return Ok(());}
                match answer {
                    Ok(ref reply)=>{
                        item.input_tokens=item.input_tokens.saturating_add(reply.input_tokens);
                        item.cached_input_tokens=item.cached_input_tokens.saturating_add(reply.cached_input_tokens);
                        item.cache_write_tokens=item.cache_write_tokens.saturating_add(reply.cache_write_tokens);
                        item.output_tokens=item.output_tokens.saturating_add(reply.output_tokens);
                        item.cost_krw+=reply.cost_krw;
                        item.event("model",
                            format!("{} · in {} / out {} tokens · {:.4} KRW",
                                reply.model,reply.input_tokens,reply.output_tokens,reply.cost_krw),
                            Some(json!({"tool_calls":0})));
                        if item.cost_krw>item.budget_krw{
                            item.status=RunStatus::Failed;
                            item.event("budget_exceeded","Model-only response exceeded the run budget",None);
                        }else if reply.text.trim().is_empty(){
                            item.status=RunStatus::Failed;
                            item.event("model_empty","Provider returned no content",None);
                        }else{
                            item.answer=Some(reply.text.clone());
                            item.status=RunStatus::Completed;
                            item.event("completed","Model-only chat turn completed without tools",None);
                        }
                    },
                    Err(ref error)=>{
                        item.status=RunStatus::Failed;
                        item.event("provider_error",error.clone(),None);
                    }
                }
                Ok(())
            }).await;
            if let Err(error)=saved{tracing::error!(%id,%error,"unable to persist model-only chat response");}
        });
    }else if let Err(e)=engine.launch(run.id).await{
        let _=engine.store.mutate(run.id,|item|{
            item.status=RunStatus::Failed;
            item.event("launch_error",e.clone(),None);
            Ok(())
        }).await;
        let _=engine.frontend.finish_turn(&record.id,run.id,None).await;
        return Err(bad(e));
    }
    let worker=Arc::clone(&engine);
    let session_id=record.id.clone();
    let run_id=run.id;
    tokio::spawn(async move{
        // Persist the answer even if a browser disconnects while the task runs.
        loop{
            match worker.store.get(run_id).await{
                Some(state) if state.terminal()=>{
                    if let Err(error)=sync_session(&worker,&session_id).await{
                        tracing::error!(%error,%session_id,"failed to persist finished chat");
                    }
                    break;
                },
                None=>break,
                _=>tokio::time::sleep(Duration::from_millis(350)).await,
            }
        }
    });
    let stream_engine=Arc::clone(&engine);
    let session_id=record.id;
    let id=run.id;
    let id_text=id.to_string();
    let stream=async_stream::stream!{
        let mut sequence=0u64;
        let mut last_approval:Option<String>=None;
        loop{
            let Some(run)=stream_engine.store.get(id).await else{break;};
            for event in &run.events{
                if event.seq<=sequence{continue;}
                sequence=event.seq;
                let output=match event.kind.as_str(){
                    "tool_started"=>Some(json!({"type":"tool_start","name":event.message})),
                    "tool_result"=>Some(json!({"type":"tool_end","name":event.message})),
                    "tool_error"=>Some(json!({"type":"tool_error","error":event.message})),
                    _=>None,
                };
                if let Some(value)=output{
                    yield Ok::<_,Infallible>(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                }
            }
            if run.status==RunStatus::AwaitingApproval{
                if let Some(call)=&run.pending_approval{
                    if last_approval.as_deref()!=Some(&call.id){
                        last_approval=Some(call.id.clone());
                        let value=json!({"type":"approval","id":id.to_string(),"tool":call.name,"args":call.arguments});
                        yield Ok(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                    }
                }
            }
            if run.terminal(){
                // Worker and stream can race; only actual persisted assistant message counts.
                let _=sync_session(&stream_engine,&session_id).await;
                let saved=stream_engine.frontend.session(&session_id).await;
                if let Some(message)=saved.and_then(|s|s.messages.into_iter().rev()
                    .find(|m|m.role=="assistant"&&m.metadata.as_ref().and_then(|v|v.get("executionId"))
                        .and_then(Value::as_str)==Some(id_text.as_str()))){
                    let out=json!({"type":"text","text":message.content});
                    yield Ok(Event::default().json_data(out).unwrap_or_else(|_|Event::default().data("{}")));
                    let saved=json!({"type":"assistant_saved","messageId":message.id});
                    yield Ok(Event::default().json_data(saved).unwrap_or_else(|_|Event::default().data("{}")));
                }else{
                    let error=json!({"type":"error","message":format!("native run ended with {:?}",run.status)});
                    yield Ok(Event::default().json_data(error).unwrap_or_else(|_|Event::default().data("{}")));
                }
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            if run.status==RunStatus::Paused && run.in_flight.is_some(){
                let value=json!({"type":"error","message":"Tool outcome is uncertain; reconcile the native run before proceeding."});
                yield Ok(Event::default().json_data(value).unwrap_or_else(|_|Event::default().data("{}")));
                yield Ok(Event::default().json_data(json!({"type":"done"})).unwrap_or_else(|_|Event::default().data("{}")));
                break;
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(8)).text("alive")))
}


async fn approvals(State(engine):State<Arc<Engine>>)->Json<Value>{
    let sessions=engine.frontend.sessions().await;
    let runs=engine.store.list().await;
    let pending=runs.iter().filter_map(|run|{
        let call=run.pending_approval.as_ref()?;
        if run.approval_decision.is_some(){return None;}
        let session=sessions.iter().find(|s|s.pending_run==Some(run.id));
        Some(json!({
            "id":run.id.to_string(),"tool":call.name,"args":call.arguments,
            "status":"pending",
            "sessionId":session.map(|s|s.id.as_str()).unwrap_or("")
        }))
    }).collect::<Vec<_>>();
    Json(json!(pending))
}
#[derive(Deserialize)]
struct Decide{id:String,allow:bool}
async fn resolve_approval(State(engine):State<Arc<Engine>>,Json(input):Json<Decide>)
    ->Result<Json<Value>,Fault>{
    let id=Uuid::parse_str(&input.id).map_err(|_|bad("approval id must be the native execution UUID"))?;
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    if run.pending_approval.is_none()||run.approval_decision.is_some(){
        return Ok(Json(json!({"ok":false,"resolved":false,"error":"approval already resolved"})));
    }
    engine.approve(id,input.allow).await.map_err(bad)?;
    Ok(Json(json!({"ok":true,"resolved":true,"status":if input.allow{"approved"}else{"denied"}})))
}
async fn policy()->Json<Value>{
    // Native engine always requires approval for shell and other sensitive tools.
    Json(json!({"id":"default","autoApprove":[],"autoApproveMcp":false,"autoApproveAll":false}))
}
async fn save_policy(Json(input):Json<Value>)->Result<Json<Value>,Fault>{
    let escalate=input["autoApproveAll"].as_bool().unwrap_or(false)
        ||input["autoApproveMcp"].as_bool().unwrap_or(false)
        ||input["autoApprove"].as_array().is_some_and(|v|!v.is_empty());
    if escalate{return Err(unsupported("Native Rust tool approval policies do not accept blanket permissions via the legacy UI."));}
    Ok(Json(json!({"id":"default","autoApprove":[],"autoApproveMcp":false,"autoApproveAll":false})))
}
fn as_job(run:&Run)->Value{
    json!({
        "id":run.id.to_string(),"kind":"native_agent","status":run.status,
        "providerId":run.provider_id,"model":run.model_name,
        "input":{"goal":run.instruction},
        "createdAt":timestamp(run.created_at_ms),
        "updatedAt":timestamp(run.updated_at_ms),
        "sessionId":Value::Null,
        "result":if run.terminal(){json!({"answer":run.answer,"costKrw":run.cost_krw})}else{Value::Null}
    })
}
async fn jobs(State(engine):State<Arc<Engine>>)->Json<Value>{
    let scheduled=engine.automations.jobs().await;
    let attached=scheduled.iter().filter_map(|item|item.native_run_id)
        .collect::<std::collections::HashSet<_>>();
    let mut all=engine.store.list().await.iter()
        .filter(|run|!attached.contains(&run.id)).map(as_job).collect::<Vec<_>>();
    all.extend(scheduled.iter().filter_map(|item|serde_json::to_value(item).ok()));
    for job in engine.workflows.jobs().await{
        if let Ok(value)=serde_json::to_value(job){all.push(value);}
    }
    for campaign in engine.campaigns.list().await{
        all.push(json!({"id":campaign.id,"kind":"rsi_evolve",
            "status":campaign.status,"input":{"rsiCampaignId":campaign.id,
                "goal":campaign.objective},
            "createdAt":timestamp(campaign.started_ms),
            "stepIndex":campaign.attempts,"stepTotal":campaign.max_attempts,
            "result":if campaign.status=="completed"{Some(campaign.view())}else{None}}));
    }
    all.sort_by(|a,b|b.get("createdAt").and_then(Value::as_str).cmp(&a.get("createdAt").and_then(Value::as_str)));
    Json(Value::Array(all))
}
#[derive(Deserialize)]
struct NewJob{kind:String,input:Value}
async fn submit_selected(engine:&Arc<Engine>,input:CreateRun,
    selection:(String,String))->Result<Run,String>{
    let mut run=Run::new(input,engine.settings.max_steps)?;
    run.provider_id=selection.0;run.model_name=selection.1;
    if run.provider_id!="dominion-native"{
        run.event("pricing_unknown","Provider token prices have not been configured; reported 0 KRW is not verified free use",None);
    }
    engine.store.put(run.clone()).await?;
    engine.launch(run.id).await?;
    engine.store.get(run.id).await.ok_or("created native run missing".into())
}
async fn create_job(State(engine):State<Arc<Engine>>,Json(req):Json<NewJob>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    if req.kind!="prompt"{return Err(unsupported("This job kind has no native Rust scheduler implementation."));}
    let text=req.input["prompt"].as_str().ok_or_else(||bad("prompt required"))?;
    let agent=if let Some(id)=req.input["agentId"].as_str(){
        Some(engine.frontend.agents().await.into_iter().find(|a|a.id==id)
            .ok_or_else(||bad("agent not found"))?)
    }else{None};
    let instruction=agent.as_ref().map(|agent|format!("User-configured agent instructions:\n{}\n\nCurrent request:\n{}",
        agent.system_prompt,text)).unwrap_or_else(||text.to_owned());
    let selection=if let Some(ref agent)=agent{
        chosen(&engine,Some(&agent.provider_id),Some(&agent.model))?
    }else{chosen(&engine,None,None)?};
    let run=submit_selected(&engine,CreateRun{
        instruction,budget_krw:None,max_steps:None
    },selection).await.map_err(bad)?;
    Ok((StatusCode::ACCEPTED,Json(as_job(&run))))
}
async fn job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    if let Some(scheduled)=engine.automations.job(id).await{
        return serde_json::to_value(scheduled).map(Json).map_err(|e|broken(e.to_string()));
    }
    if let Some(campaign)=engine.campaigns.get(id).await{
        return Ok(Json(campaign.view()));
    }
    if let Some(job)=engine.workflows.job(id).await{
        return serde_json::to_value(job).map(Json).map_err(|e|broken(e.to_string()));
    }
    engine.store.get(id).await.map(|r|Json(as_job(&r))).ok_or_else(||missing("job not found"))
}
async fn cancel_job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    if engine.automations.job(id).await.is_some(){
        let result=crate::frontend_automations::cancel(&engine,id).await.map_err(bad)?;
        return serde_json::to_value(result).map(Json).map_err(|e|broken(e.to_string()));
    }
    if engine.campaigns.get(id).await.is_some(){
        let campaign=crate::frontend_rsi::stop(&engine,id).await.map_err(bad)?;
        return Ok(Json(campaign.view()));
    }
    if engine.workflows.job(id).await.is_some(){
        let job=crate::frontend_workflows::cancel(&engine,id).await.map_err(bad)?;
        return serde_json::to_value(job).map(Json).map_err(|e|broken(e.to_string()));
    }
    let run=engine.cancel(id).await.map_err(bad)?;
    Ok(Json(json!({"cancelled":run.status==RunStatus::Cancelled,"status":run.status})))
}
async fn resume_job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    if engine.automations.job(id).await.is_some(){
        let result=crate::frontend_automations::resume(&engine,id).await.map_err(bad)?;
        return serde_json::to_value(result).map(Json).map_err(|e|broken(e.to_string()));
    }
    if engine.workflows.job(id).await.is_none(){
        return Err(missing("workflow job not found"));
    }
    let job=crate::frontend_workflows::resume(&engine,id).await.map_err(bad)?;
    serde_json::to_value(job).map(Json).map_err(|e|broken(e.to_string()))
}
async fn executions(State(engine):State<Arc<Engine>>)->Json<Value>{
    let list=engine.store.list().await.into_iter().map(|run|{
        let tools=run.events.iter().filter(|event|event.kind=="tool_started").count();
        json!({
            "id":run.id.to_string(),"sessionId":run.id.to_string(),
            "providerId":if run.provider_id.is_empty(){"dominion-native"}else{&run.provider_id},
            "model":if run.model_name.is_empty(){&engine.settings.main.name}else{&run.model_name},
            "status":run.status,
            "startedAt":timestamp(run.created_at_ms),
            "finishedAt":if run.terminal(){Some(timestamp(run.updated_at_ms))}else{None},
            "reason":if run.status==RunStatus::Failed{run.events.last().map(|e|e.message.clone())}else{None},
            "iterations":run.steps,"modelCalls":run.steps,
            "toolCalls":tools,"tokens":run.input_tokens+run.output_tokens
        })
    }).collect::<Vec<_>>();
    Json(json!(list))
}
async fn execution_operations(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    let list=run.events.iter().filter(|event|[
        "tool_started","tool_result","tool_error","tool_reconciled",
        "tool_denied","tool_policy_denied","tool_hook_denied"
    ].contains(&event.kind.as_str())).map(|event|json!({
        "id":format!("{}:{}",id,event.seq),"callId":event.detail.as_ref()
            .and_then(|v|v.get("call_id")).and_then(Value::as_str).unwrap_or(""),
        "tool":event.message,"status":event.kind,
        "startedAt":timestamp(event.at_ms)
    })).collect::<Vec<_>>();
    Ok(Json(json!(list)))
}
async fn tools_catalog(State(engine):State<Arc<Engine>>)->Json<Value>{
    let list=Dominion::from_engine(engine).tool_catalog().await.into_iter()
        .map(|tool|json!({"name":tool.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
            "description":tool.pointer("/function/description").cloned().unwrap_or(Value::Null)}))
        .collect::<Vec<_>>();
    Json(json!(list))
}
async fn global_events(State(engine):State<Arc<Engine>>)
    ->Sse<impl Stream<Item=Result<Event,Infallible>>>{
    let stream=async_stream::stream!{
        let mut known=std::collections::HashMap::<Uuid,u64>::new();
        loop{
            let observed=engine.store.list().await;
            for run in &observed{
                let prev=known.insert(run.id,run.updated_at_ms);
                if prev.is_some() && prev!=Some(run.updated_at_ms){
                    let event=json!({"type":"job_event","id":run.id.to_string(),
                        "status":run.status,"updatedAt":timestamp(run.updated_at_ms)});
                    yield Ok::<_,Infallible>(Event::default().json_data(event)
                        .unwrap_or_else(|_|Event::default().data("{}")));
                }
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)).text("alive"))
}
async fn memories(State(engine):State<Arc<Engine>>)->Json<Value>{
    let mut result=Vec::new();
    for scope in engine.knowledge.scopes().await{
        for entry in engine.knowledge.list(&scope).await{
            let owner=scope.strip_prefix("agent:");
            let display_scope=if scope=="global"{"shared"}else if owner.is_some(){"agent"}else{"global"};
            result.push(json!({
                "id":entry.id.to_string(),
                "text":entry.content,"scope":display_scope,"agentId":owner,
                "partitionKey":if display_scope=="global"{Some(scope.clone())}else{None},
                "status":"active","retrievalEligible":true,
                "source":"unknown",
                "createdAt":timestamp(entry.created_at_ms),"lastUsedAt":Value::Null,
                "importance":Value::Null,"verification":Value::Null
            }))
        }
    }
    Json(json!(result))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct MemoryDraft{text:String,scope:Option<String>,agent_id:Option<String>}
async fn save_memory(State(engine):State<Arc<Engine>>,Json(input):Json<MemoryDraft>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    let target=match input.scope.as_deref(){
        Some("agent")=>{
            let id=input.agent_id.as_deref().filter(|s|!s.is_empty()).ok_or_else(||bad("agentId required for agent memory"))?;
            if !engine.frontend.agents().await.iter().any(|agent|agent.id==id){
                return Err(bad("agent memory target not found"));
            }
            format!("agent:{id}")
        },
        Some("shared")|None=>String::from("global"),
        Some(value) if !value.is_empty()=>value.into(),
        _=>return Err(bad("invalid memory scope"))
    };
    let title=format!("User note {}",Uuid::new_v4());
    let saved=engine.knowledge.remember(&target,&title,&input.text).await.map_err(bad)?;
    Ok((StatusCode::CREATED,Json(json!({
        "id":saved.id.to_string(),"scope":input.scope.as_deref().unwrap_or("shared"),
        "text":saved.content,"status":"active","retrievalEligible":true,"source":"user"
    }))))
}
async fn delete_memory(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    for scope in engine.knowledge.scopes().await{
        if engine.knowledge.list(&scope).await.iter().any(|m|m.id==id){
            engine.knowledge.forget(&scope,id).await.map_err(bad)?;
            return Ok(Json(json!({"ok":true})));
        }
    }
    Err(missing("memory entry not found"))
}
fn sdk_view(run:&Run)->Value{
    let status=if run.status==RunStatus::Paused&&run.in_flight.is_some(){
        String::from("in_doubt")
    }else if run.status==RunStatus::Paused{
        String::from("interrupted")
    }else{serde_json::to_value(&run.status).ok()
        .and_then(|v|v.as_str().map(str::to_owned)).unwrap_or_else(||"unknown".into())};
    let current=if run.in_flight.is_some(){json!({"state":"in_doubt"})}
        else{json!({"state":status})};
    json!({
        "id":run.id.to_string(),"status":status,"instruction":run.instruction,
        "answer":run.answer,"error":if run.status==RunStatus::Failed{
            run.events.last().map(|e|e.message.clone())}else{None},
        "checkpoint":{
            "state":current,
            "rounds":run.steps,
            "toolCalls":run.events.iter().filter(|event|event.kind=="tool_started").count(),
            "usage":{
                "prompt_tokens":run.input_tokens,"completion_tokens":run.output_tokens,
                "cached_prompt_tokens":run.cached_input_tokens
            },
            "events":run.events.iter().map(|e|json!({
                "kind":e.kind,"detail":e.detail,"message":e.message
            })).collect::<Vec<_>>(),
            "pendingCall":run.pending_approval,
            "answer":run.answer
        }
    })
}
async fn sdk_runs(State(engine):State<Arc<Engine>>)->Json<Value>{
    Json(Value::Array(engine.store.list().await.iter().take(30).map(sdk_view).collect()))
}
async fn sdk_run(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.store.get(id).await.map(|r|Json(sdk_view(&r))).ok_or_else(||missing("execution not found"))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct SdkStart{
    instruction:String,provider_id:Option<String>,model:Option<String>,
    verify_command:Option<String>,max_rounds:Option<usize>
}
async fn sdk_start(State(engine):State<Arc<Engine>>,Json(input):Json<SdkStart>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    agent_model(&engine,input.provider_id.as_deref(),input.model.as_deref())?;
    if input.verify_command.as_deref().is_some_and(|command|!command.trim().is_empty()){
        return Err(unsupported("This native runner does not yet accept an arbitrary verifier command. It will not pretend to run it."));
    }
    let selection=chosen(&engine,input.provider_id.as_deref(),input.model.as_deref())?;
    let run=submit_selected(&engine,CreateRun{
        instruction:input.instruction,budget_krw:None,max_steps:input.max_rounds
    },selection).await.map_err(bad)?;
    Ok((StatusCode::ACCEPTED,Json(sdk_view(&run))))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct SdkApprove{call_id:String}
async fn sdk_approve(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(req):Json<SdkApprove>)->Result<Json<Value>,Fault>{
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    if run.pending_approval.as_ref().map(|c|c.id.as_str())!=Some(req.call_id.as_str()){
        return Err(bad("approval call ID does not match the pending tool"));
    }
    engine.approve(id,true).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn sdk_resume(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.launch(id).await.map_err(bad)?;
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    Ok(Json(sdk_view(&run)))
}
async fn sdk_cancel(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let run=engine.cancel(id).await.map_err(bad)?;
    Ok(Json(sdk_view(&run)))
}
#[derive(Deserialize)]
struct SdkReconcile{applied:bool,details:String}
async fn sdk_reconcile(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(body):Json<SdkReconcile>)->Result<Json<Value>,Fault>{
    if body.details.trim().is_empty(){return Err(bad("observed tool outcome required"));}
    let run=engine.reconcile(id,ReconcileTool{
        succeeded:body.applied,result:body.details
    }).await.map_err(bad)?;
    Ok(Json(sdk_view(&run)))
}
