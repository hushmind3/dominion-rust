use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Root tasks are engine-owned. This is a read-only projection of the episode,
/// its frozen contract and linked attempts.
pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("task_episode")))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("task_episode", &id) {
        Some(task) => Json(task).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "작업 기록을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}
