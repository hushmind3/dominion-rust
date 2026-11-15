//! Exact latest-main WorkflowsPage API. Workflows are durable orchestration,
//! each real step is an ordinary native Rust Run with its own tool journal,
//! approvals, model usage, budget, and recovery checkpoints.
use crate::{
    domain::{CreateRun,Run,RunStatus,now_ms},
    engine::Engine,
    frontend_state::{UiMessage,UiSession,timestamp},
    workflow::{Workflow,WorkflowStep,WorkflowJob,StepResult},
};
use axum::{
    extract::{Path,State},http::StatusCode,routing::{get,post},
    Json,Router,
};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn fault(status:StatusCode,message:impl Into<String>)->Fault{
    (status,Json(json!({"error":message.into()})))
}
fn bad(message:impl Into<String>)->Fault{fault(StatusCode::BAD_REQUEST,message)}
fn missing()->Fault{fault(StatusCode::NOT_FOUND,"workflow not found")}
fn conflict(message:impl Into<String>)->Fault{fault(StatusCode::CONFLICT,message)}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/workflows",get(list).post(save))
        .route("/workflows/{id}",get(one).delete(remove))
        .route("/workflows/{id}/run",post(start))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct NewStep {
    #[serde(default)]id:Option<String>,
    name:String,prompt:String,agent_id:String,
    #[serde(default)]provider_id:String,
    #[serde(default)]model:String,
}
#[derive(Deserialize)]
struct NewWorkflow {
    #[serde(default)]id:Option<String>,
    name:String,#[serde(default)]description:String,steps:Vec<NewStep>,
}
async fn validate(engine:&Engine,input:NewWorkflow)->Result<(Workflow,bool),Fault>{
    let name=input.name.trim();
    if name.is_empty() || name.len()>160 || input.description.len()>6000{
        return Err(bad("workflow name or description is invalid"));
    }
    if input.steps.is_empty() || input.steps.len()>30{
        return Err(bad("a workflow requires 1 to 30 steps"));
    }
    let agents=engine.frontend.agents().await;
    let mut steps=Vec::with_capacity(input.steps.len());
    let mut ids=std::collections::HashSet::new();
    for step in input.steps{
        let id=step.id.filter(|id|!id.is_empty()).unwrap_or_else(||Uuid::new_v4().to_string());
        if id.len()>128 || !ids.insert(id.clone()){return Err(bad("duplicate or invalid step ID"));}
        if step.name.trim().is_empty() || step.name.len()>160 ||
            step.prompt.trim().is_empty() || step.prompt.len()>18_000 {
            return Err(bad("workflow step name/prompt must be nonempty and within limits"));
        }
        let agent=agents.iter().find(|agent|agent.id==step.agent_id)
            .ok_or_else(||bad(format!("unknown workflow agent {}",step.agent_id)))?;
        // If main's UI selects a model, it must be *the model used by the
        // actual Rust worker*. Reject anything the native engine can't route.
        let provider=if step.provider_id.is_empty(){agent.provider_id.as_str()}
            else{step.provider_id.as_str()};
        let model=if step.model.is_empty(){agent.model.as_str()}else{step.model.as_str()};
        engine.providers.resolve(provider,model,&engine.settings.main).map_err(bad)?;
        steps.push(WorkflowStep{
            id,name:step.name.trim().into(),prompt:step.prompt,
            agent_id:step.agent_id,provider_id:provider.into(),model:model.into()
        });
    }
    let update=input.id.as_ref().is_some_and(|id|!id.is_empty());
    let id=input.id.filter(|id|!id.is_empty()).unwrap_or_else(||Uuid::new_v4().to_string());
    if id.len()>128{return Err(bad("invalid workflow ID"));}
    Ok((Workflow{id,name:name.into(),description:input.description,steps},update))
}
async fn list(State(engine):State<Arc<Engine>>)->Json<Vec<Workflow>>{
    Json(engine.workflows.list().await)
}
async fn one(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Workflow>,Fault>{
    engine.workflows.get(&id).await.map(Json).ok_or_else(missing)
}
async fn save(State(engine):State<Arc<Engine>>,Json(input):Json<NewWorkflow>)
    ->Result<Json<Workflow>,Fault>{
    let (record,update)=validate(&engine,input).await?;
    engine.workflows.save(record,update).await.map(Json).map_err(bad)
}
async fn remove(State(engine):State<Arc<Engine>>,Path(id):Path<String>)
    ->Result<Json<Value>,Fault>{
    engine.workflows.delete(&id).await.map_err(conflict)?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct RunRequest{#[serde(default)]input:String}
async fn start(State(engine):State<Arc<Engine>>,Path(id):Path<String>,
    Json(body):Json<RunRequest>)->Result<(StatusCode,Json<WorkflowJob>),Fault>{
    if body.input.len()>15_000{return Err(bad("workflow input limit is 15000 bytes"));}
    let job=engine.workflows.start(&id,body.input).await.map_err(bad)?;
    let worker=Arc::clone(&engine);
    let job_id=job.id;
    tokio::spawn(async move{if let Err(error)=drive(worker.clone(),job_id).await{
        tracing::error!(workflow=%job_id,error=%error,"workflow worker failed");
        let _=worker.workflows.mutate_job(job_id,|state|{
            if !matches!(state.status.as_str(),"completed"|"cancelled"|"failed"){
                state.status="needs_review".into();state.error=Some(error.clone());
            }
            Ok(())
        }).await;
    }});
    Ok((StatusCode::ACCEPTED,Json(job)))
}
/// Runs at most once per job in one process, never a second copy of a
/// mutating agent step. Restart retains the exact active_run_id.
pub async fn drive(engine:Arc<Engine>,id:Uuid)->Result<(),String>{
    engine.workflows.reserve(id).await?;
    let result=drive_reserved(Arc::clone(&engine),id).await;
    engine.workflows.release(id).await;
    result
}
async fn drive_reserved(engine:Arc<Engine>,id:Uuid)->Result<(),String>{
    loop{
        let job=engine.workflows.job(id).await.ok_or("workflow job vanished")?;
        if job.cancelled || matches!(job.status.as_str(),"completed"|"failed"|"cancelled"){
            return Ok(());
        }
        if job.step_index>=job.step_total{
            commit_transcript(&engine,&job).await?;
            engine.workflows.mutate_job(id,|state|{
                if state.cancelled{return Err("workflow was cancelled".into());}
                state.status="completed".into();
                state.error=None;
                state.result=Some(json!({
                    "sessionId":format!("workflow-{}",state.id),
                    "output":state.outputs.last().map(|v|v.answer.as_str()).unwrap_or(""),
                    "steps":state.outputs,
                    "costKrw":state.outputs.iter().map(|v|v.cost_krw).sum::<f64>(),
                    "tokens":state.outputs.iter().map(|v|v.tokens).sum::<u64>(),
                }));
                Ok(())
            }).await?;
            return Ok(());
        }
        if job.active_run_id.is_none(){
            let step=&job.workflow.steps[job.step_index];
            let agent=engine.frontend.agents().await.into_iter()
                .find(|p|p.id==step.agent_id)
                .ok_or_else(||format!("workflow agent {} was removed",step.agent_id))?;
            engine.providers.resolve(&step.provider_id,&step.model,
                &engine.settings.main)?;
            if engine.frontend.agents().await.iter().all(|p|p.id!=agent.id){
                return Err("workflow-selected agent disappeared".into());
            }
            let previous=job.outputs.last().map(|o|o.answer.as_str()).unwrap_or("");
            let body=step.prompt.replace("{{input}}",&job.user_input)
                .replace("{{previous}}",previous);
            let instruction=format!(
                "WORKFLOW: {}\nSTEP {} OF {}: {}\nAGENT ID: {}\nAGENT CONFIGURATION (user-authored, lower priority than engine policy):\n{}\nTASK:\n{}",
                job.workflow.name,job.step_index+1,job.step_total,step.name,
                agent.id,agent.system_prompt,body
            );
            if instruction.len()>20_000{return Err("workflow step input exceeds native run limit".into());}
            let spent=job.outputs.iter().map(|v|v.cost_krw).sum::<f64>();
            let remaining=job.budget_krw-spent;
            if !remaining.is_finite() || remaining<=0.0{
                engine.workflows.mutate_job(id,|state|{
                    state.status="failed".into();
                    state.error=Some("workflow-wide model cost budget exhausted".into());
                    Ok(())
                }).await?;
                return Ok(());
            }
            let mut run=Run::new(CreateRun{
                instruction,budget_krw:Some(remaining),max_steps:None
            },engine.settings.max_steps)?;
            run.provider_id=step.provider_id.clone();
            run.model_name=step.model.clone();
            if run.provider_id!="dominion-native"{
                run.event("pricing_unknown","External provider tariff is not configured; cost limit cannot be verified from zero-price estimates",None);
            }
            // Store the real run before checkpointing the workflow. In a crash
            // between these writes the extra queued run is not launched; it
            // remains an inspectable orphan, never a duplicate tool execution.
            engine.store.put(run.clone()).await?;
            engine.workflows.mutate_job(id,|state|{
                if state.cancelled{return Err("workflow was cancelled".into());}
                if state.active_run_id.is_some(){return Err("step already started".into());}
                state.active_run_id=Some(run.id);
                state.status="running".into();state.error=None;
                Ok(())
            }).await?;
            engine.launch(run.id).await?;
        }
        let snapshot=engine.workflows.job(id).await.ok_or("workflow job vanished")?;
        if snapshot.cancelled{return Ok(());}
        let run_id=snapshot.active_run_id.ok_or("missing workflow native run checkpoint")?;
        let run=engine.store.get(run_id).await.ok_or("native step run missing")?;
        match run.status{
            RunStatus::Completed=>{
                let answer=run.answer.as_deref()
                    .filter(|v|!v.trim().is_empty())
                    .ok_or("native step completed with no answer")?;
                let step=snapshot.workflow.steps.get(snapshot.step_index)
                    .ok_or("workflow step index out of range")?;
                engine.workflows.mutate_job(id,|state|{
                    if state.cancelled{return Err("workflow cancelled during step".into());}
                    if state.active_run_id!=Some(run_id){
                        return Err("stale workflow step completion".into());
                    }
                    state.outputs.push(StepResult{
                        step_id:step.id.clone(),step_name:step.name.clone(),
                        run_id,answer:answer.to_owned(),cost_krw:run.cost_krw,
                        tokens:run.input_tokens.saturating_add(run.output_tokens)
                    });
                    state.step_index+=1;state.active_run_id=None;
                    state.status="running".into();
                    Ok(())
                }).await?;
            },
            RunStatus::Cancelled=>{
                engine.workflows.mutate_job(id,|state|{
                    state.cancelled=true;state.status="cancelled".into();
                    state.error=Some("native step was cancelled".into());Ok(())
                }).await?;
                return Ok(());
            },
            RunStatus::Failed=>{
                engine.workflows.mutate_job(id,|state|{
                    state.status="failed".into();
                    state.error=Some(run.events.last()
                        .map(|e|e.message.clone()).unwrap_or_else(||"native step failed".into()));
                    Ok(())
                }).await?;
                return Ok(());
            },
            RunStatus::AwaitingApproval=>{
                // Main App surfaces native approvals. Resume the same native
                // Run ID after human action; no second model/tool dispatch.
                if snapshot.status!="running" ||
                    snapshot.error.as_deref()!=Some("Awaiting native tool approval"){
                    engine.workflows.mutate_job(id,|state|{
                        state.status="running".into();
                        state.error=Some("Awaiting native tool approval".into());
                        Ok(())
                    }).await?;
                }
                tokio::time::sleep(Duration::from_millis(1200)).await;
            },
            RunStatus::Paused=>{
                engine.workflows.mutate_job(id,|state|{
                    state.status="needs_review".into();
                    state.error=Some("Native step paused; inspect and reconcile before resuming".into());
                    Ok(())
                }).await?;
                return Ok(());
            },
            _=>tokio::time::sleep(Duration::from_millis(350)).await,
        }
    }
}
async fn commit_transcript(engine:&Arc<Engine>,job:&WorkflowJob)->Result<(),String>{
    let session_id=format!("workflow-{}",job.id);
    let now=timestamp(now_ms());
    let mut messages=Vec::new();
    messages.push(UiMessage{
        id:Uuid::new_v4().to_string(),session_id:session_id.clone(),
        role:"user".into(),content:job.user_input.clone(),created_at:now.clone(),
        agent_id:None,metadata:Some(json!({"workflowId":job.workflow.id}))
    });
    for item in &job.outputs{
        messages.push(UiMessage{
            id:Uuid::new_v4().to_string(),session_id:session_id.clone(),
            role:"assistant".into(),content:item.answer.clone(),created_at:now.clone(),
            agent_id:job.workflow.steps.iter().find(|step|step.id==item.step_id)
                .map(|step|step.agent_id.clone()),
            metadata:Some(json!({
                "executionId":item.run_id,"stepId":item.step_id,
                "stepName":item.step_name,"costKrw":item.cost_krw,
                "usage":{"totalTokens":item.tokens}
            }))
        });
    }
    let record=UiSession{
        id:session_id.clone(),
        title:format!("{} · 실행 결과",job.workflow.name),
        agent_id:job.workflow.steps.first().map(|s|s.agent_id.clone()).unwrap_or("general".into()),
        provider_id:job.workflow.steps.first().map(|s|s.provider_id.clone()).unwrap_or("dominion-native".into()),
        model:job.workflow.steps.first().map(|s|s.model.clone()).unwrap_or_else(||engine.settings.main.name.clone()),
        created_at:job.created_at.clone(),updated_at:now,
        pending_run:None,room_id:None,messages
    };
    engine.frontend.mutate(|state|{
        // Exactly the same job transcript can be recovered after a failed
        // workflow status write, so the session write must be idempotent.
        if !state.sessions.contains_key(&session_id){
            state.sessions.insert(session_id,record);
        }
        Ok(())
    }).await
}
pub async fn cancel(engine:&Arc<Engine>,id:Uuid)->Result<WorkflowJob,String>{
    let snapshot=engine.workflows.job(id).await.ok_or("workflow job not found")?;
    if matches!(snapshot.status.as_str(),"completed"|"failed"|"cancelled"){
        return Err("cannot cancel a terminal workflow".into());
    }
    // Cancel the actual native step first. A mutating tool with unknown
    // outcome is NOT safe to mark completed/cancelled in the UI.
    let pending_reconcile=if let Some(run_id)=snapshot.active_run_id{
        match engine.store.get(run_id).await{
            Some(run) if !run.terminal()=>{
                let after=engine.cancel(run_id).await?;
                after.status==RunStatus::Paused || after.cancel_after_tool
            },
            None=>return Err("native step run missing; cannot safely cancel".into()),
            _=>false,
        }
    }else{false};
    engine.workflows.mutate_job(id,|state|{
        state.cancelled=true;
        if pending_reconcile{
            state.status="needs_review".into();
            state.error=Some("Cancellation requested; a native tool requires outcome reconciliation".into());
        }else{
            state.status="cancelled".into();
            state.error=Some("workflow cancelled by user".into());
        }
        Ok(())
    }).await
}

pub async fn resume(engine:&Arc<Engine>,id:Uuid)->Result<WorkflowJob,String>{
    let job=engine.workflows.job(id).await.ok_or("workflow job not found")?;
    if job.cancelled || !matches!(job.status.as_str(),"needs_review"|"interrupted"){
        return Err("workflow does not require recovery".into());
    }
    if let Some(run_id)=job.active_run_id{
        let run=engine.store.get(run_id).await.ok_or("native step missing")?;
        if run.status==RunStatus::AwaitingApproval{
            return Err("approve or deny the pending native tool before resuming".into());
        }
        if run.status==RunStatus::Paused{
            if run.in_flight.is_some() || !run.read_batch_in_flight.is_empty(){
                return Err("reconcile the uncertain native tool outcome first".into());
            }
            engine.launch(run_id).await?;
        }
    }
    let updated=engine.workflows.mutate_job(id,|state|{
        if state.cancelled{return Err("workflow cancelled".into());}
        state.status="running".into();state.error=None;Ok(())
    }).await?;
    let worker=Arc::clone(engine);
    tokio::spawn(async move{
        if let Err(error)=drive(Arc::clone(&worker),id).await{
            tracing::warn!(workflow=%id,%error,"resumed workflow error");
            let _=worker.workflows.mutate_job(id,|j|{
                if !matches!(j.status.as_str(),"completed"|"cancelled"){
                    j.status="needs_review".into();j.error=Some(error.clone());
                }
                Ok(())
            }).await;
        }
    });
    Ok(updated)
}
