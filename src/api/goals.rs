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

pub async fn changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::changes(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}
pub async fn rollback(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::rollback(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Explicit owner action: apply only the exact SHA-256-reviewed isolated patch.
pub async fn apply(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if body.as_object().is_none_or(|object|object.len()!=1 || !object.contains_key("expectedPatchSha256")){
        return error(StatusCode::BAD_REQUEST,"expectedPatchSha256 항목만 제출하세요.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else {
        return error(StatusCode::BAD_REQUEST,"검토한 Git 패치 SHA-256을 문자열로 제출하세요.");
    };
    match engine::goal_git::apply_reviewed(&state,&id,digest).await {
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
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
    let mut criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let isolate=body["isolateWorktree"].as_bool().unwrap_or(false);
    let publish=body["publishPr"].as_bool().unwrap_or(false);
    if publish && !isolate {return error(StatusCode::BAD_REQUEST,
        "GitHub PR을 자동으로 만들려면 독립 Git 작업공간을 선택해야 합니다.");}
    let commands=body["verifyCommands"].as_array().cloned().unwrap_or_default();
    if commands.len()>4 || commands.iter().any(|v|v.as_str().is_none_or(|cmd|{
        cmd.trim().is_empty() || cmd.len()>180 || cmd.chars().any(char::is_control)
            || cmd.chars().any(|ch|matches!(ch,';'|'|'|'&'|'>'|'<'|'`'))
    })) {
        return error(StatusCode::BAD_REQUEST,"검증 명령은 최대 4개, 개당 180자 이하여야 합니다.");
    }
    let Some(items)=criteria.as_array_mut() else {
        return error(StatusCode::BAD_REQUEST,"검증 조건은 배열이어야 합니다.");
    };
    for command in &commands{
        items.push(json!({"type":"command_succeeded","command":command}));
    }
    if publish {items.push(json!({"type":"github_pr_created"}));}
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
    if body.get("tokenBudget").is_some_and(|v|v.as_u64().is_none_or(|n|n>2_000_000)) {
        return error(StatusCode::BAD_REQUEST,"목표 실행 토큰 상한은 0~2000000 사이여야 합니다.");
    }
    let Some(record) = state.store.put(
        "goal",
        &json!({
            "taskId": uuid::Uuid::new_v4().to_string(),
            "goal": text,
            "successCriteria": criteria,
            "isolateWorktree":isolate,"publishPr":publish,
            "verifyCommands":commands,
            "executor": executor,
            "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
            "planner":body.get("planner").cloned().unwrap_or(json!({})),
            "tokenBudget":body["tokenBudget"].as_u64().unwrap_or(0),
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
    let goal_id = record
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Do not leave a permanent "queued" goal without a corresponding durable
    // job. The goal and the job are separate records with a recoverable link.
    let Some(job) = crate::engine::jobs::enqueue(
        &state,"goal",json!({"goalId":goal_id})
    ) else {
        state.store.remove("goal",&goal_id);
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표 실행 작업을 저장할 수 없습니다.");
    };
    let job_id=job["id"].clone();
    let saved=state.store.patch_if("goal",&goal_id,"status",&json!("queued"),
        &json!({"jobId":job_id})).unwrap_or_else(||record.clone());
    state.bus.record("goal",&saved);
    Json(saved).into_response()
}
