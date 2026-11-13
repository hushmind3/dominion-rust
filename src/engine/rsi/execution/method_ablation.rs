//! Live matched-source method intervention for the compiled RSI descendant.
//! The OFF arm runs an actual generation with procedure injection and operator
//! dispatch disabled; the ON arm uses the same executable, source revision,
//! provider, model and frozen profile. Neither a hypothetical prompt nor a
//! synthetic score may serve as a control. OFF candidates are never adopted.
use super::*;

const SOURCE:&str="compiled_method_on_off_real_worker_v1";

fn digest(v:&Value)->String {
    format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}

fn method_selected(input:&Value)->bool {
    input["rsiSystemEvolution"]==true &&
        input["rsiImprovementAlgebra"]["selected"]["rule"]["method"]["path"]
            .as_str().is_some_and(crate::engine::rsi::safe_path)
        && input["rsiImprovementAlgebra"]["selected"]["target"]["path"]
            !=input["rsiImprovementAlgebra"]["selected"]["rule"]["method"]["path"]
}

/// This is called on the normal source evolution route, BEFORE the ON worker
/// is queued. It consumes real time and model tokens under the campaign budget.
/// If the compiled plan has no executable improvement method, no ablation is
/// needed and no control success is fabricated.
pub(super) async fn run_control(
    context:&JobContext,record:&mut Value,input:&Value,child_input:&mut Value,
    lab:&Path,binary:&Path,started:Instant,
)->Result<(),String>{
    if input["rsiMethodAblation"]!=true || !method_selected(child_input){
        return Ok(());
    }
    let original=child_input.clone();
    let sealed=digest(&original);
    let mut off=original.clone();
    off["rsiMethodAblationArm"]=json!("without_methods");
    let job=context.state.store.put("job",&json!({
        "kind":"rsi_generation","status":"running","input":off,
        "rsiControlExperiment":true,"rsiMethodAblationInputSha256":sealed,
        "queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1
    })).ok_or("개선법 대조군 작업 저장 실패")?;
    let id=job["id"].as_str().ok_or("개선법 대조군 작업 ID 없음")?.to_string();
    record["phase"]=json!("evaluating_method_control");
    record["activeGenerationJobId"]=json!(id);
    save(context,record)?;
    let mut worker=Command::new(binary);
    worker.arg("--rsi-worker-job").arg(&id).current_dir(lab)
        .env("DOMINION_DATA",std::fs::canonicalize(&context.state.config.data_dir)
            .map_err(|e|e.to_string())?)
        .env("DOMINION_WORKSPACE",lab)
        .env_remove("DOMINION_DESKTOP_PARENT");
    let remaining=record["limits"]["timeBudgetSeconds"].as_u64()
        .map(|budget|budget.saturating_sub(
            started.elapsed().as_secs().saturating_sub(
                record["waitingMs"].as_u64().unwrap_or(0)/1000)))
        .unwrap_or(u64::MAX);
    if remaining==0{return Err("rsi_method_ablation_time_budget".into());}
    let timeout=if remaining==u64::MAX{7*24*3600}else{remaining.max(1)};
    let launched=command(context,worker,Duration::from_secs(timeout)).await;
    let observed=context.state.store.get("job",&id)
        .ok_or("실행된 개선법 대조군 작업이 없습니다")?;
    let control_id=verified_candidate_id(&observed["result"]).map(str::to_string);
    let mut evaluation=Value::Null;
    let mut error=Value::Null;
    if let Some(candidate_id)=control_id.as_deref(){
        match rsi_worktree::compare_at(context,candidate_id,lab,
            Path::new(record["sourceDir"].as_str()
                .ok_or("대조군의 독립 평가 기준 소스 없음")?)).await {
            Ok(measured)=>evaluation=measured,
            Err(reason)=>error=json!(reason)
        }
    }
    let session=observed["sessionId"].as_str().unwrap_or("");
    let operations=context.state.store.all("execution_operation");
    let no_procedure=!session.is_empty() && !operations.iter().any(|op|
        op["sessionId"]==session && op["origin"]=="procedure");
    let disabled=no_procedure
        && observed["input"]["rsiMethodAblationArm"]=="without_methods"
        && observed["rsiOperatorDispatch"].is_null();
    let control=json!({
        "id":format!("rsi-method-control-{id}"),
        "source":SOURCE,"campaignId":context.job_id,
        "controlJobId":id,"controlCandidateId":control_id,
        "originalInput":original,"inputSha256":sealed,
        "sourceCommit":child_input["rsiGeneratorCommit"],
        "frozenCases":child_input["rsiFrozenCases"],
        "providerId":child_input["providerId"],"model":child_input["model"],
        "controlMethodDisabled":disabled,"controlAdopted":false,
        "workerStatus":observed["status"],
        "workerCommandError":launched.as_ref().err().map(String::as_str),
        "measurementError":error,"evaluation":evaluation,
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    context.state.store.put("rsi_method_control",&control)
        .ok_or("개선법 실제 대조군 평가 영수증 저장 실패")?;
    context.state.store.audit("rsi.compiled_method_control",json!({
        "controlId":control["id"],"jobId":id,
        "candidateId":control_id,"methodsDisabled":disabled,
        "evaluation":control["evaluation"]["id"]
    }));
    // Parent budgets must include OFF + ON. Never let an expensive control
    // become invisible to the subsequent model-budget check.
    record["tokens"]=json!(campaign_model_tokens(&context.state,&context.job_id));
    record["activeGenerationJobId"]=Value::Null;
    child_input["rsiMethodAblationArm"]=json!("with_methods");
    child_input["rsiMethodAblationControlId"]=control["id"].clone();
    child_input["rsiMethodAblationInputSha256"]=json!(sealed);
    save(context,record)?;
    Ok(())
}

fn quality_delta(control:&Value,treatment:&Value)->(usize,usize) {
    let mut better=0;let mut worse=0;
    for lhs in control["metrics"].as_array().into_iter().flatten()
        .filter(|m|matches!(m["key"].as_str(),Some("quality"|"discoveryScore"))) {
        let Some(rhs)=treatment["metrics"].as_array().into_iter().flatten()
            .find(|m|m["key"]==lhs["key"] && m["direction"]==lhs["direction"]
                && m["baseline"]==lhs["baseline"]) else {continue};
        let (Some(a),Some(b))=(lhs["candidate"].as_f64(),rhs["candidate"].as_f64())
            else {continue};
        if !a.is_finite()||!b.is_finite(){continue;}
        let gain=if lhs["direction"]=="higher"{b-a}else{a-b};
        if gain>1e-9{better+=1;}
        if gain< -1e-9{worse+=1;}
    }
    (better,worse)
}

/// Recompute the ON/OFF association from the original durable ledger, not
/// from a generation JSON field. Both arms must be independently paired
/// evaluated against the same frozen cases and original source commit.
pub(crate) fn measure(state:&AppState,g:&Value)->Value {
    let Some(on)=g["jobId"].as_str().and_then(|id|state.store.get("job",id))
        else{return Value::Null};
    let Some(control_id)=on["input"]["rsiMethodAblationControlId"].as_str()
        else{return Value::Null};
    let Some(saved)=state.store.get("rsi_method_control",control_id)
        else{return Value::Null};
    let Some(off)=saved["controlJobId"].as_str()
        .and_then(|id|state.store.get("job",id)) else{return Value::Null};
    let session=off["sessionId"].as_str().unwrap_or("");
    let methods_disabled=!session.is_empty()
        && !state.store.all("execution_operation").iter().any(|op|
            op["sessionId"]==session && op["origin"]=="procedure")
        && off["rsiOperatorDispatch"].is_null();
    let sealed=&saved["originalInput"];
    let stable=[
        "rsiCampaignId","rsiGeneratorCommit","rsiSourceDir",
        "rsiFrozenCases","rsiEvaluationConfig","rsiMetaCases",
        "rsiControllerPlan","rsiImprovementAlgebra","rsiMetaPolicy",
        "rsiIndexFingerprint","rsiSystemBlueprint","rsiTargetDomain",
        "providerId","model"
    ];
    let matched=saved["source"]==SOURCE
        && saved["inputSha256"]==digest(sealed)
        && saved["inputSha256"]==on["input"]["rsiMethodAblationInputSha256"]
        && saved["inputSha256"]==off["rsiMethodAblationInputSha256"]
        && saved["controlMethodDisabled"]==true && methods_disabled
        && saved["controlAdopted"]==false
        && off["input"]["rsiMethodAblationArm"]=="without_methods"
        && on["input"]["rsiMethodAblationArm"]=="with_methods"
        && saved["sourceCommit"]==g["generatorCommit"]
        && saved["campaignId"]==g["campaignId"]
        && sealed["rsiCampaignId"]==g["campaignId"]
        && off["input"]["rsiCampaignId"]==g["campaignId"]
        && on["input"]["rsiCampaignId"]==g["campaignId"]
        && on["input"]["rsiGeneratorCommit"]==g["generatorCommit"]
        && off["input"]["rsiGeneratorCommit"]==g["generatorCommit"]
        && stable.iter().all(|k|sealed[*k]==off["input"][*k]
            && sealed[*k]==on["input"][*k])
        && g["accepted"]==true
        && saved["controlCandidateId"].as_str()
            ==verified_candidate_id(&off["result"])
        && off["input"]["providerId"]==on["input"]["providerId"]
        && off["input"]["model"]==on["input"]["model"]
        && saved["workerStatus"]==off["status"];
    if !matched{return json!({"status":"mismatched_method_control",
        "methodAssociationObserved":false,"qualityProven":false});}
    let Some(control)=saved["controlCandidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id))
        else{return json!({"status":"no_independent_control_candidate",
            "methodAssociationObserved":false,"qualityProven":false});};
    let Some(treatment)=g["candidateId"].as_str()
        .and_then(|id|state.store.get("rsi_candidate",id))
        else{return Value::Null};
    let ctrl_eval=&control["evaluation"];
    let on_eval=&treatment["evaluation"];
    if crate::engine::rsi::replayed_pair_metrics(state,&control)
            .as_ref()!=Some(&ctrl_eval["metrics"])
        || crate::engine::rsi::replayed_pair_metrics(state,&treatment)
            .as_ref()!=Some(&on_eval["metrics"])
        || ctrl_eval["source"]!="server_invoked_local_paired_git_worktrees"
        || on_eval["source"]!="server_invoked_local_paired_git_worktrees"
        || ctrl_eval["id"]!=saved["evaluation"]["evaluationId"]
        || ctrl_eval["evidenceSha256"]!=saved["evaluation"]["evidenceSha256"]
        || ctrl_eval["metrics"]!=saved["evaluation"]["metrics"]
        || control["baseCommit"]!=treatment["baseCommit"]
        || control["rsiJobId"]!=off["id"]
        || treatment["rsiJobId"]!=on["id"]
        || g["evaluation"]["evidenceSha256"]!=on_eval["evidenceSha256"]
        || g["evaluation"]["metrics"]!=on_eval["metrics"]
        || control["cases"]!=treatment["cases"]
        || saved["controlCandidateId"]==g["candidateId"] {
        return json!({"status":"invalid_paired_receipts",
            "methodAssociationObserved":false,"qualityProven":false});
    }
    let selected=&g["improvementAlgebraPlan"]["selected"]["rule"];
    let operator=crate::engine::rsi_operator_provenance::verified(state,g,selected);
    let before=ctrl_eval["metrics"]["measurements"].as_array();
    let after=on_eval["metrics"]["measurements"].as_array();
    let (Some(before),Some(after))=(before,after)else{return Value::Null};
    let mut cases=0usize;let mut gains=0usize;let mut losses=0usize;
    let mut quality_gains=0usize;let mut quality_losses=0usize;
    for prior in before.iter().filter(|row|row["holdout"]==true){
        let Some(next)=after.iter().find(|row|row["holdout"]==true
            && row["id"]==prior["id"] && row["domain"]==prior["domain"]
            && row["baselinePass"]==prior["baselinePass"]) else{continue};
        cases+=1;
        if prior["candidatePass"]==false && next["candidatePass"]==true{gains+=1;}
        if prior["candidatePass"]==true && next["candidatePass"]==false{losses+=1;}
        if prior["candidatePass"]==true && next["candidatePass"]==true{
            let (win,lose)=quality_delta(prior,next);
            quality_gains+=win;quality_losses+=lose;
        }
    }
    let all_cases=cases>=2 && cases==before.iter().filter(|row|row["holdout"]==true).count();
    let observed=operator && all_cases && (gains+quality_gains)>0
        && losses+quality_losses==0;
    json!({
        "source":SOURCE,"status":if observed{"method_enabled_gain_observed"}else{"no_proven_method_gain"},
        "controlId":saved["id"],"controlJobId":off["id"],
        "treatmentJobId":on["id"],
        "controlCandidateId":control["id"],"treatmentCandidateId":treatment["id"],
        "generatorCommit":g["generatorCommit"],
        "controlEvaluationId":ctrl_eval["id"],"treatmentEvaluationId":on_eval["id"],
        "controlEvidenceSha256":ctrl_eval["evidenceSha256"],
        "treatmentEvidenceSha256":on_eval["evidenceSha256"],
        "methodFamilyId":g["operatorExecution"]["methodFamilyId"],
        "methodSourceSha256":g["operatorExecution"]["methodSourceSha256"],
        "methodActuallyExecuted":operator,
        "sameOriginalSourceAndFrozenCases":true,
        "pairedHoldoutCases":cases,"treatmentOnlyWins":gains,
        "controlOnlyWins":losses,"treatmentQualityWins":quality_gains,
        "controlQualityWins":quality_losses,
        "methodAssociationObserved":observed,"qualityProven":observed,
        "causalAblationProven":false,
        "limitation":"single on/off model samples are not a randomized causal estimate"
    })
}

#[cfg(test)]
#[path="method_ablation/tests.rs"]
mod tests;
