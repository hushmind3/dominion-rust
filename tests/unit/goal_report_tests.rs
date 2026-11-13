use super::*;
use crate::{config::Config, db::Store, state::AppState, vault::Vault};
use serde_json::json;

fn fixture()->(AppState,std::path::PathBuf){
    let path=std::env::temp_dir().join(format!("dominion-goal-report-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    let config=Config{port:0,data_dir:path.clone(),web_dir:path.join("web"),workspace:path.clone()};
    let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
    (state,path)
}

#[test]
fn tool_claims_do_not_replace_final_file_proof(){
    let (state,dir)=fixture();
    let output=dir.join("done.txt");
    let checks=json!([{"type":"file_equals","path":"done.txt","content":"finished"}]);
    let goal=json!({"goal":"write done.txt","acceptanceChecks":checks,
        "evidence":[{"tool":"workspace_write","result":{"written":true},"arguments":{"path":"done.txt"}}]});
    let before=build(&state,&goal);
    assert_eq!(before["accepted"],false);
    std::fs::write(&output,"finished").unwrap();
    let after=build(&state,&goal);
    assert_eq!(after["accepted"],true);
    assert_eq!(after["checks"][0]["passed"],true);
    assert_eq!(after["artifacts"][0]["bytes"],8);
    assert_eq!(after["artifacts"][0]["sha256"].as_str().unwrap().len(),64);
    std::fs::write(&output,"regressed").unwrap();
    assert_eq!(build(&state,&goal)["accepted"],false);
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn read_only_activity_cannot_certify_an_actionable_goal(){
    let (state,dir)=fixture();
    let goal=json!({"goal":"create a report","acceptanceChecks":[],
        "evidence":[{"tool":"workspace_read","result":{"content":"ok"}}]});
    assert_eq!(build(&state,&goal)["accepted"],false);
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}
