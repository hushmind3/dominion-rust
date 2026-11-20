//! Sub4 main-UI memory adapter; endpoint behavior preserved.
use super::*;

pub(super) async fn memories(State(engine):State<Arc<Engine>>)->Json<Value>{
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
pub(super) struct MemoryDraft{text:String,scope:Option<String>,agent_id:Option<String>}
pub(super) async fn save_memory(State(engine):State<Arc<Engine>>,Json(input):Json<MemoryDraft>)
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
pub(super) async fn delete_memory(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    for scope in engine.knowledge.scopes().await{
        if engine.knowledge.list(&scope).await.iter().any(|m|m.id==id){
            engine.knowledge.forget(&scope,id).await.map_err(bad)?;
            return Ok(Json(json!({"ok":true})));
        }
    }
    Err(missing("memory entry not found"))
}
