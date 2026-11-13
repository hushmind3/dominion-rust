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
    if matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return error(StatusCode::CONFLICT,"실행 중인 목표는 먼저 중단하세요.");
    }
    state.store.remove("goal",&id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
}

/// Owner changes are accepted only before the first agent/tool turn.
pub async fn edit_plan(
    State(state):State<AppState>, Path(id):Path<String>, Json(body):Json<Value>,
)->Response {
    let Some(plan)=body["plan"].as_str().map(str::trim)
        .filter(|value|!value.is_empty()&&value.chars().count()<=6000) else {
        return error(StatusCode::BAD_REQUEST,"계획은 1~6000자여야 합니다.");
    };
    if body.as_object().is_none_or(|map|map.len()!=1) {
        return error(StatusCode::BAD_REQUEST,"plan만 변경할 수 있습니다.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["status"]!="queued" {
        return error(StatusCode::CONFLICT,"실행 시작 후에는 계획이 자동 검증·재계획으로 관리됩니다.");
    }
    match state.store.patch_if("goal",&id,"status",&json!("queued"),
        &json!({"plan":plan,"planHistory":[],"planEditedAt":chrono::Utc::now().to_rfc3339()})) {
        Some(saved)=>{state.bus.record("goal",&saved);Json(saved).into_response()},
        None=>error(StatusCode::CONFLICT,"목표 실행 상태가 변경됐습니다.")
    }
}

/// Cancels the durable root and its queued/running job, not merely its UI.
pub async fn cancel_goal(State(state):State<AppState>,Path(id):Path<String>)->Response {
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return error(StatusCode::CONFLICT,"이 목표는 이미 종료됐습니다.");
    }
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
            "finishedAt":chrono::Utc::now().to_rfc3339()})) else {
        return error(StatusCode::CONFLICT,"목표 상태가 변경됐습니다.");
    };
    state.bus.record("goal",&saved);
    if let Some(session)=saved["sessionId"].as_str() {state.cancel_session(session);}
    let job=state.store.all("job").into_iter().find(|job|
        job["kind"]=="goal" && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running")));
    if let Some(job)=job {
        let job_id=job["id"].as_str().unwrap_or("");
        if let Some(cancelled)=state.store.patch_if("job",job_id,"status",&job["status"],
            &json!({"status":"cancelled","cancelRequested":true,
                "finishedAt":chrono::Utc::now().to_rfc3339()})) {
            state.bus.record("job",&cancelled);
        }
    }
    Json(saved).into_response()
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
            "executor": executor,
            "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
            "planner":body.get("planner").cloned().unwrap_or(json!({})),
            "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
            "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
            "replans":0,"planHistory":[],"reviewHistory":[],
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
    let durable_id=format!("goal-job-{goal_id}");
    let job = crate::engine::jobs::enqueue_once(&state,&durable_id,
        "goal",json!({"goalId":goal_id}));
    let job_id = job
        .as_ref()
        .and_then(|job| job.get("id"))
        .cloned()
        .unwrap_or(Value::Null);

    if job.is_none(){
        return error(StatusCode::INTERNAL_SERVER_ERROR,"목표 실행 작업을 등록하지 못했습니다.");
    }
    let saved=state.store.patch_if("goal",&goal_id,"id",&json!(goal_id),
        &json!({"jobId":job_id})).unwrap_or(record.clone());
    state.bus.record("goal",&saved);
    let mut payload = saved;
    payload["jobId"] = job_id;
    Json(payload).into_response()
}
