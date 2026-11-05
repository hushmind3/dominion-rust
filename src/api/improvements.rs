use crate::{engine::improve, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(improve::list(&state)))
}

pub async fn decide(
    State(state): State<AppState>,
    Path((id, action)): Path<(String, String)>,
) -> Response {
    let status = match action.as_str() {
        "approve" => "adopted",
        "reject" => "rejected",
        _ => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"approve 또는 reject"})))
                .into_response()
        }
    };
    match improve::set_status(&state, &id, status) {
        Some(record) => Json(record).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"개선안을 찾을 수 없습니다."})),
        )
            .into_response(),
    }
}
