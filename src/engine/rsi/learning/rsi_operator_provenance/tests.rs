use super::*;
use crate::{config::Config,db::Store,vault::Vault};

fn state()->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("rsi-op-provenance-{}",uuid::Uuid::new_v4()));
    let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),
        workspace:root.clone()};
    let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
        Vault::open(&config.key_path()));
    (state,root)
}

#[test]
fn no_job_or_model_tool_writes_means_no_operator_training(){
    let (state,root)=state();
    let forged=json!({"jobId":"missing-worker","candidateId":"fake",
        "operatorExecution":{"source":"verified_compiled_operator_tool_edit_v1"}});
    assert!(observe(&state,&forged).is_null());
    assert!(!verified(&state,&forged,&json!({"name":"invented"})));
    let _=std::fs::remove_dir_all(root);
}

#[test]
fn false_execution_claim_cannot_replace_compiled_worker_dispatch(){
    let (state,root)=state();
    let commit="a".repeat(40);
    state.store.put("job",&json!({"id":"worker",
        "input":{"rsiImprovementAlgebra":{"selected":{"ruleId":"b".repeat(64)}}},
        "rsiCompiledPlanExecuted":{"source":"running_compiled_descendant_binary",
            "generatorCommit":commit}})).unwrap();
    let fake=json!({"jobId":"worker","generatorCommit":commit,
        "operatorExecution":{"source":"verified_compiled_operator_tool_edit_v1",
            "ruleId":"b".repeat(64)},"candidateId":"fake"});
    assert!(observe(&state,&fake).is_null());
    assert!(dispatch(&state,&json!({}),"a",false).unwrap().is_null());
    let _=std::fs::remove_dir_all(root);
}
