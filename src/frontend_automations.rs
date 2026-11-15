//! Original main OperationsPage schedules real Rust executions.
//! Claims are durably recorded before any external model/tool action starts.
use crate::{
    automations::{Automation,Automations,ScheduledJob,iso_millis},
    domain::{CreateRun,Run,RunStatus,now_ms},
    engine::Engine,frontend_state::timestamp,
};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn fail(status:StatusCode,message:impl Into<String>)->Fault{
    (status,Json(json!({"error":message.into()})))
}
fn bad(message:impl Into<String>)->Fault{fail(StatusCode::BAD_REQUEST,message)}
fn missing()->Fault{fail(StatusCode::NOT_FOUND,"scheduled job or automation not found")}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/automations",get(list).post(save))
        .route("/automations/{id}",axum::routing::delete(remove))
        .route("/audit",get(audit))
        .route("/browser/status",get(browser_status))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct NewAutomation{
    id:Option<String>,kind:String,input:Value,
    interval_minutes:u64,next_at:String,enabled:bool,
}
async fn list(State(engine):State<Arc<Engine>>)->Json<Vec<Automation>>{
    Json(engine.automations.list().await)
}
async fn validate(engine:&Engine,input:&NewAutomation)->Result<(),Fault>{
    if !(1..=525_600).contains(&input.interval_minutes){
        return Err(bad("automation interval must be 1-525600 minutes"));
    }
    if input.next_at.len()>64{return Err(bad("automation nextAt is too long"));}
    if !matches!(input.kind.as_str(),"prompt"|"meeting"){
        return Err(bad("only agent prompt and room meeting automations are supported"));
    }
    if input.input.to_string().len()>18_000{return Err(bad("scheduled input exceeds 18 KB"));}
    match input.kind.as_str(){
        "prompt"=>{
            let prompt=input.input.get("prompt").and_then(Value::as_str)
                .filter(|v|!v.trim().is_empty()&&v.len()<=10_000)
                .ok_or_else(||bad("scheduled prompt is required (max 10000 bytes)"))?;
            let id=input.input.get("agentId").and_then(Value::as_str)
                .filter(|s|!s.is_empty()).ok_or_else(||bad("scheduled agentId required"))?;
            let agent=engine.frontend.agents().await.into_iter()
                .find(|a|a.id==id).ok_or_else(||bad("scheduled agent not found"))?;
            let provider=input.input.get("providerId").and_then(Value::as_str)
                .filter(|s|!s.is_empty()).unwrap_or(&agent.provider_id);
            let model=input.input.get("model").and_then(Value::as_str)
                .filter(|s|!s.is_empty()).unwrap_or(&agent.model);
            engine.model_for_selection(provider,model).map_err(bad)?;
            let _=prompt;
        },
        "meeting"=>{
            let goal=input.input.get("goal").and_then(Value::as_str)
                .filter(|s|!s.trim().is_empty()&&s.len()<=10_000)
                .ok_or_else(||bad("scheduled room agenda is required"))?;
            let room=input.input.get("roomId").and_then(Value::as_str)
                .ok_or_else(||bad("scheduled roomId required"))?;
            let id=Uuid::parse_str(room).map_err(|_|bad("invalid scheduled room ID"))?;
            let record=engine.groups.get(id).await.ok_or_else(||bad("scheduled room not found"))?;
            if !matches!(record.status,crate::group::RoomStatus::Open){
                return Err(bad("scheduled room is closed"));
            }
            let _=goal;
        },
        _=>unreachable!(),
    }
    Ok(())
}
async fn save(State(engine):State<Arc<Engine>>,Json(input):Json<NewAutomation>)
    ->Result<Json<Automation>,Fault>{
    validate(&engine,&input).await?;
    let next=iso_millis(&input.next_at).map_err(bad)?;
    let id=input.id.as_deref().filter(|x|!x.is_empty())
        .map(str::to_owned).unwrap_or_else(||Uuid::new_v4().to_string());
    if Uuid::parse_str(&id).is_err(){return Err(bad("automation ID must be UUID"));}
    let update=input.id.as_ref().is_some_and(|x|!x.is_empty());
    let record=Automation{
        id,kind:input.kind,input:input.input,interval_minutes:input.interval_minutes,
        next_at:timestamp(next),next_at_ms:next,enabled:input.enabled
    };
    engine.automations.save(record,update).await.map(Json).map_err(bad)
}
async fn remove(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    engine.automations.delete(&id).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
async fn browser_status()->Json<Value>{
    // Main UI can show unavailable, not a screenshot from another user's tab.
    Json(json!({"status":{"available":false,"running":false},
        "screenshot":null,"engine":"not_connected"}))
}
async fn audit(State(engine):State<Arc<Engine>>)->Json<Value>{
    let mut events=engine.store.list().await.into_iter().flat_map(|run|{
        run.events.into_iter().map(move|event|json!({
            "id":format!("{}:{}",run.id,event.seq),
            "event":event.kind,"detail":{
                "runId":run.id,"message":event.message,
                "data":event.detail
            },"created_at":timestamp(event.at_ms),
            "at_ms":event.at_ms
        }))
    }).collect::<Vec<_>>();
    events.sort_by(|a,b|b["at_ms"].as_u64().cmp(&a["at_ms"].as_u64()));
    events.truncate(200);
    Json(Value::Array(events))
}
pub fn spawn_scheduler(engine:&Arc<Engine>){
    let weak=Arc::downgrade(engine);
    tokio::spawn(async move{
        // Starting after SDK recovery prevents a due job from observing stale
        // native Run checkpoints during boot.
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop{
            let Some(engine)=weak.upgrade() else{return};
            // Do not claim or launch new scheduled work during the
            // operator's consistent encrypted backup/restore window.
            if crate::frontend_system::maintenance(&engine.settings){
                drop(engine);
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            match engine.automations.claim_due(now_ms()).await{
                Ok(items)=>for job in items{
                    let worker=Arc::clone(&engine);
                    tokio::spawn(async move{
                        if let Err(error)=execute(Arc::clone(&worker),job.clone()).await{
                            tracing::error!(automation=%job.automation_id,job=%job.id,
                                %error,"scheduled execution failed");
                            let _=worker.automations.finish(job.id,"failed",None,Some(error)).await;
                        }
                    });
                },
                Err(error)=>tracing::error!(%error,"automation scheduler could not persist the due claim"),
            }
            drop(engine);
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}
async fn execute(engine:Arc<Engine>,job:ScheduledJob)->Result<(),String>{
    match job.kind.as_str(){
        "prompt"=>execute_prompt(engine,job).await,
        "meeting"=>execute_meeting(engine,job).await,
        _=>Err("unknown scheduled job kind".into()),
    }
}
async fn execute_prompt(engine:Arc<Engine>,job:ScheduledJob)->Result<(),String>{
    let agent_id=job.input["agentId"].as_str().ok_or("scheduled agent ID missing")?;
    let agent=engine.frontend.agents().await.into_iter()
        .find(|a|a.id==agent_id).ok_or("scheduled agent has been deleted")?;
    let prompt=job.input["prompt"].as_str().ok_or("scheduled instruction missing")?;
    let provider=job.input["providerId"].as_str().filter(|s|!s.is_empty())
        .unwrap_or(&agent.provider_id);
    let model=job.input["model"].as_str().filter(|s|!s.is_empty())
        .unwrap_or(&agent.model);
    engine.model_for_selection(provider,model)?;
    let instruction=format!(
        "SCHEDULED AGENT WORK\nAutomation: {}\nAgent: {}\nAgent profile (user-provided, lower trust than system policy):\n{}\n\nSCHEDULED REQUEST:\n{}",
        job.automation_id,agent.name,agent.system_prompt,prompt
    );
    if instruction.len()>20_000{return Err("scheduled instruction exceeds native run limit".into());}
    let mut run=Run::new(CreateRun{
        instruction,budget_krw:None,max_steps:None
    },engine.settings.max_steps)?;
    run.provider_id=provider.to_owned();run.model_name=model.to_owned();
    if provider!="dominion-native"{
        run.event("pricing_unknown",
            "External provider pricing is not configured; 0 KRW recorded does not mean zero billing",
            None);
    }
    engine.store.put(run.clone()).await?;
    // The unique Run ID is committed before launch. No automatic restart of
    // an unconfirmed tool after a crash is permitted.
    engine.automations.bind_run(job.id,run.id).await?;
    engine.launch(run.id).await?;
    observe_run(engine,job.id,run.id).await
}
async fn observe_run(engine:Arc<Engine>,job:Uuid,run_id:Uuid)->Result<(),String>{
    loop{
        let run=engine.store.get(run_id).await.ok_or("scheduled native run missing")?;
        if run.terminal(){
            let status=match run.status{
                RunStatus::Completed=>"completed",
                RunStatus::Cancelled=>"cancelled",
                _=>"failed",
            };
            let result=if run.status==RunStatus::Completed{
                Some(json!({
                    "answer":run.answer,"executionId":run.id,
                    "costKrw":run.cost_krw,
                    "tokens":run.input_tokens.saturating_add(run.output_tokens)
                }))
            }else{None};
            let error=if status=="failed"{
                run.events.last().map(|e|e.message.clone())
            }else{None};
            engine.automations.finish(job,status,result,error).await?;
            return Ok(());
        }
        if run.status==RunStatus::Paused&&run.in_flight.is_some(){
            engine.automations.finish(job,"needs_review",None,
                Some("native tool outcome is uncertain; reconcile before continuing".into())).await?;
            return Ok(());
        }
        if run.status==RunStatus::Paused&&!run.waiting_for_children{
            engine.automations.finish(job,"needs_review",None,
                Some("native execution paused; review and resume the existing Run".into())).await?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(750)).await;
    }
}
async fn execute_meeting(engine:Arc<Engine>,job:ScheduledJob)->Result<(),String>{
    let goal=job.input["goal"].as_str().ok_or("scheduled meeting agenda missing")?;
    let raw_id=job.input["roomId"].as_str().ok_or("scheduled meeting room missing")?;
    let room=Uuid::parse_str(raw_id).map_err(|e|e.to_string())?;
    let key=format!("scheduled-{}",job.id);
    let (_,new_message,_)=engine.groups.say_once(room,goal.to_owned(),&key,vec![]).await?;
    if !new_message{
        return Err("meeting message has already been dispatched; inspect its real room transcript".into());
    }
    engine.automations.finish(job.id,"running",None,None).await?;
    let (updated,turns)=crate::frontend_rooms::generate(&engine,room).await
        .map_err(|(status,Json(value))|format!("group meeting {status}: {value}"))?;
    engine.automations.finish(job.id,"completed",
        Some(json!({"roomId":room,"sessionId":format!("room-{room}"),
            "turns":turns,"totalTokens":updated.used_tokens})),None).await?;
    Ok(())
}
pub async fn cancel(engine:&Arc<Engine>,id:Uuid)->Result<ScheduledJob,String>{
    let record=engine.automations.job(id).await.ok_or("scheduled job missing")?;
    if matches!(record.status.as_str(),"completed"|"cancelled"|"failed"){
        return Err("cannot cancel a finished scheduled job".into());
    }
    if let Some(native)=record.native_run_id{
        let run=engine.store.get(native).await.ok_or("scheduled native run missing")?;
        if !run.terminal(){
            let result=engine.cancel(native).await?;
            let unknown=result.in_flight.is_some()||result.cancel_after_tool;
            return engine.automations.finish(id,
                if unknown{"needs_review"}else{"cancelled"},None,
                if unknown{Some("tool outcome requires manual reconciliation".into())}
                    else{Some("cancelled by operator".into())}).await;
        }
    }
    // Meeting dispatch may have started an irreversible external model call.
    // Persist a review state rather than pretending the remote call stopped.
    engine.automations.finish(id,"needs_review",None,
        Some("meeting may already be executing; inspect recorded room history".into())).await
}
pub async fn resume(engine:&Arc<Engine>,id:Uuid)->Result<ScheduledJob,String>{
    let job=engine.automations.job(id).await.ok_or("scheduled job missing")?;
    if job.status!="needs_review"{return Err("job is not awaiting recovery".into());}
    let run_id=job.native_run_id.ok_or("meeting jobs cannot automatically resume an uncertain group turn")?;
    let run=engine.store.get(run_id).await.ok_or("original scheduled native run missing")?;
    match run.status{
        RunStatus::Paused=>{
            if run.in_flight.is_some()||!run.read_batch_in_flight.is_empty(){
                return Err("reconcile uncertain side effect before resuming".into());
            }
            engine.launch(run_id).await?;
        },
        RunStatus::Queued=>{engine.launch(run_id).await?;},
        RunStatus::Running|RunStatus::AwaitingApproval=>{
            return Err("native run is already active or requires approval".into());
        },
        _=>{},
    }
    let result=engine.automations.finish(id,"running",None,None).await?;
    let worker=Arc::clone(engine);
    tokio::spawn(async move{
        if let Err(error)=observe_run(Arc::clone(&worker),id,run_id).await{
            tracing::error!(%id,%error,"scheduled native job monitor failed");
            let _=worker.automations.finish(id,"needs_review",None,Some(error)).await;
        }
    });
    Ok(result)
}
