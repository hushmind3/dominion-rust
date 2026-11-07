use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::engine::browser;
use crate::state::AppState;

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let status = browser::execute(
        "status",
        &json!({}),
        &state.config.data_dir,
        &state.config.workspace,
    )
    .await
    .unwrap_or_else(|error| json!({ "available": false, "running": false, "error": error }));
    Json(json!({
        "status": status,
        "screenshot": latest_screenshot(&state)
            .map(|path| path.to_string_lossy().to_string()),
    }))
}

pub async fn action(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let action = body["action"].as_str().unwrap_or("status");
    match browser::execute(
        action,
        &body,
        &state.config.data_dir,
        &state.config.workspace,
    )
    .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn screenshot(State(state): State<AppState>) -> Response {
    let Some(path) = latest_screenshot(&state) else {
        return (StatusCode::NOT_FOUND, "아직 캡처한 화면이 없습니다.").into_response();
    };
    match std::fs::read(&path) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Body::from(bytes),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

fn latest_screenshot(state: &AppState) -> Option<std::path::PathBuf> {
    let dir = state.config.workspace.join(".dominion").join("browser");
    let mut latest: Option<(String, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("png") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if latest.as_ref().map_or(true, |(current, _)| name > *current) {
            latest = Some((name, path));
        }
    }
    latest.map(|(_, path)| path)
}
