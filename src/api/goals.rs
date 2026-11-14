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
            "isolateWorktree":body["isolateWorktree"].as_bool().unwrap_or(false),
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
    let Some(job) = crate::engine::jobs::enqueue(&state, "goal", json!({ "goalId": goal_id })) else {
        // Never report an autonomous goal as queued without a durable worker.
        if let Some(saved) = state.store.patch_if("goal",&goal_id,"id",&json!(goal_id),&json!({
            "status":"failed","phase":"enqueue_failed","error":"목표 작업을 등록하지 못했습니다."
        })) {state.bus.record("goal",&saved);}
        return error(StatusCode::INTERNAL_SERVER_ERROR,"목표 작업을 등록하지 못했습니다.");
    };
    // Persist the same worker identity that the UI uses for cancellation,
    // progress and post-crash reconciliation.
    let job_id = job["id"].clone();
    let saved = state.store.patch_if("goal",&goal_id,"id",&json!(goal_id),
        &json!({"jobId":job_id})).unwrap_or(record);
    state.bus.record("goal",&saved);
    let mut payload = saved;
    payload["jobId"] = job_id;
    Json(payload).into_response()
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
