use super::*;
use std::process::Command as StdCommand;

fn run(root:&Path,args:&[&str]){
    let output=StdCommand::new("git").current_dir(root).args(args).output().unwrap();
    assert!(output.status.success(),"git {:?}: {}",args,String::from_utf8_lossy(&output.stderr));
}
#[tokio::test]
async fn reviewed_digest_controls_fast_forward_and_source_protection(){
    let root=std::env::temp_dir().join(format!("dominion-isolated-apply-{}",uuid::Uuid::new_v4()));
    let source=root.join("src");let data=root.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    run(&source,&["init","-q"]);
    run(&source,&["config","user.name","Goal Test"]);
    run(&source,&["config","user.email","goals@example.invalid"]);
    std::fs::write(source.join("a.txt"),"before\n").unwrap();
    run(&source,&["add","."]);run(&source,&["commit","-qm","base"]);
    let cfg=crate::config::Config{port:0,data_dir:data,web_dir:root.join("web"),workspace:source.clone()};
    let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
        crate::vault::Vault::open(&cfg.key_path()));
    let goal_id=uuid::Uuid::new_v4().to_string();
    state.store.put("goal",&json!({"id":goal_id,"status":"queued","isolateWorktree":true})).unwrap();
    prepare(&state,&goal_id).await.unwrap();
    let child=state.store.put("session",&json!({"id":"child","goalId":goal_id})).unwrap();
    state.store.put("session",&json!({"id":"grandchild","parentSessionId":child["id"],"delegated":true})).unwrap();
    let work=workspace_for_session(&state,"grandchild").unwrap();
    assert_ne!(work,source.canonicalize().unwrap());
    std::fs::write(work.join("a.txt"),"after\n").unwrap();
    std::fs::write(work.join("b.txt"),"new-file\n").unwrap();
    assert_eq!(std::fs::read_to_string(source.join("a.txt")).unwrap(),"before\n");
    let first=changes(&state,&goal_id).await.unwrap();
    assert!(first["diff"].as_str().unwrap().contains("after"));
    assert_eq!(first["untrackedPreviews"][0]["content"],"new-file\n");
    let digest=first["reviewDigest"].as_str().unwrap().to_owned();
    std::fs::write(work.join("a.txt"),"tampered\n").unwrap();
    let record=state.store.get("goal",&goal_id).unwrap();
    state.store.patch_if("goal",&goal_id,"status",&record["status"],
        &json!({"status":"completed"})).unwrap();
    assert!(apply(&state,&goal_id,&digest).await.is_err());
    assert_eq!(std::fs::read_to_string(source.join("a.txt")).unwrap(),"before\n");
    std::fs::write(work.join("a.txt"),"after\n").unwrap();
    let reviewed=changes(&state,&goal_id).await.unwrap();
    assert_eq!(reviewed["reviewDigest"],digest);
    let applied=apply(&state,&goal_id,&digest).await.unwrap();
    assert_eq!(applied["sourceFastForwarded"],true);
    assert_eq!(std::fs::read_to_string(source.join("a.txt")).unwrap(),"after\n");
    assert_eq!(std::fs::read_to_string(source.join("b.txt")).unwrap(),"new-file\n");
    assert!(apply(&state,&goal_id,&digest).await.is_err());
    assert_eq!(state.store.get("goal",&goal_id).unwrap()["gitWorktree"]["applied"],true);
    drop(state);
    let _=std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn worktree_discard_preserves_original_branch_and_stops_workspace_fallback(){
    let root=std::env::temp_dir().join(format!("dominion-isolated-discard-{}",uuid::Uuid::new_v4()));
    let source=root.join("src");let data=root.join("data");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    run(&source,&["init","-q"]);run(&source,&["config","user.name","Test"]);
    run(&source,&["config","user.email","test@example.invalid"]);
    std::fs::write(source.join("safe.txt"),"original").unwrap();
    run(&source,&["add","."]);run(&source,&["commit","-qm","base"]);
    let cfg=crate::config::Config{port:0,data_dir:data,web_dir:root.join("web"),workspace:source.clone()};
    let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),
        crate::vault::Vault::open(&cfg.key_path()));
    let id=uuid::Uuid::new_v4().to_string();
    state.store.put("goal",&json!({"id":id,"status":"queued","isolateWorktree":true})).unwrap();
    assert!(workspace_for_goal(&state,&state.store.get("goal",&id).unwrap()).is_err());
    prepare(&state,&id).await.unwrap();
    let work=workspace_for_goal(&state,&state.store.get("goal",&id).unwrap()).unwrap();
    std::fs::write(work.join("safe.txt"),"modified").unwrap();
    let goal=state.store.get("goal",&id).unwrap();
    state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"completed"})).unwrap();
    rollback(&state,&id).await.unwrap();
    assert_eq!(std::fs::read_to_string(source.join("safe.txt")).unwrap(),"original");
    assert!(workspace_for_goal(&state,&state.store.get("goal",&id).unwrap()).is_err());
    drop(state);
    let _=std::fs::remove_dir_all(root);
}
