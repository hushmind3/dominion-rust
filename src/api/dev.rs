use crate::engine::dev;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    Json(dev::status(&state).await)
}

pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(dev::config(&state))
}

pub async fn save_config(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(dev::save_config(&state, &body))
}

pub async fn connect(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match dev::connect(&state, &body).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn diff(State(state): State<AppState>) -> Response {
    match dev::diff(&state).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("dev_job", &id) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "개발 작업을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn start_job(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !matches!(
        kind.as_str(),
        "setup" | "test" | "build" | "package" | "commit"
    ) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 개발 작업입니다: {kind}") })),
        )
            .into_response();
    }
    match dev::start_job(state, &kind, body) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "개발 작업을 만들 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn preview(State(state): State<AppState>, Path(action): Path<String>) -> Response {
    match action.as_str() {
        "start" => match dev::start_preview(&state).await {
            Ok(result) => Json(result).into_response(),
            Err(error) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
            }
        },
        "stop" => Json(dev::stop_preview()).into_response(),
        other => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 미리보기 작업입니다: {other}") })),
        )
            .into_response(),
    }
}

pub async fn update(Json(_body): Json<Value>) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "업데이트 적용은 데스크톱 셸의 설치 기능이 필요합니다. Rust 웹 모드에서는 지원하지 않습니다.",
            "ported": false,
        })),
    )
        .into_response()
}
