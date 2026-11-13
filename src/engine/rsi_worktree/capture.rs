//! Detached source capture, provenance audit and candidate verification.
//! No authority to alter the frozen original evaluator or deploy to production.
use super::*;

pub(super) fn classify(patch:&str)->&'static str {
    if patch.lines().any(|line|line.starts_with("diff --git ")
        && (line.contains("src/engine/rsi/")
            // Include newly self-authored RSI policies such as
            // rsi_meta_controller.rs, rsi_improvement_algebra.rs, and
            // rsi_method_generalization/*. An A2 method rewrite must not
            // be misclassified as an ordinary harness change.
            || line.contains("src/engine/rsi_")
            || line.contains("src/engine/rsi.rs ")
            || line.contains("src/engine/rsi_worktree.rs ")
            || line.contains("src/engine/rsi_evolve.rs ")
            || line.contains("src/engine/rsi_archive.rs ")
            || line.contains("src/engine/rsi_mutation.rs ")
            || line.contains("src/engine/rsi_domain_accumulator.rs ")
            || line.contains("src/engine/rsi_metrics.rs ")
            || line.contains("src/engine/algorithms.rs ")
            || line.contains("src/engine/algorithm_eval.rs ")
            || line.contains("src/engine/policy_patch.rs ")
            || line.contains("src/api/rsi.rs "))) {
        "improver"
    } else if patch.lines().filter(|line|line.starts_with("diff --git "))
        .all(|line|line.contains("src/engine/memory")
            || line.contains("src/engine/algorithms.rs")
            || line.contains("src/engine/room_state.rs")) {
        "algorithm"
    } else {"harness"}
}
/// Link edit proposals to their actual executable receipts. Merely seeing
/// workspace_replace_range in a model response is not evidence that the
/// isolated source changed. This observation is persisted with each RSI run.
fn edit_outcomes(state:&crate::state::AppState, session_id:&str)->Value {
    let mut attempts=0u64;
    let mut verified_writes=0u64;
    let mut failures=Vec::<String>::new();
    let mut operations=state.store.all("execution_operation");
    operations.sort_by(|a,b| a["startedAt"].as_str().cmp(&b["startedAt"].as_str()));
    for operation in operations {
        if operation["sessionId"].as_str()!=Some(session_id)
            || operation["origin"].as_str()!=Some("model")
            || !matches!(operation["tool"].as_str(),
                Some("workspace_write"|"workspace_replace"|"workspace_replace_range")) {
            continue;
        }
        attempts+=1;
        if operation["status"]=="completed" && operation["result"]["written"]==true
            && operation["toolReceipt"]["verified"]==true {
            verified_writes+=1;
        } else {
            let reason=operation["result"]["error"].as_str()
                .or_else(||operation["error"].as_str())
                .unwrap_or(if operation["result"]["unchanged"]==true {
                    "source was not changed"
                }else{"the edit was not verified"});
            failures.push(format!("{}: {}",operation["tool"].as_str().unwrap_or("edit"),
                reason.chars().take(300).collect::<String>()));
        }
    }
    json!({"attempts":attempts,"verifiedWrites":verified_writes,
        "recentFailures":failures.into_iter().rev().take(3).collect::<Vec<_>>()})
}

/// Preserve the first actionable compiler diagnostic, not only the end of
/// cargo's stderr (which often contains warnings and "could not compile").
/// Losing the real file:line:error makes every model repair a blind rewrite.
pub(super) fn focused_verifier_stderr(stderr:&str)->String {
    let lines=stderr.lines().collect::<Vec<_>>();
    let error_at=lines.iter().position(|line|{
        let trimmed=line.trim_start();
        trimmed.starts_with("error[") || trimmed.starts_with("error:")
            || trimmed.contains("error[E0")
    });
    if let Some(at)=error_at {
        let first=lines[at.saturating_sub(1)..(at+24).min(lines.len())].join("\n");
        return first.chars().take(3200).collect();
    }
    stderr.chars().rev().take(2600).collect::<String>()
        .chars().rev().collect()
}
fn check_result(label:&str, result:Result<std::process::Output,String>)->Value {
    match result {
        Ok(output)=>{
            let stderr=String::from_utf8_lossy(&output.stderr);
            json!({
                "label":label,"ran":true,"passed":output.status.success(),
                "exitCode":output.status.code(),
                "stdout":String::from_utf8_lossy(&output.stdout).chars().rev().take(2600).collect::<String>().chars().rev().collect::<String>(),
                "stderr":focused_verifier_stderr(&stderr)
            })
        }
        Err(error)=>json!({"label":label,"ran":false,"passed":false,"error":error}),
    }
}
async fn validate(worktree:&Worktree,patch:&str)->Vec<Value> {
    let mut checks=Vec::new();
    let rust_change=patch.lines().any(|line|line.starts_with("diff --git a/src/")||line.starts_with("diff --git a/tests/"));
    let web_change=patch.lines().any(|line|line.starts_with("diff --git a/web/"));
    if rust_change {
        let mut command=Command::new(cargo_executable());
        command.current_dir(&worktree.path).args(["check","--all-targets"]);
        checks.push(check_result("cargo check --all-targets",run(command,CHECK_LIMIT).await));
    }
    if web_change {
        let installed=worktree.source.join("web/node_modules");
        let deps=worktree.path.join("web/node_modules");
        #[cfg(unix)]
        if installed.is_dir() && !deps.exists() {
            let _=std::os::unix::fs::symlink(&installed,&deps);
        }
        if deps.exists() {
            let mut command=Command::new(npm_executable());
            command.current_dir(worktree.path.join("web")).args(["run","typecheck"]);
            checks.push(check_result("npm run typecheck",run(command,Duration::from_secs(120)).await));
        } else {
            checks.push(json!({"label":"npm run typecheck","ran":false,"passed":false,
                "error":"원본 web/node_modules가 없어 독립 타입 검사를 실행하지 않았습니다."}));
        }
    }
    checks
}
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
    let mut candidate_id=Value::Null;
    let mut capture_error=String::new();
    let mut checks=Vec::<Value>::new();
    let mut unsafe_audit=Value::Null;
    // Compute the actual model-write proof BEFORE registration. Otherwise a
    // worker that makes NO change to an inherited neutral patch re-registers
    // the same candidate and overwrites its independent paired evaluation.
    let edit_evidence=edit_outcomes(&context.state,session_id);
    let model_calls=context.state.store.session_usage(session_id)["calls"]
        .as_u64().unwrap_or(0);
    let expected_provider=input["providerId"].as_str().unwrap_or("");
    let expected_model=input["model"].as_str().unwrap_or("");
    let model_route_usage=context.state.store.api_route_usage(
        session_id,expected_provider,expected_model);
    let model_routes=context.state.store.session_model_routes(session_id);
    let model_route_exclusive=model_routes.as_ref().is_ok_and(|routes|
        routes.len()==1
        && routes[0]["providerId"]==expected_provider
        && routes[0]["model"]==expected_model);
    let model_route_verified=model_route_exclusive
        && model_route_usage["matchedExactProviderModelSession"]==true;
    // A native induction is a separate accountable code generator. Its
    // receipt must match this job, source and patch bytes; never credit the
    // model merely because the resulting code compiles.
    let native_receipt=context.state.store.get("rsi_mutation",&context.job_id)
        .filter(|receipt|input["rsiNativeInduction"]==true
            && receipt["origin"]=="native_inductive_source_mutation"
            && receipt["campaignId"]==input["rsiCampaignId"]
            && receipt["sourceDir"].as_str()==Some(worktree.source.to_string_lossy().as_ref())
            && receipt["path"].as_str().is_some_and(crate::engine::rsi::safe_path)
            && receipt["path"].as_str().is_some_and(|p|
                patch.contains(&format!("diff --git a/{p} b/{p}"))));
    let mutation_after_matches=native_receipt.as_ref().is_some_and(|receipt|{
        let Some(path)=receipt["path"].as_str() else {return false;};
        std::fs::read(worktree.path.join(path)).ok().is_some_and(|bytes|
            format!("{:x}",Sha256::digest(&bytes))==receipt["afterSha256"])
    });
    let prior_native=context.state.store.get("rsi_run",
            &format!("rsi-run-{}",context.job_id))
        .is_some_and(|row|row["candidateOrigin"]
            .as_str().is_some_and(|origin|
                origin.starts_with("native_inductive_source_mutation")));
    let native_origin=mutation_after_matches || (native_receipt.is_some() && prior_native);
    let inherited_patch_changed=input["rsiInheritedCandidateId"].as_str()
        .and_then(|id|context.state.store.get("rsi_candidate",id))
        .is_some_and(|ancestor|ancestor["patchSha256"]!=patch_sha
            && edit_evidence["verifiedWrites"].as_u64().unwrap_or(0)>0);
    if patch.trim().is_empty(){
        capture_error="모델이 실제 소스 파일을 변경하지 않았습니다.".into();
    } else if !native_origin && model_calls==0 {
        capture_error="실제 모델 API 호출 기록이 없으므로 이 패치를 모델 주도 개선 후보로 등록하지 않았습니다.".into();
    } else if !native_origin && !model_route_verified {
        capture_error="요청 모델의 실제 제공자·모델 호출 기록이 없거나 다른 모델이 동일 세션에 섞여 있어 후보를 등록하지 않았습니다.".into();
    } else if !native_origin && edit_evidence["verifiedWrites"]
        .as_u64().unwrap_or(0)==0 {
        capture_error="모델이 수행한 검증된 소스 쓰기 도구 기록이 없습니다.".into();
    } else if input["rsiInheritedCandidateId"].is_string()
        && !inherited_patch_changed {
        capture_error="계승된 실패 패치에 새로 검증된 소스 수정이 없습니다. 이전 평가 후보는 그대로 보존합니다.".into();
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
            "patch":patch,"hypothesis":hypothesis,
            "cases":input["rsiFrozenCases"],"oracleVersion":"rsi-worktree-v2",
            "generatorModel":if native_origin {json!(format!("dominion-native:{}",
                native_receipt.as_ref().and_then(|row|row["operator"].as_str()).unwrap_or("unknown")))}else{input["model"].clone()},
            "generatorEffort":if native_origin {Value::Null}else{input["reasoningEffort"].clone()},
            "generatorSourceCommit":worktree.base
        });
        match rsi::propose(&context.state,&proposal) {
            Ok(mut candidate)=>{
                candidate["rsiJobId"]=json!(context.job_id);
                candidate["modelSessionId"]=json!(session_id);
                candidate["actualModelRoute"]=json!({
                    "providerId":expected_provider,"model":expected_model
                });
                candidate["modelRouteUsage"]=model_route_usage.clone();
                candidate["modelRouteSessionExclusive"]=json!(model_route_exclusive);
                candidate["origin"]=json!(if native_origin {
                    "native_inductive_source_mutation"
                }else{"model_driven_detached_worktree"});
                if native_origin {
                    candidate["nativeMutation"]=json!(native_receipt);
                }
                candidate["pairedComparisonComplete"]=json!(false);
                candidate["reviewRequired"]=json!(true);
                candidate["experimentCampaignId"]=input["rsiCampaignId"].clone();
                candidate["actualGeneratorCommit"]=input["rsiGeneratorCommit"].clone();
                candidate_id=candidate["id"].clone();
                context.state.store.put("rsi_candidate",&candidate);
                context.checkpoint(json!({"rsiPhase":"compiling_candidate"}))?;
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
    let result=json!({
        "id":format!("rsi-run-{}",context.job_id),
        "jobId":context.job_id,"sessionId":session_id,
        "baseCommit":worktree.base,"candidateId":candidate_id,
        "sourceClean":source_clean,"diffSha256":patch_sha,"diffBytes":patch_size,
        "checks":checks,"checksPassed":all_checks_pass,
        "unsafeAudit":unsafe_audit,
        "pairedComparisonComplete":false,"autoMerged":false,"autoDeployed":false,
        "modelError":model_error,"captureError":capture_error,
        "candidateOrigin":if native_origin{"native_inductive_source_mutation"}
            else{"model_driven_detached_worktree"},
        "nativeMutation":native_receipt,
        "editEvidence":edit_evidence,
        "modelProviderCalls":model_calls,
        "modelRouteUsage":model_route_usage,
        "modelRouteSessionExclusive":model_route_exclusive,
        "modelRouteVerified":model_route_verified,
        "actualModelRoute":{"providerId":expected_provider,"model":expected_model},
        "modelSessionId":session_id,
        "originAttribution":"model_usage_and_verified_tool_write_or_native_receipt",
        "inheritedCandidateId":input["rsiInheritedCandidateId"],
        "inheritedPatchChanged":inherited_patch_changed,
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

