//! Goal-owned disposable Git worktrees. They isolate tracked source changes,
//! not processes, networks or absolute shell paths. Creation/discard never
//! touches the source checkout or silently publishes a change.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::HashSet,path::{Path,PathBuf},process::Stdio,time::Duration};
use tokio::process::Command;

const DIFF_LIMIT:usize=100_000;
fn goal_path(state:&AppState,id:&str)->Result<PathBuf,String>{
    if id.len()<12 || id.len()>100
        || !id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'){
        return Err("유효하지 않은 목표 식별자입니다.".into());
    }
    let data=state.config.data_dir.canonicalize().map_err(|e|e.to_string())?;
    let source=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    // Default DOMINION_DATA is <repository>/runtime. A worktree placed there
    // would be inside the original checkout and fail the source-isolation
    // contract. Keep its deterministic owner directory beside the checkout.
    let parent=if data.starts_with(&source){
        let ancestor=source.parent().ok_or("Git 원본 폴더의 부모 경로가 없습니다.")?;
        let digest=format!("{:x}",sha2::Sha256::digest(source.to_string_lossy().as_bytes()));
        ancestor.join(format!(".dominion-goal-worktrees-{}",&digest[..16]))
    }else{data.join("goal-worktrees")};
    Ok(parent.join(id))
}
async fn git(root:&Path,args:&[&str])->Result<String,String>{
    let out=tokio::time::timeout(Duration::from_secs(30),
        Command::new("git").current_dir(root).args(args)
            .stdin(Stdio::null()).kill_on_drop(true).output())
        .await.map_err(|_|"Git 작업 제한 시간을 초과했습니다.".to_string())?
        .map_err(|e|format!("Git 실행 실패: {e}"))?;
    if !out.status.success(){
        return Err(format!("Git {}: {}",args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).chars().take(600).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).chars().take(DIFF_LIMIT).collect())
}
fn checked_path(state:&AppState,id:&str,goal:&Value)->Result<PathBuf,String>{
    let expected=goal_path(state,id)?;
    let metadata=&goal["gitWorktree"];
    if goal["isolateWorktree"]!=true || metadata["discarded"]==true
        || metadata["path"].as_str()!=expected.to_str()
        || !expected.join(".git").is_file(){
        return Err("목표 전용 Git 작업공간이 없거나 폐기되어 공용 폴더로 대체하지 않습니다.".into());
    }
    let actual=expected.canonicalize().map_err(|e|e.to_string())?;
    let parent=expected.parent().ok_or("격리 디렉터리 오류")?
        .canonicalize().map_err(|e|e.to_string())?;
    if !actual.starts_with(&parent)
        || metadata["source"].as_str()!=state.config.workspace.canonicalize()
            .ok().as_deref().and_then(Path::to_str){
        return Err("격리 작업공간의 실제 경로 또는 원본이 변경됐습니다.".into());
    }
    Ok(actual)
}
pub(crate) async fn prepare(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표 기록이 없습니다.")?;
    if goal["isolateWorktree"]!=true{return Ok(Value::Null);}
    if goal["gitWorktree"].is_object(){
        checked_path(state,id,&goal)?;
        return Ok(goal["gitWorktree"].clone());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let top=git(&root,&["rev-parse","--show-toplevel"]).await?;
    if Path::new(top.trim()).canonicalize().map_err(|e|e.to_string())?!=root{
        return Err("Git 저장소 최상위 폴더에서 격리 작업을 시작하세요.".into());
    }
    if !git(&root,&["status","--porcelain","--untracked-files=normal"]).await?.trim().is_empty(){
        return Err("공용 Git 작업폴더에 저장되지 않은 변경이 있어 격리 작업을 시작하지 않았습니다.".into());
    }
    let base=git(&root,&["rev-parse","HEAD"]).await?.trim().to_owned();
    if base.len()!=40||!base.bytes().all(|b|b.is_ascii_hexdigit()){
        return Err("기준 Git 커밋이 유효하지 않습니다.".into());
    }
    let path=goal_path(state,id)?;
    if path.exists(){return Err("기존 목표 작업공간을 덮어쓸 수 없습니다.".into());}
    let parent=path.parent().ok_or("작업공간 부모 경로가 없습니다.")?;
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    // Never create a second Git working tree inside the source checkout.
    if path.starts_with(&root){return Err("격리 데이터 폴더가 소스 저장소 안에 있습니다.".into());}
    let dst=path.to_str().ok_or("목표 작업공간 경로 인코딩 실패")?;
    git(&root,&["worktree","add","--detach",dst,&base]).await?;
    let info=json!({"path":dst,"baseCommit":base,"source":root,
        "mode":"detached_git_worktree","discarded":false,
        "createdAt":chrono::Utc::now().to_rfc3339()});
    if state.store.patch_if("goal",id,"id",&json!(id),
        &json!({"gitWorktree":info})).is_none(){
        let _=git(&root,&["worktree","remove","--force",dst]).await;
        return Err("격리 작업공간 메타데이터 저장 실패".into());
    }
    Ok(info)
}
/// Inherit the goal from a bounded parent chain so delegated children share
/// its worktree; cycles and broken goal references fail closed.
pub(crate) fn goal_for_session(state:&AppState,session_id:&str)->Result<Option<String>,String>{
    let mut current=session_id.to_owned();
    let mut visited=HashSet::new();
    for _ in 0..8{
        if !visited.insert(current.clone()){return Err("위임 세션 계보에 순환이 있습니다.".into());}
        let Some(session)=state.store.get("session",&current)else{
            return if visited.len()==1{Ok(None)}
                else{Err("위임 세션의 상위 기록이 사라져 공용 작업폴더로 전환할 수 없습니다.".into())};
        };
        if let Some(id)=session["goalId"].as_str().filter(|id|!id.is_empty()){
            if state.store.get("goal",id).is_none(){return Err("목표가 삭제된 세션입니다.".into());}
            return Ok(Some(id.to_owned()));
        }
        let Some(parent)=session["parentSessionId"].as_str().filter(|id|!id.is_empty())else{
            return Ok(None);
        };
        current=parent.to_owned();
    }
    Err("위임 세션 계보가 허용 깊이를 넘었습니다.".into())
}
pub(crate) fn workspace_for_goal(state:&AppState,goal:&Value)->Result<PathBuf,String>{
    if goal["isolateWorktree"]!=true{return Ok(state.config.workspace.clone());}
    let id=goal["id"].as_str().ok_or("목표 식별자가 없습니다.")?;
    checked_path(state,id,goal)
}
pub(crate) fn workspace_for_session(state:&AppState,session_id:&str)->Result<PathBuf,String>{
    let Some(id)=goal_for_session(state,session_id)? else{return Ok(state.config.workspace.clone());};
    let goal=state.store.get("goal",&id).ok_or("목표 기록이 삭제됐습니다.")?;
    workspace_for_goal(state,&goal)
}
pub(crate) async fn changes(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표 기록이 없습니다.")?;
    let workspace=workspace_for_goal(state,&goal)?;
    if goal["isolateWorktree"]!=true{return Err("Git 격리를 선택하지 않은 목표입니다.".into());}
    let base=goal["gitWorktree"]["baseCommit"].as_str().ok_or("기준 커밋이 없습니다.")?;
    let status=git(&workspace,&["status","--short"]).await?;
    let diff=git(&workspace,&["diff","--no-ext-diff","--no-textconv","--no-color",base]).await?;
    let untracked=git(&workspace,&["ls-files","--others","--exclude-standard"]).await?;
    Ok(json!({"goalId":id,"workspaceMode":"isolated","baseCommit":base,
        "status":status,"diff":diff,"untracked":untracked,
        "truncated":diff.chars().count()>=DIFF_LIMIT}))
}
pub(crate) async fn discard(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표 기록이 없습니다.")?;
    if matches!(goal["status"].as_str(),Some("running"|"queued")){
        return Err("실행 중인 목표를 먼저 중단하세요.".into());
    }
    if state.store.all("job").iter().any(|job|{
        if !matches!(job["status"].as_str(),Some("queued"|"running")){return false;}
        if job["kind"]=="goal" && job["input"]["goalId"].as_str()==Some(id){return true;}
        job["input"]["sessionId"].as_str().is_some_and(|sid|
            goal_for_session(state,sid).ok().flatten().as_deref()==Some(id))
    }){return Err("실행 중인 하위 작업이 있어 작업공간을 보존했습니다.".into());}
    let work=workspace_for_goal(state,&goal)?;
    if goal["isolateWorktree"]!=true{return Err("Git 격리 작업이 아닙니다.".into());}
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let path=work.to_str().ok_or("작업공간 경로 오류")?;
    git(&root,&["worktree","remove","--force",path]).await?;
    let mut info=goal["gitWorktree"].clone();
    info["discarded"]=json!(true);
    info["discardedAt"]=json!(chrono::Utc::now().to_rfc3339());
    let saved=state.store.patch_if("goal",id,"status",&goal["status"],
        &json!({"gitWorktree":info})).ok_or("폐기 상태 기록 실패")?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.worktree_discarded",json!({"goalId":id,
        "baseCommit":goal["gitWorktree"]["baseCommit"]}));
    Ok(json!({"goalId":id,"discarded":true,"sourceCheckoutPreserved":true}))
}
#[cfg(test)]
mod tests{
    use super::*;
    fn setup()->(AppState,PathBuf,PathBuf){
        let root=std::env::temp_dir().join(format!("dominion-goal-workspace-{}",uuid::Uuid::new_v4()));
        let repo=root.join("source");
        let data=root.join("data");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let git=|args:&[&str]|{
            let output=std::process::Command::new("git").current_dir(&repo)
                .args(args).output().unwrap();
            assert!(output.status.success(),"git {:?}: {}",args,
                String::from_utf8_lossy(&output.stderr));
        };
        git(&["init","-q"]);
        git(&["config","user.name","DOMINION Test"]);
        git(&["config","user.email","test@example.invalid"]);
        std::fs::write(repo.join("src.txt"),"original").unwrap();
        git(&["add","src.txt"]);
        git(&["commit","-qm","baseline"]);
        let config=crate::config::Config{port:0,data_dir:data,
            workspace:repo.clone(),web_dir:root.join("web")};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        (state,root,repo)
    }
    #[tokio::test]
    async fn goal_and_child_share_detached_source_that_can_be_discarded(){
        let (state,root,source)=setup();
        let goal_id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":goal_id,"status":"queued",
            "isolateWorktree":true})).unwrap();
        let meta=prepare(&state,&goal_id).await.unwrap();
        assert_eq!(meta["mode"],"detached_git_worktree");
        state.store.put("session",&json!({"id":"owner","goalId":goal_id})).unwrap();
        state.store.put("session",&json!({"id":"child","parentSessionId":"owner",
            "delegated":true,"delegatedReadOnly":true})).unwrap();
        let owner=workspace_for_session(&state,"owner").unwrap();
        let child=workspace_for_session(&state,"child").unwrap();
        assert_eq!(owner,child,"delegated investigation must see the goal's source");
        assert_ne!(owner,source.canonicalize().unwrap());
        std::fs::write(owner.join("src.txt"),"changed").unwrap();
        let result=changes(&state,&goal_id).await.unwrap();
        assert!(result["diff"].as_str().unwrap().contains("changed"));
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"original");
        let goal=state.store.get("goal",&goal_id).unwrap();
        state.store.patch_if("goal",&goal_id,"status",&goal["status"],
            &json!({"status":"needs_review"})).unwrap();
        let discarded=discard(&state,&goal_id).await.unwrap();
        assert_eq!(discarded["sourceCheckoutPreserved"],true);
        assert!(workspace_for_session(&state,"child").is_err(),
            "a removed worktree must never fall back to the source checkout");
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"original");
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn missing_or_dirty_git_source_does_not_create_a_worktree(){
        let (state,root,source)=setup();
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":id,"status":"queued",
            "isolateWorktree":true})).unwrap();
        std::fs::write(source.join("src.txt"),"existing owner edit").unwrap();
        assert!(prepare(&state,&id).await.is_err());
        assert!(!goal_path(&state,&id).unwrap().exists());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn path_identifier_rejects_traversal(){
        assert!(!"../../etc".bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'));
    }
}
