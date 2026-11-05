use crate::state::AppState;
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::harness_evolution::list(&state))
}

pub async fn propose(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match crate::engine::harness_evolution::propose(&state, &body) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn validate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::harness_evolution::validate(&state, &id).await {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn promote(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::harness_evolution::promote(&state, &id).await {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn rollback(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::harness_evolution::rollback(&state, &id) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn activate(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::harness_evolution::activate(&state, &id) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

pub async fn generate_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match crate::engine::harness_evolution::generate_patch(&state, &id).await {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}
