//! candidate: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

pub async fn finish(
    context:&JobContext, job:&Value, worktree:&Worktree, model_result:Result<Option<Value>,String>,
    session_id:&str,
)->Result<Value,String>{
    let input=&job["input"];
    // Include newly created files without staging actual content; only the
    // disposable worktree index is touched, never the real branch/index.
    git(&worktree.path,&["add","-N","-A","."]).await?;
    // Preserve the final newline: unlike SHA/status output, Git patches
    // must not be trimmed or git apply may reject them as corrupt.
    let mut diff_command=Command::new("git");
    diff_command.current_dir(&worktree.path)
        .args(["diff","--no-ext-diff","--no-color","--binary","HEAD"]);
    let raw=run(diff_command,GIT_LIMIT).await?;
    if !raw.status.success(){
        return Err(String::from_utf8_lossy(&raw.stderr).to_string());
    }
    let patch=String::from_utf8_lossy(&raw.stdout).to_string();
    let patch_size=patch.len();
    let patch_sha=digest(&patch);
    let inherited_parent=input["rsiFailedPatchRecovery"]["candidateId"]
        .as_str().or_else(||input["rsiInheritedCandidateId"].as_str());
    let inherited_unchanged=inherited_parent
        .and_then(|id|context.state.store.get("rsi_candidate",id))
        .is_some_and(|prior|prior["patchSha256"]==patch_sha);
    // A neutral stepping stone is already in the detached worktree before
    // the new model calls tools. A changed diff is not sufficient proof of
    // a descendant edit unless the newly executed tool left a written
    // source receipt in THIS session.
    let archived_source_edited=context.state.store.all("execution_operation")
        .iter().any(|op|op["sessionId"]==session_id
            && op["status"]=="completed"
            && matches!(op["tool"].as_str(),
                Some("workspace_write"|"workspace_replace"|"workspace_replace_function"))
            && op["result"]["written"]==true);
    // A restored parent diff already exists before this generation runs.
    // Count a descendant improvement attempt only if its final diff differs.
    let novel_diff_bytes=if inherited_unchanged {0}else{patch_size};
    let mut candidate_id=Value::Null;
    let mut capture_error=String::new();
    let mut checks=Vec::<Value>::new();
    let mut unsafe_audit=Value::Null;
    if patch.trim().is_empty(){
        capture_error="모델이 실제 소스 파일을 변경하지 않았습니다.".into();
    } else if input["rsiInheritedCandidateId"].is_string()
        && !archived_source_edited {
        capture_error="영구 아카이브의 중립 패치만 복원되었고 이번 후손의 실제 소스 편집이 없습니다.".into();
    } else if inherited_unchanged {
        // Git diff is unchanged from the source patch restored before the
        // child started. The inherited failure cannot be relabeled as a new
        // implementation or another candidate, even if a model answered.
        capture_error="후손이 계승된 실패 패치를 실제로 수정하지 않았습니다.".into();
    } else if patch_size>=MAX_CAPTURE_BYTES {
        capture_error="코드 변경량이 79KB를 초과해 증거를 안전하게 수집하지 않았습니다.".into();
    } else {
        let objective=input["rsiObjective"].as_str().unwrap_or("DOMINION 자체 코드 개선");
        let hypothesis=if objective.chars().count()>=15 {objective.to_string()}
            else {format!("DOMINION 실행 엔진의 결함 개선: {objective}")};
        // Cases were fixed by the API before the model ran. Registration is
        // NOT a claim that baseline/candidate paired tests have been run.
        let proposal=json!({
            "target":classify(&patch),"baseCommit":worktree.base,
            "sourceBranch":input["rsiSourceBranch"],
            "patch":patch,"hypothesis":hypothesis,
            "cases":input["rsiFrozenCases"],"oracleVersion":"rsi-worktree-v2",
            "generatorModel":if context.state.store.get("rsi_mutation",&context.job_id).is_some(){
                Value::Null
            }else{input["model"].clone()},
            "generatorEffort":if context.state.store.get("rsi_mutation",&context.job_id).is_some(){
                Value::Null
            }else{input["reasoningEffort"].clone()},
            "generatorSourceCommit":worktree.base
        });
        match rsi::propose(&context.state,&proposal) {
            Ok(mut candidate)=>{
                candidate["rsiJobId"]=json!(context.job_id);
                // Attribute the bytes to their actual author. A native
                // source transform is a separate autonomous candidate,
                // never a tool call the local model supposedly made.
                let mutation=context.state.store.get("rsi_mutation",&context.job_id);
                candidate["origin"]=json!(if mutation.is_some(){
                    "native_inductive_source_mutation"
                } else {
                    "model_driven_detached_worktree"
                });
                candidate["nativeMutation"]=mutation.unwrap_or(Value::Null);
                candidate["pairedComparisonComplete"]=json!(false);
                candidate["reviewRequired"]=json!(true);
                candidate["experimentCampaignId"]=input["rsiCampaignId"].clone();
                candidate["actualGeneratorCommit"]=input["rsiGeneratorCommit"].clone();
                candidate_id=candidate["id"].clone();
                context.state.store.put("rsi_candidate",&candidate);
                checks=validate(worktree,&patch).await;
                // An added `unsafe` construct is checked against the same
                // SAFETY rule as the CLI and the `unsafe_audit` tool. This is
                // an actual engine invocation, not a prompt instruction.
                if let Some(audit)=unsafe_audit_for_patch(&worktree.path,&patch) {
                    let check=unsafe_audit_check(&audit);
                    checks.push(check);
                    candidate["unsafeAudit"]=audit.clone();
                    context.state.store.put("rsi_candidate",&candidate);
                    unsafe_audit=audit;
                }
            }
            Err(err)=>capture_error=format!("후보 등록 거부: {err}"),
        }
    }
    let source_clean=git(&worktree.source,&["status","--porcelain","--untracked-files=normal"])
        .await.map(|v|v.is_empty()).unwrap_or(false);
    let model_error=model_result.as_ref().err().cloned().unwrap_or_default();
    let all_checks_pass=!checks.is_empty()&&checks.iter().all(|item|item["passed"]==true);
    // Preserve the *actual* writes and rejected edits from this generation.
    // A model's tool call is not a source change. Preserve error text so the
    // next built generator can repair the failed write instead of re-reading
    // the entire source on every attempt. Do not store file contents here.
    let edit_actions=context.state.store.all("execution_operation").into_iter()
        .filter(|operation|operation["sessionId"].as_str()==Some(session_id)
            && matches!(operation["tool"].as_str(),
                Some("workspace_write"|"workspace_replace"|"workspace_replace_function")))
        .take(16)
        .map(|operation|{
            let error=operation["result"]["error"].as_str()
                .or_else(||operation["error"].as_str()).unwrap_or("");
            json!({
                "id":operation["id"],"tool":operation["tool"],
                "path":operation["arguments"]["path"],"status":operation["status"],
                "written":operation["status"]=="completed"
                    && operation["result"]["written"]==true,
                "preflightBlocked":operation["result"]["preflightBlocked"]==true,
                "staleVersion":operation["result"]["staleVersion"]==true,
                "error":error.chars().take(500).collect::<String>()
            })
        }).collect::<Vec<_>>();
    let result=json!({
        "id":format!("rsi-run-{}",context.job_id),
        "jobId":context.job_id,"sessionId":session_id,
        "baseCommit":worktree.base,"candidateId":candidate_id,
        "sourceBranch":input["rsiSourceBranch"],
        "sourceClean":source_clean,"diffSha256":patch_sha,"diffBytes":patch_size,
        "novelDiffBytes":novel_diff_bytes,
        "inheritedPatchUnchanged":inherited_unchanged,
        "inheritedCandidateId":input["rsiInheritedCandidateId"],
        "inheritedCandidateActuallyEdited":input["rsiInheritedCandidateId"].is_string()
            && archived_source_edited && !inherited_unchanged,
        "checks":checks,"checksPassed":all_checks_pass,
        "unsafeAudit":unsafe_audit,
        "pairedComparisonComplete":false,"autoMerged":false,"autoDeployed":false,
        "modelError":model_error,"captureError":capture_error,
        "editActions":edit_actions,
        "modelMessageId":model_result.ok().flatten().and_then(|m|m["id"].as_str().map(str::to_string)),
        "createdAt":chrono::Utc::now().to_rfc3339(),
    });
    context.state.store.put("rsi_run",&result);
    context.state.store.audit("rsi.run_evidence",json!({
        "jobId":context.job_id,"candidateId":candidate_id,
        "patchSha256":patch_sha,"sourceClean":source_clean,"checksPassed":all_checks_pass,
        "unsafeAuditPassed":unsafe_audit["passed"],
        "modelError":model_error,"captureError":capture_error
    }));
    context.checkpoint(json!({"rsiPhase":"review","rsiEvidenceId":result["id"],
        "rsiCandidateId":candidate_id}))?;
    let (status,reason)=run_status(
        candidate_id.is_string(),all_checks_pass,
        unsafe_audit["passed"]==false,
        source_clean,&model_error,&capture_error,
    );
    Ok(json!({
        "status":status,"reason":reason,
        "sessionId":session_id,
        "rsi":result,
    }))
}

