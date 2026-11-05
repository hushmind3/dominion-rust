use crate::{engine::attachments, state::AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
pub async fn upload(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match attachments::upload(&state, &body) {
        Ok(file) => Json(file).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error":error}))).into_response(),
    }
}
pub async fn content(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match attachments::read(&state, &id) {
        Ok((file, bytes)) => {
            let mime = file["mimeType"]
                .as_str()
                .unwrap_or("application/octet-stream")
                .to_string();
            let encoded = file["name"]
                .as_str()
                .unwrap_or("attachment")
                .bytes()
                .map(|byte| {
                    if byte.is_ascii_alphanumeric() || b"-_.".contains(&byte) {
                        char::from(byte).to_string()
                    } else {
                        format!("%{byte:02X}")
                    }
                })
                .collect::<String>();
            let disposition = if file["isImage"] == true {
                "inline"
            } else {
                "attachment"
            };
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (
                        header::CONTENT_DISPOSITION,
                        format!(
                            "{disposition}; filename=\"attachment\"; filename*=UTF-8''{encoded}"
                        ),
                    ),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
                    (header::CACHE_CONTROL, "private, no-store".into()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"첨부 파일을 찾을 수 없습니다."})),
        )
            .into_response(),
    }
}
