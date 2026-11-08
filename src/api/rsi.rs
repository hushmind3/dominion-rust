use crate::{engine::rsi as engine, state::AppState};
use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};

fn error(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"version":"manual-rsi-v1","candidates":engine::list(&state),
        "minPairedCases":engine::MIN_PAIRED_CASES}))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match engine::one(&state, &id) {
        Some(candidate) => Json(candidate).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"후보를 찾을 수 없습니다."}))).into_response()
    }
}

pub async fn propose(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match engine::propose(&state,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}

pub async fn evaluate(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match engine::evaluate(&state,&id,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}

pub async fn decide(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match engine::decide(&state,&id,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}
