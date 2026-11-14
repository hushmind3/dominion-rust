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

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("goal", &id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "goal", "removed": id }));
    Json(json!({ "ok": true }))
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
    let job = crate::engine::jobs::enqueue(&state, "goal", json!({ "goalId": goal_id }));
    let job_id = job
        .as_ref()
        .and_then(|job| job.get("id"))
        .cloned()
        .unwrap_or(Value::Null);

    let mut payload = record.clone();
    payload["jobId"] = job_id;
    Json(payload).into_response()
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
