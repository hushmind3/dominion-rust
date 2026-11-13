//! checkout: isolated RSI worktree responsibility; no automatic production deployment.
use super::*;

pub async fn prepare(context:&JobContext,input:&Value)->Result<Worktree,String>{
    let base=input["rsiBaseCommit"].as_str().filter(|v|valid_sha(v))
        .ok_or("자기개선 시작 시 기준 Git 커밋이 고정되지 않았습니다.")?;
    let raw=input["rsiSourceDir"].as_str().ok_or("자기개선 원본 경로가 없습니다.")?;
    let source=std::fs::canonicalize(raw).map_err(|e|e.to_string())?;
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file() {
        return Err("DOMINION 소스가 아닙니다.".into());
    }
    let campaign=crate::engine::rsi_evolve::campaign_source(&context.state,input,&source,base);
    if !campaign {
        let actual=git(&source,&["rev-parse","--abbrev-ref","HEAD"]).await?;
        if !owns_application_branch(actual.trim(),input["rsiSourceBranch"].as_str()) {
            return Err("실험의 원본 브랜치와 현재 체크아웃 브랜치가 일치하지 않습니다.".into());
        }
    }
    if git(&source,&["rev-parse","HEAD"]).await?.trim()!=base {
        return Err("작업을 기다리는 동안 원본 커밋이 바뀌었습니다. 다시 시작하세요.".into());
    }
    if !git(&source,&["status","--porcelain","--untracked-files=normal"]).await?.is_empty() {
        return Err("원본에 다른 변경사항이 있어 자기개선 작업을 시작하지 않았습니다.".into());
    }
    let path=std::env::temp_dir().join(format!("dominion-rsi-{}",uuid::Uuid::new_v4()));
    let name=path.to_str().ok_or("분리 작업공간 경로를 읽을 수 없습니다.")?;
    // Detached Git worktree, not a new branch; the running server checkout stays pristine.
    git(&source,&["worktree","add","--detach",name,base]).await?;
    if let Err(error)=context.checkpoint(json!({
        "rsiWorkspace":name,"rsiBaseCommit":base,
        "rsiPhase":"model_editing","rsiIsolation":"detached_git_worktree_not_os_sandbox"
    })) {
        let _=git(&source,&["worktree","remove","--force",name]).await;
        return Err(error);
    }
    Ok(Worktree{source,path,base:base.to_string()})
}
/// Restore an independently REPLAYED neutral stepping stone. An archive
/// pointer alone is never authorization to execute a patch: the candidate,
/// its exact source branch/base/patch hash, and original raw paired receipt
/// must all agree. The accepted generator HEAD remains unchanged.
pub(crate) async fn restore_archived_parent(
    context:&JobContext,input:&Value,worktree:&Worktree,
)->Result<Value,String>{
    let Some(id)=input["rsiInheritedCandidateId"].as_str()
        .filter(|s|!s.is_empty()) else {return Ok(Value::Null)};
    let choice=&input["rsiArchiveSelection"];
    let candidate=context.state.store.get("rsi_candidate",id)
        .ok_or("영구 아카이브에 기록된 개선 후보가 없습니다.")?;
    let patch=candidate["patch"].as_str()
        .ok_or("계승할 원본 패치가 없습니다.")?;
    let source_branch=input["rsiSourceBranch"].as_str()
        .ok_or("계승 실험의 원본 브랜치가 없습니다.")?;
    if candidate["id"]!=id
        || candidate["sourceBranch"]!=source_branch
        || !is_rsi_work_branch(source_branch)
        || candidate["baseCommit"]!=worktree.base
        || choice["candidateId"]!=id
        || choice["baseCommit"]!=worktree.base
        || choice["patchSha256"]!=candidate["patchSha256"]
        || choice["persisted"]!=true || choice["adopted"]!=false
        || patch.is_empty() || patch.len()>=80_000
        || candidate["patchSha256"]!=digest(patch)
        || candidate["paths"].as_array().is_none_or(|paths|
            paths.is_empty() || paths.iter().any(|row|
                row.as_str().is_none_or(|p|!rsi::safe_path(p))))
        || crate::engine::rsi_archive::parent_metrics(
            &context.state,&candidate,&worktree.base).is_none() {
        return Err("보관된 후보의 브랜치·쌍대 검증·패치 계보가 다릅니다.".into());
    }
    let file=std::env::temp_dir().join(format!(
        "dominion-rsi-archive-{}.patch",uuid::Uuid::new_v4()));
    std::fs::write(&file,patch).map_err(|e|e.to_string())?;
    let target=file.to_str().ok_or("아카이브 패치 경로 오류")?.to_string();
    let applied=async{
        git(&worktree.path,&["apply","--check",&target]).await?;
        git(&worktree.path,&["apply",&target]).await?;
        if git(&worktree.path,&["status","--porcelain","--untracked-files=normal"])
            .await?.is_empty() {
            return Err("보관 패치가 새로운 실험 작업공간을 변경하지 않았습니다.".into());
        }
        Ok::<(),String>(())
    }.await;
    let _=std::fs::remove_file(file);
    applied?;
    let restored=json!({
        "status":"restored_unverified",
        "kind":"neutral_pareto_stepping_stone",
        "candidateId":id,"baseCommit":worktree.base,
        "paths":candidate["paths"],"patchSha256":candidate["patchSha256"],
        "inheritedAdoption":false,"qualityGain":Value::Null,
        "originalPairReplayed":true
    });
    context.checkpoint(json!({"rsiRecoveredPatch":restored}))?;
    context.state.store.audit("rsi.archived_neutral_restored",json!({
        "jobId":context.job_id,"candidateId":id,
        "baseCommit":worktree.base,"sourceBranch":source_branch
    }));
    Ok(restored)
}

/// Restore only a compiler-failed candidate from the same campaign and
/// unchanged source revision. This is a repair starting point, not a
/// successful generation. The prior patch is never applied to the checkout:
/// it lives only in this new disposable descendant worktree.
pub async fn restore_failed_patch(
    context:&JobContext,input:&Value,worktree:&Worktree,
)->Result<Value,String> {
    let Some(recovery)=input.get("rsiFailedPatchRecovery")
        .filter(|value|value.is_object()) else {
        return Ok(Value::Null);
    };
    let id=recovery["candidateId"].as_str().ok_or("재개 후보 ID 없음")?;
    let base=recovery["baseCommit"].as_str().ok_or("재개 기준 커밋 없음")?;
    let campaign=input["rsiCampaignId"].as_str().ok_or("재개 캠페인 없음")?;
    let candidate=context.state.store.get("rsi_candidate",id)
        .ok_or("이전 세대의 소스 패치가 저장되지 않았습니다.")?;
    let patch=candidate["patch"].as_str().ok_or("재개 패치 본문 없음")?;
    if base!=worktree.base
        || candidate["baseCommit"]!=base
        || candidate["experimentCampaignId"]!=campaign
        || candidate["patchSha256"]!=digest(patch)
        || patch.is_empty() || patch.len()>=80_000
        || candidate["paths"].as_array().is_none_or(|paths|
            paths.is_empty() || paths.iter().any(|path|path.as_str()
                .is_none_or(|path|!rsi::safe_path(path)))) {
        return Err("이전 개선 패치의 계보·해시·수정 경로 검증 실패".into());
    }
    // Check before applying, and remove the temporary patch in all cases.
    let file=std::env::temp_dir().join(format!(
        "dominion-rsi-recover-{}.patch",uuid::Uuid::new_v4()));
    std::fs::write(&file,patch).map_err(|e|e.to_string())?;
    let target=file.to_str().ok_or("임시 패치 파일 경로 오류")?.to_string();
    let applied=async {
        git(&worktree.path,&["apply","--check",&target]).await?;
        git(&worktree.path,&["apply",&target]).await?;
        Ok::<(),String>(())
    }.await;
    let _=std::fs::remove_file(&file);
    applied?;
    let restored=json!({
        "status":"restored_unverified",
        "candidateId":id,"baseCommit":base,
        "failedGenerationJobId":recovery["failedGenerationJobId"],
        "paths":candidate["paths"],"patchSha256":candidate["patchSha256"],
        "independentGain":Value::Null
    });
    context.checkpoint(json!({"rsiRecoveredPatch":restored}))?;
    context.state.store.audit("rsi.descendant_failed_patch_restored",json!({
        "jobId":context.job_id,"candidateId":id,
        "campaignId":campaign,"baseCommit":base
    }));
    Ok(restored)
}

pub fn is_rsi_work_branch(branch:&str)->bool {
    if branch=="dominion-sub" {return true;}
    let Some(suffix)=branch.strip_prefix("dominion-sub") else{return false;};
    !suffix.is_empty() && suffix.len()<=2 && !suffix.starts_with('0')
        && suffix.bytes().all(|ch|ch.is_ascii_digit())
}

/// A patch is applicable only to the EXACT branch that produced and
/// independently evaluated it. Main-origin research may inform another
/// branch but its exact tested patch cannot be transplanted as verified.
pub(super) fn owns_application_branch(current:&str,recorded:Option<&str>)->bool {
    is_rsi_work_branch(current)
        && recorded.is_some_and(|source|source==current)
}

