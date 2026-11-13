use super::*;
use crate::{config::Config, db::Store, state::AppState, vault::Vault};
use serde_json::json;

fn fixture()->(AppState,std::path::PathBuf){
    let dir=std::env::temp_dir().join(format!("dominion-delegate-evidence-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,dir)
}
#[test]
fn completed_delegate_patch_is_parent_action_only_with_real_child_receipt(){
    let (state,dir)=fixture();
    std::fs::write(dir.join("main.rs"),"old").unwrap();
    let checks=baseline_checks(&state,vec![json!({"type":"file_changed","path":"main.rs"})]);
    let delegate=json!({"tool":"delegate_task","result":{"status":"completed","operations":[
        {"tool":"workspace_replace","arguments":{"path":"main.rs"},"result":{"written":true,"replaced":true}}
    ]}});
    // A valid child receipt alone does not bypass the filesystem contract.
    assert!(!acceptance_gaps(&state,"edit main.rs",&checks,&[delegate.clone()]).is_empty());
    std::fs::write(dir.join("main.rs"),"new").unwrap();
    assert!(acceptance_gaps(&state,"edit main.rs",&checks,&[delegate.clone()]).is_empty());
    assert!(is_action_evidence(&delegate));
    let fail=json!({"tool":"delegate_task","result":{"status":"completed","operations":[
        {"tool":"workspace_replace_range","result":{"written":false,"unchanged":true}}
    ]}});
    assert!(!is_action_evidence(&fail));
    let queued=json!({"tool":"delegate_task","result":{"status":"queued","operations":[
        {"tool":"workspace_write","result":{"written":true}}
    ]}});
    assert!(!is_action_evidence(&queued));
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn delegated_test_must_be_the_exact_successful_command(){
    let (state,dir)=fixture();
    let child=json!({"tool":"delegate_task","result":{"status":"completed","operations":[
        {"tool":"terminal_exec","arguments":{"command":"cargo","args":["test"]},
            "result":{"exitCode":0}},
    ]}});
    let checks=json!([{"type":"command_succeeded","command":"cargo test"},
        {"type":"tool_succeeded","tool":"terminal_exec"}]);
    assert!(acceptance_gaps(&state,"run tests",&checks,&[child.clone()]).is_empty());
    let bad=json!({"tool":"delegate_task","result":{"status":"completed","operations":[
        {"tool":"terminal_exec","arguments":{"command":"cargo","args":["test"]},
            "result":{"exitCode":1}},
    ]}});
    assert!(!acceptance_gaps(&state,"run tests",&checks,&[bad]).is_empty());
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}
