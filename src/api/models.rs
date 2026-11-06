use crate::engine::models;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn catalog(Query(query): Query<HashMap<String, String>>) -> Response {
    let search = query.get("q").cloned().unwrap_or_default();
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(40);
    match models::catalog(&search, limit).await {
        Ok(items) => Json(json!(items)).into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn details(Query(query): Query<HashMap<String, String>>) -> Response {
    let id = query.get("id").cloned().unwrap_or_default();
    match models::details(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn installed(State(state): State<AppState>) -> Json<Value> {
    Json(json!(models::installed(&state)))
}

pub async fn jobs(State(state): State<AppState>) -> Json<Value> {
    Json(json!(models::jobs(&state)))
}

pub async fn runtime(State(state): State<AppState>) -> Json<Value> {
    Json(models::runtime_snapshot(&state))
}

pub async fn load(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "모델 id 가 필요합니다." })),
        )
            .into_response();
    }
    let context = body
        .get("context")
        .and_then(|value| value.as_u64())
        .unwrap_or(4096);
    let _change = models::RUNTIME_CHANGE.lock().await;
    match models::load(&state, id, context).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            models::mark_load_failure(&state, &error);
            (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response()
        }
    }
}

pub async fn unload(State(state): State<AppState>) -> Json<Value> {
    let _change = models::RUNTIME_CHANGE.lock().await;
    Json(models::unload(&state))
}

pub async fn download(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let repo = body
        .get("repo")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let option = body
        .get("optionId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match models::start_download(state, repo.to_string(), option.to_string()) {
        Ok(job) => Json(job).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn import(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let path = body
        .get("filePath")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let state_for_import = state.clone();
    let path = path.to_owned();
    let reference = body["reference"].as_bool().unwrap_or(false);
    let imported = tokio::task::spawn_blocking(move || {
        if reference { models::register_local(&state_for_import, &path) }
        else { models::import_local(&state_for_import, &path) }
    }).await;
    match imported.unwrap_or_else(|_| Err("모델을 가져올 수 없습니다.".into())) {
        Ok(item) => Json(item).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn remove_installed(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _change = models::RUNTIME_CHANGE.lock().await;
    match models::remove_installed(&state, &id) {
        Ok(()) => Json(json!({"ok":true})).into_response(),
        Err(message) => super::response::error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn job_action(
    State(state): State<AppState>,
    Path((id, action)): Path<(String, String)>,
) -> Response {
    match action.as_str() {
        "cancel" => {
            if models::cancel_job(&state, &id) {
                Json(json!({ "ok": true, "status": "cancelled" })).into_response()
            } else {
                (StatusCode::NOT_FOUND, Json(json!({ "error": "작업을 찾을 수 없습니다." })))
                    .into_response()
            }
        }
        "pause" | "resume" => (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "다운로드 일시정지는 아직 Rust 백엔드에서 지원하지 않습니다. 취소 후 다시 시작하세요.",
                "ported": false,
            })),
        )
            .into_response(),
        other => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 작업입니다: {other}") })),
        )
            .into_response(),
    }
}
