//! Goal API state-transition regression tests (not run by this change).
use super::*;
use crate::{config::Config,db::Store,vault::Vault};
use std::path::PathBuf;
fn fixture(status:&str,in_flight:bool)->(AppState,PathBuf){
    let dir=std::env::temp_dir().join(format!("dominion-goal-api-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
    let db=Store::open(&cfg.db_path()).unwrap();
    let vault=Vault::open(&cfg.key_path());
    let state=AppState::new(cfg,db,vault);
    state.store.put("session",&json!({"id":"goal-session"}));
    state.store.put("goal",&json!({"id":"g","status":status,"goal":"수정해","sessionId":"goal-session",
        "maxIterations":16,"maxExecutorTokens":80000,"iterations":2,"jobId":"prior",
        "plan":"1. 원인 점검\n2. 실제 수정\n3. 검증","planHistory":[],"reviewHistory":[],
        "successCriteria":[{"type":"file_exists","path":"output.txt"}]}));
    state.store.put("job",&json!({"id":"prior","kind":"goal","status":"needs_review",
        "queuedAt":"2026-11-14T00:00:00Z","input":{"goalId":"g"},
        "goalState":{"nextIteration":2,"inFlight":in_flight,
            "evidence":[{"tool":"workspace_write","arguments":{"path":"output.txt"},
                "result":{"success":true}}],"transcript":"실행 기록"}}));
    (state,dir)
}
#[tokio::test]
async fn goal_creation_persists_exactly_one_runnable_job_with_its_identity(){
    let (state,dir)=fixture("needs_review",false);
    let goal_count=state.store.count("goal");
    let job_count=state.store.count("job");
    let response=create_goal(State(state.clone()),Json(json!({
        "goal":"Write and verify an actual artifact",
        "executor":{"providerId":"test","model":"mock"},
        "maxIterations":4
    }))).await;
    assert_eq!(response.status(),StatusCode::OK);
    assert_eq!(state.store.count("goal"),goal_count+1);
    assert_eq!(state.store.count("job"),job_count+1);
    let saved=state.store.all("goal").into_iter()
        .find(|row|row["goal"]=="Write and verify an actual artifact").unwrap();
    let id=saved["id"].as_str().unwrap();
    let job_id=saved["jobId"].as_str().unwrap();
    let job=state.store.get("job",job_id).unwrap();
    assert_eq!(job["status"],"queued");
    assert_eq!(job["kind"],"goal");
    assert_eq!(job["input"]["goalId"],id);
    assert_eq!(job["attempts"],0);
    assert!(job["queuedAt"].as_str().is_some());
    drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn queue_write_failure_rolls_back_goal_in_same_transaction(){
    let (state,dir)=fixture("needs_review",false);
    let initial_goals=state.store.count("goal");
    let initial_jobs=state.store.count("job");
    // Deliberately fail only the job insert. Goal INSERT succeeds first, and
    // must still be rolled back when the same transaction is aborted.
    let db=rusqlite::Connection::open(state.config.db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER deny_goal_job_enqueue
        BEFORE INSERT ON records
        WHEN NEW.kind='job'
        BEGIN SELECT RAISE(ABORT,'injected job write failure'); END;").unwrap();
    let response=create_goal(State(state.clone()),Json(json!({
        "goal":"This goal must never appear without its executable job"
    }))).await;
    assert_eq!(response.status(),StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(state.store.count("goal"),initial_goals);
    assert_eq!(state.store.count("job"),initial_jobs);
    db.execute_batch("DROP TRIGGER deny_goal_job_enqueue;").unwrap();
    drop(db);drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn incomplete_side_effect_requires_owner_acknowledgement(){
    let (state,dir)=fixture("needs_review",true);
    let response=resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"resolutionNote":"확인했습니다"}))).await;
    assert_eq!(response.status(),StatusCode::BAD_REQUEST);
    assert_eq!(state.store.count("job"),1);
    assert_eq!(state.store.get("goal","g").unwrap()["status"],"needs_review");
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn resumed_goal_skips_uncertain_iteration_and_preserves_evidence(){
    let (state,dir)=fixture("needs_review",true);
    let response=resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"acknowledgeExternalEffects":true,
            "resolutionNote":"이전 작업의 외부 결과와 생성 파일을 직접 확인했습니다."}))).await;
    assert_eq!(response.status(),StatusCode::ACCEPTED);
    let goal=state.store.get("goal","g").unwrap();
    assert_eq!(goal["status"],"queued");
    assert_eq!(goal["resumeState"]["nextIteration"],3);
    assert_eq!(goal["resumeState"]["inFlight"],false);
    assert_eq!(goal["resumeState"]["evidence"].as_array().unwrap().len(),1);
    assert_eq!(state.store.count("job"),2);
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn owner_plan_edits_are_durable_but_cannot_change_a_running_goal(){
    let (state,dir)=fixture("needs_review",false);
    let response=edit_plan(State(state.clone()),Path("g".into()),Json(json!({
        "plan":"1. 구조를 다시 살펴본다\n2. 수정한다\n3. 검증한다"
    }))).await;
    assert_eq!(response.status(),StatusCode::OK);
    let goal=state.store.get("goal","g").unwrap();
    assert_eq!(goal["planHistory"].as_array().unwrap().len(),1);
    state.store.patch_if("goal","g","id",&json!("g"),&json!({"status":"running"}));
    let denied=edit_plan(State(state.clone()),Path("g".into()),Json(json!({"plan":"무단 변경"}))).await;
    assert_eq!(denied.status(),StatusCode::CONFLICT);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn goal_approval_scope_includes_children_but_excludes_other_rooms(){
    let (state,dir)=fixture("needs_review",false);
    state.store.put("session",&json!({"id":"child","parentSessionId":"goal-session",
        "delegated":true}));
    state.store.put("session",&json!({"id":"other","parentSessionId":"unrelated",
        "delegated":true}));
    assert!(belongs_to_goal(&state,"goal-session","goal-session"));
    assert!(belongs_to_goal(&state,"child","goal-session"));
    assert!(!belongs_to_goal(&state,"other","goal-session"));
    assert!(!belongs_to_goal(&state,"unknown","goal-session"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn queued_goal_cancel_immediately_sets_parent_status(){
    let (state,dir)=fixture("queued",false);
    state.store.patch_if("job","prior","status",&json!("needs_review"),
        &json!({"status":"queued"}));
    let stopped=crate::api::jobs::cancel(State(state.clone()),Path("prior".into())).await;
    assert_eq!(stopped.status(),StatusCode::OK);
    assert_eq!(state.store.get("goal","g").unwrap()["status"],"cancelled");
    assert_eq!(state.store.get("job","prior").unwrap()["status"],"cancelled");
    std::fs::remove_dir_all(dir).unwrap();
}
#[tokio::test]
async fn cancellation_can_be_resumed_only_after_old_job_is_terminal(){
    let (state,dir)=fixture("cancelled",false);
    state.cancel_session("goal-session");
    let response=resume_goal(State(state.clone()),Path("g".into()),
        Json(json!({"resolutionNote":"작업 상태를 확인했습니다."}))).await;
    assert_eq!(response.status(),StatusCode::ACCEPTED);
    assert!(!state.is_cancelled("goal-session"));
    std::fs::remove_dir_all(dir).unwrap();
}
