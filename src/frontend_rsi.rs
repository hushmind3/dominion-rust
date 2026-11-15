//! Original main RSI screen → native frozen-judge trial engine.
//! Real Git worktrees, measured scores and explicit/automatic adoption only.
//! No synthetic "frontier generation" or invented downstream meta-evaluation.
use crate::{
    domain::now_ms,engine::Engine,improvement::{ImprovementStatus,NewImprovement},
    rsi_campaign::{Campaign,from_trial},
};
use axum::{extract::{Path,State},http::StatusCode,routing::{get,post},Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{sync::Arc,time::Duration};
use uuid::Uuid;
type Fault=(StatusCode,Json<Value>);
fn fail(code:StatusCode,message:impl Into<String>)->Fault{
    (code,Json(json!({"error":message.into()})))
}
fn bad(message:impl Into<String>)->Fault{fail(StatusCode::BAD_REQUEST,message)}
fn missing()->Fault{fail(StatusCode::NOT_FOUND,"RSI campaign not found")}
fn conflict(message:impl Into<String>)->Fault{fail(StatusCode::CONFLICT,message)}
pub fn routes()->Router<Arc<Engine>>{
    Router::new()
        .route("/rsi/evaluation-profiles",get(profiles))
        .route("/rsi/campaigns",get(campaigns))
        .route("/rsi/campaigns/{id}",get(campaign))
        .route("/rsi/campaigns/{id}/verify",get(verify))
        .route("/rsi/campaigns/{id}/choose",post(choose))
        .route("/rsi/campaigns/{id}/apply",post(apply))
        .route("/rsi/start",post(start))
        .route("/algorithms",get(algorithms))
        .route("/dev/jobs/{id}",get(install_job))
}
fn configured()->Result<(String,String,String),Fault>{
    let repo=std::env::var("DOMINION_RSI_REPOSITORY")
        .map_err(|_|bad("Configure DOMINION_RSI_REPOSITORY as a checkout relative to the Rust workspace"))?;
    let tests=std::env::var("DOMINION_RSI_TEST_COMMAND")
        .map_err(|_|bad("Configure DOMINION_RSI_TEST_COMMAND before running real source evaluation"))?;
    let score=std::env::var("DOMINION_RSI_SCORE_COMMAND")
        .map_err(|_|bad("Configure DOMINION_RSI_SCORE_COMMAND to emit a measured numeric score"))?;
    if repo.trim().is_empty()||tests.trim().is_empty()||score.trim().is_empty(){
        return Err(bad("RSI repository, frozen test, and numeric judge must not be empty"));
    }
    if repo.len()>200||tests.len()>2048||score.len()>2048{
        return Err(bad("RSI source or judge command exceeds safe size limits"));
    }
    Ok((repo,tests,score))
}
async fn profiles()->Json<Value>{
    let available=configured().is_ok();
    let profiles=if available{vec![json!({
        "id":"default","name":"Native frozen repository judge",
        "taskCount":1,"requiredComponents":[]
    })]}else{Vec::new()};
    Json(json!({"profiles":profiles}))
}
async fn algorithms()->Json<Value>{
    Json(json!({"kinds":[{"kind":"selfimprove.search",
        "active":{"id":"rsi-discovery","name":"Evidence-ranked Git source discovery",
            "implementation":"rsi_discovery.rs","revision":"native-rust-2026"}}]}))
}
async fn campaigns(State(engine):State<Arc<Engine>>)->Json<Value>{
    Json(json!({"campaigns":engine.campaigns.list().await.iter().map(Campaign::view)
        .collect::<Vec<_>>()}))
}
async fn campaign(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.campaigns.get(id).await.map(|c|Json(c.view())).ok_or_else(missing)
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Start {
    provider_id:String,model:String,
    #[serde(default)]request:String,
    #[serde(default)]selection_policy:String,
    #[serde(default)]local_only:bool,
    #[serde(default)]max_attempts:usize,
    #[serde(default)]token_budget:u64,
    #[serde(default)]time_budget_seconds:u64,
    #[serde(default)]confirmation:String,
    #[serde(default)]evaluation_profile_id:String,
    #[serde(default)]mode:String,
    #[serde(default)]continue_from:Option<Uuid>,
}
async fn start(State(engine):State<Arc<Engine>>,Json(input):Json<Start>)
    ->Result<(StatusCode,Json<Value>),Fault>{
    if input.confirmation!="START_RSI_EVOLUTION" {
        return Err(bad("RSI campaign requires the UI's explicit start confirmation"));
    }
    if input.mode!=""&&input.mode!="continuous"{
        return Err(bad("Only the native bounded continuous evaluation mode is supported"));
    }
    if input.evaluation_profile_id!=""&&input.evaluation_profile_id!="default"{
        return Err(bad("Selected judge profile is not configured in the native engine"));
    }
    if !matches!(input.selection_policy.as_str(),""|"manual"|"automatic"){
        return Err(bad("Invalid RSI selection policy"));
    }
    if !(1..=100).contains(&input.max_attempts){
        return Err(bad("RSI must have 1-100 bounded attempts"));
    }
    if !engine.settings.shell_enabled{
        return Err(bad("The frozen judge needs DOMINION_ENABLE_SHELL=true"));
    }
    let (repo,tests,scoring)=configured()?;
    let selected=engine.providers.resolve(&input.provider_id,&input.model,
        &engine.settings.main).map_err(bad)?;
    let local=reqwest::Url::parse(&selected.base_url).ok()
        .and_then(|url|url.host_str().map(str::to_owned))
        .is_some_and(|host|host=="localhost"||
            host.parse::<std::net::IpAddr>().is_ok_and(|ip|ip.is_loopback()));
    if input.local_only&&!local{
        return Err(bad("Local-only RSI cannot call a remote provider"));
    }
    if input.request.len()>8_000{return Err(bad("RSI objective exceeds 8000 bytes"));}
    let objective=if input.request.trim().is_empty(){
        "Improve Dominion native agent planning, recovery, cost efficiency and correctness while preserving existing functionality".into()
    }else{input.request.trim().to_owned()};
    if engine.campaigns.list().await.iter().any(|c|
        matches!(c.status.as_str(),"running"|"queued")||c.phase=="waiting_for_choice"){
        return Err(conflict("Another RSI campaign is active or awaiting an explicit version choice"));
    }
    if let Some(previous)=input.continue_from{
        let existing=engine.campaigns.get(previous).await.ok_or_else(missing)?;
        if engine.campaigns.active(previous).await || existing.current_trial.is_some()
            || existing.phase=="waiting_for_choice" ||
            !matches!(existing.status.as_str(),"completed"|"failed"){
            return Err(conflict("RSI can continue only a settled completed/failed campaign; reconcile its pending trial first"));
        }
        if existing.repository!=repo || existing.test_command!=tests || existing.score_command!=scoring{
            return Err(conflict("frozen source and judge changed; start a separate campaign for a different experiment"));
        }
        let record=engine.campaigns.change(previous,|c|{
            c.max_attempts=c.attempts.saturating_add(input.max_attempts);
            c.token_budget=if input.token_budget==0{0}
                else{c.tokens.saturating_add(input.token_budget)};
            c.time_budget_seconds=input.time_budget_seconds;
            c.provider_id=input.provider_id.clone();
            c.model=input.model.clone();
            c.selection_policy=if input.selection_policy=="manual"{"manual".into()}
                else{"automatic".into()};
            c.started_ms=now_ms();
            c.status="queued".into();c.phase="planning_next_generator".into();
            c.error=None;c.stop_reason=None;c.requested_cancel=false;
            Ok(())
        }).await.map_err(conflict)?;
        spawn_driver(&engine,previous);
        return Ok((StatusCode::ACCEPTED,Json(record.view())));
    }
    let run=Campaign{
        id:Uuid::new_v4(),status:"queued".into(),phase:"preparing_lab".into(),
        objective,provider_id:input.provider_id,model:input.model,
        selection_policy:if input.selection_policy=="manual"{"manual".into()}else{"automatic".into()},
        repository:repo,test_command:tests,score_command:scoring,
        max_attempts:input.max_attempts,token_budget:input.token_budget,
        time_budget_seconds:input.time_budget_seconds,started_ms:now_ms(),
        attempts:0,accepted_generations:0,tokens:0,cost_krw:0.0,
        current_trial:None,trials:Vec::new(),history:Vec::new(),
        base_commit:None,head_commit:None,error:None,stop_reason:None,
        requested_cancel:false,
    };
    let run=engine.campaigns.create(run).await.map_err(bad)?;
    // Campaign is durably queued before starting source-editing tasks.
    spawn_driver(&engine,run.id);
    Ok((StatusCode::ACCEPTED,Json(run.view())))
}
fn spawn_driver(engine:&Arc<Engine>,id:Uuid){
    let worker=Arc::clone(engine);
    tokio::spawn(async move{
        if let Err(error)=drive(Arc::clone(&worker),id).await{
            tracing::error!(campaign=%id,%error,"RSI controller failed");
            let _=worker.campaigns.change(id,|c|{
                if !matches!(c.status.as_str(),"completed"|"cancelled"){
                    c.status="needs_review".into();c.error=Some(error.clone());
                }
                Ok(())
            }).await;
        }
    });
}
async fn drive(engine:Arc<Engine>,id:Uuid)->Result<(),String>{
    engine.campaigns.reserve(id).await?;
    let outcome=drive_reserved(&engine,id).await;
    engine.campaigns.release(id).await;
    outcome
}
async fn drive_reserved(engine:&Arc<Engine>,id:Uuid)->Result<(),String>{
    loop{
        let state=engine.campaigns.get(id).await.ok_or("campaign removed")?;
        if state.requested_cancel{
            if let Some(trial_id)=state.current_trial{
                let trial=engine.improvements.get(trial_id).await.ok_or("active trial unavailable")?;
                if trial.status==ImprovementStatus::Running ||
                    trial.status==ImprovementStatus::AwaitingApproval{
                    let _=engine.improvements.request_cancel(trial_id).await;
                    tokio::time::sleep(Duration::from_millis(450)).await;
                    continue;
                }
            }
            engine.campaigns.change(id,|c|{
                c.status="cancelled".into();c.phase="review".into();
                c.stop_reason=Some("operator_stop".into());Ok(())
            }).await?;
            return Ok(());
        }
        if state.attempts>=state.max_attempts{
            engine.campaigns.change(id,|c|{
                c.status="completed".into();c.phase="review".into();
                c.stop_reason=Some("attempt_limit".into());Ok(())
            }).await?;return Ok(());
        }
        if state.token_budget>0&&state.tokens>=state.token_budget{
            engine.campaigns.change(id,|c|{
                c.status="completed".into();c.phase="review".into();
                c.stop_reason=Some("token_budget".into());Ok(())
            }).await?;return Ok(());
        }
        if state.time_budget_seconds>0 &&
            now_ms().saturating_sub(state.started_ms)>state.time_budget_seconds.saturating_mul(1000){
            engine.campaigns.change(id,|c|{
                c.status="completed".into();c.phase="review".into();
                c.stop_reason=Some("time_budget".into());Ok(())
            }).await?;return Ok(());
        }
        let trial_id=if let Some(trial)=state.current_trial{trial}else{
            let trial=engine.improvements.create(NewImprovement{
                repository:state.repository.clone(),target_file:String::new(),
                target_files:None,instruction:state.objective.clone(),
                test_command:state.test_command.clone(),
                score_command:Some(state.score_command.clone()),parent:None,
            }).await?;
            engine.campaigns.change(id,|c|{
                if c.requested_cancel{return Err("campaign cancelled during trial setup".into());}
                c.current_trial=Some(trial.id);
                c.trials.push(trial.id);
                c.attempts+=1;
                c.status="running".into();
                c.phase="diagnosing_and_editing".into();
                Ok(())
            }).await?;
            let profile=engine.providers.resolve(&state.provider_id,&state.model,
                &engine.settings.main)?;
            let client=if state.provider_id=="dominion-native"{
                engine.model.clone()
            }else{
                let kind=engine.providers.get(&state.provider_id)
                    .ok_or("RSI provider missing")?.kind;
                engine.model.with_primary_protocol(profile,&kind)?
            };
            engine.providers.throttle(&state.provider_id).await;
            engine.improvements.start(client,trial.id,engine.settings.shell_enabled).await?;
            trial.id
        };
        loop{
            let trial=engine.improvements.get(trial_id).await.ok_or("active trial not found")?;
            if !matches!(trial.status,ImprovementStatus::Running|ImprovementStatus::AwaitingApproval){
                break;
            }
            let current=engine.campaigns.get(id).await.ok_or("campaign missing")?;
            if current.requested_cancel || (current.time_budget_seconds>0
                && now_ms().saturating_sub(current.started_ms)
                   >=current.time_budget_seconds.saturating_mul(1000)){
                let _=engine.improvements.request_cancel(trial_id).await;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let trial=engine.improvements.get(trial_id).await.ok_or("trial lost after evaluation")?;
        let number=engine.campaigns.get(id).await.ok_or("campaign missing")?.attempts;
        engine.campaigns.change(id,|c|{
            if c.history.iter().all(|row|row.trial_id!=trial.id){
                c.tokens=c.tokens.saturating_add(trial.input_tokens.saturating_add(trial.output_tokens));
                c.cost_krw+=trial.cost_krw;
                if c.base_commit.is_none(){c.base_commit=trial.base_commit.clone();}
                c.history.push(from_trial(&trial,number));
            }
            c.current_trial=None;c.phase="review";
            Ok(())
        }).await?;
        match trial.status{
            ImprovementStatus::Evaluated if trial.accepted=>{
                let c=engine.campaigns.get(id).await.ok_or("campaign missing")?;
                if c.requested_cancel{continue;}
                if c.selection_policy=="manual"{
                    engine.campaigns.change(id,|c|{
                        c.status="running".into();c.phase="waiting_for_choice".into();Ok(())
                    }).await?;
                    return Ok(());
                }
                // Adoption uses native frozen baseline/tree verification and
                // is the only source-changing operation in the controller.
                let adopted=engine.improvements.adopt(trial.id).await?;
                let actual_head=engine.improvements.adopted_head(trial.id).await?;
                engine.campaigns.change(id,|c|{
                    let last=c.history.last_mut().ok_or("trial record missing")?;
                    last.selected=Some("candidate".into());
                    last.accepted_commit=Some(actual_head.clone());
                    c.accepted_generations+=1;
                    c.head_commit=Some(actual_head.clone());
                    Ok(())
                }).await?;
            },
            ImprovementStatus::Cancelled=>{
                engine.campaigns.change(id,|c|{
                    c.status="cancelled".into();c.phase="review".into();
                    c.stop_reason=Some("operator_stop".into());Ok(())
                }).await?;return Ok(());
            },
            ImprovementStatus::Rejected|ImprovementStatus::Failed=>{
                engine.campaigns.change(id,|c|{
                    let last=c.history.last_mut().ok_or("missing trial record")?;
                    last.selected=Some("baseline".into());
                    c.error=trial.error.clone();Ok(())
                }).await?;
            },
            _=>return Err(format!("trial not in a final judged state: {:?}",trial.status)),
        }
    }
}
#[derive(Deserialize)]
struct Choose{choice:String,attempt:usize}
async fn choose(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(input):Json<Choose>)->Result<Json<Value>,Fault>{
    if !matches!(input.choice.as_str(),"baseline"|"candidate"){
        return Err(bad("select baseline or candidate"));
    }
    let current=engine.campaigns.get(id).await.ok_or_else(missing)?;
    if current.phase!="waiting_for_choice"||engine.campaigns.active(id).await{
        return Err(conflict("this campaign is not awaiting a choice"));
    }
    let last=current.history.last().ok_or_else(||bad("no evaluated trial"))?;
    if last.attempt!=input.attempt{return Err(conflict("stale comparison attempt"));}
    if input.choice=="candidate"&&!last.accepted{
        return Err(bad("independent judge did not accept the candidate"));
    }
    let chosen=if input.choice=="candidate"{
        engine.improvements.adopt(last.trial_id).await.map_err(conflict)?;
        Some(engine.improvements.adopted_head(last.trial_id).await.map_err(conflict)?)
    }else{None};
    let choice=input.choice.clone();
    let result=engine.campaigns.change(id,|c|{
        if c.phase!="waiting_for_choice"{return Err("choice was already resolved".into());}
        let last=c.history.last_mut().ok_or("no trial result")?;
        last.selected=Some(choice.clone());
        if let Some(ref actual_head)=chosen{
            last.accepted_commit=Some(actual_head.clone());
            c.accepted_generations+=1;
            c.head_commit=Some(actual_head.clone());
        }
        c.status="queued".into();c.phase="selecting".into();c.error=None;
        Ok(())
    }).await.map_err(conflict)?;
    spawn_driver(&engine,id);
    Ok(Json(result.view()))
}
pub async fn stop(engine:&Arc<Engine>,id:Uuid)->Result<Campaign,String>{
    let current=engine.campaigns.get(id).await.ok_or("campaign not found")?;
    if matches!(current.status.as_str(),"completed"|"cancelled"|"failed"){
        return Err("cannot cancel an already finished campaign".into());
    }
    let changed=engine.campaigns.change(id,|c|{
        c.requested_cancel=true;
        c.phase="review".into();
        if c.current_trial.is_none(){c.status="cancelled".into();}
        else{c.phase="stopping".into();}
        Ok(())
    }).await?;
    if let Some(trial)=changed.current_trial{
        let _=engine.improvements.request_cancel(trial).await;
    }
    Ok(changed)
}
async fn verify(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let c=engine.campaigns.get(id).await.ok_or_else(missing)?;
    let mut verified=Vec::new();
    let mut consecutive=0usize;
    for (i,row) in c.history.iter().enumerate(){
        let trial=engine.improvements.get(row.trial_id).await;
        let actual=trial.as_ref().is_some_and(|t|
            t.status==ImprovementStatus::Adopted && t.accepted &&
            t.baseline.is_some() && t.candidate.as_ref().is_some_and(|m|m.tests_passed)
            && row.accepted_commit.is_some() && t.candidate_commit.is_some());
        if actual{consecutive+=1}else{consecutive=0;}
        verified.push(json!({
            "index":i+1,"attempt":row.attempt,"accepted":actual,
            "domain":row.domain,"generatorCommit":row.generator_commit,
            "acceptedCommit":row.accepted_commit,
            "target":row.focus_files.first(),
            "independentReceipt":actual,
            "metaTransfer":false,"regressions":Value::Null,
            "improvedHoldoutDomains":[],
            "reason":if actual{"frozen native judge accepted and repository adoption recorded"}
                else{"no independently adopted trial receipt"}
        }));
    }
    Ok(Json(json!({
        "campaignId":c.id,"capabilityDemonstrated":false,
        "verifiedStreak":consecutive,
        "required":{"verifiedStreak":3,"holdoutDomains":2,"metaTransfer":true},
        "generations":verified
    })))
}
#[derive(Deserialize)]
struct Apply{commit:String,confirmation:String}
async fn apply(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(body):Json<Apply>)->Result<Json<Value>,Fault>{
    let c=engine.campaigns.get(id).await.ok_or_else(missing)?;
    if body.confirmation!="APPLY_VERIFIED_CAMPAIGN_TO_SUB"{
        return Err(bad("application requires main UI confirmation"));
    }
    if c.head_commit.as_deref()!=Some(body.commit.as_str())||
        !c.history.iter().any(|h|h.accepted_commit.as_deref()==Some(body.commit.as_str())){
        return Err(conflict("requested Git commit has no adopted frozen-judge receipt"));
    }
    Ok(Json(json!({
        "id":id,"status":"completed",
        "result":{"sourceApplied":true,"requiresRebuild":true,
            "installed":Value::Null},
        "message":"Git source was adopted; build/restart must be performed separately"
    })))
}
async fn install_job(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    if let Some(job)=engine.developer.job(id).await{
        return Ok(Json(json!(job)));
    }
    let c=engine.campaigns.get(id).await.ok_or_else(missing)?;
    let applied=c.head_commit.is_some();
    Ok(Json(json!({"id":id,"status":if applied{"completed"}else{"not_applied"},
        "result":{"sourceApplied":applied,"requiresRebuild":applied},
        "error":Value::Null})))
}
