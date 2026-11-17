//! Sub4 main-UI sessions adapter; endpoint behavior preserved.
use super::*;

pub(super) fn public_session(session:&UiSession)->Value{
    json!({
        "id":session.id,"title":session.title,"agentId":session.agent_id,
        "providerId":session.provider_id,"model":session.model,
        "createdAt":session.created_at,"updatedAt":session.updated_at,
        "roomId":session.room_id,"archived":false
    })
}
pub(super) async fn sync_session(engine:&Arc<Engine>,id:&str)->Result<(),String>{
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
pub(super) async fn sessions(State(engine):State<Arc<Engine>>)->Json<Value>{
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
pub(super) struct CreateSession{
    title:Option<String>,agent_id:Option<String>,provider_id:Option<String>,model:Option<String>
}
pub(super) async fn create_session(State(engine):State<Arc<Engine>>,Json(body):Json<CreateSession>)
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
pub(super) async fn get_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    sync_session(&engine,&id).await.map_err(broken)?;
    let record=engine.frontend.session(&id).await.ok_or_else(||missing("session not found"))?;
    Ok(Json(public_session(&record)))
}
pub(super) async fn delete_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) async fn session_messages(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) async fn session_usage(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) struct Rename{title:String}
pub(super) async fn rename_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>,Json(input):Json<Rename>)
    ->Result<Json<Value>,Fault>{
    let title=input.title.trim();
    if title.is_empty()||title.len()>200{return Err(bad("title must be 1-200 bytes"));}
    engine.frontend.mutate(|store|{
        let record=store.sessions.get_mut(&id).ok_or("session not found")?;
        record.title=title.into();record.updated_at=timestamp(now_ms());
        Ok(public_session(record))
    }).await.map(Json).map_err(bad)
}
pub(super) async fn clear_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) async fn fork_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) async fn cancel_session(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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

