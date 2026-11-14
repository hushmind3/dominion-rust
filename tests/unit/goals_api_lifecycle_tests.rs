use super::*;
use crate::{config::Config, db::Store, state::AppState, vault::Vault};
use axum::{extract::{Path,State},Json};
use serde_json::json;

fn state()->(AppState,std::path::PathBuf){
    let dir=std::env::temp_dir().join(format!("dominion-goal-api-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,dir)
}

#[tokio::test]
async fn goal_creation_plan_change_and_owner_cancel_are_real_job_changes(){
    let (state,dir)=state();
    let response=create_goal(State(state.clone()),Json(json!({
        "goal":"write report.txt","tokenBudget":4000,
        "successCriteria":[{"type":"file_exists","path":"report.txt"}]
    }))).await;
    assert_eq!(response.status(),StatusCode::OK);
    let goal=state.store.all("goal").pop().unwrap();
    let id=goal["id"].as_str().unwrap().to_string();
    let job_id=format!("goal-job-{id}");
    assert_eq!(goal["jobId"],job_id);
    assert_eq!(goal["tokenBudget"],4000);
    assert_eq!(state.store.get("job",&job_id).unwrap()["status"],"queued");
    let updated=edit_plan(State(state.clone()),Path(id.clone()),Json(json!({"plan":"1. Inspect. 2. Edit. 3. Verify."}))).await;
    assert_eq!(updated.status(),StatusCode::OK);
    assert!(state.store.get("goal",&id).unwrap()["plan"].as_str().unwrap().contains("Inspect"));
    let cancelled=cancel_goal(State(state.clone()),Path(id.clone())).await;
    assert_eq!(cancelled.status(),StatusCode::OK);
    assert_eq!(state.store.get("goal",&id).unwrap()["status"],"cancelled");
    assert_eq!(state.store.get("job",&job_id).unwrap()["cancelRequested"],true);
    assert_eq!(edit_plan(State(state.clone()),Path(id),Json(json!({"plan":"retry"}))).await.status(),StatusCode::CONFLICT);
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn reviewed_resume_is_new_job_and_preserves_provenance(){
    let (state,dir)=state();
    state.store.put("goal",&json!({"id":"g","goal":"fix code","status":"needs_review",
        "jobId":"old","sessionId":"s","iterations":2,"maxIterations":2,
        "plan":"Fix and verify","evidence":[{"tool":"workspace_read","result":{"ok":true}}]}));
    assert_eq!(resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"acknowledgeUnknownEffects":false}))).await.status(),StatusCode::BAD_REQUEST);
    let resumed=resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"acknowledgeUnknownEffects":true}))).await;
    assert_eq!(resumed.status(),StatusCode::OK);
    let goal=state.store.get("goal","g").unwrap();
    assert_eq!(goal["status"],"queued");
    assert_eq!(goal["jobId"],"goal-job-g-resume-1");
    assert_eq!(goal["sessionId"],"s");
    assert_eq!(goal["plan"],"Fix and verify");
    assert_eq!(goal["evidence"].as_array().unwrap().len(),1);
    assert_eq!(goal["resumeHistory"][0]["previousJobId"],"old");
    assert_eq!(state.store.get("job","goal-job-g-resume-1").unwrap()["input"]["resumeAfterReview"],true);
    assert_eq!(resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"acknowledgeUnknownEffects":true}))).await.status(),StatusCode::CONFLICT);
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}
