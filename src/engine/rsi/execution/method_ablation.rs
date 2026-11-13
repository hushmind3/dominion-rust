//! Paired, explicitly authorized method ablation against independently run
//! source-task workers. Both arms start with the same source commit, provider,
//! model, prompt and sealed evaluation profile. OFF cannot inherit procedures;
//! ON may execute them. Control patches are evaluated but NEVER adopted.
//! Run-to-run stochastic variation is recorded, not suppressed or called proof.
use super::*;

fn fingerprint(input:&Value)->String {
    let mut clean=input.clone();
    if let Some(fields)=clean.as_object_mut(){
        fields.remove("rsiMethodAblationArm");
        fields.remove("rsiMethodAblationControlId");
    }
    format!("{:x}",Sha256::digest(serde_json::to_vec(&clean).unwrap_or_default()))
}

/// Separate detached worker; it uses the same base as the ON worker but
/// cannot use any stored executable improvement procedure. Its patch is
/// measured through the frozen evaluator and intentionally not promoted.
pub(super) async fn run_method_control(
    context:&JobContext,record:&mut Value,input:&Value,child_input:&mut Value,
    lab:&PathBuf,binary:&PathBuf,_attempt:u64,started:Instant,
)->Result<(),String>{
    if input["rsiMethodAblation"]!=true{return Ok(());}
    // Own the domain before mutating child_input; retaining a borrowed str
    // across arm assignment would conflict with the treatment input update.
    let domain=child_input["rsiTargetDomain"].as_str().unwrap_or("").to_owned();
    let choice=method_reuse_decision(&context.state,child_input,&domain);
    if choice["action"]!="execute_methods"{return Ok(());}
    let mut decision=child_input["rsiDecisionInput"].clone();
    if !decision.is_object(){decision=json!({});}
    decision["nextDomain"]=json!(domain);
    decision["reuseDecision"]=choice.clone();
    child_input["rsiDecisionInput"]=decision;
    let fp=fingerprint(child_input);
    let profile=crate::engine::rsi_profile::from_input(child_input)?;
    let mut control_input=child_input.clone();
    control_input["rsiMethodAblationArm"]=json!("without_methods");
    let job=context.state.store.put("job",&json!({
        "kind":"rsi_generation","status":"running","input":control_input,
        "queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1,
        "rsiControlExperiment":true,
        // Keep the immutable pre-worker pairing hash outside input; worker
        // legitimately updates its own input with runtime-derived fields.
        "rsiMethodAblationInputSha256":fp
    })).ok_or("개선법 대조군 작업 저장 실패")?;
    let control_id=job["id"].as_str().ok_or("개선법 대조군 ID 없음")?;
    record["phase"]=json!("evaluating_method_control");
    record["activeGenerationJobId"]=json!(control_id);
    save(context,record)?;
    let mut command_line=Command::new(binary);
    command_line.arg("--rsi-worker-job").arg(control_id)
        .current_dir(lab)
        .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir)
            .map_err(|e|e.to_string())?)
        .env("DOMINION_WORKSPACE",lab)
        .env_remove("DOMINION_DESKTOP_PARENT");
    let remaining=record["limits"]["timeBudgetSeconds"].as_u64()
        .map(|budget|budget.saturating_sub(
            started.elapsed().as_secs().saturating_sub(
                record["waitingMs"].as_u64().unwrap_or(0)/1000)))
        .unwrap_or(u64::MAX);
    if remaining==0 {return Err("improver_method_ablation_time_budget".into());}
    let timeout=if remaining==u64::MAX{7*24*3600}else{remaining.max(1)};
    let launched=command(context,command_line,Duration::from_secs(timeout)).await;
    let observed=context.state.store.get("job",control_id)
        .ok_or("개선법 대조군 실행 결과 없음")?;
    let session=observed["sessionId"].as_str().unwrap_or("");
    let executed_procedures=context.state.store.all("execution_operation").iter().any(|op|
        op["sessionId"]==session && op["origin"]=="procedure");
    let candidate_id=verified_candidate_id(&observed["result"]);
    let mut result=Value::Null;
    let mut measurement_error=Value::Null;
    if let Some(id)=candidate_id {
        match rsi_worktree::compare_at(context,id,lab,
            Path::new(record["sourceDir"].as_str().ok_or("평가기 경로 없음")?)).await {
            Ok(measured)=>result=measured,
            Err(err)=>measurement_error=json!(err),
        }
    }
    // The controller must account for BOTH API calls before the treatment
    // is launched. The next budget check is performed by the campaign.
    record["tokens"]=json!(campaign_model_tokens(&context.state,&context.job_id));
    record["activeGenerationJobId"]=Value::Null;
    let control=json!({
        "id":format!("rsi-method-ablation-{control_id}"),
        "source":"real_detached_method_disabled_worker_v1",
        "campaignId":context.job_id,"controlJobId":control_id,
        "generatorCommit":child_input["rsiGeneratorCommit"],
        "sourceDir":child_input["rsiSourceDir"],
        "domain":domain,"candidateId":candidate_id,
        "providerId":child_input["providerId"],"model":child_input["model"],
        "profileSha256":profile["sha256"],
        "inputSha256":fp,"disabledMethodIds":choice["methodIds"],
        "methodsActuallyDisabled":!executed_procedures
            && observed["input"]["rsiMethodAblationArm"]=="without_methods",
        "workerStatus":observed["status"],
        "workerModelError":observed["result"]["rsi"]["modelError"],
        "workerCommandError":launched.as_ref().err().map(String::as_str),
        "evaluation":result,"measurementError":measurement_error,
        "adopted":false,"status":"control_measured_not_promoted",
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    let saved=context.state.store.put("rsi_method_ablation",&control)
        .ok_or("대조군 실측 영수증 저장 실패")?;
    context.state.store.audit("rsi.method_ablation_control",json!({
        "id":saved["id"],"jobId":control_id,
        "methodsActuallyDisabled":saved["methodsActuallyDisabled"],
        "candidateId":saved["candidateId"],"measurementError":saved["measurementError"]
    }));
    child_input["rsiMethodAblationArm"]=json!("with_methods");
    child_input["rsiMethodAblationControlId"]=saved["id"].clone();
    child_input["rsiMethodAblationInputSha256"]=json!(fp);
    save(context,record)?;
    Ok(())
}

/// Frozen quality dimensions are evidence of solving better tasks, unlike
/// cheaper execution that merely returns the same answer. The metric baseline
/// and direction must match in both independently evaluated candidate reports.
fn paired_quality_delta(control:&Value,treatment:&Value)->(usize,usize){
    let mut gains=0usize;let mut losses=0usize;
    let left=control["metrics"].as_array().into_iter().flatten();
    for old in left.filter(|m|matches!(m["key"].as_str(),Some("quality"|"discoveryScore"))) {
        let Some(next)=treatment["metrics"].as_array().into_iter().flatten()
            .find(|m|m["key"]==old["key"] && m["direction"]==old["direction"]
                && m["baseline"]==old["baseline"]) else{continue};
        let (Some(before),Some(after))=(old["candidate"].as_f64(),next["candidate"].as_f64()) else{continue};
        if !before.is_finite() || !after.is_finite(){continue;}
        let improvement=if old["direction"]=="higher"{after-before}else{before-after};
        if improvement>1e-9 {gains+=1;}
        if improvement < -1e-9 {losses+=1;}
    }
    (gains,losses)
}

/// Bind treatment and control to independently saved candidate receipts.
/// Comparison is honest about model sampling: a single paired run supports
/// a task-quality observation, not an isolated causal effect estimate.
pub(crate) fn measured_method_ablation(state:&AppState,generation:&Value)->Value{
    let Some(treatment_job)=generation["jobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return Value::Null};
    let Some(id)=treatment_job["input"]["rsiMethodAblationControlId"].as_str() else{return Value::Null};
    let Some(control)=state.store.get("rsi_method_ablation",id) else{return Value::Null};
    let Some(control_job)=control["controlJobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return Value::Null};
    let control_session=control_job["sessionId"].as_str().unwrap_or("");
    let disabled_at_runtime=!control_session.is_empty()
        && !state.store.all("execution_operation").iter().any(|operation|
            operation["sessionId"]==control_session && operation["origin"]=="procedure");
    let bound=control["source"]=="real_detached_method_disabled_worker_v1"
        && control["methodsActuallyDisabled"]==true
        && disabled_at_runtime
        && control["inputSha256"]==control_job["rsiMethodAblationInputSha256"]
        && control["inputSha256"]==treatment_job["rsiMethodAblationInputSha256"]
        && control["candidateId"].as_str()==verified_candidate_id(&control_job["result"])
        && control["adopted"]==false
        && generation["accepted"]==true
        && control["generatorCommit"]==generation["generatorCommit"]
        && control_job["input"]["rsiGeneratorCommit"]==generation["generatorCommit"]
        && control["sourceDir"]==treatment_job["input"]["rsiSourceDir"]
        && control["domain"]==generation["domain"]
        && control["providerId"]==treatment_job["input"]["providerId"]
        && control["model"]==treatment_job["input"]["model"]
        && treatment_job["input"]["rsiMethodAblationArm"]=="with_methods"
        && crate::engine::rsi_profile::from_input(&treatment_job["input"])
            .is_ok_and(|profile|profile["sha256"]==control["profileSha256"]);
    if !bound{return json!({"status":"unmatched","qualityProven":false});}
    let Some(control_candidate)=control["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{
        return json!({"status":"control_no_candidate","qualityProven":false});
    };
    let Some(treatment)=generation["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id)) else{return Value::Null};
    if crate::engine::rsi::replayed_pair_metrics(state,&control_candidate).is_none()
        || !crate::engine::rsi::verified_pair_receipt(state,&treatment)
        || control_candidate["baseCommit"]!=treatment["baseCommit"]
        || control_candidate["evaluation"]["id"]!=control["evaluation"]["evaluationId"]
        || control_candidate["evaluation"]["metrics"]!=control["evaluation"]["metrics"]
        || control_candidate["evaluation"]["evidenceSha256"]!=control["evaluation"]["evidenceSha256"]
        || control_candidate["evaluation"]["source"]!=control["evaluation"]["source"]
        || control_candidate["cases"]!=treatment["cases"] {
        return json!({"status":"independent_receipts_or_case_plan_mismatch","qualityProven":false});
    }
    let controls=control_candidate["evaluation"]["metrics"]["measurements"]
        .as_array().cloned().unwrap_or_default();
    let treatments=treatment["evaluation"]["metrics"]["measurements"]
        .as_array().cloned().unwrap_or_default();
    let mut better=0usize;let mut worse=0usize;let mut paired=0usize;
    let mut quality_gains=0usize;let mut quality_regressions=0usize;
    for before in controls.iter().filter(|case|case["holdout"]==true) {
        let Some(after)=treatments.iter().find(|case|case["id"]==before["id"]
            && case["domain"]==before["domain"] && case["holdout"]==true
            && case["baselinePass"]==before["baselinePass"]) else{continue};
        paired+=1;
        if before["candidatePass"]==false && after["candidatePass"]==true {better+=1;}
        if before["candidatePass"]==true && after["candidatePass"]==false {worse+=1;}
        if before["candidatePass"]==true && after["candidatePass"]==true {
            let (gain,loss)=paired_quality_delta(before,after);
            quality_gains+=gain;quality_regressions+=loss;
        }
    }
    let real_method=generation["methodExecutions"].as_array()
        .is_some_and(|methods|methods.iter().any(|m|
            control["disabledMethodIds"].as_array().is_some_and(|ids|ids.contains(&m["methodId"]))
            && m["result"]["verified"]==true && m["result"]["followed"]==true));
    let observed=real_method && paired>=2 && (better+quality_gains)>0
        && worse==0 && quality_regressions==0
        && paired==controls.iter().filter(|case|case["holdout"]==true).count();
    json!({
        "source":"real_method_enabled_disabled_paired_source_task_v1",
        "controlId":control["id"],"controlJobId":control["controlJobId"],
        "treatmentJobId":generation["jobId"],
        "controlCandidateId":control_candidate["id"],"treatmentCandidateId":treatment["id"],
        "sourceCommit":generation["generatorCommit"],
        "controlEvaluationId":control_candidate["evaluation"]["id"],
        "treatmentEvaluationId":treatment["evaluation"]["id"],
        "sameOriginalSourceAndFrozenProfile":true,
        "actualInheritedMethodExecuted":real_method,
        "pairedHoldoutCases":paired,"treatmentOnlyWins":better,
        "controlOnlyWins":worse,
        "treatmentQualityMetricWins":quality_gains,
        "controlQualityMetricWins":quality_regressions,
        "methodAssociationObserved":observed,
        "causalAblationProven":false,
        "qualityProven":observed,
        "limitation":"single paired model runs may differ by sampling and shared persistent context; repeat controlled seeds needed"
    })
}

