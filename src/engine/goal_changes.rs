//! Goal-owned repository changes with bounded Git diff and SHA-guarded undo.
//! Never undo shared-workspace changes unless the workspace was clean BEFORE
//! the goal, the recorded tool wrote that path, HEAD is unchanged, and the
//! owner's previewed current file hash still matches.
use crate::state::AppState;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{collections::BTreeSet,path::{Component,Path,PathBuf},time::Duration};

const MAX_DIFF_BYTES:usize=60_000;
const MAX_UNDO_BYTES:u64=60_000;

async fn git(root:&Path,args:&[&str])->Result<Vec<u8>,String>{
    let mut cmd=tokio::process::Command::new("git");
    cmd.arg("-C").arg(root).args(args);
    let output=crate::engine::tools::run_command(&mut cmd,Duration::from_secs(12)).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).chars().take(300).collect());
    }
    Ok(output.stdout)
}
fn parse_text(data:Vec<u8>)->Result<String,String>{
    String::from_utf8(data).map_err(|_|"Git 출력이 UTF-8이 아닙니다.".into())
}

pub(crate) async fn baseline(root:&Path)->Value {
    let Ok(root)=root.canonicalize() else {
        return json!({"available":false,"clean":false})
    };
    let Ok(top)=git(&root,&["rev-parse","--show-toplevel"]).await else {
        return json!({"available":false,"clean":false})
    };
    let Ok(top)=parse_text(top) else {return json!({"available":false,"clean":false})};
    if Path::new(top.trim()).canonicalize().ok().as_deref()!=Some(root.as_path()) {
        // Do not revert the parent repository from a nested workspace.
        return json!({"available":false,"clean":false,"reason":"workspace_not_repo_root"});
    }
    let Ok(head)=git(&root,&["rev-parse","--verify","HEAD"]).await else {
        return json!({"available":false,"clean":false})
    };
    let Ok(head)=parse_text(head) else {return json!({"available":false,"clean":false})};
    let Ok(status)=git(&root,&["status","--porcelain","--untracked-files=all"]).await else {
        return json!({"available":false,"clean":false})
    };
    json!({"available":true,"clean":status.is_empty(),"head":head.trim(),
        "capturedAt":chrono::Utc::now().to_rfc3339()})
}

fn relative(root:&Path,raw:&str)->Result<String,String>{
    let source=Path::new(raw);
    if raw.is_empty(){return Err("변경 경로가 비어 있습니다.".into());}
    let relative=if source.is_absolute() {
        source.strip_prefix(root).map_err(|_|"작업 공간 밖 경로입니다.")?
    } else { source };
    if relative.components().any(|c| !matches!(c,Component::Normal(_))) {
        return Err("작업 공간 밖이나 특수 경로는 되돌릴 수 없습니다.".into());
    }
    let resolved=crate::engine::tools::canonical_target(&root.join(relative))
        .map_err(|e|e.to_string())?;
    if !resolved.starts_with(root) {return Err("작업 공간 밖 경로입니다.".into());}
    let text=relative.to_str().ok_or("UTF-8 경로가 필요합니다.")?.replace('\\',"/");
    if text==".git" || text.starts_with(".git/") {return Err("Git 내부 경로는 변경할 수 없습니다.".into());}
    Ok(text)
}

fn paths(root:&Path,goal:&Value)->Vec<String>{
    let mut set=BTreeSet::new();
    for evidence in goal["evidence"].as_array().into_iter().flatten(){
        if !crate::engine::verification::successful_evidence(evidence){continue;}
        if !matches!(crate::engine::verification::tool_name(evidence),
            "workspace_write"|"workspace_replace"|"workspace_replace_range"|
            "workspace_replace_function"|"file_write") {continue;}
        let Some(raw)=evidence["arguments"]["path"].as_str() else {continue};
        if let Ok(path)=relative(root,raw){set.insert(path);}
        if set.len()>=40{break;}
    }
    set.into_iter().collect()
}

async fn current_head(root:&Path)->Result<String,String>{
    Ok(parse_text(git(root,&["rev-parse","--verify","HEAD"]).await?)?.trim().into())
}
async fn tracked_blob_size(root:&Path,head:&str,path:&str)->Result<u64,String>{
    let listing=parse_text(git(root,&["ls-tree","-r",head,"--",path]).await?)?;
    let (metadata,actual)=listing.lines().next().and_then(|line|line.split_once('\t'))
        .ok_or("작업 시작 커밋에 추적된 파일이 없습니다.")?;
    if actual!=path {return Err("Git 파일 경로가 일치하지 않습니다.".into());}
    let mode=metadata.split_whitespace().next().unwrap_or("");
    if !matches!(mode,"100644"|"100755"){
        return Err("Git 원본이 일반 파일이 아닙니다.".into());
    }
    let spec=format!("{head}:{path}");
    parse_text(git(root,&["cat-file","-s",&spec]).await?)?.trim()
        .parse::<u64>().map_err(|_|"Git 원본 크기를 확인할 수 없습니다.".into())
}

pub(crate) async fn preview(state:&AppState,goal:&Value)->Result<Value,String>{
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let recorded=goal["gitBaseline"]["head"].as_str().unwrap_or("");
    let head=current_head(&root).await?;
    let same_head=recorded==head;
    let clean=goal["gitBaseline"]["clean"]==true;
    let mut files=Vec::new();
    for relative in paths(&root,goal){
        let diff=git(&root,&["diff","HEAD","--no-ext-diff","--unified=3","--",&relative]).await;
        let diff=diff.ok().and_then(|bytes|parse_text(bytes).ok()).unwrap_or_default();
        let file=root.join(&relative);
        let (sha,regular)=match std::fs::symlink_metadata(&file){
            Ok(m) if m.is_file()&&m.len()<=MAX_UNDO_BYTES => {
                let bytes=std::fs::read(&file).ok();
                (bytes.as_ref().map(|b|format!("{:x}",Sha256::digest(b))).unwrap_or_default(),true)
            },
            _=>(String::new(),false),
        };
        let tracked=tracked_blob_size(&root,&head,&relative).await
            .is_ok_and(|size|size<=MAX_UNDO_BYTES);
        files.push(json!({"path":relative,"diff":diff.chars().take(MAX_DIFF_BYTES).collect::<String>(),
            "currentSha256":sha,
            "canUndo":clean&&same_head&&regular&&!sha.is_empty()&&tracked,
            "undoReason":if !clean {"baseline_not_clean"} else if !same_head {"head_changed"}
                else if !regular {"missing_or_large_file"} else if !tracked {"untracked_or_unsupported"}
                else {""}}));
    }
    Ok(json!({"goalId":goal["id"],"head":head,"sameHead":same_head,
        "baselineClean":clean,"files":files,
        "notice":"Only source edits with successful receipts are listed. Shared workspace edits cannot be attributed if external software changed the same file."}))
}

/// Owner-only, narrow inverse of the ONE verified current file. It rejects
/// preexisting dirty repositories and moving HEAD. No git reset/clean or
/// command string supplied by the caller is executed.
pub(crate) async fn undo(state:&AppState,goal:&Value,raw:&str,expected:&str)->Result<Value,String>{
    if !matches!(goal["status"].as_str(),
        Some("completed"|"failed"|"needs_review"|"cancelled")) {
        return Err("진행 중인 목표는 먼저 중단 또는 완료해야 되돌릴 수 있습니다.".into());
    }
    if goal["gitBaseline"]["clean"]!=true {
        return Err("작업 시작 전 변경 파일이 있어 안전한 자동 되돌리기를 지원하지 않습니다.".into());
    }
    let root=state.config.workspace.canonicalize().map_err(|e|e.to_string())?;
    let target=relative(&root,raw)?;
    if !paths(&root,goal).iter().any(|p|p==&target) {
        return Err("이 목표의 성공한 수정 도구에 기록되지 않은 파일입니다.".into());
    }
    let head=goal["gitBaseline"]["head"].as_str().ok_or("시작 커밋이 없습니다.")?;
    if current_head(&root).await?!=head {
        return Err("작업 시작 후 HEAD가 변경되어 안전한 복원이 불가능합니다.".into());
    }
    if expected.len()!=64||!expected.bytes().all(|b|b.is_ascii_hexdigit()){
        return Err("미리보기에서 확인한 SHA-256이 필요합니다.".into());
    }
    // Refuse another active DOMINION job before touching shared files.
    if state.store.all("job").iter().any(|job|matches!(job["status"].as_str(),
        Some("running"|"queued"))&&job["kind"]!="learn") {
        return Err("다른 작업이 진행 중입니다. 편집이 끝난 뒤 되돌리세요.".into());
    }
    let file=root.join(&target);
    let metadata=std::fs::symlink_metadata(&file).map_err(|e|e.to_string())?;
    if !metadata.is_file()||metadata.len()>MAX_UNDO_BYTES{
        return Err("작거나 일반적인 파일만 안전하게 복원할 수 있습니다.".into());
    }
    let current=std::fs::read(&file).map_err(|e|e.to_string())?;
    let digest=format!("{:x}",Sha256::digest(&current));
    if digest!=expected.to_ascii_lowercase(){
        return Err("미리보기 이후 내용이 변경됐습니다. 다시 비교해야 합니다.".into());
    }
    let size=tracked_blob_size(&root,head,&target).await?;
    let spec=format!("{head}:{target}");
    if size>MAX_UNDO_BYTES {
        return Err("Git 원본 파일이 복원 크기 제한을 초과합니다.".into());
    }
    let original=git(&root,&["show",&spec]).await?;
    if original.len() as u64!=size{
        return Err("Git 원본 내용이 잘렸습니다. 복원을 중단합니다.".into());
    }
    // Recheck at write time after the Git subprocess returned. Unknown
    // external concurrent editors still cannot be ruled out completely.
    let just_before=std::fs::read(&file).map_err(|e|e.to_string())?;
    if format!("{:x}",Sha256::digest(&just_before))!=digest {
        return Err("복원 직전 파일 내용이 바뀌었습니다.".into());
    }
    std::fs::write(&file,&original).map_err(|e|e.to_string())?;
    let verified=std::fs::read(&file).map_err(|e|e.to_string())?;
    if verified!=original{
        return Err("복원 후 내용 검증에 실패했습니다.".into());
    }
    let receipt=json!({"goalId":goal["id"],"path":target,
        "beforeSha256":digest,"restoredSha256":format!("{:x}",Sha256::digest(&original)),
        "sourceCommit":head,"verified":true,"restoredAt":chrono::Utc::now().to_rfc3339()});
    state.store.audit("goal.file_undo",receipt.clone());
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn clean_git_goal_preview_and_pinned_undo_return_exact_original_bytes(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("goal-diff-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let run=|args:&[&str]|{
            let status=std::process::Command::new("git").arg("-C").arg(&dir)
                .args(args).status().expect("git installed");
            assert!(status.success(),"git {args:?}");
        };
        run(&["init","-q"]);
        std::fs::write(dir.join("a.txt"),"before\\n").unwrap();
        run(&["add","a.txt"]);
        run(&["-c","user.name=Dominion Test","-c","user.email=test@example.com",
            "commit","-qm","initial"]);
        let data_dir=std::env::temp_dir().join(format!("goal-diff-data-{}",uuid::Uuid::new_v4()));
        let cfg=Config{port:0,data_dir:data_dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        std::fs::create_dir_all(&cfg.data_dir).unwrap();
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        // A user-owned untracked file must deny rollback regardless of
        // whether this repository happens to ignore runtime databases.
        std::fs::write(dir.join("untracked-marker.txt"),"user content").unwrap();
        let prior=baseline(&dir).await;
        assert!(prior["available"]==true);
        assert!(prior["clean"]==false,"untracked user files must disable undo");
        std::fs::remove_file(dir.join("untracked-marker.txt")).unwrap();
        let clean=baseline(&dir).await;
        assert_eq!(clean["clean"],true);
        std::fs::write(dir.join("a.txt"),"after\\n").unwrap();
        let goal=json!({"id":"g","status":"completed","gitBaseline":clean,
            "evidence":[{"tool":"workspace_write","arguments":{"path":"a.txt"},
                "result":{"written":true}}]});
        let preview=preview(&state,&goal).await.unwrap();
        assert_eq!(preview["files"][0]["canUndo"],true);
        assert!(preview["files"][0]["diff"].as_str().unwrap().contains("before"));
        let sha=preview["files"][0]["currentSha256"].as_str().unwrap();
        let receipt=undo(&state,&goal,"a.txt",sha).await.unwrap();
        assert_eq!(receipt["verified"],true);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(),"before\\n");
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn path_guard_never_escapes_workspace() {
        let root=std::env::temp_dir().join(format!("goal-git-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root=root.canonicalize().unwrap();
        assert_eq!(relative(&root,"src/main.rs").unwrap(),"src/main.rs");
        for name in ["../secret","/etc/passwd","./src/main.rs",".git/config",""] {
            assert!(relative(&root,name).is_err(),"{name}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
