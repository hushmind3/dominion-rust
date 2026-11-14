//! Disposable Git worktrees for goal-owned code changes and observable diffs.
//! The original checkout is never overwritten to roll back an agent attempt.
//! Git worktrees are source isolation, NOT OS/sandbox confinement: shell and
//! absolute-path tools still require the existing owner approval policy.
use crate::state::AppState;
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::process::Command;

const LIMIT:usize=100_000;
fn goal_path(state:&AppState,id:&str)->Result<PathBuf,String>{
    if id.len()<12 || id.len()>100 || !id.bytes().all(|x|x.is_ascii_alphanumeric()||x==b'-') {
        return Err("유효하지 않은 목표 식별자입니다.".into());
    }
    Ok(state.config.data_dir.join("goal-worktrees").join(id))
}
async fn git(root:&Path,args:&[&str])->Result<String,String>{
    let mut cmd=Command::new("git");
    cmd.current_dir(root).args(args).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped());
    let output=tokio::time::timeout(Duration::from_secs(30),cmd.output())
        .await.map_err(|_|"Git 작업이 제한 시간을 초과했습니다.".to_string())?
        .map_err(|e|format!("git 실행 실패: {e}"))?;
    if !output.status.success(){
        return Err(format!("git {}: {}",args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).chars().take(1200).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).chars().take(LIMIT).collect())
}
pub(crate) async fn prepare(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if goal["gitWorktree"].is_object(){
        let info=&goal["gitWorktree"];
        let path=goal_path(state,id)?;
        if info["rolledBack"]==true || !path.join(".git").is_file(){
            return Err("이 목표의 Git 작업공간이 없어졌습니다. 변경을 자동 재실행하지 않습니다.".into());
        }
        return Ok(info.clone());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let top=git(&root,&["rev-parse","--show-toplevel"]).await?;
    if Path::new(top.trim()).canonicalize().map_err(|e|e.to_string())?!=root {
        return Err("격리 작업은 Git 저장소 최상위 워크스페이스에서만 시작합니다.".into());
    }
    if !git(&root,&["status","--porcelain","--untracked-files=normal"]).await?.trim().is_empty(){
        return Err("기존 워크스페이스에 커밋되지 않은 변경이 있습니다. 정리한 후 격리 작업을 시작하세요.".into());
    }
    let base=git(&root,&["rev-parse","HEAD"]).await?.trim().to_string();
    let path=goal_path(state,id)?;
    if path.exists() {return Err("이미 존재하는 작업공간을 덮어쓸 수 없습니다.".into());}
    let parent=path.parent().ok_or("작업공간 상위 디렉터리가 없습니다.")?;
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    let branch=format!("dominion/goal-{id}");
    let absolute=path.to_str().ok_or("작업공간 경로를 인코딩할 수 없습니다.")?;
    git(&root,&["worktree","add","-b",&branch,absolute,&base]).await?;
    let info=json!({"path":absolute,"branch":branch,"baseCommit":base,
        "source":root.to_string_lossy(),"rolledBack":false});
    // Persist before any model/tool execution. The saved source/base/branch
    // are immutable metadata and the only workspace eligible for rollback.
    if state.store.patch_if("goal",id,"id",&json!(id),
        &json!({"gitWorktree":info})).is_none() {
        let _=git(&root,&["worktree","remove","--force",absolute]).await;
        return Err("격리 작업공간 상태를 저장하지 못했습니다.".into());
    }
    Ok(info)
}
pub(crate) fn workspace_for_session(state:&AppState,session_id:&str)->Result<PathBuf,String>{
    let Some(session)=state.store.get("session",session_id) else {
        return Ok(state.config.workspace.clone());
    };
    // Delegated children inherit the same goal-owned checkout. Do not let a
    // child silently fall back to the shared source tree.
    let mut cursor=Some(session);
    let mut seen=std::collections::HashSet::new();
    let mut goal_id=None;
    for _ in 0..=8 {
        let Some(current)=cursor.take() else {break};
        if let Some(id)=current["goalId"].as_str(){goal_id=Some(id.to_string());break;}
        let Some(parent)=current["parentSessionId"].as_str() else {break};
        if !seen.insert(parent.to_string()){return Err("위임 세션 참조가 순환합니다.".into());}
        cursor=state.store.get("session",parent);
    }
    let Some(id)=goal_id else { return Ok(state.config.workspace.clone()); };
    let Some(goal)=state.store.get("goal",&id) else {
        return Err("연결된 목표가 삭제되었습니다.".into());
    };
    if goal["gitWorktree"].is_null(){return Ok(state.config.workspace.clone());}
    checked_path(state,&id,&goal)
}
pub(crate) fn workspace_for_goal(state:&AppState,goal:&Value)->Result<PathBuf,String>{
    let Some(id)=goal["id"].as_str() else{return Err("목표 식별자가 없습니다.".into());};
    if goal["gitWorktree"].is_null(){return Ok(state.config.workspace.clone());}
    checked_path(state,id,goal)
}
fn checked_path(state:&AppState,id:&str,goal:&Value)->Result<PathBuf,String>{
    let path=goal_path(state,id)?;
    if goal["gitWorktree"]["rolledBack"]==true
        || goal["gitWorktree"]["path"].as_str()!=path.to_str()
        || !path.join(".git").is_file(){
        return Err("작업공간이 변경되거나 제거되었습니다. 자동으로 원본에 쓰지 않습니다.".into());
    }
    let canonical=path.canonicalize().map_err(|e|e.to_string())?;
    let parent=path.parent().ok_or("작업공간 상위 디렉터리가 없습니다.")?
        .canonicalize().map_err(|e|e.to_string())?;
    if !canonical.starts_with(parent){return Err("격리 경로가 지정한 디렉터리를 벗어났습니다.".into());}
    Ok(canonical)
}
pub(crate) async fn changes(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    let work=checked_path(state,id,&goal)?;
    let base=goal["gitWorktree"]["baseCommit"].as_str()
        .filter(|s|s.len()==40 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        .ok_or("기준 커밋이 없습니다.")?;
    let status=git(&work,&["status","--short"]).await?;
    let diff=git(&work,&["diff","--no-ext-diff","--no-color",base]).await?;
    let untracked=git(&work,&["ls-files","--others","--exclude-standard"]).await?;
    Ok(json!({"goalId":id,"branch":goal["gitWorktree"]["branch"],
        "baseCommit":base,"status":status,"diff":diff,
        "untracked":untracked,"truncated":diff.chars().count()>=LIMIT,
        "reviewDigest":review_digest(state,id).await?}))
}
pub(crate) async fn rollback(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if matches!(goal["status"].as_str(),Some("running"|"queued")){
        return Err("진행 중인 작업을 먼저 중단하세요.".into());
    }
    // A parent may have finished while its delegated child is still editing.
    // Never remove a worktree underneath an active nested session.
    let active_children=state.store.all("job").into_iter().any(|job|{
        if !matches!(job["status"].as_str(),Some("queued"|"running")){return false;}
        let Some(session_id)=job.pointer("/input/sessionId").and_then(Value::as_str) else{
            return false;
        };
        state.store.get("session",session_id)
            .is_some_and(|session|session["goalId"].as_str()==Some(id))
    });
    if active_children{return Err("하위 에이전트가 작업 중입니다. 모두 종료한 뒤 되돌리세요.".into());}
    let work=checked_path(state,id,&goal)?;
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    if goal["gitWorktree"]["source"].as_str()!=root.to_str(){
        return Err("작업공간 원본이 달라졌습니다. 제거하지 않습니다.".into());
    }
    let work_str=work.to_str().ok_or("유효하지 않은 작업공간 경로")?;
    git(&root,&["worktree","remove","--force",work_str]).await?;
    let saved=state.store.patch_if("goal",id,"status",&goal["status"],
        &json!({"gitWorktree":{"path":goal["gitWorktree"]["path"],
            "baseCommit":goal["gitWorktree"]["baseCommit"],
            "source":goal["gitWorktree"]["source"],
            "branch":goal["gitWorktree"]["branch"],"rolledBack":true},
            "rollbackAt":chrono::Utc::now().to_rfc3339()}))
        .ok_or("작업공간은 제거됐지만 상태 저장에 실패했습니다.")?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.worktree_discarded",
        json!({"goalId":id,"branch":goal["gitWorktree"]["branch"]}));
    Ok(json!({"ok":true,"goalId":id,"localWorktreeRemoved":true,
        "remoteBranchUnchanged":true}))
}
/// Verify a review receipt against the *entire* worktree, including new files.
/// A patch preview alone cannot authenticate untracked file contents.
async fn review_digest(state:&AppState,id:&str)->Result<String,String>{
    use sha2::{Digest,Sha256};
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    let work=checked_path(state,id,&goal)?;
    let status=git(&work,&["status","--porcelain=v1","--untracked-files=all"]).await?;
    let diff=git(&work,&["diff","--binary","--no-ext-diff","HEAD"]).await?;
    if status.chars().count()>=LIMIT || diff.chars().count()>=LIMIT{
        return Err("Git 변경량이 검토 가능 크기를 초과합니다. 범위를 나눠 검토하세요.".into());
    }
    let extra=git(&work,&["ls-files","--others","--exclude-standard"]).await?;
    let mut hash=Sha256::new();
    hash.update(b"dominion-goal-worktree-review-v1\\0");
    hash.update(goal["gitWorktree"]["baseCommit"].as_str().unwrap_or("").as_bytes());
    hash.update(status.as_bytes());
    hash.update(diff.as_bytes());
    let paths=extra.lines().collect::<Vec<_>>();
    if paths.len()>100 {return Err("새 파일이 100개를 초과하여 검토를 중단했습니다.".into());}
    for name in paths {
        let path=work.join(name);
        let meta=std::fs::symlink_metadata(&path).map_err(|e|e.to_string())?;
        if !meta.file_type().is_file() || meta.len()>8*1024*1024 {
            return Err("검토 대상에 심볼릭 링크 또는 8MiB 초과 새 파일이 있습니다.".into());
        }
        let bytes=std::fs::read(&path).map_err(|e|e.to_string())?;
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(format!("{:x}",hash.finalize()))
}

/// Explicit user-approved integration. The source checkout must be clean and
/// still point to the exact commit from which this worktree was created.
/// Never force, reset, stash, checkout or perform an automatic merge commit.
pub(crate) async fn apply(state:&AppState,id:&str,expected:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if !matches!(goal["status"].as_str(),Some("completed"|"needs_review")){
        return Err("실행 중이거나 중단된 목표의 변경은 반영할 수 없습니다.".into());
    }
    if goal["gitWorktree"]["applied"]==true || goal["gitWorktree"]["rolledBack"]==true{
        return Err("이미 반영하거나 폐기한 작업공간입니다.".into());
    }
    // The same session may have spawned asynchronous children still editing.
    let outstanding=state.store.all("job").into_iter().any(|job|{
        if !matches!(job["status"].as_str(),Some("queued"|"running")){return false;}
        let Some(child)=job.pointer("/input/sessionId").and_then(Value::as_str) else{return false;};
        let mut cursor=Some(child.to_string());
        let mut seen=std::collections::HashSet::new();
        for _ in 0..=8 {
            let Some(sid)=cursor.take() else{break};
            if !seen.insert(sid.clone()){return true;}
            let Some(session)=state.store.get("session",&sid) else{break};
            if session["goalId"].as_str()==Some(id){return true;}
            cursor=session["parentSessionId"].as_str().map(str::to_string);
        }
        false
    });
    if outstanding {return Err("하위 에이전트 실행이 끝나지 않아 병합할 수 없습니다.".into());}
    let current=review_digest(state,id).await?;
    if expected.len()!=64 || expected!=current{
        return Err("변경된 파일이 마지막 검토와 달라졌습니다. 새 미리보기를 열어 다시 확인하세요.".into());
    }
    let work=checked_path(state,id,&goal)?;
    let source=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    if goal["gitWorktree"]["source"].as_str()!=source.to_str(){
        return Err("원본 checkout 경로가 변경됐습니다.".into());
    }
    let base=goal["gitWorktree"]["baseCommit"].as_str().ok_or("기준 커밋이 없습니다.")?;
    let head=git(&source,&["rev-parse","HEAD"]).await?;
    if head.trim()!=base {
        return Err("원본 HEAD가 목표 시작 이후 이동했습니다. 강제 병합하지 않습니다.".into());
    }
    if !git(&source,&["status","--porcelain=v1","--untracked-files=all"]).await?.trim().is_empty(){
        return Err("원본 저장소에 아직 검토되지 않은 변경이 있습니다. 강제 병합하지 않습니다.".into());
    }
    if git(&work,&["status","--porcelain=v1","--untracked-files=all"]).await?.trim().is_empty(){
        return Err("병합할 작업 변경이 없습니다.".into());
    }
    // One deliberate add/commit in the private branch; no model-provided
    // command or commit metadata is executed. Disable arbitrary repository
    // commit hooks on this application-owned operation.
    git(&work,&["add","-A","--","."]).await?;
    git(&work,&["-c","user.name=DOMINION Agent",
        "-c","user.email=dominion-agent@localhost.invalid",
        "-c","core.hooksPath=/dev/null",
        "commit","-m","Apply reviewed DOMINION autonomous goal"]).await?;
    let adopted=git(&work,&["rev-parse","HEAD"]).await?;
    if git(&source,&["rev-parse","HEAD"]).await?.trim()!=base
        || !git(&source,&["status","--porcelain=v1"]).await?.trim().is_empty(){
        return Err("최종 병합 직전에 원본이 변경됐습니다. 격리 브랜치에 커밋만 보존했습니다.".into());
    }
    let branch_name=goal["gitWorktree"]["branch"].as_str().ok_or("작업 브랜치가 없습니다.")?;
    git(&source,&["-c","core.hooksPath=/dev/null","merge","--ff-only",branch_name]).await?;
    let saved=state.store.patch_if("goal",id,"status",&goal["status"],&json!({
        "gitWorktree":{"path":goal["gitWorktree"]["path"],"source":goal["gitWorktree"]["source"],
            "baseCommit":base,"branch":branch_name,"rolledBack":false,
            "applied":true,"appliedCommit":adopted.trim()},
        "changesAppliedAt":chrono::Utc::now().to_rfc3339()
    })).ok_or("원본 반영은 완료됐지만 목표 기록을 저장하지 못했습니다. Git HEAD를 확인하세요.")?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.worktree_applied",json!({"goalId":id,"commit":adopted.trim(),"reviewDigest":expected}));
    Ok(json!({"ok":true,"goalId":id,"commit":adopted.trim(),"sourceFastForwarded":true}))
}

#[cfg(test)]
mod tests{
 use super::*;
 #[tokio::test]
 async fn isolated_goal_diff_and_discard_never_change_source_checkout(){
     use std::process::Command as StdCommand;
     let root=std::env::temp_dir().join(format!("dominion-goal-git-{}",uuid::Uuid::new_v4()));
     let source=root.join("source");
     let data=root.join("data");
     std::fs::create_dir_all(&source).unwrap();
     std::fs::create_dir_all(&data).unwrap();
     let git_ok=|args:&[&str]|{
         let output=StdCommand::new("git").current_dir(&source)
             .args(args).output().unwrap();
         assert!(output.status.success(),
             "git {:?}: {}",args,String::from_utf8_lossy(&output.stderr));
     };
     git_ok(&["init","-q"]);
     git_ok(&["config","user.name","DOMINION Test"]);
     git_ok(&["config","user.email","ci@example.invalid"]);
     std::fs::write(source.join("sample.rs"),"fn original() {}").unwrap();
     git_ok(&["add","sample.rs"]);
     git_ok(&["commit","-qm","original source"]);
     let config=crate::config::Config{port:0,data_dir:data.clone(),
         web_dir:root.join("web"),workspace:source.clone()};
     let store=crate::db::Store::open(&config.db_path()).unwrap();
     let vault=crate::vault::Vault::open(&config.key_path());
     let state=AppState::new(config,store,vault);
     let id=uuid::Uuid::new_v4().to_string();
     state.store.put("goal",&json!({"id":id,"status":"queued",
         "isolateWorktree":true})).unwrap();
     let info=prepare(&state,&id).await.unwrap();
     assert_eq!(info["rolledBack"],false);
     state.store.put("session",&json!({"id":"session","goalId":id})).unwrap();
     let isolated=workspace_for_session(&state,"session").unwrap();
     assert_ne!(isolated,source.canonicalize().unwrap());
     std::fs::write(isolated.join("sample.rs"),"fn changed() {}").unwrap();
     let diff=changes(&state,&id).await.unwrap();
     assert!(diff["diff"].as_str().unwrap().contains("fn changed()"));
     assert_eq!(std::fs::read_to_string(source.join("sample.rs")).unwrap(),
         "fn original() {}");
     let goal=state.store.get("goal",&id).unwrap();
     state.store.patch_if("goal",&id,"status",&goal["status"],
         &json!({"status":"completed"})).unwrap();
     let result=rollback(&state,&id).await.unwrap();
     assert_eq!(result["localWorktreeRemoved"],true);
     assert!(!isolated.exists());
     assert!(workspace_for_session(&state,"session").is_err());
     assert_eq!(std::fs::read_to_string(source.join("sample.rs")).unwrap(),
         "fn original() {}");
 }
 #[test]
 fn rejects_attacker_selected_ids(){
     let root=std::env::temp_dir();
     let config=crate::config::Config{port:0,data_dir:root.clone(),
         web_dir:root.join("web"),workspace:root.clone()};
     let store=crate::db::Store::open(&root.join(format!("test-{}.db",uuid::Uuid::new_v4()))).unwrap();
     let vault=crate::vault::Vault::open(&root.join(format!("test-{}.key",uuid::Uuid::new_v4())));
     let state=AppState::new(config,store,vault);
     assert!(goal_path(&state,"../../../../../etc").is_err());
     assert!(goal_path(&state,"a/b").is_err());
 }
}
