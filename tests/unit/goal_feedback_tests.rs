use super::*;
use crate::{config::Config,db::Store,state::AppState,vault::Vault};
fn state()->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-goal-feedback-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,root)
}
#[test]
fn goal_receipt_preserves_rsi_evaluation_boundary_and_reuses_job_id(){
    let (app,dir)=state();
    let goal=json!({
        "id":"g","status":"completed","iterations":3,"isolateWorktree":true,
        "finalReport":{
            "accepted":true,"successfulToolEvidence":4,"sessionUsage":{"totalTokens":2451},
            "gaps":[],"artifacts":[{"path":"src/engine/goals.rs","bytes":12}],
            "checks":[{"path":"src/engine/goals.rs","passed":true}],
            "gitChanges":{"changedFiles":[{"path":"src/engine/goals.rs","change":"M"},
                {"path":"../../escape","change":"M"}]}
        }
    });
    record(&app,"job-1",&goal);
    record(&app,"job-1",&goal);
    assert_eq!(app.store.count("goal_receipt"),1);
    let receipt=app.store.get("goal_receipt","goal-receipt-job-1").unwrap();
    assert_eq!(receipt["verifiedCompletion"],true);
    assert_eq!(receipt["independentRsiGain"],false);
    assert_eq!(receipt["sourcePathsObserved"],json!(["src/engine/goals.rs"]));
    let context=context(&app,&json!(["src/engine/goals.rs"]));
    assert!(context.contains("owner_goal_execution_v1"));
    assert!(context.contains("independentRsiGain"));
    assert!(!context.contains("../../escape"));
    assert!(!context.contains("private-user-request"));
    assert!(super::context(&app,&json!(["src/engine/unrelated.rs"])).is_empty());
    drop(app);std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn failed_goals_do_not_create_false_success_or_rsi_adoption(){
    let (app,dir)=state();
    record(&app,"job-fail",&json!({"id":"g2","status":"needs_review",
        "finalReport":{"accepted":false,"gaps":["missing effect proof"],"artifacts":[]}}));
    let receipt=app.store.get("goal_receipt","goal-receipt-job-fail").unwrap();
    assert_eq!(receipt["verifiedCompletion"],false);
    assert_eq!(receipt["independentPairedEvaluation"],false);
    assert_eq!(receipt["independentRsiGain"],false);
    drop(app);std::fs::remove_dir_all(dir).unwrap();
}
