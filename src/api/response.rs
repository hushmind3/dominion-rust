use axum::{http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json};

pub(super) fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

