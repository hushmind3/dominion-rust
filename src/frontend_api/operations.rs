//! Sub4 main-UI operations adapter; endpoint behavior preserved.
use super::*;

pub(super) async fn approvals(State(engine):State<Arc<Engine>>)->Json<Value>{
    let sessions=engine.frontend.pending_run_sessions().await;
    let pending=engine.store.pending_approvals().await.into_iter()
        .map(|(id,call)|json!({
            "id":id.to_string(),"tool":call.name,"args":call.arguments,
            "status":"pending",
            "sessionId":sessions.get(&id).map(String::as_str).unwrap_or("")
        })).collect::<Vec<_>>();
    Json(json!(pending))
}
#[derive(Deserialize)]
pub(super) struct Decide{id:String,allow:bool}
pub(super) async fn resolve_approval(State(engine):State<Arc<Engine>>,Json(input):Json<Decide>)
    ->Result<Json<Value>,Fault>{
    let id=Uuid::parse_str(&input.id).map_err(|_|bad("approval id must be the native execution UUID"))?;
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    if run.pending_approval.is_none()||run.approval_decision.is_some(){
        return Ok(Json(json!({"ok":false,"resolved":false,"error":"approval already resolved"})));
    }
    engine.approve(id,input.allow).await.map_err(bad)?;
    Ok(Json(json!({"ok":true,"resolved":true,"status":if input.allow{"approved"}else{"denied"}})))
}
pub(super) async fn policy()->Json<Value>{
    // Native engine always requires approval for shell and other sensitive tools.
    Json(json!({"id":"default","autoApprove":[],"autoApproveMcp":false,"autoApproveAll":false}))
}
pub(super) async fn save_policy(Json(input):Json<Value>)->Result<Json<Value>,Fault>{
    let escalate=input["autoApproveAll"].as_bool().unwrap_or(false)
        ||input["autoApproveMcp"].as_bool().unwrap_or(false)
        ||input["autoApprove"].as_array().is_some_and(|v|!v.is_empty());
    if escalate{return Err(unsupported("Native Rust tool approval policies do not accept blanket permissions via the legacy UI."));}
    Ok(Json(json!({"id":"default","autoApprove":[],"autoApproveMcp":false,"autoApproveAll":false})))
}
pub(super) fn as_job(run:&Run)->Value{
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
pub(super) async fn jobs(State(engine):State<Arc<Engine>>)->Json<Value>{
    let all=engine.store.headers().await.iter().map(|run|json!({
        "id":run.id.to_string(),"kind":"native_agent","status":run.status,
        "providerId":run.provider_id,"model":run.model_name,
        "input":{"goal":run.instruction},
        "createdAt":timestamp(run.created_at_ms),
        "updatedAt":timestamp(run.updated_at_ms),
        "sessionId":Value::Null,
        "result":if run.terminal(){json!({"answer":run.answer,"costKrw":run.cost_krw})}
            else{Value::Null}
    })).collect::<Vec<_>>();
    Json(Value::Array(all))
}
#[derive(Deserialize)]
pub(super) struct NewJob{kind:String,input:Value}
pub(super) async fn submit_selected(engine:&Arc<Engine>,input:CreateRun,
    selection:(String,String))->Result<Run,String>{
    Dominion::from_engine(Arc::clone(engine))
        .submit_selected(input,crate::sdk::RunOptions{
            provider_id:selection.0,model:selection.1,
            strategy_learning:false,model_only:false
        }).await
}
pub(super) async fn create_job(State(engine):State<Arc<Engine>>,Json(req):Json<NewJob>)
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
pub(super) async fn job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.store.get(id).await.map(|r|Json(as_job(&r))).ok_or_else(||missing("job not found"))
}
pub(super) async fn cancel_job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let run=engine.cancel(id).await.map_err(bad)?;
    Ok(Json(json!({"cancelled":run.status==RunStatus::Cancelled,"status":run.status})))
}
pub(super) async fn resume_job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let current=engine.store.get(id).await.ok_or_else(||missing("job not found"))?;
    if current.status!=RunStatus::Paused{
        return Err(busy("only a paused native job may be resumed"));
    }
    // The Rust kernel owns all restrictions: approval, unresolved side
    // effects, delegated children, model-only and already-active workers.
    engine.launch(id).await.map_err(busy)?;
    let resumed=engine.store.get(id).await.ok_or_else(||missing("job not found"))?;
    Ok(Json(as_job(&resumed)))
}
pub(super) async fn executions(State(engine):State<Arc<Engine>>)->Json<Value>{
    let list=engine.store.headers().await.into_iter().map(|run|{
        json!({
            "id":run.id.to_string(),"sessionId":run.id.to_string(),
            "providerId":if run.provider_id.is_empty(){"dominion-native"}else{&run.provider_id},
            "model":if run.model_name.is_empty(){&engine.settings.main.name}else{&run.model_name},
            "status":run.status,
            "startedAt":timestamp(run.created_at_ms),
            "finishedAt":if run.terminal(){Some(timestamp(run.updated_at_ms))}else{None},
            "reason":run.last_error,
            "iterations":run.steps,"modelCalls":run.steps,
            "toolCalls":run.tool_calls,"tokens":run.input_tokens+run.output_tokens
        })
    }).collect::<Vec<_>>();
    Json(json!(list))
}
pub(super) async fn execution_operations(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
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
pub(super) async fn tools_catalog(State(engine):State<Arc<Engine>>)->Json<Value>{
    let list=Dominion::from_engine(engine).tool_catalog().await.into_iter()
        .map(|tool|json!({"name":tool.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
            "description":tool.pointer("/function/description").cloned().unwrap_or(Value::Null)}))
        .collect::<Vec<_>>();
    Json(json!(list))
}
pub(super) async fn global_events(State(engine):State<Arc<Engine>>)
    ->Sse<impl Stream<Item=Result<Event,Infallible>>>{
    // Subscribe to the native Store's post-fsync change feed. No 1.5-second
    // per-client full-history snapshots or idle CPU scans.
    let mut updates=engine.store.subscribe();
    let stream=async_stream::stream!{
        loop{
            match updates.recv().await{
                Ok(change)=>{
                    let payload=json!({"type":"job_event","id":change.id.to_string(),
                        "status":change.status,"updatedAt":timestamp(change.updated_at_ms)});
                    yield Ok::<_,Infallible>(Event::default().json_data(payload)
                        .unwrap_or_else(|_|Event::default().data("{}")));
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{
                    // A slow tab must refresh its snapshot rather than treat
                    // dropped status updates as a durable sequence.
                    let payload=json!({"type":"job_event","status":"resync"});
                    yield Ok::<_,Infallible>(Event::default().json_data(payload)
                        .unwrap_or_else(|_|Event::default().data("{}")));
                },
                Err(tokio::sync::broadcast::error::RecvError::Closed)=>break
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)).text("alive"))
}

#[cfg(test)]
mod resume_tests{
 use super::*;
 use crate::{group::Groups,store::Store};
 #[tokio::test]
 async fn job_resume_reuses_same_run_and_refuses_uncertain_mutation(){
  let root=std::env::temp_dir().join(format!("dominion-resume-job-{}",Uuid::new_v4()));
  let mut settings=crate::config::Settings::from_env().unwrap();
  settings.data_dir=root.join("data");settings.workspace_dir=root.join("workspace");
  let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
  let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());
  let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
  let mut run=Run::new(CreateRun{instruction:"already satisfied".into(),
    budget_krw:Some(1.0),max_steps:Some(1)},1).unwrap();
  run.plan=Some("none".into());run.steps=1;run.status=RunStatus::Paused;
  let id=run.id;
  store.put(run).await.unwrap();
  let response=resume_job(State(Arc::clone(&engine)),Path(id)).await.unwrap().0;
  assert_eq!(response["id"],id.to_string());
  let persisted=store.get(id).await.unwrap();
  assert!(persisted.events.iter().any(|e|e.kind=="resumed"));
  // No new run was allocated by the legacy resume adapter.
  assert_eq!(store.headers().await.len(),1);

  let mut blocked=Run::new(CreateRun{instruction:"uncertain external write".into(),
    budget_krw:Some(1.0),max_steps:Some(1)},1).unwrap();
  blocked.status=RunStatus::Paused;
  blocked.in_flight=Some(crate::domain::ToolCall{id:"pending".into(),
    name:"github_commit_file".into(),arguments:json!({})});
  let blocked_id=blocked.id;
  store.put(blocked).await.unwrap();
  let result=resume_job(State(Arc::clone(&engine)),Path(blocked_id)).await;
  assert_eq!(result.unwrap_err().0,StatusCode::CONFLICT);
  assert!(store.get(blocked_id).await.unwrap().in_flight.is_some());
  drop(engine);drop(store);
  tokio::fs::remove_dir_all(root).await.unwrap();
 }
}
