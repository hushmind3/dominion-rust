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
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"진행 중인 목표를 먼저 중단하세요.");
    }
    if goal["gitWorktree"].is_object() && goal["gitWorktree"]["rolledBack"]!=true{
        return error(StatusCode::CONFLICT,"독립 Git 작업공간을 먼저 폐기한 후 목표를 삭제하세요.");
    }
    state.store.remove("goal", &id);
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
    let isolate=body["isolateWorktree"].as_bool().unwrap_or(false);
    if body.get("isolateWorktree").is_some() && !body["isolateWorktree"].is_boolean(){
        return error(StatusCode::BAD_REQUEST,"isolateWorktree must be a boolean.");
    }
    let executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(16));

    for key in ["executor","reviewer","planner"] { if body.get(key).is_some_and(|v|!v.is_object()){return error(StatusCode::BAD_REQUEST,"실행자·검토자·계획자는 객체로 지정하세요.");} }
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    let Some(record) = state.store.put(
        "goal",
        &json!({
            "taskId": uuid::Uuid::new_v4().to_string(),
            "goal": text,
            "successCriteria": criteria,
            "isolateWorktree":isolate,
            "executor": executor,
            "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
            "planner":body.get("planner").cloned().unwrap_or(json!({})),
            "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
            "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
            "replans":0,"planRevision":0,"appliedPlanRevision":0,"ownerPlan":null,
            "planHistory":[],"reviewHistory":[],"artifacts":[],
            "status": "queued",
            "iterations": 0,
            "maxIterations": max_iterations,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    ) else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "목표를 저장할 수 없습니다.",
        );
    };
    state.bus.record("goal", &record);
    let goal_id = record
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let job_id = format!("goal-job-{goal_id}");
    let job = crate::engine::jobs::enqueue_once(&state,&job_id,"goal",json!({"goalId":goal_id}));
    if job.is_none() {
        if let Some(failed)=state.store.patch_if("goal",&goal_id,"status",&json!("queued"),
            &json!({"status":"needs_review","phase":"enqueue_failed",
                "error":"영속 실행 작업을 등록하지 못했습니다."})) {
            state.bus.record("goal",&failed);
        }
        state.store.audit("goal.enqueue_failed",json!({"goalId":goal_id,"jobId":job_id}));
        return error(StatusCode::INTERNAL_SERVER_ERROR,"목표가 보존됐지만 실행 작업 등록에 실패했습니다. 목표 목록에서 상태를 확인하세요.");
    }
    let Some(saved)=state.store.patch_if("goal",&goal_id,"status",&json!("queued"),
        &json!({"jobId":job_id})) else {
        return error(StatusCode::CONFLICT,"실행 작업은 등록됐지만 목표 상태가 먼저 변경됐습니다.");
    };
    state.bus.record("goal",&saved);
    Json(saved).into_response()
}


/// Owner-approved recovery of a failed/review-blocked goal. A fresh job
/// continues after the last completed iteration; it never replays the old
/// job's uncertain in-flight tool call or assumes external writes succeeded.
pub async fn resume_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response {
    if body.as_object().is_none_or(|object|object.len()!=1
        || object.get("acknowledgeUnknownEffects")!=Some(&json!(true))) {
        return error(StatusCode::BAD_REQUEST,
            "이전 파일·외부 작업 결과를 확인했다면 acknowledgeUnknownEffects=true를 보내세요.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")) {
        return error(StatusCode::CONFLICT,"검토 대기 또는 실패한 목표만 이어갈 수 있습니다.");
    }
    if state.store.all("job").iter().any(|job|job["kind"]=="goal"
        && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running"))) {
        return error(StatusCode::CONFLICT,"이전 목표 작업이 아직 실행 중입니다.");
    }
    let iterations=goal["iterations"].as_u64().unwrap_or(0);
    let old_max=goal["maxIterations"].as_u64().unwrap_or(16).clamp(1,64);
    let next_max=if old_max<=iterations {old_max.saturating_add(8).min(64)}else{old_max};
    if next_max<=iterations {
        return error(StatusCode::CONFLICT,"목표 반복 한도 64회에 도달했습니다.");
    }
    let next=goal["resumeCount"].as_u64().unwrap_or(0).saturating_add(1);
    if next>12 {
        return error(StatusCode::CONFLICT,"검토 후 이어가기 횟수 한도에 도달했습니다.");
    }
    let old_job_id=goal["jobId"].as_str().map(str::to_string)
        .or_else(||state.store.all("job").into_iter()
            .find(|j|j["kind"]=="goal"&&j["input"]["goalId"]==id)
            .and_then(|j|j["id"].as_str().map(str::to_string)));
    let old_job=old_job_id.as_deref()
        .and_then(|job_id|state.store.get("job",job_id))
        .unwrap_or(Value::Null);
    let old_checkpoint=&old_job["goalState"];
    // A tool call whose result was unknown at the interruption boundary
    // must not be repeated as if it had never run.
    let interrupted=old_checkpoint["inFlight"]==true;
    let unfinished=old_checkpoint["nextIteration"].as_u64().unwrap_or(iterations);
    let next_iteration=iterations.max(if interrupted {
        unfinished.saturating_add(1)
    }else{unfinished});
    if next_iteration>=next_max {
        return error(StatusCode::CONFLICT,"이전 실행 단계가 반복 한도를 모두 사용했습니다.");
    }
    let new_job_id=format!("goal-job-{id}-resume-{next}");
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"jobId":new_job_id,"previousJobId":old_job_id,
        "previousStatus":goal["status"],"startIteration":next_iteration,
        "acknowledgedBy":"owner","at":chrono::Utc::now().to_rfc3339()}));
    // The new durable worker starts after all completed iterations. Previous
    // inFlight/reviewPending flags remain on the old job as forensic evidence.
    let checkpoint=json!({
        "sessionId":goal["sessionId"],
        "nextIteration":next_iteration,"inFlight":false,"reviewPending":false,
        "appliedPlanRevision":goal["appliedPlanRevision"].as_u64().unwrap_or(0),
        "evidence":goal.get("evidence").cloned()
            .filter(|v|v.is_array()).unwrap_or_else(||json!([])),
        "transcript":old_checkpoint["transcript"].as_str().unwrap_or(""),
        "gaps":goal.get("gaps").cloned()
            .filter(|v|v.is_array()).unwrap_or_else(||json!([])),
        "stalled":0,"resumeAfterReview":true
    });
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"queued","phase":"resume","jobId":new_job_id,
            "resumeCount":next,"maxIterations":next_max,"resumeHistory":history,
            "finishedAt":null,"error":null,"finalReport":null})) else {
        return error(StatusCode::CONFLICT,"목표가 다른 요청에서 변경됐습니다.");
    };
    let created=crate::engine::jobs::enqueue_goal_resume(
        &state,&new_job_id,&id,checkpoint,next_iteration,next_max);
    if created.is_none() {
        let restored=state.store.patch_if("goal",&id,"jobId",&json!(new_job_id),
            &json!({"status":"needs_review","phase":"resume_enqueue_failed",
                "error":"이어가기 실행 작업을 등록하지 못했습니다."}));
        if let Some(restored)=restored {state.bus.record("goal",&restored);}
        return error(StatusCode::INTERNAL_SERVER_ERROR,"이어가기 작업을 등록하지 못했습니다.");
    }
    if let Some(session)=saved["sessionId"].as_str(){state.clear_cancel(session);}
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_reviewed_resume",json!({
        "goalId":id,"jobId":new_job_id,"previousJobId":old_job_id,
        "startIteration":next_iteration
    }));
    Json(saved).into_response()
}

/// Versioned steering; edits become effective at a safe executor turn boundary.
pub async fn steer_goal(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response {
    if body.as_object().is_none_or(|fields|fields.len()!=2||
        !fields.contains_key("expectedRevision")||!fields.contains_key("plan")){
        return error(StatusCode::BAD_REQUEST,"plan과 expectedRevision만 지정하세요.");
    }
    let Some(revision)=body["expectedRevision"].as_u64() else{
        return error(StatusCode::BAD_REQUEST,"expectedRevision은 정수여야 합니다.");
    };
    let Some(plan)=body["plan"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"plan은 문자열이어야 합니다.");
    };
    use crate::engine::goal_steering::{submit,SteeringError};
    match submit(&state,&id,revision,plan){
        Ok(value)=>Json(value).into_response(),
        Err(SteeringError::Missing)=>error(StatusCode::NOT_FOUND,"목표가 없습니다."),
        Err(SteeringError::Immutable)=>error(StatusCode::CONFLICT,"종료된 목표는 수정할 수 없습니다."),
        Err(SteeringError::Conflict)=>error(StatusCode::CONFLICT,"계획 버전이 달라졌습니다. 최신 내용을 불러오세요."),
        Err(SteeringError::Invalid)=>error(StatusCode::BAD_REQUEST,"계획은 1~6000자여야 합니다."),
    }
}

/// Stop the durable job, not only the browser view. The goal controller
/// checks the cancellation epoch while awaiting independent model completion.
pub async fn cancel_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표가 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("completed"|"cancelled"|"failed"|"needs_review")){
        return Json(json!({"id":id,"status":goal["status"]})).into_response();
    }
    if let Some(session)=goal["sessionId"].as_str(){state.cancel_session(session);}
    for job in state.store.all("job").into_iter().filter(|job|
        job["kind"]=="goal"&&job["input"]["goalId"]==id&&
        matches!(job["status"].as_str(),Some("queued"|"running"))) {
        let Some(job_id)=job["id"].as_str() else{continue};
        if let Some(saved)=state.store.patch_if("job",job_id,"status",&job["status"],
            &json!({"cancelRequested":true,"status":"cancelled",
                "finishedAt":chrono::Utc::now().to_rfc3339()})){
            state.bus.record("job",&saved);
        }
    }
    if let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
            "finishedAt":chrono::Utc::now().to_rfc3339()})){
        state.bus.record("goal",&saved);
    }
    Json(json!({"id":id,"status":"cancelled"})).into_response()
}


/// Read-only diff for files attributed to this goal by observed tool receipts.
/// Returns no unrelated workspace changes when the goal has no file outputs.
pub async fn diff_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["isolateWorktree"]==true {
        return match engine::goal_git::changes(&state,&id).await{
            Ok(value)=>Json(value).into_response(),
            Err(message)=>error(StatusCode::CONFLICT,&message),
        };
    }
    let paths=goal["artifacts"].as_array().into_iter().flatten()
        .filter_map(|row|row["path"].as_str().map(str::to_string))
        .take(20).collect::<Vec<_>>();
    if paths.is_empty(){
        return Json(json!({"goalId":id,"paths":[],"status":"",
            "unstaged":"","staged":"","observed":false,
            "note":"관측된 파일 수정 산출물이 없어 이 목표의 diff를 구분할 수 없습니다."})).into_response();
    }
    match crate::engine::agent_diff::inspect(&state.config.workspace,&paths).await{
        Ok(mut value)=>{
            value["goalId"]=json!(id);
            Json(value).into_response()
        }
        Err(message)=>error(StatusCode::BAD_REQUEST,&message),
    }
}

/// Explicit owner discard: only the goal-owned linked worktree is removed.
pub async fn rollback_goal(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::rollback(&state,&id).await{
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Only an owner-submitted digest of the previously reviewed patch can be
/// applied to the clean original checkout.
pub async fn apply_goal(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if body.as_object().is_none_or(|obj|obj.len()!=1 || !obj.contains_key("expectedPatchSha256")){
        return error(StatusCode::BAD_REQUEST,"expectedPatchSha256 is required.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"Expected patch digest must be a string.");
    };
    match engine::goal_git::apply_reviewed(&state,&id,digest).await{
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Shared-checkout preview is independent from isolated worktree /diff.
/// It only enumerates files with successful goal edit receipts.
pub async fn shared_changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"Goal not found");};
    if goal["isolateWorktree"]==true{return error(StatusCode::CONFLICT,"Use isolated /diff instead");}
    match engine::goal_changes::preview(&state,&goal).await{
        Ok(v)=>Json(v).into_response(),
        Err(e)=>error(StatusCode::CONFLICT,&e),
    }
}
/// Restore one SHA-pinned, goal-edited tracked file from the frozen baseline.
pub async fn shared_undo(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"Goal not found");};
    if goal["isolateWorktree"]==true{return error(StatusCode::CONFLICT,"Isolated goals use rollback");}
    if body.as_object().is_none_or(|map|map.len()!=2 ||
        !map.contains_key("path")||!map.contains_key("expectedSha256")){
        return error(StatusCode::BAD_REQUEST,"path and expectedSha256 are required.");
    }
    let Some(path)=body["path"].as_str() else{return error(StatusCode::BAD_REQUEST,"Invalid path");};
    let Some(sha)=body["expectedSha256"].as_str() else{return error(StatusCode::BAD_REQUEST,"Invalid digest");};
    match engine::goal_changes::undo(&state,&goal,path,sha).await{
        Ok(v)=>Json(v).into_response(),
        Err(e)=>error(StatusCode::CONFLICT,&e),
    }
}

#[cfg(test)]
mod goal_resume_api_tests {
    use super::*;

    fn state() -> (AppState,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-resume-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config {
            port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()
        };
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[tokio::test]
    async fn resume_starts_at_completed_checkpoint_and_preserves_old_job() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"session-1"})).unwrap();
        state.store.put("goal",&json!({
            "id":"goal-1","goal":"repair source","status":"needs_review",
            "phase":"needs_review","iterations":2,"maxIterations":4,
            "jobId":"job-old","sessionId":"session-1","planRevision":1,
            "appliedPlanRevision":1,"evidence":[{"tool":"file_read"}],
            "gaps":["external outcome unknown"]
        })).unwrap();
        state.store.put("job",&json!({
            "id":"job-old","kind":"goal","status":"needs_review",
            "input":{"goalId":"goal-1"},
            "goalState":{"nextIteration":2,"inFlight":true,
                "transcript":"two prior completed iterations"}
        })).unwrap();
        let accepted=resume_goal(State(state.clone()),Path("goal-1".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(accepted.status(),StatusCode::OK);
        let updated=state.store.get("goal","goal-1").unwrap();
        assert_eq!(updated["status"],"queued");
        assert_eq!(updated["resumeCount"],1);
        assert_eq!(updated["jobId"],"goal-job-goal-1-resume-1");
        let new_job=state.store.get("job","goal-job-goal-1-resume-1").unwrap();
        assert_eq!(new_job["status"],"queued");
        assert_eq!(new_job["goalState"]["nextIteration"],3);
        assert_eq!(new_job["goalState"]["inFlight"],false);
        assert_eq!(new_job["input"]["resumeAfterReview"],true);
        assert_eq!(new_job["goalState"]["evidence"].as_array().unwrap().len(),1);
        assert_eq!(new_job["goalState"]["sessionId"],"session-1");
        assert_eq!(state.store.get("job","job-old").unwrap()["goalState"]["inFlight"],true);
        let duplicate=resume_goal(State(state.clone()),Path("goal-1".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(duplicate.status(),StatusCode::CONFLICT);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn review_ack_and_active_job_are_mandatory() {
        let (state,dir)=state();
        state.store.put("goal",&json!({
            "id":"g","goal":"inspect","status":"failed","iterations":0,
            "maxIterations":16
        })).unwrap();
        let missing_ack=resume_goal(State(state.clone()),Path("g".to_string()),
            Json(json!({}))).await;
        assert_eq!(missing_ack.status(),StatusCode::BAD_REQUEST);
        state.store.put("job",&json!({"id":"active","kind":"goal",
            "input":{"goalId":"g"},"status":"running"})).unwrap();
        let still_active=resume_goal(State(state.clone()),Path("g".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(still_active.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("goal","g").unwrap()["status"],"failed");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
