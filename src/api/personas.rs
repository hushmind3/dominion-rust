use crate::{engine::personas, state::AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn preview(State(state): State<AppState>) -> Json<Value> {
    Json(personas::preview(&state))
}
pub async fn import(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match personas::import(&state, &body) {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response(),
    }
}
pub async fn avatar(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match personas::avatar(&state, &id) {
        Ok((bytes, mime)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, max-age=60".into()),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"아바타를 찾을 수 없습니다."})),
        )
            .into_response(),
    }
}
