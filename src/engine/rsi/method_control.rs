//! An independently executed, non-adopting method-disabled control worker.
//! The task, source commit, model, provider, frozen cases and initial prompt
//! are matched to the ON worker. Source worktrees are separate. Only learned
//! executable procedures and induced action policy are disabled; the compiled
//! planner itself stays intact, so this measures method use, not source causality.
use super::*;

fn input_fingerprint(input:&Value)->String {
    let fields=json!({
        "generatorCommit":input["rsiGeneratorCommit"],
        "prompt":input["prompt"],
        "domain":input["rsiTargetDomain"],
        "providerId":input["providerId"],"model":input["model"],
        "frozenCases":input["rsiFrozenCases"],
        "nativePlan":input["rsiNativePlan"]["systemAgenda"]["target"],
        "methodSnapshot":input["rsiMethodSnapshot"]
    });
    format!("{:x}",Sha256::digest(serde_json::to_vec(&fields).unwrap_or_default()))
}

/// Called by the normal campaign BEFORE its ON job is created. There is no
/// manual control path and no ability for the control to update headCommit.
pub(super) async fn run_method_control(
    context:&JobContext,record:&mut Value,child_input:&mut Value,
    lab:&Path,binary:&Path,head:&str,started:Instant
)->Result<(),String>{
    if child_input["rsiMethodAblation"]!=true
        || child_input["rsiSystemEvolution"]!=true
        || !child_input["rsiMethodSnapshot"].is_object()
        || child_input["providerId"].as_str().is_none_or(str::is_empty)
        || child_input["model"].as_str().is_none_or(str::is_empty)
    {return Ok(());}
    let fingerprint=input_fingerprint(child_input);
    let control_dir=context.state.config.data_dir.join("rsi-labs")
        .join(format!("method-control-{}",uuid::Uuid::new_v4()));
    let control_path=control_dir.to_str().ok_or("대조군 경로 오류")?;
    git(context,lab,&["worktree","add","--detach",control_path,head]).await?;
    let observed=async {
        let mut input=child_input.clone();
        input["rsiSourceDir"]=json!(control_dir);
        input["rsiIndexId"]=crate::engine::code_index::refresh(
            &context.state,&control_dir)?["id"].clone();
        input["rsiMethodAblationArm"]=json!("without_methods");
        input["rsiMethodAblationInputSha256"]=json!(fingerprint);
        let job=context.state.store.put("job",&json!({
            "kind":"rsi_generation","status":"running",
            "input":input,"attempts":1,
            "rsiControlExperiment":true,
            "queuedAt":chrono::Utc::now().to_rfc3339()
        })).ok_or("대조군 실행 작업 저장 실패")?;
        let id=job["id"].as_str().ok_or("대조군 작업 ID 없음")?.to_string();
        record["phase"]=json!("measuring_method_control");
        record["activeGenerationJobId"]=json!(id);
        save(context,record)?;
        let mut process=Command::new(binary);
        process.arg("--rsi-worker-job").arg(&id).current_dir(&control_dir)
            .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir)
                .map_err(|e|e.to_string())?)
            .env("DOMINION_WORKSPACE",&control_dir)
            .env_remove("DOMINION_DESKTOP_PARENT");
        let remaining=record["limits"]["timeBudgetSeconds"].as_u64()
            .map(|budget|budget.saturating_sub(
                started.elapsed().as_secs().saturating_sub(
                    record["waitingMs"].as_u64().unwrap_or(0)/1000)))
            .unwrap_or(u64::MAX);
        let outcome=if remaining==0{
            Err("method_control_time_budget".to_string())
        }else{
            command(context,process,Duration::from_secs(
                if remaining==u64::MAX{900}else{remaining.min(900)})).await
                .map(|p|p.status.success())
        };
        let job=context.state.store.get("job",&id).ok_or("대조군 결과 소실")?;
        let session=job["sessionId"].as_str();
        let used=session.map(|sid|context.state.store.all("execution_operation")
            .iter().any(|op|op["sessionId"]==sid &&
                op["origin"]=="procedure")).unwrap_or(false);
        let candidate_id=verified_candidate_id(&job["result"]);
        let mut evaluation=Value::Null;
        let mut error=outcome.as_ref().err().map(|e|json!(e)).unwrap_or(Value::Null);
        if let Some(candidate)=candidate_id {
            match rsi_worktree::compare_at(context,candidate,&control_dir,
                Path::new(record["sourceDir"].as_str().ok_or("동결 평가기 경로 없음")?)).await{
                Ok(value)=>evaluation=value,
                Err(message)=>error=json!(message)
            }
        }
        let tokens=session.map(|sid|context.state.store.session_usage(sid)
            ["totalTokens"].as_u64().unwrap_or(0)).unwrap_or(0);
        let saved=context.state.store.put("rsi_method_control",&json!({
            "id":format!("rsi-method-control-{id}"),
            "source":"isolated_actual_worker_method_off_v1",
            "campaignId":context.job_id,"controlJobId":id,
            "generatorCommit":head,"methodSnapshot":child_input["rsiMethodSnapshot"],
            "promptAndCasesSha256":fingerprint,
            "providerId":child_input["providerId"],"model":child_input["model"],
            "domain":child_input["rsiTargetDomain"],
            "candidateId":candidate_id,
            "evaluation":evaluation,"executionError":error,
            "modelUsedTokens":tokens,
            "runtimeDisabledMethods":job["rsiMethodAblationDisabled"]==true,
            "procedureOriginObserved":used,
            "workerStatus":job["status"],"adopted":false,
            "createdAt":chrono::Utc::now().to_rfc3339()
        })).ok_or("원본 대조군 실험 결과 저장 실패")?;
        Ok::<(Value,u64),String>((saved,tokens))
    }.await;
    // The control worktree must never become the next accepted source.
    let cleanup=git(context,lab,&["worktree","remove","--force",control_path]).await;
    record["activeGenerationJobId"]=Value::Null;
    match observed {
        Ok((control,tokens))=>{
            record["tokens"]=json!(record["tokens"].as_u64().unwrap_or(0)
                .saturating_add(tokens));
            record["methodControlLast"]=control["id"].clone();
            child_input["rsiMethodAblationArm"]=json!("with_methods");
            child_input["rsiMethodAblationControlId"]=control["id"].clone();
            child_input["rsiMethodAblationInputSha256"]=json!(fingerprint);
            if let Err(error)=cleanup {record["methodControlCleanupError"]=json!(error);}
            save(context,record)?;
            Ok(())
        }
        Err(error)=>{
            record["methodControlError"]=json!(error);
            if let Err(cleanup)=cleanup {record["methodControlCleanupError"]=json!(cleanup);}
            save(context,record)?;
            // Do not claim any contrast; continue the independently measured
            // ON generation when an experimental control could not run.
            Ok(())
        }
    }
}
