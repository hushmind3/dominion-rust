//! Sub4 main-UI agents adapter; endpoint behavior preserved.
use super::*;
use base64::Engine as _;

pub(super) async fn owner_profile(State(engine):State<Arc<Engine>>)->Json<Value>{
    let record=engine.owner_profile.get().await;
    Json(json!({"name":record.name,"avatar":record.avatar,"updatedAt":timestamp(record.updated_at_ms)}))
}
pub(super) async fn save_owner_profile(State(engine):State<Arc<Engine>>,
    Json(value):Json<crate::profile::NewOwnerProfile>)->Result<Json<Value>,Fault>{
    let record=engine.owner_profile.save(value).await.map_err(bad)?;
    Ok(Json(json!({"name":record.name,"avatar":record.avatar,"updatedAt":timestamp(record.updated_at_ms)})))
}
pub(super) async fn model_runtime()->Json<Value>{
    Json(json!({"status":"idle","engine":null,"model":null,"managedBy":"external"}))
}
pub(super) async fn model_installed()->Json<Value>{Json(json!([]))}
pub(super) async fn channels()->Json<Value>{Json(json!([]))}
pub(super) async fn agents(State(engine):State<Arc<Engine>>)->Json<Vec<UiAgent>>{
    Json(engine.frontend.agents().await)
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct AgentDraft{
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
pub(super) fn agent_model(engine:&Engine,provider:Option<&str>,model:Option<&str>)->Result<(),Fault>{
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
pub(super) fn chosen(engine:&Engine,provider:Option<&str>,model:Option<&str>)
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
pub(super) async fn save_agent(State(engine):State<Arc<Engine>>,Json(draft):Json<AgentDraft>)
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
pub(super) async fn delete_agent(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
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
pub(super) struct BulkAgentModel{agent_ids:Vec<String>,provider_id:String,model:String,reasoning_effort:Option<String>}
pub(super) async fn bulk_agent_model(State(engine):State<Arc<Engine>>,Json(body):Json<BulkAgentModel>)
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


