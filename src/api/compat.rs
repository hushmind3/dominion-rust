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

pub async fn youtube_status(State(state): State<AppState>) -> Json<Value> {
    let connection = crate::engine::oauth::status(&state)
        .into_iter()
        .find(|p| p["id"] == "youtube")
        .unwrap_or(json!({}));
    Json(
        json!({"connected":connection["connected"],"uploadAllowed":connection["connected"]==true&&connection["grantedScopes"].as_str().is_some_and(|s|s.split_whitespace().any(|s|s=="https://www.googleapis.com/auth/youtube.upload")),"account":connection["account"],"uploads":state.store.all("job").iter().filter(|j|j["kind"]=="youtube_upload").map(crate::engine::video::public_job).collect::<Vec<_>>(),"channel":null}),
    )
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
