//! Sub4 main-UI sdk_endpoints adapter; endpoint behavior preserved.
use super::*;

pub(super) fn sdk_view(run:&Run)->Value{
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
        "answer":run.answer,"usage":native_run_usage(run),
        "artifactEvidence":run.events.iter().rev()
            .find(|e|matches!(e.kind.as_str(),
                "artifact_evidence_checked"|"artifact_evidence_missing"))
            .and_then(|e|e.detail.clone()),
        "error":if run.status==RunStatus::Failed{
            run.events.last().map(|e|e.message.clone())}else{None},
        "checkpoint":{
            "state":current,
            "rounds":run.steps,
            "cost_krw":run.cost_krw,
            "budget_krw":run.budget_krw,
            "program":run.program_session.as_ref().map(|p|json!({
                "id":p.id,"completed":p.cursor,"total":p.steps.len(),
                "current":p.steps.get(p.cursor).map(|s|json!({"id":s.id,"tool":s.tool})),
                "pending_step":p.pending_step,
                "stopping_at_receipt":p.halt_requested
            })),
            "toolCalls":run.events.iter().filter(|event|event.kind=="tool_started").count(),
            "usage":{
                "prompt_tokens":run.input_tokens,"completion_tokens":run.output_tokens,
                "cached_prompt_tokens":run.cached_input_tokens,
                "uncached_prompt_tokens":run.input_tokens.saturating_sub(run.cached_input_tokens),
                "cache_write_tokens":run.cache_write_tokens,
                "provider_reported":native_run_usage(run)["providerUsageReported"]
            },
            "events":run.events.iter().map(|e|json!({
                "kind":e.kind,"detail":e.detail,"message":e.message
            })).collect::<Vec<_>>(),
            "pendingCall":run.pending_approval,
            "answer":run.answer
        }
    })
}
pub(super) async fn sdk_runs(State(engine):State<Arc<Engine>>)->Json<Value>{
    Json(Value::Array(engine.store.list().await.iter().take(30).map(sdk_view).collect()))
}
pub(super) async fn sdk_run(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.store.get(id).await.map(|r|Json(sdk_view(&r))).ok_or_else(||missing("execution not found"))
}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
pub(super) struct SdkStart{
    instruction:String,provider_id:Option<String>,model:Option<String>,
    verify_command:Option<String>,max_rounds:Option<usize>
}
pub(super) async fn sdk_start(State(engine):State<Arc<Engine>>,Json(input):Json<SdkStart>)
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
pub(super) struct SdkApprove{call_id:String}
pub(super) async fn sdk_approve(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(req):Json<SdkApprove>)->Result<Json<Value>,Fault>{
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    if run.pending_approval.as_ref().map(|c|c.id.as_str())!=Some(req.call_id.as_str()){
        return Err(bad("approval call ID does not match the pending tool"));
    }
    engine.approve(id,true).await.map_err(bad)?;
    Ok(Json(json!({"ok":true})))
}
pub(super) async fn sdk_resume(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    engine.launch(id).await.map_err(bad)?;
    let run=engine.store.get(id).await.ok_or_else(||missing("execution not found"))?;
    Ok(Json(sdk_view(&run)))
}
pub(super) async fn sdk_cancel(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>)
    ->Result<Json<Value>,Fault>{
    let run=engine.cancel(id).await.map_err(bad)?;
    Ok(Json(sdk_view(&run)))
}
#[derive(Deserialize)]
pub(super) struct SdkReconcile{applied:bool,details:String}
pub(super) async fn sdk_reconcile(State(engine):State<Arc<Engine>>,Path(id):Path<Uuid>,
    Json(body):Json<SdkReconcile>)->Result<Json<Value>,Fault>{
    if body.details.trim().is_empty(){return Err(bad("observed tool outcome required"));}
    let run=engine.reconcile(id,ReconcileTool{
        succeeded:body.applied,result:body.details
    }).await.map_err(bad)?;
    Ok(Json(sdk_view(&run)))
}



#[cfg(test)]
mod program_view_tests{
    use super::*;
    #[test]
    fn ui_checkpoint_exposes_durable_program_progress_and_provider_cost_without_hidden_steps(){
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Inspect and update source".into(),budget_krw:Some(200.0),
            max_steps:Some(5)},5).unwrap();
        run.program_session=Some(crate::program_session::ProgramSession{
            id:"program-test".into(),
            steps:vec![crate::program_session::ProgramStep{
                id:"edit".into(),tool:"edit_file".into(),
                arguments:json!({"path":"a.rs","old_text":"old","new_text":"new"}),
                when:None,on_error:None
            }],
            cursor:0,pending_step:None,
            halt_requested:false,stop_when:None,observations:Default::default()
        });
        run.cost_krw=3.5;
        run.input_tokens=350;
        run.cached_input_tokens=110;
        run.cache_write_tokens=15;
        run.output_tokens=40;
        run.event("model","observed metered response",Some(json!({"usage_reported":true})));
        let view=sdk_view(&run);
        assert_eq!(view["usage"]["promptTokens"],350);
        assert_eq!(view["usage"]["cachedPromptTokens"],110);
        assert_eq!(view["usage"]["uncachedPromptTokens"],240);
        assert_eq!(view["usage"]["totalTokens"],390);
        assert_eq!(view["usage"]["modelRequests"],1);
        assert_eq!(view["usage"]["providerUsageReported"],true);
        assert_eq!(view["checkpoint"]["program"]["total"],1);
        assert_eq!(view["checkpoint"]["program"]["current"]["tool"],"edit_file");
        assert_eq!(view["checkpoint"]["cost_krw"],3.5);
        assert_eq!(view["checkpoint"]["budget_krw"],200.0);
    }
}
