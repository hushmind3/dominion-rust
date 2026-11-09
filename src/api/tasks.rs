use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Default)]
pub struct Page {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Root tasks are engine-owned. This is a read-only projection of the episode,
/// its frozen contract and linked attempts. The response is bounded because a
/// task episode can carry a large set of attempts and evidence.
pub async fn list(State(state): State<AppState>, Query(page): Query<Page>) -> Json<Value> {
    let mut items = state.store.all("task_episode");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
    });
    let limit = page.limit.unwrap_or(200).clamp(1, 1000);
    let offset = page.offset.unwrap_or(0);
    Json(json!(
        items
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<Value>>()
    ))
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
