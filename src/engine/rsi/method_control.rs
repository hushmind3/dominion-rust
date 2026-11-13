//! Real source-level method control: run the PREVIOUS compiled descendant on
//! the SAME current source tree and frozen task suite as its revised child.
//! The control has its own model/tool worker, isolated Git worktree and native
//! paired evaluator. No control candidate can be adopted by this module.
use super::{
    AppState,JobContext,Value,Path,PathBuf,Command,Duration,Instant,
    json,command,save,rsi,rsi_worktree,verified_candidate_id,
    bind_action_to_source,Sha256,Digest,
};

fn compiled_method_parent(state:&AppState,record:&Value,old:&str,head:&str)->Option<Value>{
    let history=record["history"].as_array()?;
    let checked=crate::engine::rsi_system::revalidated_method_history(state,history);
    let last=checked.last()?;
    let candidate=state.store.get("rsi_candidate",last["candidateId"].as_str()?)?;
    let paths=candidate["paths"].as_array()?;
    (last["accepted"]==true
        && last["sourceLineageVerified"]==true
        && last["acceptedCommit"]==head
        && last["generatorCommit"]==old
        && candidate["status"]=="verified"
        && !paths.is_empty()
        && paths.iter().all(|p|p.as_str()
            .is_some_and(crate::engine::rsi_system::executable_method_source)))
            .then_some(last.clone())
}

fn install_control_plan(input:&mut Value,plan:&Value,old:&str){
    input["rsiSelectedDomain"]=plan["domain"].clone();
    input["rsiTargetDomain"]=plan["domain"].clone();
    input["rsiTargetPlan"]=plan["targetPlan"].clone();
    input["rsiFocusFiles"]=plan["focusFiles"].clone();
    input["rsiFocusSelection"]=json!({
        "source":"old_compiled_method_control_worker",
        "generatorCommit":old,"domain":plan["domain"],
        "candidates":plan["ranked"],
        "sourceIndexId":plan["indexId"]
    });
    for (input_key,plan_key) in [
        ("rsiSystemBlueprint","systemBlueprint"),
        ("rsiImprovementDirective","improvementDirective"),
        ("rsiMetaPolicy","metaPolicy"),
        ("rsiMetaActionPolicy","metaActionPolicy"),
        ("rsiPeerLineages","peerLineages"),
        ("rsiMutualCurriculum","mutualCurriculum"),
        ("rsiFactorialCurriculum","factorialCurriculum"),
        ("rsiRecursiveDiscovery","recursiveDiscovery"),
        ("rsiRecursiveScienceActive","recursiveScienceActive"),
        ("rsiPrincipleProgramme","principleProgramme"),
        ("rsiPrincipleActive","principleActive"),
        ("rsiOpenProgramPlan","openProgramPlan"),
        ("rsiArchiveSelection","archiveSelection"),
        ("rsiEditPolicy","editPolicy"),
        ("rsiModelRoute","modelRoute"),
        ("rsiSuccessorStage","successorStage")
    ]{
        input[input_key]=plan[plan_key].clone();
    }
    input["rsiInheritedCandidateId"]=plan["archiveSelection"]["candidateId"].clone();
    input["rsiDescendantPlan"]=plan.clone();
    input["rsiExperimentContract"]=plan["experimentContract"].clone();
}

/// The control is allowed only when a real, independently adopted A changed
/// EXECUTABLE method sources and we have the binary from before A. Its output
/// is never added to rsi_generation history or used for promotion.
pub(super) async fn run_compiled_method_control(
    context:&JobContext,record:&mut Value,child_input:&mut Value,
    lab:&PathBuf,old_binary:&Path,old_commit:&str,head:&str,
    attempt:u64,started:Instant
)->Result<(),String>{
    if child_input["rsiSystemEvolution"]!=true
        || child_input["rsiAutoFocus"]!=true
        || child_input["rsiMethodControlEnabled"]!=true
        || !old_binary.is_file()
        || child_input["rsiTrialAlgorithmId"].is_string() {
        return Ok(());
    }
    let Some(parent)=compiled_method_parent(&context.state,record,old_commit,head)
        else{return Ok(())};
    let previous_sha=record["controlGeneratorBinarySha256"].as_str()
        .unwrap_or("").to_owned();
    let new_sha=record["generatorBinarySha256"].as_str().unwrap_or("").to_owned();
    if previous_sha.len()!=64 || new_sha.len()!=64 || previous_sha==new_sha {
        return Ok(());
    }
    let old_bytes=std::fs::read(old_binary).map_err(|e|e.to_string())?;
    if format!("{:x}",Sha256::digest(&old_bytes))!=previous_sha {
        return Err("개선법 대조 바이너리의 원본 SHA-256이 변경됐습니다.".into());
    }
    let frozen=child_input["rsiFrozenCases"].clone();
    let frozen_sha=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&frozen).map_err(|e|e.to_string())?));
    let treatment_domain=child_input["rsiTargetDomain"].clone();
    let treatment_provider=child_input["providerId"].clone();
    let treatment_model=child_input["model"].clone();
    let mut control_input=child_input.clone();
    control_input["rsiGeneratorCommit"]=json!(old_commit);
    control_input["rsiControlExperiment"]=json!(true);
    control_input["rsiMethodControlArm"]=json!("previous_compiled_method");
    // Both arms edit an isolated checkout of CURRENT source HEAD. The
    // counterfactual changes only which already-compiled agent makes plans.
    control_input["rsiBaseCommit"]=json!(head);
    let snapshot=json!({
        "campaignId":context.job_id,
        "generatorCommit":old_commit,
        "sourceHeadCommit":head,
        "controlExperiment":true,
        "attempt":attempt,
        "historyCount":record["history"].as_array()
            .map(Vec::len).unwrap_or(0),
        "input":control_input
    });
    let dir=context.state.config.data_dir.join("rsi-method-controls");
    std::fs::create_dir_all(&dir).map_err(|e|e.to_string())?;
    let snapshot_path=dir.join(format!("{}-{}-{}.json",
        context.job_id,attempt,uuid::Uuid::new_v4()));
    std::fs::write(&snapshot_path,serde_json::to_vec(&snapshot)
        .map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut planner=Command::new(old_binary);
    planner.arg("--rsi-plan-counterfactual").arg(&snapshot_path)
        .current_dir(lab)
        .env("DOMINION_DATA",std::fs::canonicalize(
            &context.state.config.data_dir).map_err(|e|e.to_string())?)
        .env("DOMINION_WORKSPACE",lab)
        .env_remove("DOMINION_DESKTOP_PARENT");
    let result=command(context,planner,Duration::from_secs(45)).await;
    let _=std::fs::remove_file(&snapshot_path);
    let output=result?;
    if !output.status.success(){
        return Err(format!("구버전 컴파일 개선기 대조 계획 실패: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars().take(800).collect::<String>()));
    }
    if output.stdout.len()>2*1024*1024 {
        return Err("대조 계획 결과가 허용 크기를 초과했습니다.".into());
    }
    let plan:Value=serde_json::from_slice(&output.stdout)
        .map_err(|e|format!("대조군 계획 JSON 오류: {e}"))?;
    if plan["source"]!="compiled_descendant_system_controller_v3"
        || plan["generatorCommit"]!=old_commit
        || plan["domain"]!=treatment_domain
        || plan["indexId"]!=child_input["rsiIndexId"]
        || plan["modelRoute"]["selected"]["providerId"]!=treatment_provider
        || plan["modelRoute"]["selected"]["model"]!=treatment_model
        || plan["focusFiles"].as_array().is_none_or(|a|a.is_empty() || a.len()>3) {
        record["methodControlSkip"]=json!({
            "status":"unmatched_old_compiled_plan",
            "oldDomain":plan["domain"],"newDomain":treatment_domain,
            "oldRoute":plan["modelRoute"]["selected"],
            "oldCommit":old_commit,"newCommit":head
        });
        return Ok(());
    }
    crate::engine::rsi_system::validate_plan_evidence(
        &context.state,lab,old_commit,
        child_input["rsiIndexId"].as_str().unwrap_or(""),
        &plan,record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]))?;
    install_control_plan(&mut control_input,&plan,old_commit);
    bind_action_to_source(&context.state,&mut control_input,
        record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]));
    let control_job=context.state.store.put("job",&json!({
        "kind":"rsi_generation","status":"running",
        "input":control_input,
        "queuedAt":chrono::Utc::now().to_rfc3339(),
        "attempts":1,"rsiControlExperiment":true
    })).ok_or("개선법 대조군 작업 저장 실패")?;
    let id=control_job["id"].as_str().ok_or("대조군 작업 ID가 없습니다.")?;
    record["phase"]=json!("evaluating_previous_compiled_method");
    record["activeGenerationJobId"]=json!(id);
    save(context,record)?;
    let remaining=record["limits"]["timeBudgetSeconds"].as_u64()
        .map(|limit|limit.saturating_sub(
            started.elapsed().as_secs().saturating_sub(
                record["waitingMs"].as_u64().unwrap_or(0)/1000)))
        .unwrap_or(u64::MAX);
    if remaining==0 {
        record["activeGenerationJobId"]=Value::Null;
        return Err("개선법 대조군 실행의 시간 예산이 종료됐습니다.".into());
    }
    let mut cmd=Command::new(old_binary);
    cmd.arg("--rsi-worker-job").arg(id)
        .current_dir(lab)
        .env("DOMINION_DATA",std::fs::canonicalize(
            &context.state.config.data_dir).map_err(|e|e.to_string())?)
        .env("DOMINION_WORKSPACE",lab)
        .env_remove("DOMINION_DESKTOP_PARENT");
    let launched=command(context,cmd,Duration::from_secs(
        if remaining==u64::MAX {7*24*3600}else{remaining.min(900).max(1)}
    )).await;
    let observed=context.state.store.get("job",id)
        .ok_or("구버전 개선기의 실제 후손 작업 기록이 없습니다.")?;
    let control_session=observed["sessionId"].as_str().unwrap_or("");
    let usage=context.state.store.session_usage(control_session);
    record["tokens"]=json!(record["tokens"].as_u64().unwrap_or(0)
        .saturating_add(usage["totalTokens"].as_u64().unwrap_or(0)));
    record["activeGenerationJobId"]=Value::Null;
    let candidate_id=verified_candidate_id(&observed["result"]);
    let mut evaluation=Value::Null;
    let mut error=Value::Null;
    if let Some(id)=candidate_id {
        match rsi_worktree::compare_at(context,id,lab,
            Path::new(record["sourceDir"].as_str().unwrap_or(""))).await {
            Ok(measured)=>evaluation=measured,
            Err(problem)=>error=json!(problem)
        }
    }
    let receipt=json!({
        "id":format!("rsi-compiled-method-control-{}",id),
        "source":"matched_previous_vs_revised_compiled_method_workers_v1",
        "campaignId":context.job_id,
        "controlJobId":id,
        "controlCandidateId":candidate_id,
        "parentMethodGenerationId":parent["id"],
        "parentMethodCandidateId":parent["candidateId"],
        "parentMethodSourceHashes":parent["acceptedSourceHashes"],
        "previousCompiledCommit":old_commit,
        "revisedCompiledCommit":head,
        "previousBinarySha256":previous_sha,
        "revisedBinarySha256":new_sha,
        "sourceHeadCommit":head,
        "sourceDir":lab,
        "model":treatment_model,
        "providerId":treatment_provider,
        "domain":treatment_domain,
        "frozenCasesSha256":frozen_sha,
        "controlPlan":plan,
        "controlStatus":observed["status"],
        "workerError":launched.as_ref().err().map(String::as_str),
        "evaluation":evaluation,
        "evaluationError":error,
        "adopted":false,
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    let stored=context.state.store.put("rsi_method_control",&receipt)
        .ok_or("개선법 대조 실험 영수증 저장 실패")?;
    child_input["rsiMethodControlId"]=stored["id"].clone();
    child_input["rsiMethodControlArm"]=json!("revised_compiled_method");
    record["lastMethodControl"]=json!({
        "id":stored["id"],"controlJobId":id,
        "candidateId":candidate_id,"evaluationError":error
    });
    save(context,record)?;
    Ok(())
}


/// Recompute treatment > control from TWO ORIGINAL native paired evaluator
/// receipts. Neither a control job's status nor a model's "causal" prose can
/// promote the revised method without a measurable held-out task win.
pub(crate) fn measured_advantage(
    state:&AppState,parent:&Value,generation:&Value
)->Value {
    let Some(control)=generation["methodControlId"].as_str()
        .and_then(|id|state.store.get("rsi_method_control",id))
        else{return Value::Null;};
    let (Some(control_job),Some(treatment_job))=(
        control["controlJobId"].as_str().and_then(|id|state.store.get("job",id)),
        generation["jobId"].as_str().and_then(|id|state.store.get("job",id))
    )else{return Value::Null;};
    let (Some(control_candidate),Some(treatment_candidate))=(
        control["controlCandidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id)),
        generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id))
    )else{return Value::Null;};
    let old=parent["generatorCommit"].as_str().unwrap_or("");
    let revised=parent["acceptedCommit"].as_str().unwrap_or("");
    let control_profile=&control["controlPlan"];
    let original_methods=parent["acceptedSourceHashes"].as_object()
        .is_some_and(|items|!items.is_empty() && items.keys()
            .all(|path|crate::engine::rsi_system::executable_method_source(path)));
    let frozen_sha=format!("{:x}",Sha256::digest(
        serde_json::to_vec(&treatment_job["input"]["rsiFrozenCases"])
            .unwrap_or_default()));
    let new_plan=&treatment_job["input"]["rsiDescendantPlan"];
    let behavior_diff=control_profile["metaPolicy"]["revision"]
            !=new_plan["metaPolicy"]["revision"]
        || control_profile["focusFiles"]!=new_plan["focusFiles"]
        || control_profile["metaActionPolicy"]!=new_plan["metaActionPolicy"]
        || control_profile["domain"]!=new_plan["domain"];
    let bound=control["source"]=="matched_previous_vs_revised_compiled_method_workers_v1"
        && parent["accepted"]==true
        && parent["sourceLineageVerified"]==true
        && original_methods
        && parent["candidateId"]==control["parentMethodCandidateId"]
        && parent["acceptedSourceHashes"]==control["parentMethodSourceHashes"]
        && control["previousCompiledCommit"]==old
        && control["revisedCompiledCommit"]==revised
        && control["sourceHeadCommit"]==revised
        && generation["generatorCommit"]==revised
        && control["previousBinarySha256"]==parent["generatorBinarySha256"]
        && control["revisedBinarySha256"]==generation["generatorBinarySha256"]
        && control["previousBinarySha256"]!=control["revisedBinarySha256"]
        && generation["accepted"]==true
        && generation["pairedRevalidated"]==true
        && control["adopted"]==false
        && control_job["rsiControlExperiment"]==true
        && control_job["input"]["rsiControlExperiment"]==true
        && control_job["input"]["rsiMethodControlArm"]=="previous_compiled_method"
        && treatment_job["input"]["rsiMethodControlArm"]=="revised_compiled_method"
        && treatment_job["input"]["rsiMethodControlId"]==control["id"]
        && control_job["input"]["rsiBaseCommit"]==revised
        && control_job["input"]["rsiGeneratorCommit"]==old
        && treatment_job["input"]["rsiGeneratorCommit"]==revised
        && treatment_job["input"]["rsiBaseCommit"]==revised
        && control_job["input"]["rsiSourceDir"]==treatment_job["input"]["rsiSourceDir"]
        && control["sourceDir"]==treatment_job["input"]["rsiSourceDir"]
        && control_job["rsiMetaPolicyExecuted"]==true
        && treatment_job["rsiMetaPolicyExecuted"]==true
        && &control_job["input"]["rsiDescendantPlan"]==control_profile
        && control["domain"]==generation["domain"]
        && control_job["rsiDomain"]==generation["domain"]
        && treatment_job["rsiDomain"]==generation["domain"]
        && control["providerId"]==treatment_job["input"]["providerId"]
        && control["model"]==treatment_job["input"]["model"]
        && control_job["input"]["providerId"]==treatment_job["input"]["providerId"]
        && control_job["input"]["model"]==treatment_job["input"]["model"]
        && control["frozenCasesSha256"]==frozen_sha
        && control_job["input"]["rsiFrozenCases"]==treatment_job["input"]["rsiFrozenCases"]
        && control_candidate["rsiJobId"]==control_job["id"]
        && treatment_candidate["rsiJobId"]==treatment_job["id"]
        && control_candidate["baseCommit"]==revised
        && treatment_candidate["baseCommit"]==revised
        && control_candidate["cases"]==treatment_candidate["cases"]
        && control_candidate["oracleVersion"]==treatment_candidate["oracleVersion"]
        && control_candidate["evaluation"]["metrics"]==control["evaluation"]["metrics"]
        && control_candidate["evaluation"]["evidenceSha256"]
            ==control["evaluation"]["evidenceSha256"]
        && crate::engine::rsi::replayed_pair_metrics(state,&control_candidate)
            .is_some_and(|m|m==control_candidate["evaluation"]["metrics"])
        && crate::engine::rsi::verified_pair_receipt(state,&treatment_candidate)
        && behavior_diff;
    if !bound {
        return json!({"source":"source_identical_old_new_compiled_method_control_v1",
            "qualityProven":false,"status":"unmatched_or_unreplayed_control",
            "controlId":control["id"],"compiledDecisionChanged":behavior_diff});
    }
    let controls=control_candidate["evaluation"]["metrics"]["measurements"]
        .as_array().cloned().unwrap_or_default();
    let treated=treatment_candidate["evaluation"]["metrics"]["measurements"]
        .as_array().cloned().unwrap_or_default();
    let mut compared=0usize;
    let mut wins=0usize;
    let mut losses=0usize;
    let mut quality_wins=0usize;
    let mut quality_losses=0usize;
    for old_case in controls.iter().filter(|case|case["holdout"]==true) {
        let Some(new_case)=treated.iter().find(|row|
            row["id"]==old_case["id"] && row["domain"]==old_case["domain"]
                && row["holdout"]==true && row["baselinePass"]==old_case["baselinePass"])
            else{continue};
        compared+=1;
        if old_case["candidatePass"]==false && new_case["candidatePass"]==true{
            wins+=1;
        }else if old_case["candidatePass"]==true && new_case["candidatePass"]==false{
            losses+=1;
        }else if old_case["candidatePass"]==true && new_case["candidatePass"]==true{
            for old_metric in old_case["metrics"].as_array().into_iter().flatten()
                .filter(|m|matches!(m["key"].as_str(),Some("quality"|"discoveryScore"))) {
                let Some(new_metric)=new_case["metrics"].as_array()
                    .into_iter().flatten().find(|m|
                        m["key"]==old_metric["key"]
                            && m["direction"]==old_metric["direction"]
                            && m["baseline"]==old_metric["baseline"])
                    else{continue};
                let (Some(before),Some(after))=(
                    old_metric["candidate"].as_f64(),
                    new_metric["candidate"].as_f64())else{continue};
                if !before.is_finite() || !after.is_finite(){continue;}
                let gain=if old_metric["direction"]=="higher"{
                    after-before
                }else{before-after};
                if gain>1e-9{quality_wins+=1;}
                if gain< -1e-9{quality_losses+=1;}
            }
        }
    }
    let expected=controls.iter().filter(|case|case["holdout"]==true).count();
    let quality_proven=expected>=2 && compared==expected
        && (wins+quality_wins)>0 && losses==0 && quality_losses==0;
    json!({
        "source":"source_identical_old_new_compiled_method_control_v1",
        "status":if quality_proven {"compiled_method_treatment_won"}
            else {"no_independent_control_advantage"},
        "controlId":control["id"],
        "controlJobId":control_job["id"],
        "treatmentJobId":treatment_job["id"],
        "controlCandidateId":control_candidate["id"],
        "treatmentCandidateId":treatment_candidate["id"],
        "oldCompiledCommit":old,"newCompiledCommit":revised,
        "oldBinarySha256":control["previousBinarySha256"],
        "newBinarySha256":control["revisedBinarySha256"],
        "frozenCasesSha256":frozen_sha,
        "sourceHeadCommit":revised,
        "compiledDecisionChanged":behavior_diff,
        "matchedHoldouts":compared,
        "treatmentOnlyWins":wins,
        "controlOnlyWins":losses,
        "treatmentQualityMetricWins":quality_wins,
        "controlQualityMetricWins":quality_losses,
        "qualityProven":quality_proven,
        "causalMethodSuperiorityProven":false,
        "limitation":"one actual old/new compiled worker per arm; model sampling variance requires repeated seeds"
    })
}
