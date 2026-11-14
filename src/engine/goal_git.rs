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
    let Some(id)=session["goalId"].as_str() else {
        return Ok(state.config.workspace.clone());
    };
    let Some(goal)=state.store.get("goal",id) else {
        return Err("연결된 목표가 삭제되었습니다.".into());
    };
    if goal["gitWorktree"].is_null(){return Ok(state.config.workspace.clone());}
    checked_path(state,id,&goal)
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
        "untracked":untracked,"truncated":diff.chars().count()>=LIMIT}))
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
#[cfg(test)]
mod tests{
 use super::*;
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
