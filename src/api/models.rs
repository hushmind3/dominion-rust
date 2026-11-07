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
    let format = query.get("format").cloned().unwrap_or_else(|| "gguf".into());
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(40);
    match models::catalog(&search, limit, &format).await {
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

pub async fn runtimes(State(state): State<AppState>) -> Json<Value> {
    Json(models::runtimes(&state))
}

pub async fn install_runtime(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let engine = body["engine"].as_str().unwrap_or("gguf");
    match models::install_runtime(state, engine).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
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
    let format = body
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("gguf")
        .to_string();
    let files = body
        .get("files")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match models::start_download_with(state, repo.to_string(), option.to_string(), format, files) {
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
        "pause" => {
            if models::pause_job(&state, &id) {
                Json(json!({ "ok": true, "status": "paused" })).into_response()
            } else {
                (StatusCode::CONFLICT, Json(json!({ "error": "다운로드 중인 작업만 일시정지할 수 있습니다." })))
                    .into_response()
            }
        }
        "resume" => match models::resume_job(&state, &id) {
            Some(job) => Json(job).into_response(),
            None => (
                StatusCode::CONFLICT,
                Json(json!({ "error": "일시정지·실패·취소된 작업만 재개할 수 있습니다." })),
            )
                .into_response(),
        },
        other => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 작업입니다: {other}") })),
        )
            .into_response(),
    }
}
