use crate::state::AppState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::algorithms::list(&state))
}

pub async fn register(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match crate::engine::algorithms::register(&state, &body) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn activate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::algorithms::activate(&state, &id) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::NOT_FOUND, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn disable(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::algorithms::disable(&state, &id) {
        Some(record) => Json(record).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, Json(json!({"error":"algorithm not found"}))).into_response(),
    }
}

pub async fn generate(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(crate::engine::algorithms::generate(&state, &body))
}
