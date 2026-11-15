//! Real Rust admission, persistence and owner policy tests.
use super::*;
use axum::{extract::State,Json};
use serde_json::json;
use crate::{config::Config,db::Store,vault::Vault};

fn fixture()->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!("dominion-goal-policy-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,root)
}

#[test]
fn reviewer_policy_does_not_skip_final_independent_verification(){
    use super::strategy::ReviewPolicy;
    assert_eq!(ReviewPolicy::parse(None).unwrap(),ReviewPolicy::Always);
    assert_eq!(ReviewPolicy::parse(Some(&json!("always"))).unwrap().as_str(),"always");
    let policy=ReviewPolicy::parse(Some(&json!("acceptance_ready"))).unwrap();
    assert!(!policy.should_review(&["missing file".into()]));
    assert!(policy.should_review(&[]));
    assert!(ReviewPolicy::Always.should_review(&["missing file".into()]));
    for invalid in [json!("fast"),json!(null),json!(false),json!(0),json!({"policy":"always"})]{
        assert!(ReviewPolicy::parse(Some(&invalid)).is_err());
    }
    let catalog=strategy_catalog();
    assert_eq!(catalog["defaultReviewPolicy"],"always");
    assert_eq!(catalog["reviewPolicies"].as_array().unwrap().len(),2);
    assert_eq!(catalog["finalAcceptanceOwner"],"dominion_rust");
}

#[tokio::test]
async fn create_goal_persists_policy_with_atomic_job_and_preserves_default(){
    let (state,root)=fixture();
    let result=crate::api::goals::create_goal(State(state.clone()),
        Json(json!({"goal":"verify a source repair","reviewPolicy":"acceptance_ready",
            "tokenBudget":1000}))).await;
    assert_eq!(result.status(),axum::http::StatusCode::OK);
    let goals=state.store.all("goal");
    assert_eq!(goals.len(),1);
    let goal=&goals[0];
    assert_eq!(goal["reviewPolicy"],"acceptance_ready");
    assert_eq!(goal["status"],"queued");
    assert!(state.store.get("job",goal["jobId"].as_str().unwrap()).is_some());
    let default=crate::api::goals::create_goal(State(state.clone()),
        Json(json!({"goal":"another verified repair"}))).await;
    assert_eq!(default.status(),axum::http::StatusCode::OK);
    assert!(state.store.all("goal").iter().any(|goal|goal["reviewPolicy"]=="always"));
    let before=state.store.all("goal").len();
    let invalid=crate::api::goals::create_goal(State(state.clone()),
        Json(json!({"goal":"invalid","reviewPolicy":"model_says_so"}))).await;
    assert_eq!(invalid.status(),axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(state.store.all("goal").len(),before);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn agent_entry_point_keeps_owner_review_strategy_not_a_model_sdk_policy(){
    let (state,root)=fixture();
    state.store.put("agent",&json!({"id":"agent-1","name":"Coding","providerId":"","model":""}));
    let result=crate::api::agents::run_agent_goal(State(state.clone()),
        axum::extract::Path("agent-1".to_string()),
        Json(json!({"goal":"repair source and test","reviewPolicy":"acceptance_ready"}))).await;
    assert_eq!(result.status(),axum::http::StatusCode::OK);
    let goal=state.store.all("goal").into_iter().next().unwrap();
    assert_eq!(goal["executor"]["agentId"],"agent-1");
    assert_eq!(goal["reviewPolicy"],"acceptance_ready");
    let invalid=crate::api::agents::run_agent_goal(State(state.clone()),
        axum::extract::Path("agent-1".to_string()),
        Json(json!({"goal":"invalid","reviewPolicy":"unchecked"}))).await;
    assert_eq!(invalid.status(),axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(state.store.all("goal").len(),1);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
