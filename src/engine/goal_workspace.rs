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
// Digest of the FULL reviewable patch, not the display-clipped diff.
// Binary/non-UTF8 and overly large changes require external Git/PR review.
const MAX_REVIEW_PATCH:usize=128*1024;
static APPLY_LOCK:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());
// The original checkout index and the goal worktree index are deliberately
// never staged for review. An ephemeral index based on the frozen base can
// mark new regular files intent-to-add, making one exact Git patch cover
// tracked edits AND newly authored source files without touching either index.
struct ReviewIndex(PathBuf);
impl Drop for ReviewIndex {
    fn drop(&mut self) {
        let _=std::fs::remove_file(&self.0);
        let _=std::fs::remove_file(self.0.with_extension("lock"));
    }
}
async fn review_git(root:&Path,index:Option<&Path>,args:&[&str])->Result<Vec<u8>,String>{
    let mut command=Command::new("git");
    command.current_dir(root).args(args).stdin(Stdio::null()).kill_on_drop(true);
    if let Some(index)=index{command.env("GIT_INDEX_FILE",index);}
    let output=tokio::time::timeout(Duration::from_secs(30),command.output())
        .await.map_err(|_|"Git 변경 검토 제한 시간 초과".to_string())?
        .map_err(|e|format!("Git 변경 검토 실행 실패: {e}"))?;
    if !output.status.success(){
        return Err(format!("Git 변경 검토 실패: {}",
            String::from_utf8_lossy(&output.stderr).chars().take(500).collect::<String>()));
    }
    Ok(output.stdout)
}
async fn untracked_review_paths(root:&Path)->Result<Vec<String>,String>{
    let bytes=review_git(root,None,&["ls-files","--others","--exclude-standard","-z"]).await?;
    if bytes.len()>32*1024{return Err("검토할 미추적 파일 경로가 너무 많습니다.".into());}
    let canonical_root=root.canonicalize().map_err(|e|e.to_string())?;
    let mut result=Vec::new();
    for raw in bytes.split(|b|*b==0).filter(|part|!part.is_empty()){
        if result.len()>=64{return Err("새 파일이 64개를 넘어 별도의 PR 검토가 필요합니다.".into());}
        let relative=std::str::from_utf8(raw)
            .map_err(|_|"UTF-8로 표시할 수 없는 새 파일 이름입니다.")?;
        let path=Path::new(relative);
        if path.is_absolute()||!path.components().all(|component|
            matches!(component,std::path::Component::Normal(_))){
            return Err("새 파일의 상대 경로가 안전하지 않습니다.".into());
        }
        let file=canonical_root.join(path);
        let metadata=std::fs::symlink_metadata(&file).map_err(|e|e.to_string())?;
        if !metadata.file_type().is_file()||metadata.len()>MAX_REVIEW_PATCH as u64{
            return Err("새 파일의 형식이나 크기가 직접 검토 한도를 넘었습니다.".into());
        }
        let canonical_file=file.canonicalize().map_err(|e|e.to_string())?;
        if !canonical_file.starts_with(&canonical_root){
            return Err("Git 격리 폴더 외부 파일은 승인 패치에 넣지 않습니다.".into());
        }
        // Reject binary and undecodable new files instead of showing a
        // misleading text patch. Existing binaries are checked below.
        let content=std::fs::read(&file).map_err(|e|e.to_string())?;
        if content.contains(&0)||std::str::from_utf8(&content).is_err(){
            return Err("새 바이너리 파일은 별도 Git/PR 검토가 필요합니다.".into());
        }
        result.push(relative.to_owned());
    }
    Ok(result)
}
async fn patch_bytes(root:&Path,base:&str)->Result<Vec<u8>,String>{
    let new_files=untracked_review_paths(root).await?;
    // A temporary Git index captures intent-to-add without staging source
    // checkout changes. It is generated anew for every preview/apply, so a
    // changed new file always changes the owner-reviewed patch digest.
    let temp=ReviewIndex(std::env::temp_dir().join(
        format!("dominion-review-index-{}",uuid::Uuid::new_v4())));
    review_git(root,Some(&temp.0),&["read-tree",base]).await?;
    if !new_files.is_empty(){
        let mut args=vec!["add".to_string(),"-N".to_string(),"--".to_string()];
        args.extend(new_files);
        let borrowed=args.iter().map(String::as_str).collect::<Vec<_>>();
        review_git(root,Some(&temp.0),&borrowed).await?;
    }
    let patch=review_git(root,Some(&temp.0),
        &["diff","--binary","--no-ext-diff","--no-textconv","--no-color",base]).await?;
    if patch.len()>MAX_REVIEW_PATCH{
        return Err("검토할 Git 변경이 128KiB를 넘어 PR 검토가 필요합니다.".into());
    }
    let text=std::str::from_utf8(&patch)
        .map_err(|_|"텍스트로 검토할 수 없는 Git 패치입니다.")?;
    if text.contains("\nGIT binary patch\n")||text.contains("\nBinary files "){
        return Err("바이너리 변경은 별도 Git/PR 검토가 필요합니다.".into());
    }
    Ok(patch)
}
async fn apply_patch(root:&Path,patch:&[u8],check:bool)->Result<(),String>{
    use tokio::io::AsyncWriteExt;
    let mut child=Command::new("git");
    child.current_dir(root).args(["apply","--binary","--whitespace=nowarn"]);
    if check{child.arg("--check");}
    let mut process=child.arg("-")
        .stdin(Stdio::piped()).stdout(Stdio::piped())
        .stderr(Stdio::piped()).kill_on_drop(true)
        .spawn().map_err(|e|e.to_string())?;
    let mut stdin=process.stdin.take().ok_or("Git 패치 입력을 열지 못했습니다.")?;
    let result=tokio::time::timeout(Duration::from_secs(30),async{
        stdin.write_all(patch).await.map_err(|e|e.to_string())?;
        drop(stdin);
        process.wait_with_output().await.map_err(|e|e.to_string())
    }).await.map_err(|_|"Git 변경 적용 제한 시간 초과".to_string())??;
    if !result.status.success(){
        return Err(format!("Git 패치 적용 거부: {}",
            String::from_utf8_lossy(&result.stderr).chars().take(500).collect::<String>()));
    }
    Ok(())
}
pub(crate) async fn preview(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if goal["isolateWorktree"]!=true{return Err("Git 격리 목표가 아닙니다.".into());}
    let work=workspace_for_goal(state,&goal)?;
    let base=goal["gitWorktree"]["baseCommit"].as_str().ok_or("기준 커밋이 없습니다.")?;
    let patch=patch_bytes(&work,base).await?;
    let untracked=git(&work,&["ls-files","--others","--exclude-standard"]).await?;
    let sha=format!("{:x}",Sha256::digest(&patch));
    Ok(json!({"patchSha256":sha,"patchBytes":patch.len(),
        "canApply":!patch.is_empty()
            && goal["gitWorktree"]["appliedAt"].is_null()
            && matches!(goal["status"].as_str(),
                Some("completed"|"failed"|"cancelled"|"needs_review")),
        "untracked":untracked,"diff":std::str::from_utf8(&patch).unwrap_or(""),
        "requiresExplicitOwnerApply":true,
        "reviewIncludesAllTrackedBytes":true,"reviewIncludesNewRegularFiles":true}))
}
/// Owner-confirmed, exact-SHA-pinned promotion of one reviewed patch to the
/// original clean checkout. Never force-reset, merge, stage or auto-commit.
pub(crate) async fn apply_reviewed(state:&AppState,id:&str,expected:&str)->Result<Value,String>{
    if expected.len()!=64||!expected.bytes().all(|b|b.is_ascii_hexdigit()){
        return Err("검토한 전체 패치의 SHA-256(64자리)이 필요합니다.".into());
    }
    let _guard=APPLY_LOCK.lock().await;
    let goal=state.store.get("goal",id).ok_or("목표를 찾을 수 없습니다.")?;
    if !matches!(goal["status"].as_str(),Some("completed"|"failed"|"cancelled"|"needs_review")){
        return Err("실행이 끝난 목표만 원본에 반영할 수 있습니다.".into());
    }
    if goal["gitWorktree"]["appliedAt"].is_string(){
        return Err("이미 원본에 적용한 목표입니다.".into());
    }
    let work=workspace_for_goal(state,&goal)?;
    // Other jobs can write the shared source checkout; the owner must stop
    // them before promotion. This guard is in addition to Git's --check.
    if state.store.all("job").iter().any(|job|
        matches!(job["status"].as_str(),Some("queued"|"running"))
        && job["kind"]!="learn"){
        return Err("다른 실행 작업을 종료한 뒤 변경을 적용하세요.".into());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    if goal["gitWorktree"]["source"].as_str()!=root.to_str(){
        return Err("원본 Git 저장소 경로가 변경됐습니다.".into());
    }
    let base=goal["gitWorktree"]["baseCommit"].as_str().ok_or("기준 커밋이 없습니다.")?;
    if git(&root,&["rev-parse","HEAD"]).await?.trim()!=base{
        return Err("원본 HEAD가 변경됐습니다. 변경을 다시 검토하세요.".into());
    }
    if !git(&root,&["status","--porcelain","--untracked-files=all"]).await?.trim().is_empty(){
        return Err("원본 체크아웃이 깨끗하지 않아 덮어쓰기를 거부했습니다.".into());
    }
    let patch=patch_bytes(&work,base).await?;
    if patch.is_empty(){return Err("원본에 적용할 추적 파일 변경이 없습니다.".into());}
    let actual=format!("{:x}",Sha256::digest(&patch));
    if actual!=expected.to_ascii_lowercase(){
        return Err("검토 이후 패치가 변경됐습니다. Diff와 SHA-256을 다시 확인하세요.".into());
    }
    apply_patch(&root,&patch,true).await?;
    if git(&root,&["rev-parse","HEAD"]).await?.trim()!=base
        || !git(&root,&["status","--porcelain","--untracked-files=all"]).await?.trim().is_empty(){
        return Err("적용 직전에 원본 Git 상태가 변경됐습니다.".into());
    }
    apply_patch(&root,&patch,false).await?;
    let observed=patch_bytes(&root,base).await?;
    if format!("{:x}",Sha256::digest(&observed))!=actual{
        state.store.audit("goal.apply_review_mismatch",
            json!({"goalId":id,"expectedPatchSha256":actual}));
        return Err("패치를 적용했으나 최종 해시가 다릅니다. 원본을 직접 확인하세요.".into());
    }
    let applied_at=chrono::Utc::now().to_rfc3339();
    let receipt=json!({"goalId":id,"baseCommit":base,"patchSha256":actual,
        "patchBytes":patch.len(),"appliedAt":applied_at,
        "verified":true,"sourceCheckoutUpdated":true,"automaticCommit":false});
    let mut workspace=goal["gitWorktree"].clone();
    workspace["appliedAt"]=json!(applied_at);
    let saved=state.store.patch_if("goal",id,"status",&goal["status"],
        &json!({"gitWorktree":workspace,"gitApply":receipt}))
        .ok_or("적용은 끝났지만 영수증 저장에 실패했습니다. 재실행하지 말고 파일을 직접 확인하세요.")?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.reviewed_patch_applied",receipt.clone());
    Ok(receipt)
}
pub(crate) async fn changes(state:&AppState,id:&str)->Result<Value,String>{
    let goal=state.store.get("goal",id).ok_or("목표 기록이 없습니다.")?;
    let workspace=workspace_for_goal(state,&goal)?;
    if goal["isolateWorktree"]!=true{return Err("Git 격리를 선택하지 않은 목표입니다.".into());}
    let base=goal["gitWorktree"]["baseCommit"].as_str().ok_or("기준 커밋이 없습니다.")?;
    let status=git(&workspace,&["status","--short"]).await?;
    let review=preview(state,id).await?;
    Ok(json!({"goalId":id,"workspaceMode":"isolated","baseCommit":base,
        "status":status,"diff":review["diff"],"untracked":review["untracked"],
        "patchSha256":review["patchSha256"],"patchBytes":review["patchBytes"],
        "canApply":review["canApply"],"truncated":false}))
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
    async fn reviewed_patch_requires_exact_digest_and_clean_source(){
        let (state,root,source)=setup();
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":id,"status":"queued",
            "isolateWorktree":true})).unwrap();
        prepare(&state,&id).await.unwrap();
        let goal=state.store.get("goal",&id).unwrap();
        let work=workspace_for_goal(&state,&goal).unwrap();
        std::fs::write(work.join("src.txt"),"reviewed change").unwrap();
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"original");
        assert!(!preview(&state,&id).await.unwrap()["canApply"].as_bool().unwrap());
        state.store.patch_if("goal",&id,"status",&json!("queued"),
            &json!({"status":"completed"})).unwrap();
        let review=changes(&state,&id).await.unwrap();
        assert_eq!(review["canApply"],true);
        let sha=review["patchSha256"].as_str().unwrap();
        assert!(apply_reviewed(&state,&id,&"0".repeat(64)).await.is_err());
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"original");
        std::fs::create_dir_all(work.join("new")).unwrap();
        std::fs::write(work.join("new/module.rs"),"pub fn created()->u8 { 7 }\n").unwrap();
        let with_new=preview(&state,&id).await.unwrap();
        assert_eq!(with_new["canApply"],true);
        assert!(with_new["diff"].as_str().unwrap().contains("new file mode"));
        assert!(with_new["diff"].as_str().unwrap().contains("new/module.rs"));
        assert!(apply_reviewed(&state,&id,sha).await.is_err(),
            "a new file invalidates a previously reviewed digest");
        let current_sha=with_new["patchSha256"].as_str().unwrap().to_string();
        let receipt=apply_reviewed(&state,&id,&current_sha).await.unwrap();
        assert_eq!(std::fs::read_to_string(source.join("new/module.rs")).unwrap(),
            "pub fn created()->u8 { 7 }\n");
        assert_eq!(receipt["verified"],true);
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"reviewed change");
        assert!(apply_reviewed(&state,&id,sha).await.is_err());
        discard(&state,&id).await.unwrap();
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn untracked_new_file_review_detects_content_tampering(){
        let (state,root,source)=setup();
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":id,"status":"completed",
            "isolateWorktree":true})).unwrap();
        prepare(&state,&id).await.unwrap();
        let goal=state.store.get("goal",&id).unwrap();
        let work=workspace_for_goal(&state,&goal).unwrap();
        std::fs::write(work.join("fresh.txt"),"first\n").unwrap();
        let first=preview(&state,&id).await.unwrap();
        assert_eq!(first["canApply"],true);
        assert_eq!(std::fs::read_to_string(source.join("src.txt")).unwrap(),"original");
        std::fs::write(work.join("fresh.txt"),"second\n").unwrap();
        assert!(apply_reviewed(&state,&id,first["patchSha256"].as_str().unwrap()).await.is_err());
        assert!(!source.join("fresh.txt").exists());
        let second=preview(&state,&id).await.unwrap();
        assert_ne!(first["patchSha256"],second["patchSha256"]);
        assert_eq!(apply_reviewed(&state,&id,second["patchSha256"].as_str().unwrap())
            .await.unwrap()["verified"],true);
        assert_eq!(std::fs::read_to_string(source.join("fresh.txt")).unwrap(),"second\n");
        discard(&state,&id).await.unwrap();
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn binary_new_file_never_gets_an_approvable_text_patch(){
        let (state,root,_source)=setup();
        let id=uuid::Uuid::new_v4().to_string();
        state.store.put("goal",&json!({"id":id,"status":"completed",
            "isolateWorktree":true})).unwrap();
        prepare(&state,&id).await.unwrap();
        let goal=state.store.get("goal",&id).unwrap();
        let work=workspace_for_goal(&state,&goal).unwrap();
        std::fs::write(work.join("opaque.bin"),[0u8,255,6,0]).unwrap();
        assert!(preview(&state,&id).await.is_err());
        assert!(apply_reviewed(&state,&id,&"0".repeat(64)).await.is_err());
        discard(&state,&id).await.unwrap();
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn worktree_path_is_outside_default_nested_runtime(){
        let (state,root,source)=setup();
        let nested=source.join("runtime");
        std::fs::create_dir_all(&nested).unwrap();
        let mut local=state.clone();
        local.config.data_dir=nested;
        let id=uuid::Uuid::new_v4().to_string();
        let chosen=goal_path(&local,&id).unwrap();
        assert!(!chosen.starts_with(source));
        assert!(chosen.file_name().unwrap().to_str().unwrap()==id);
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
