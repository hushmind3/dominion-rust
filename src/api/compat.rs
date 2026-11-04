use crate::state::AppState;
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn empty_list() -> Json<Value> {
    Json(json!([]))
}

pub async fn empty_object() -> Json<Value> {
    Json(json!({}))
}


pub async fn skill_catalog(State(state): State<AppState>) -> Json<Value> {
    let items = state.store.all("skill");
    Json(json!({
        "items": items,
        "total": items.len(),
        "updatedAt": chrono::Utc::now().to_rfc3339(),
    }))
}

pub async fn model_runtime() -> Json<Value> {
    Json(json!({
        "status": "stopped",
        "engine": "rust",
        "name": null,
        "id": null,
    }))
}

pub async fn oauth_providers() -> Json<Value> {
    Json(json!([
        { "id": "google", "configured": false, "connected": false, "callback": "" },
        { "id": "github", "configured": false, "connected": false, "callback": "" }
    ]))
}

pub async fn youtube_status() -> Json<Value> {
    Json(json!({ "connected": false, "channel": null }))
}

pub async fn dev_status() -> Json<Value> {
    Json(json!({
        "connected": false,
        "message": "개발 스튜디오는 Rust 백엔드에서 아직 제공되지 않습니다.",
    }))
}

pub async fn not_ported() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "이 기능은 아직 Rust 백엔드로 이식되지 않았습니다.",
            "ported": false,
        })),
    )
        .into_response()
}

pub async fn upload(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let raw = query
        .get("name")
        .map(String::as_str)
        .unwrap_or("upload.bin")
        .to_string();
    let name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let dir = state.config.data_dir.join("uploads");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response();
    }
    let path = dir.join(&name);
    match std::fs::write(&path, &body) {
        Ok(_) => Json(json!({
            "ok": true,
            "name": name,
            "path": path.to_string_lossy(),
            "bytes": body.len(),
        }))
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}
