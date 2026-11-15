//! Read-only goal state and metrics, plus guarded deletion.
use super::*;

/// Runtime observations, not an externally certified model benchmark.
pub async fn goal_metrics(State(state):State<AppState>)->Response{
    match engine::goals::metrics::snapshot(&state){
        Ok(snapshot)=>Json(snapshot).into_response(),
        Err(reason)=>error(StatusCode::INTERNAL_SERVER_ERROR,
            &format!("목표 실행 사용량 집계 실패: {reason}")),
    }
}

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
    if goal["gitWorktree"].is_object() && goal["gitWorktree"]["rolledBack"]!=true
        && goal["gitWorktree"]["applied"]!=true {
        return error(StatusCode::CONFLICT,
            "격리 작업공간이 남아 있습니다. 먼저 변경을 반영하거나 폐기하세요.");
    }
    state.store.remove("goal",&id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
}
