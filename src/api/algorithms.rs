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
    body: Option<Json<Value>>,
) -> Response {
    let force = body
        .and_then(|Json(value)| value["force"].as_bool())
        .unwrap_or(false);
    match crate::engine::algorithms::activate_checked(&state, &id, force) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
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

pub async fn evaluate(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::algorithm_eval::run(&state))
}

pub async fn evaluations(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "items": crate::engine::algorithm_eval::history(&state, 20),
    }))
}
