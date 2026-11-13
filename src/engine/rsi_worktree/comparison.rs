//! comparison: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

/// Execute the existing paired-runner against the frozen baseline and the
/// exact model patch. Tests run on the operator's host: Git worktrees provide
/// source isolation, NOT an OS security sandbox. Never call this from a model.
pub async fn compare(context:&JobContext, job:&Value)->Result<Value,String>{
    let id=job["input"]["candidateId"].as_str().ok_or("비교할 후보가 없습니다.")?;
    let status=crate::engine::dev::status(&context.state).await;
    let work_branch=status["branch"].as_str().unwrap_or("");
    if status["git"]!=true || !is_rsi_work_branch(work_branch) {
        return Err("비교 실험은 허용된 dominion-sub 작업 브랜치에서 시작하세요.".into());
    }
    let candidate=context.state.store.get("rsi_candidate",id).ok_or("비교할 후보 기록이 없습니다.")?;
    if !owns_application_branch(work_branch,candidate["sourceBranch"].as_str()) {
        return Err("실험 후보와 현재 소스 브랜치의 계보가 일치하지 않습니다.".into());
    }
    let source=std::fs::canonicalize(status["config"]["sourceDir"].as_str().ok_or("소스 경로 없음")?).map_err(|e|e.to_string())?;
    compare_at(context,id,&source,&source).await
}

pub(crate) async fn compare_at(context:&JobContext,id:&str,source:&Path,_runner_source:&Path)->Result<Value,String>{
    let state=&context.state;
    let candidate=state.store.get("rsi_candidate",id)
        .ok_or("비교할 후보 기록이 없습니다.")?;
    if !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive")) {
        return Err("평가 대기 또는 근거 부족 후보만 재비교할 수 있습니다.".into());
    }
    if candidate["cases"].as_array().is_none_or(|c|c.len()<rsi::MIN_PAIRED_CASES) {
        return Err("고정된 평가 사례가 8개 미만이므로 승격 평가를 실행할 수 없습니다.".into());
    }
    let patch=candidate["patch"].as_str().ok_or("코드 패치가 없습니다.")?;
    if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {
        return Err("평가 전 코드 패치 SHA-256이 일치하지 않습니다.".into());
    }
    let prior=state.store.get("rsi_run",
        &format!("rsi-run-{}",candidate["rsiJobId"].as_str().unwrap_or("")))
        .ok_or("모델이 생성한 후보의 원본 실행 증거가 없습니다.")?;
    if prior["candidateId"]!=candidate["id"]||prior["checksPassed"]!=true {
        return Err("컴파일 검사를 통과한 현재 실행 후보만 A/B 실험을 실행합니다.".into());
    }
    if !git(source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty(){
        return Err("비교할 기준 작업공간에 추적 중인 변경이 있습니다.".into());
    }
    // Older frozen candidates remain evaluable after the checkout advances.
    // Both lab variants are created from candidate.baseCommit, never HEAD.
    git(source,&["cat-file","-e",&format!("{}^{{commit}}",candidate["baseCommit"].as_str().ok_or("기준 커밋 없음")?)]).await?;
    let output_dir=state.config.data_dir.join("rsi-paired")
        .join(format!("{}-{}",id,uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&output_dir).map_err(|e|e.to_string())?;
    let output_dir=std::fs::canonicalize(output_dir).map_err(|e|e.to_string())?;
    let patch_file=output_dir.join("candidate.patch");
    let plan_file=output_dir.join("plan.json");
    let report_file=output_dir.join("report.json");
    std::fs::write(&patch_file,patch).map_err(|e|e.to_string())?;
    let runner=output_dir.join("rsi-lab.sh");
    std::fs::write(&runner,include_str!("../../../scripts/rsi-lab.sh")).map_err(|e|e.to_string())?;
    std::fs::write(output_dir.join("rsi-measure.mjs"),include_str!("../../../scripts/rsi-measure.mjs")).map_err(|e|e.to_string())?;
    let mut plan=json!({"oracleVersion":candidate["oracleVersion"],"cases":candidate["cases"]});
    if candidate["cases"].as_array().into_iter().flatten().any(|c|c["filter"].as_str().is_some_and(|f|f.starts_with("rsi_frozen_oracle::"))){
        let oracle=include_str!("../rsi_oracle_fixture.rs");
        let file=std::fs::canonicalize(&output_dir).map_err(|e|e.to_string())?.join("frozen-oracle.rs");
        std::fs::write(&file,oracle).map_err(|e|e.to_string())?;
        plan["oracleFixture"]=json!(file);plan["oracleSha256"]=json!(digest(oracle));
    }
    std::fs::write(&plan_file,plan.to_string()).map_err(|e|e.to_string())?;
    context.checkpoint(json!({
        "rsiCompareInFlight":true,"rsiPhase":"paired_experiment",
        "rsiCandidateId":id
    }))?;
    let mut path_parts = Vec::<PathBuf>::new();
    for tool in [cargo_executable(), npm_executable(), node_executable()] {
        if let Some(parent) = tool.parent() {
            path_parts.push(parent.to_path_buf());
        }
    }
    path_parts.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(path_parts).map_err(|e|e.to_string())?;
    let mut command=Command::new("bash");
    command.current_dir(&source).arg(&runner)
        .arg(candidate["baseCommit"].as_str().unwrap_or(""))
        .arg(&patch_file).arg(&plan_file).arg(&report_file)
        .env("PATH", path)
        // Reuse only Cargo's compiled artifacts between sequential generations.
        // Cargo fingerprints the exact source/dependencies and recompiles a
        // changed candidate; baseline/candidate still have separate targets
        // in rsi-measure.mjs and always execute every frozen test afresh.
        // A per-job target directory previously forced two cold release builds
        // for each descendant and could exhaust the live campaign budget.
        .env("RSI_BUILD_CACHE",std::fs::canonicalize(&state.config.data_dir)
            .map_err(|e|e.to_string())?.join("rsi-paired-build").join("shared-release-v1"));
    let outcome=crate::engine::rsi_evolve::command(context,command,Duration::from_secs(1200)).await?;
    if !outcome.status.success() {
        return Err(format!("독립 A/B 실험 실행 실패: {}",String::from_utf8_lossy(&outcome.stderr)
            .chars().rev().take(1200).collect::<String>().chars().rev().collect::<String>()));
    }
    let report_text=std::fs::read_to_string(&report_file).map_err(|e|e.to_string())?;
    let report:Value=serde_json::from_str(&report_text).map_err(|e|e.to_string())?;
    let evidence_path=output_dir.join("report.json.evidence.txt");
    let evidence_meta=std::fs::metadata(&evidence_path).map_err(|e|e.to_string())?;
    if evidence_meta.len()>24*1024*1024 {
        return Err("실험 출력이 증거 보관 상한(24MiB)을 초과했습니다.".into());
    }
    let evidence=std::fs::read(&evidence_path).map_err(|e|e.to_string())?;
    let evidence_sha=format!("{:x}",Sha256::digest(&evidence));
    if report["reportSha256"].as_str()!=Some(evidence_sha.as_str())
        || report["baseCommit"]!=candidate["baseCommit"]
        || report["patchSha256"]!=candidate["patchSha256"]
        || report["oracleVersion"]!=candidate["oracleVersion"] {
        return Err("원본·패치·평가 기준 또는 실행 증거의 해시가 일치하지 않습니다.".into());
    }
    let mut updated=rsi::evaluate(state,id,&report)?;
    let verdict=updated["evaluation"]["metrics"]["verdict"].clone();
    let metrics=updated["evaluation"]["metrics"].clone();
    // This evaluation came from a real local script invocation, not a
    // client-supplied verdict. It is still host execution, not a VM sandbox.
    updated["pairedComparisonComplete"]=json!(true);
    updated["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
    updated["evaluation"]["evidenceSha256"]=json!(evidence_sha);
    state.store.put("rsi_candidate",&updated)
        .ok_or("A/B 평가 후보 갱신에 실패했습니다.")?;
    state.store.put("rsi_evaluation",&updated["evaluation"]);
    let receipt=json!({
        "id":format!("rsi-pair-{id}"),
        "candidateId":id,"jobId":context.job_id,
        "report":report,"verdict":verdict,"metrics":metrics,
        "evidenceSha256":evidence_sha,
        "runner":"local_gitrepo_not_os_sandbox",
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    state.store.put("rsi_pair",&receipt);
    let origin_id=format!("rsi-run-{}",candidate["rsiJobId"].as_str().unwrap_or(""));
    if let Some(mut origin)=state.store.get("rsi_run",&origin_id) {
        origin["paired"]=json!({"verdict":verdict,"metrics":metrics,
            "reportSha256":evidence_sha,"comparedAt":receipt["createdAt"]});
        origin["pairedComparisonComplete"]=json!(true);
        state.store.put("rsi_run",&origin);
    }
    state.store.audit("rsi.paired_evaluation",
        json!({"id":id,"verdict":verdict,"evidenceSha256":evidence_sha}));
    context.checkpoint(json!({
        "rsiCompareInFlight":false,"rsiPhase":"paired_review"
    }))?;
    Ok(json!({
        "candidateId":id,"candidateStatus":updated["status"],
        "verdict":verdict,"metrics":metrics,
        "evidenceSha256":evidence_sha,
        "pairedCases":report["cases"].as_array().map(Vec::len).unwrap_or(0),
        "performanceImprovementClaimed":metrics["metricGains"].as_u64().unwrap_or(0)>0,
        "measurementScope":"frozen_native_cases_only"
    }))
}

