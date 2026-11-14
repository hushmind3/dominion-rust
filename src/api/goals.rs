use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_goals(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("goal")))
}

pub async fn get_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("goal", &id) {
        Some(goal) => Json(goal).into_response(),
        None => error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다."),
    }
}

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"실행 중인 목표는 작업을 먼저 중단하세요.");
    }
    if goal["isolateWorktree"]==true
        && goal["gitWorktree"].is_object()
        && goal["gitWorktree"]["rolledBack"]!=true {
        return error(StatusCode::CONFLICT,
            "격리 작업공간이 남아 있습니다. 먼저 변경을 검토하고 폐기하세요.");
    }
    state.store.remove("goal",&id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
}

pub async fn create_goal(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "목표 내용이 필요합니다.");
    }
    let criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Err(message) = engine::goals::validate_criteria(&criteria) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    if body.get("isolateWorktree").is_some_and(|value|!value.is_boolean()){
        return error(StatusCode::BAD_REQUEST,"Git 격리 옵션은 참/거짓으로 지정해야 합니다.");
    }
    let executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    if body.get("tokenBudget").is_some_and(|value|
        value.as_u64().is_none_or(|n| n > 2_000_000)) {
        return error(StatusCode::BAD_REQUEST, "목표 토큰 상한은 0~200만으로 입력하세요.");
    }
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(16));

    for key in ["executor","reviewer","planner"] { if body.get(key).is_some_and(|v|!v.is_object()){return error(StatusCode::BAD_REQUEST,"실행자·검토자·계획자는 객체로 지정하세요.");} }
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    // An autonomous goal and its runnable worker are ONE durable intent.
    // Never expose a queued goal whose job is absent, or a worker with no
    // matching goal. The initial enqueue used three independent DB writes;
    // process death between any two could orphan a goal or strand cancellation.
    let goal_id=uuid::Uuid::new_v4().to_string();
    let job_id=uuid::Uuid::new_v4().to_string();
    let queued_at=chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true);
    let goal=json!({
        "id":goal_id,
        "taskId":uuid::Uuid::new_v4().to_string(),
        "jobId":job_id,
        "goal":text,
        "successCriteria":criteria,
        "isolateWorktree":body["isolateWorktree"].as_bool().unwrap_or(false),
        "executor":executor,
        "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
        "planner":body.get("planner").cloned().unwrap_or(json!({})),
        "tokenBudget":body["tokenBudget"].as_u64().unwrap_or(150_000),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "replans":0,"planHistory":[],"reviewHistory":[],
        "status":"queued","iterations":0,"maxIterations":max_iterations,
        "createdAt":chrono::Utc::now().to_rfc3339(),
    });
    let job=json!({
        "id":job_id,
        "kind":"goal","status":"queued","queuedAt":queued_at,
        "input":{"goalId":goal_id},"attempts":0,
        "stepIndex":0,"stepTotal":0,"events":[],
    });
    let records=vec![("goal".to_string(),goal.clone()),("job".to_string(),job.clone())];
    if state.store.transaction(&records).is_err(){
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표와 작업을 원자적으로 등록하지 못했습니다.");
    }
    // Publish only after COMMIT. Event consumers can immediately resolve both
    // records with their finalized cross-references.
    state.bus.record("goal",&goal);
    state.bus.record("job",&job);
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id,"goalId":goal_id}));
    Json(goal).into_response()
}

/// Cancel the durable goal and its worker, not just the frontend button.
pub async fn cancel_goal(
    State(state): State<AppState>, Path(id): Path<String>,
) -> Response {
    let Some(goal) = state.store.get("goal", &id) else {
        return error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(), Some("queued" | "running")) {
        return error(StatusCode::CONFLICT, "이미 종료된 목표는 중단할 수 없습니다.");
    }
    let Some(saved) = state.store.patch_if("goal", &id, "status", &goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
            "finishedAt":chrono::Utc::now().to_rfc3339()})) else {
        return error(StatusCode::CONFLICT, "목표 상태가 변경됐습니다.");
    };
    state.bus.record("goal", &saved);
    if let Some(session) = saved["sessionId"].as_str() {
        state.cancel_session(session);
    }
    // The durable job must also reject any in-flight checkpoint or restart.
    for job in state.store.all("job").into_iter().filter(|job| {
        job["kind"] == "goal" && job["input"]["goalId"] == id
            && matches!(job["status"].as_str(), Some("queued" | "running"))
    }) {
        if let Some(job_id) = job["id"].as_str() {
            if let Some(cancelled) = state.store.patch_if("job", job_id, "status",
                &job["status"], &json!({"status":"cancelled","cancelRequested":true,
                "finishedAt":chrono::Utc::now().to_rfc3339()})) {
                state.bus.record("job", &cancelled);
            }
        }
    }
    state.store.audit("goal.owner_cancelled", json!({"goalId":id}));
    Json(saved).into_response()
}

/// Owner-reviewed continuation. A fresh durable job starts at the last
/// *completed* iteration, carrying only persisted evidence and plan history.
/// Unknown external effects are never replayed automatically.
pub async fn resume_goal(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>,
) -> Response {
    if body.as_object().is_none_or(|fields|
        fields.len()!=1 || body["acknowledgeUnknownEffects"]!=true) {
        return error(StatusCode::BAD_REQUEST,
            "이전 외부 작업을 확인했으면 acknowledgeUnknownEffects=true를 제출하세요.");
    }
    let Some(goal) = state.store.get("goal", &id) else {
        return error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(), Some("needs_review" | "failed")) {
        return error(StatusCode::CONFLICT, "검토 대기 또는 실패한 목표만 이어갈 수 있습니다.");
    }
    if state.store.all("job").iter().any(|job| {
        job["kind"]=="goal" && job["input"]["goalId"]==id
            && matches!(job["status"].as_str(), Some("queued"|"running"))
    }) {
        return error(StatusCode::CONFLICT, "이전 목표 작업이 아직 실행 중입니다.");
    }
    let iterations = goal["iterations"].as_u64().unwrap_or(0);
    let max = goal["maxIterations"].as_u64().unwrap_or(16).min(64);
    let next_max = if max<=iterations {max.saturating_add(8).min(64)} else {max};
    if next_max<=iterations {
        return error(StatusCode::CONFLICT, "목표 최대 반복 횟수(64회)에 도달했습니다.");
    }
    let next = goal["resumeCount"].as_u64().unwrap_or(0)+1;
    if next>12 {
        return error(StatusCode::CONFLICT, "사용자 승인 재개 한도(12회)에 도달했습니다.");
    }
    let prior_job=goal["jobId"].as_str().and_then(|job_id|state.store.get("job",job_id))
        .unwrap_or(Value::Null);
    // If a tool was in flight, the owner sees the uncertainty and a new
    // executor turn starts AFTER the last committed iteration. The old job
    // checkpoint cannot be silently replayed by the new worker.
    let prior_state=&prior_job["goalState"];
    let next_iteration=iterations.max(prior_state["nextIteration"].as_u64().unwrap_or(0));
    let checkpoint=json!({
        "nextIteration":next_iteration,
        "evidence":goal.get("evidence").cloned().unwrap_or_else(||json!([])),
        "transcript":prior_state["transcript"].as_str().unwrap_or(""),
        "gaps":goal.get("gaps").cloned().unwrap_or_else(||json!([])),
        "stalled":prior_state["stalled"].as_u64().unwrap_or(0),
        "priorInFlight":prior_state["inFlight"]==true,
        "readOnlyReinspection":true,
    });
    let job_id=format!("goal-job-{id}-resume-{next}");
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"jobId":job_id,"previousJobId":goal["jobId"],
        "previousStatus":goal["status"],"startIteration":next_iteration,
        "acknowledgedBy":"owner","at":chrono::Utc::now().to_rfc3339()}));
    // Atomic adoption: a restarted process must never observe a queued goal
    // without the matching durable worker, or a worker without its checkpoint.
    let mut updated=goal.clone();
    let patch=json!({
        "status":"queued","phase":"resume","jobId":job_id,"resumeCount":next,
        "resumeHistory":history,"resumeCheckpoint":checkpoint,"maxIterations":next_max,
        "finishedAt":null,"metrics":null,"error":null
    });
    for (key,value) in patch.as_object().expect("resume patch object") {
        updated[key]=value.clone();
    }
    let queued_job=json!({
        "id":job_id,"kind":"goal","input":{"goalId":id,"resumeAfterReview":true},
        "status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    match state.store.transaction_if_status(
        "goal",&id,goal["status"].as_str().unwrap_or(""),
        &[("goal".to_string(),updated.clone()),("job".to_string(),queued_job.clone())],
    ) {
        Ok(true)=>{},
        Ok(false)=>return error(StatusCode::CONFLICT,"목표가 다른 작업에서 변경됐습니다."),
        Err(_)=>return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표 및 재개 작업을 함께 저장하지 못했습니다."),
    }
    let saved=state.store.get("goal",&id).unwrap_or(updated);
    if let Some(job)=state.store.get("job",&job_id) {
        state.bus.record("job",&job);
    }
    if let Some(session) = saved["sessionId"].as_str() {
        state.clear_cancel(session);
    }
    state.bus.record("goal", &saved);
    state.store.audit("goal.owner_reviewed_resume",json!({
        "goalId":id,"jobId":job_id,"startIteration":next_iteration,
        "previousJobId":goal["jobId"],"priorInFlight":prior_state["inFlight"]==true
    }));
    Json(saved).into_response()
}

pub async fn update_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(plan)=body["plan"].as_str().map(str::trim)
        .filter(|p|p.len()>=5&&p.len()<=6000) else {
        return error(StatusCode::BAD_REQUEST,"수정 계획은 5~6000바이트로 입력하세요.");
    };
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return error(StatusCode::CONFLICT,"진행 중인 목표의 계획만 수정할 수 있습니다.");
    }
    // The owner may change the method, never the acceptance checks, route
    // permissions or the authority of an already running tool invocation.
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"planOverride":plan,"planOverrideRequestedAt":chrono::Utc::now().to_rfc3339()})) else {
        return error(StatusCode::CONFLICT,"작업 상태가 변경됐습니다. 새로고침 후 다시 입력하세요.");
    };
    state.bus.record("goal",&saved);
    state.store.audit("goal.plan_override_requested",json!({"goalId":id}));
    Json(saved).into_response()
}

pub async fn changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    let result=if goal["isolateWorktree"]==true {
        engine::goal_workspace::changes(&state,&goal).await
    } else {
        engine::goal_changes::preview(&state,&goal).await
    };
    match result {
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Deliberate owner-initiated revert of one exact, previewed file; no reset,
/// clean, bulk checkout or untracked-file deletion is permitted.
pub async fn undo_change(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["isolateWorktree"]==true {
        return error(StatusCode::CONFLICT,
            "격리 목표는 원본 파일을 되돌리지 않습니다. 검토 후 격리 작업공간 폐기를 선택하세요.");
    }
    let Some(path)=body["path"].as_str() else {
        return error(StatusCode::BAD_REQUEST,"되돌릴 파일 경로가 필요합니다.");
    };
    let Some(sha)=body["expectedSha256"].as_str() else {
        return error(StatusCode::BAD_REQUEST,"미리보기에서 확인한 파일 SHA-256이 필요합니다.");
    };
    match engine::goal_changes::undo(&state,&goal,path,sha).await {
        Ok(mut receipt)=>{
            // A previously completed goal is no longer fulfilled after
            // its verified source edit is deliberately undone by the owner.
            // The historical job receipt remains intact; the live goal and
            // its root outcome must reflect the new workspace state.
            if let Some(current)=state.store.get("goal",&id) {
                let mut metrics=current["metrics"].clone();
                if metrics.is_object() {
                    metrics["verifiedCompleted"]=json!(false);
                    metrics["ownerUndoCount"]=json!(
                        metrics["ownerUndoCount"].as_u64().unwrap_or(0)+1
                    );
                }
                let note=format!("사용자가 {} 파일을 되돌렸습니다. 완료 조건을 다시 확인하세요.",path);
                let mut gaps=current["gaps"].as_array().cloned().unwrap_or_default();
                gaps.push(json!(note));
                let patch=json!({"status":"needs_review","phase":"owner_undo",
                    "metrics":metrics,"gaps":gaps,"lastUndo":receipt});
                if let Some(saved)=state.store.patch_if("goal",&id,"status",&current["status"],&patch) {
                    state.bus.record("goal",&saved);
                    if let Some(task_id)=saved["taskId"].as_str(){
                        engine::episode::set_root_outcome(&state,task_id,
                            engine::episode::goal_outcome("needs_review"),
                            &gaps.iter().filter_map(Value::as_str).map(str::to_owned)
                                .collect::<Vec<_>>());
                    }
                    receipt["goalStatus"]=json!("needs_review");
                }
            }
            Json(receipt).into_response()
        },
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

pub async fn discard_worktree(
    State(state):State<AppState>,Path(id):Path<String>,
)->Response{
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["isolateWorktree"]!=true {
        return error(StatusCode::BAD_REQUEST,"이 목표는 격리 작업공간을 사용하지 않습니다.");
    }
    match engine::goal_workspace::discard(&state,&goal).await {
        Ok(receipt)=>{
            // A promoted patch is already present in the owner checkout:
            // deleting its disposable worktree must NOT invalidate success.
            if goal["promotion"]["applied"]!=true {
            if let Some(current)=state.store.get("goal",&id) {
                let mut metrics=current["metrics"].clone();
                if metrics.is_object(){
                    metrics["verifiedCompleted"]=json!(false);
                    metrics["ownerUndoCount"]=json!(metrics["ownerUndoCount"].as_u64().unwrap_or(0)+1);
                }
                let gaps=vec!["사용자가 격리 작업공간을 폐기하여 결과 소스가 더 이상 존재하지 않습니다.".to_string()];
                if let Some(saved)=state.store.patch_if("goal",&id,"status",&current["status"],
                    &json!({"status":"needs_review","phase":"worktree_discarded",
                        "metrics":metrics,"gaps":gaps})){
                    state.bus.record("goal",&saved);
                    if let Some(task_id)=saved["taskId"].as_str() {
                        engine::episode::set_root_outcome(&state,task_id,
                            engine::episode::goal_outcome("needs_review"),&gaps);
                    }
                }
            }
            }
            Json(receipt).into_response()
        },
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}
pub async fn promote_worktree(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["isolateWorktree"]!=true {
        return error(StatusCode::BAD_REQUEST,"격리 작업공간을 사용하지 않는 목표입니다.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else {
        return error(StatusCode::BAD_REQUEST,
            "변경 미리보기의 SHA-256과 명시적 승인 내용이 필요합니다.");
    };
    match engine::goal_workspace::promote(&state,&goal,digest).await {
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}


#[cfg(test)]
mod sub1_goal_lifecycle_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state() -> (AppState,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("sub1-goal-resume-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        (state,dir)
    }
    #[tokio::test]
    async fn reviewed_resume_is_idempotent_and_carries_only_committed_evidence() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("goal",&json!({"id":"g","goal":"complete task",
            "status":"needs_review","phase":"needs_review","sessionId":"s",
            "iterations":4,"maxIterations":4,"jobId":"prior","evidence":[{"tool":"test"}],
            "gaps":["check external effects"],"successCriteria":[{"type":"file_exists","path":"a"}]}));
        state.store.put("job",&json!({"id":"prior","kind":"goal",
            "input":{"goalId":"g"},"status":"needs_review",
            "goalState":{"nextIteration":4,"inFlight":true,
                "transcript":"previous completion","stalled":2}}));
        let missing_ack=resume_goal(State(state.clone()),Path("g".into()),
            Json(json!({"acknowledgeUnknownEffects":false}))).await;
        assert_eq!(missing_ack.status(),StatusCode::BAD_REQUEST);
        let resumed=resume_goal(State(state.clone()),Path("g".into()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(resumed.status(),StatusCode::OK);
        let goal=state.store.get("goal","g").unwrap();
        assert_eq!(goal["status"],"queued");
        assert_eq!(goal["phase"],"resume");
        assert_eq!(goal["maxIterations"],12);
        assert_eq!(goal["resumeCount"],1);
        assert_eq!(goal["resumeCheckpoint"]["nextIteration"],4);
        assert_eq!(goal["resumeCheckpoint"]["priorInFlight"],true);
        assert_eq!(goal["resumeCheckpoint"]["evidence"][0]["tool"],"test");
        assert_eq!(goal["successCriteria"][0]["path"],"a");
        let new_job_id=goal["jobId"].as_str().unwrap();
        assert_ne!(new_job_id,"prior");
        assert_eq!(state.store.get("job",new_job_id).unwrap()["input"]["resumeAfterReview"],true);
        assert_eq!(state.store.get("job","prior").unwrap()["status"],"needs_review");
        assert_eq!(resume_goal(State(state.clone()),Path("g".into()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await.status(),
            StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn owner_cancel_stops_goal_and_queued_job_without_losing_review() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("goal",&json!({"id":"g","status":"queued","sessionId":"s",
            "jobId":"goal-job-g","goal":"cancel it"}));
        state.store.put("job",&json!({"id":"goal-job-g","kind":"goal",
            "input":{"goalId":"g"},"status":"queued"}));
        let cancelled=cancel_goal(State(state.clone()),Path("g".into())).await;
        assert_eq!(cancelled.status(),StatusCode::OK);
        assert_eq!(state.store.get("goal","g").unwrap()["status"],"cancelled");
        let job=state.store.get("job","goal-job-g").unwrap();
        assert_eq!(job["status"],"cancelled");
        assert_eq!(job["cancelRequested"],true);
        assert!(state.is_cancelled("s"));
        assert_eq!(cancel_goal(State(state.clone()),Path("g".into())).await.status(),
            StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn resume_rejects_active_and_terminal_prior_jobs() {
        let (state,dir)=state();
        state.store.put("goal",&json!({"id":"g","status":"needs_review",
            "iterations":1,"maxIterations":4,"goal":"continue"}));
        state.store.put("job",&json!({"id":"old","kind":"goal",
            "input":{"goalId":"g"},"status":"running"}));
        assert_eq!(resume_goal(State(state.clone()),Path("g".into()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await.status(),StatusCode::CONFLICT);
        state.store.patch_if("job","old","status",&json!("running"),
            &json!({"status":"failed"}));
        state.store.patch_if("goal","g","status",&json!("needs_review"),
            &json!({"status":"completed"}));
        assert_eq!(resume_goal(State(state.clone()),Path("g".into()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await.status(),StatusCode::CONFLICT);
        let _=std::fs::remove_dir_all(dir);
    }
}
