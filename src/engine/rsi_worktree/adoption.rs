//! Explicit owner-reviewed application of an independently evaluated
//! candidate or an entire experimental campaign. Production refs stay frozen.
use super::*;

/// Apply a model-generated patch only after explicit owner review. A successful
/// compile is NOT proof of improved behavior. Never commit, push or deploy.
pub async fn apply_to_sub(state:&crate::state::AppState,id:&str,body:&Value)->Result<Value,String>{
    use std::sync::OnceLock;
    static APPLY_LOCK:OnceLock<tokio::sync::Mutex<()>>=OnceLock::new();
    let _lock=APPLY_LOCK.get_or_init(||tokio::sync::Mutex::new(())).lock().await;
    let mut candidate=state.store.get("rsi_candidate",id).ok_or("개선 후보를 찾을 수 없습니다.")?;
    if !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive"|"verified")) {
        return Err("현재 상태에서는 적용할 수 없습니다.".into());
    }
    if !matches!(body["confirmation"].as_str(),Some("APPLY_UNVERIFIED_TO_SUB"|"APPLY_VERIFIED_TO_SUB"))
        || body["baseCommit"]!=candidate["baseCommit"]
        || body["patchSha256"]!=candidate["patchSha256"] {
        return Err("기준 커밋·패치 해시·수동 적용 의사를 확인하세요.".into());
    }
    let job_id=candidate["rsiJobId"].as_str().ok_or("실행 증거가 없습니다.")?.to_string();
    let record_id=format!("rsi-run-{job_id}");
    let mut evidence=state.store.get("rsi_run",&record_id).ok_or("실제 실행 증거가 없습니다.")?;
    if evidence["candidateId"]!=candidate["id"] || evidence["diffSha256"]!=candidate["patchSha256"]
        || evidence["checksPassed"]!=true || evidence["sourceClean"]!=true {
        return Err("코드 출처 및 컴파일·타입 검사 통과 증거를 만족하지 못했습니다.".into());
    }
    let verified=candidate["status"]=="verified"
        &&candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        &&candidate["evaluation"]["metrics"]["eligible"]==true
        &&evidence["pairedComparisonComplete"]==true
        &&candidate["evaluation"]["evidenceSha256"].is_string()
        &&candidate["evaluation"]["evidenceSha256"]==evidence["paired"]["reportSha256"];
    if (candidate["target"]=="improver"||body["confirmation"]=="APPLY_VERIFIED_TO_SUB")&&!verified {
        return Err("독립 비교에서 우세함이 확인된 개선기만 적용할 수 있습니다.".into());
    }
    if evidence["pairedComparisonComplete"]==true && candidate["evaluation"]["metrics"]["regressions"].as_u64()!=Some(0){
        return Err("비교에서 회귀가 발견된 후보는 적용할 수 없습니다.".into());
    }
    let patch=candidate["patch"].as_str().ok_or("패치 본문이 없습니다.")?;
    if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {
        return Err("패치 해시가 저장된 값과 다릅니다.".into());
    }
    let status=crate::engine::dev::status(state).await;
    if status["git"]!=true || !status["branch"].as_str().is_some_and(rsi_work_branch)
        || status["dirty"]!=0 || status["commit"]!=candidate["baseCommit"] {
        return Err("원본 작업 브랜치가 기준과 다르거나 수정 중이므로 적용하지 않았습니다.".into());
    }
    let recorded_branch=state.store.get("job",&job_id)
        .and_then(|job|job["input"]["rsiSourceBranch"].as_str().map(str::to_string));
    if !status["branch"].as_str().is_some_and(|branch|
        owns_application_branch(branch,recorded_branch.as_deref())) {
        return Err("후보를 생성한 소스 브랜치와 현재 적용 브랜치가 다릅니다.".into());
    }
    let remote=status["remote"].as_str().unwrap_or("");
    if !remote.is_empty() && !remote.contains("hushmind3/dominion-rust") {
        return Err("연결된 소스가 DOMINION Rust 저장소가 아닙니다.".into());
    }
    let source=status["config"]["sourceDir"].as_str().ok_or("소스 폴더가 연결되지 않았습니다.")?;
    let source=std::fs::canonicalize(source).map_err(|e|e.to_string())?;
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file(){
        return Err("DOMINION Rust 프로젝트 폴더가 아닙니다.".into());
    }
    let temporary=std::env::temp_dir().join(format!("dominion-rsi-patch-{}.diff",uuid::Uuid::new_v4()));
    std::fs::write(&temporary,patch).map_err(|e|e.to_string())?;
    let path=temporary.to_str().ok_or("임시 패치 경로 오류")?;
    let applied=async {
        git(&source,&["apply","--check",path]).await?;
        if !git(&source,&["status","--porcelain","--untracked-files=normal"]).await?.is_empty()
            || git(&source,&["rev-parse","HEAD"]).await?!=candidate["baseCommit"].as_str().unwrap_or("") {
            return Err("최종 적용 직전 기준 소스가 변경되어 중단했습니다.".into());
        }
        git(&source,&["apply",path]).await?;
        Ok::<(),String>(())
    }.await;
    let _=std::fs::remove_file(temporary);
    applied?;
    let now=chrono::Utc::now().to_rfc3339();
    candidate["status"]=json!("applied_to_sub");
    candidate["manualApplication"]=json!({
        "mode":if verified{"explicit_verified_paired_application"}else{"explicit_trial_without_paired_proof"},
        "appliedAt":now,"sourceCommit":status["commit"],
        "requiresRebuild":true,"requiresCommit":true,"autoDeployed":false
    });
    evidence["manuallyApplied"]=json!(true);
    evidence["appliedAt"]=json!(now);
    state.store.put("rsi_run",&evidence);
    let saved=state.store.put("rsi_candidate",&candidate).ok_or("코드 적용 후 기록 저장에 실패했습니다.")?;
    state.store.audit("rsi.applied_trial_to_sub",
        json!({"candidateId":id,"jobId":job_id,"baseCommit":status["commit"]}));
    state.bus.record("rsi_candidate",&saved);
    Ok(saved)
}

/// Publish the selected experimental lineage to the test branch. The main
/// branch is never a target. Every commit is checked against its measured
/// patch using a private Git index before the source checkout can move.
pub async fn apply_campaign_to_sub(state:&crate::state::AppState,id:&str,expected:&str)->Result<Value,String>{
    if !state.begin_session("rsi-source-application") {return Err("검증된 버전을 적용 중입니다.".into());}
    let _guard=state.session_guard("rsi-source-application");
    let mut campaign=state.store.get("rsi_campaign",id).ok_or("개선 작업을 찾지 못했습니다.")?;
    if !matches!(campaign["status"].as_str(),Some("completed"|"cancelled"|"needs_review"|"failed")) || campaign["headCommit"]==campaign["baseCommit"]
        || campaign["headCommit"]!=expected || !valid_sha(expected) {
        return Err("완료된 작업에서 선택한 검증 버전만 적용할 수 있습니다.".into());
    }
    let source=std::fs::canonicalize(campaign["sourceDir"].as_str().ok_or("소스 경로 없음")?).map_err(|e|e.to_string())?;
    let status=crate::engine::dev::status(state).await;
    let configured=status["config"]["sourceDir"].as_str().and_then(|s|std::fs::canonicalize(s).ok());
    let work_branch=status["branch"].as_str()
        .filter(|branch|rsi_work_branch(branch))
        .ok_or("허용된 RSI 작업 브랜치에서만 적용합니다.")?;
    if configured.as_ref()!=Some(&source)
        || !owns_application_branch(work_branch,campaign["sourceBranch"].as_str()) {
        return Err("캠페인 원본 브랜치와 연결된 작업 브랜치가 다릅니다.".into());
    }
    if !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("미저장 코드를 보존했습니다. 현재 소스를 정리한 뒤 반영하세요.".into());
    }
    if campaign["application"]["commit"]==expected && status["commit"]==expected {
        return Ok(campaign["application"].clone());
    }
    if campaign["sourceSnapshot"]==true || status["commit"]!=campaign["baseCommit"]
        || !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("작업 시작 이후 소스가 바뀌었거나 미저장 코드가 있어 보존했습니다. 현재 소스에서 다시 이어가세요.".into());
    }
    if state.store.all("job").iter().any(|j|matches!(j["kind"].as_str(),Some("rsi_evolve"|"rsi_generation"))
        && matches!(j["status"].as_str(),Some("queued"|"running"))) {
        return Err("진행 중인 발전 작업이 끝난 뒤 적용하세요.".into());
    }
    let base=campaign["baseCommit"].as_str().ok_or("기준 커밋 없음")?;
    git(&source,&["merge-base","--is-ancestor",base,expected]).await?;
    let commits=git(&source,&["rev-list","--reverse",&format!("{base}..{expected}")]).await?;
    let candidates=state.store.all("rsi_candidate");
    for commit in commits.lines() {
        let candidate=candidates.iter().find(|c|c["experimentAdoption"]["commit"]==commit
            && c["status"]=="verified" && c["evaluation"]["metrics"]["eligible"]==true
            && c["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees")
            .ok_or("검증되지 않은 커밋이 섞여 있어 적용하지 않았습니다.")?;
        let parent=git(&source,&["rev-parse",&format!("{commit}^")]).await?;
        if candidate["baseCommit"]!=parent {return Err("검증한 기준과 채택 계보가 다릅니다.".into());}
        let patch=candidate["patch"].as_str().ok_or("검증 패치 없음")?;
        if digest(patch)!=candidate["patchSha256"].as_str().unwrap_or("") {return Err("검증 패치 해시가 다릅니다.".into());}
        let temp=std::env::temp_dir().join(format!("dominion-rsi-publish-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).map_err(|e|e.to_string())?;
        let patch_path=temp.join("candidate.patch");
        std::fs::write(&patch_path,patch).map_err(|e|e.to_string())?;
        let checked=async {
            let mut tree=String::new();
            for args in [vec!["read-tree".to_string(),parent.clone()],
                vec!["apply".into(),"--cached".into(),patch_path.to_string_lossy().to_string()],vec!["write-tree".into()]] {
                let mut cmd=Command::new("git");
                cmd.current_dir(&source).args(args).env("GIT_INDEX_FILE",temp.join("index"));
                let out=run(cmd,GIT_LIMIT).await?;
                if !out.status.success(){return Err("검증 패치를 재구성하지 못했습니다.".to_string());}
                tree=String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
            if tree!=git(&source,&["rev-parse",&format!("{commit}^{{tree}}")]).await? {
                return Err("실제 채택 코드가 검증 패치와 다릅니다.".into());
            }
            Ok::<(),String>(())
        }.await;
        let _=std::fs::remove_dir_all(temp);checked?;
    }
    if commits.is_empty() {return Err("적용할 검증 변경이 없습니다.".into());}
    if git(&source,&["rev-parse","HEAD"]).await?!=base
        || git(&source,&["symbolic-ref","--quiet","HEAD"]).await?!=format!("refs/heads/{work_branch}")
        || !git(&source,&["status","--porcelain","--untracked-files=no"]).await?.is_empty() {
        return Err("적용 직전에 작업 브랜치나 소스가 바뀌었습니다.".into());
    }
    git(&source,&["-c","core.hooksPath=","merge","--ff-only","--no-edit",expected]).await?;
    let application=json!({"commit":expected,"branch":work_branch,"sourceDir":source,
        "requiresRebuild":true,"productionDeployed":false,"appliedAt":chrono::Utc::now().to_rfc3339()});
    campaign["application"]=application.clone();
    state.store.put("rsi_campaign",&campaign).ok_or("소스 반영 후 기록 저장 실패")?;
    state.bus.record("rsi_campaign",&campaign);
    state.store.audit("rsi.campaign_applied_to_sub",json!({"campaignId":id,"commit":expected}));
    Ok(application)
}

