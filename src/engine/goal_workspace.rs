//! Optional per-goal Git worktree: isolated source mutations and durable
//! reviewable results. This is Git-source isolation, NOT an OS sandbox.
//! Never fall back to the live checkout if an isolated worktree is missing.
use crate::state::AppState;
use serde_json::{json, Value};
use std::{path::{Path,PathBuf},sync::OnceLock,time::Duration};

const MAX_TEXT:usize=60_000;
fn git_lock()->&'static tokio::sync::Mutex<()> {
    static LOCK:OnceLock<tokio::sync::Mutex<()>>=OnceLock::new();
    LOCK.get_or_init(||tokio::sync::Mutex::new(()))
}
fn path_for(state:&AppState,id:&str)->Result<PathBuf,String>{
    if id.len()<12||id.len()>100
        ||!id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){
        return Err("유효하지 않은 목표 ID입니다.".into());
    }
    Ok(state.config.data_dir.join("goal-workspaces").join(id))
}
async fn git(root:&Path,args:&[&str])->Result<String,String>{
    let mut command=tokio::process::Command::new("git");
    command.arg("-C").arg(root).args(args);
    let output=crate::engine::tools::run_command(&mut command,Duration::from_secs(30)).await?;
    if !output.status.success() {
        return Err(format!("Git {}: {}",args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).chars().take(900).collect::<String>()));
    }
    String::from_utf8(output.stdout).map_err(|_|"Git 응답이 UTF-8이 아닙니다.".into())
}
fn git_sha(text:&str)->bool{
    text.len()==40&&text.bytes().all(|b|b.is_ascii_hexdigit())
}
fn checked_path(state:&AppState,id:&str,goal:&Value)->Result<PathBuf,String>{
    let info=&goal["gitWorktree"];
    let expected=path_for(state,id)?;
    if info["rolledBack"]==true||info["path"].as_str()!=expected.to_str()
        ||!expected.join(".git").is_file()
        ||!info["baseCommit"].as_str().is_some_and(git_sha) {
        return Err("이 목표의 격리 작업공간이 유실되거나 폐기됐습니다. 원본 작업공간으로 되돌려 실행하지 않습니다.".into());
    }
    let parent=expected.parent().ok_or("격리 작업공간 상위 경로가 없습니다.")?
        .canonicalize().map_err(|e|e.to_string())?;
    let canonical=expected.canonicalize().map_err(|e|e.to_string())?;
    if !canonical.starts_with(&parent) || canonical==parent {
        return Err("격리 작업공간 경로가 올바르지 않습니다.".into());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    if info["source"].as_str()!=root.to_str() && root!=canonical {
        // A scoped AppState may already point at the same verified goal
        // worktree. Other checkouts must never be treated as its source.
        return Err("현재 원본 저장소와 격리 작업의 시작 저장소가 다릅니다.".into());
    }
    Ok(canonical)
}
pub(crate) fn workspace_for_session(state:&AppState,id:&str)->Result<PathBuf,String>{
    let Some(session)=state.store.get("session",id) else {
        return Ok(state.config.workspace.clone());
    };
    let Some(goal_id)=session["goalId"].as_str() else {
        return Ok(state.config.workspace.clone());
    };
    let goal=state.store.get("goal",goal_id)
        .ok_or("격리 목표가 없어 원본에 쓰지 않습니다.")?;
    if goal["isolateWorktree"]!=true {
        return Ok(state.config.workspace.clone());
    }
    checked_path(state,goal_id,&goal)
}
pub(crate) fn workspace_for_goal(state:&AppState,goal:&Value)->Result<PathBuf,String>{
    if goal["isolateWorktree"]!=true {return Ok(state.config.workspace.clone());}
    let id=goal["id"].as_str().ok_or("목표 ID가 없습니다.")?;
    checked_path(state,id,goal)
}
pub(crate) async fn prepare(state:&AppState,id:&str)->Result<Value,String>{
    let _exclusive=git_lock().lock().await;
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if goal["isolateWorktree"]!=true {
        return Err("격리 옵션이 없는 목표입니다.".into());
    }
    if goal["gitWorktree"].is_object() {
        checked_path(state,id,&goal)?;
        return Ok(goal["gitWorktree"].clone());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let top=git(&root,&["rev-parse","--show-toplevel"]).await?;
    if Path::new(top.trim()).canonicalize().map_err(|e|e.to_string())?!=root {
        return Err("Git 격리는 저장소 최상위에서만 시작할 수 있습니다.".into());
    }
    if !git(&root,&["status","--porcelain","--untracked-files=all"]).await?.trim().is_empty() {
        return Err("원본 저장소에 변경 파일이 있습니다. 안전한 격리 시작을 위해 먼저 확인하세요.".into());
    }
    let base=git(&root,&["rev-parse","HEAD"]).await?.trim().to_owned();
    if !git_sha(&base){return Err("유효한 Git 시작 커밋이 없습니다.".into());}
    let path=path_for(state,id)?;
    if path.exists(){return Err("기존 격리 디렉터리는 자동으로 덮어쓰지 않습니다.".into());}
    let parent=path.parent().ok_or("작업공간 상위 폴더가 없습니다.")?;
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    let path_text=path.to_str().ok_or("작업공간 경로가 UTF-8이 아닙니다.")?;
    let branch=format!("dominion/goal-{id}");
    git(&root,&["worktree","add","-b",&branch,path_text,&base]).await?;
    let record=json!({"path":path_text,"source":root.to_string_lossy(),
        "baseCommit":base,"branch":branch,"rolledBack":false,
        "createdAt":chrono::Utc::now().to_rfc3339()});
    if state.store.patch_if("goal",id,"gitWorktree",&Value::Null,
        &json!({"gitWorktree":record})).is_none(){
        let _=git(&root,&["worktree","remove","--force",path_text]).await;
        return Err("격리 작업공간 상태를 저장하지 못했습니다.".into());
    }
    state.store.audit("goal.worktree_created",
        json!({"goalId":id,"baseCommit":base,"branch":branch}));
    Ok(record)
}
pub(crate) async fn changes(state:&AppState,goal:&Value)->Result<Value,String>{
    let id=goal["id"].as_str().ok_or("목표 ID가 없습니다.")?;
    let work=checked_path(state,id,goal)?;
    let base=goal["gitWorktree"]["baseCommit"].as_str()
        .ok_or("격리 기준 커밋이 없습니다.")?;
    let status=git(&work,&["status","--short","--untracked-files=all"]).await?;
    let diff=git(&work,&["diff","--no-ext-diff","--no-color",base]).await?;
    let staged=git(&work,&["diff","--cached","--no-ext-diff","--no-color",base]).await?;
    let untracked=git(&work,&["ls-files","--others","--exclude-standard"]).await?;
    // Store no untracked file contents: these may contain local credentials.
    Ok(json!({"goalId":id,"isolated":true,"branch":goal["gitWorktree"]["branch"],
        "baseCommit":base,"workspace":work.to_string_lossy(),
        "status":status.chars().take(MAX_TEXT).collect::<String>(),
        "diff":diff.chars().take(MAX_TEXT).collect::<String>(),
        "stagedDiff":staged.chars().take(MAX_TEXT).collect::<String>(),
        "untracked":untracked.chars().take(MAX_TEXT).collect::<String>(),
        "truncated":diff.len()>MAX_TEXT||staged.len()>MAX_TEXT}))
}
pub(crate) async fn discard(state:&AppState,goal:&Value)->Result<Value,String>{
    let id=goal["id"].as_str().ok_or("목표 ID가 없습니다.")?;
    if matches!(goal["status"].as_str(),Some("running"|"queued")){
        return Err("목표를 종료한 후 격리 작업공간을 폐기할 수 있습니다.".into());
    }
    // Prevent deleting a worktree with any live delegated child job.
    if state.store.all("job").iter().any(|job|{
        if !matches!(job["status"].as_str(),Some("queued"|"running")){return false;}
        job.pointer("/input/goalId").and_then(Value::as_str)==Some(id)
            || job.pointer("/input/sessionId").and_then(Value::as_str)
                .and_then(|s|state.store.get("session",s))
                .is_some_and(|session|session["goalId"].as_str()==Some(id))
    }){
        return Err("이 목표 또는 하위 작업이 진행 중이어서 폐기할 수 없습니다.".into());
    }
    let _exclusive=git_lock().lock().await;
    let work=checked_path(state,id,goal)?;
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let text=work.to_str().ok_or("격리 경로 UTF-8이 필요합니다.")?;
    git(&root,&["worktree","remove","--force",text]).await?;
    let patch=json!({"gitWorktree":{"path":goal["gitWorktree"]["path"],
        "source":goal["gitWorktree"]["source"],
        "baseCommit":goal["gitWorktree"]["baseCommit"],
        "branch":goal["gitWorktree"]["branch"],"rolledBack":true},
        "discardedAt":chrono::Utc::now().to_rfc3339()});
    let saved=state.store.patch_if("goal",id,"status",&goal["status"],&patch)
        .ok_or("Git 작업공간은 폐기됐지만 목표 상태 갱신에 실패했습니다.")?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.worktree_discarded",
        json!({"goalId":id,"branch":goal["gitWorktree"]["branch"]}));
    Ok(json!({"goalId":id,"discarded":true,"originalUntouched":true,
        "branch":goal["gitWorktree"]["branch"],"branchRetained":true}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn goal_tools_edit_only_isolated_checkout_and_discard_keeps_original(){
        use crate::{config::Config,db::Store,vault::Vault};
        let base=std::env::temp_dir().join(format!("goal-scope-{}",uuid::Uuid::new_v4()));
        let source=base.join("source");
        let data=base.join("data");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let git_ok=|arguments:&[&str]|{
            let out=std::process::Command::new("git").arg("-C").arg(&source)
                .args(arguments).output().unwrap();
            assert!(out.status.success(),"{arguments:?}: {}",
                String::from_utf8_lossy(&out.stderr));
        };
        git_ok(&["init","-q"]);
        std::fs::write(source.join("module.rs"),"fn source() {}\\n").unwrap();
        git_ok(&["add","module.rs"]);
        git_ok(&["-c","user.name=Goal Test","-c","user.email=goal@test.invalid",
            "commit","-qm","initial"]);
        let cfg=Config{port:0,data_dir:data,web_dir:base.join("web"),workspace:source.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":id,"status":"queued",
            "isolateWorktree":true})).unwrap();
        let info=prepare(&state,&id).await.unwrap();
        let session_id="goal-session";
        state.store.put("session",&json!({"id":session_id,"goalId":id,
            "goalWorktree":info["path"]})).unwrap();
        let work=workspace_for_session(&state,session_id).unwrap();
        assert_ne!(work,source.canonicalize().unwrap());
        let ctx=crate::engine::tools::ToolContext{
            workspace:&state.config.workspace,store:&state.store,state:&state,
            session_id,agent_id:None,tools_enabled:true,tools_execution_enabled:true,
        };
        let result=crate::engine::tools::execute(&ctx,"workspace_write",
            &json!({"path":"module.rs","content":"fn isolated() {}\\n"}),"fixture-run").await.unwrap();
        assert_eq!(result["written"],true);
        assert_eq!(std::fs::read_to_string(source.join("module.rs")).unwrap(),
            "fn source() {}\\n");
        assert!(std::fs::read_to_string(work.join("module.rs")).unwrap()
            .contains("isolated"));
        assert!(crate::engine::tools::execute(&ctx,"file_write",
            &json!({"path":source.join("module.rs").to_str().unwrap(),
                "content":"corruption"}),"fixture-run").await.is_err());
        let goal=state.store.get("goal",&id).unwrap();
        assert!(changes(&state,&goal).await.unwrap()["diff"].as_str().unwrap()
            .contains("fn isolated"));
        state.store.patch_if("goal",&id,"status",&json!("queued"),
            &json!({"status":"completed"})).unwrap();
        let goal=state.store.get("goal",&id).unwrap();
        assert_eq!(discard(&state,&goal).await.unwrap()["originalUntouched"],true);
        assert!(!work.exists());
        assert!(workspace_for_session(&state,session_id).is_err());
        assert_eq!(std::fs::read_to_string(source.join("module.rs")).unwrap(),
            "fn source() {}\\n");
        drop(state);
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn refuses_invalid_ids_without_touching_files(){
        let root=std::env::temp_dir();
        let config=crate::config::Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let store=crate::db::Store::open(&root.join(format!("goals-{}.db",uuid::Uuid::new_v4()))).unwrap();
        let vault=crate::vault::Vault::open(&root.join(format!("goals-{}.key",uuid::Uuid::new_v4())));
        let state=AppState::new(config,store,vault);
        assert!(path_for(&state,"../escape").is_err());
        assert!(path_for(&state,"a/b").is_err());
    }
}
