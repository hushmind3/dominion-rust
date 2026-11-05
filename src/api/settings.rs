use crate::state::AppState;
use axum::{extract::{State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn assistant_settings(State(state): State<AppState>) -> Json<Value> {
    Json(
        state
            .store
            .get("assistant_settings", "global")
            .unwrap_or_else(|| json!({"id":"global","instructions":"","enabled":true})),
    )
}

pub async fn save_assistant_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(instructions) = body["instructions"]
        .as_str()
        .filter(|text| text.chars().count() <= 16000)
    else {
        return error(
            StatusCode::BAD_REQUEST,
            "공통 지침은 16000자 이내 문자열이어야 합니다.",
        );
    };
    if body
        .get("enabled")
        .is_some_and(|enabled| !enabled.is_boolean())
    {
        return error(StatusCode::BAD_REQUEST, "enabled는 true/false여야 합니다.");
    }
    match state.store.put("assistant_settings",&json!({"id":"global","instructions":instructions,"enabled":body["enabled"].as_bool().unwrap_or(true),"source":"owner","updatedAt":chrono::Utc::now().to_rfc3339()})){
        Some(saved)=>{state.bus.record("assistant_settings",&saved);Json(saved).into_response()},None=>error(StatusCode::INTERNAL_SERVER_ERROR,"공통 지침을 저장할 수 없습니다."),
    }
}
