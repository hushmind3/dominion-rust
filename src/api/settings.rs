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

fn default_owner_profile() -> Value {
    json!({"id":"global","name":"나","avatar":"","updatedAt":null})
}

/// The owner is a person, not an agent. Keep their visual identity in one
/// durable record shared by Chat, Rooms and the workspace navigation.
pub async fn owner_profile(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.get("owner_profile", "global")
        .unwrap_or_else(default_owner_profile))
}

pub(crate) fn valid_profile_avatar(value: &str) -> bool {
    use base64::Engine as _;
    if value.is_empty() { return true; }
    let Some((kind, encoded)) = value.split_once(";base64,") else { return false; };
    if !matches!(kind, "data:image/png" | "data:image/jpeg" | "data:image/webp")
        || encoded.len() > 1_500_000 || encoded.is_empty()
    { return false; }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    if bytes.is_empty() || bytes.len() > 1_048_576 { return false; }
    match kind {
        "data:image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "data:image/jpeg" => bytes.starts_with(&[0xff,0xd8,0xff]),
        "data:image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF"
            && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

pub async fn save_owner_profile(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let Some(name) = body.get("name").and_then(Value::as_str)
        .map(str::trim).filter(|name| !name.is_empty() && name.chars().count() <= 60)
    else { return error(StatusCode::BAD_REQUEST, "사용자 이름은 1~60자여야 합니다."); };
    let Some(avatar) = body.get("avatar").and_then(Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "사용자 아이콘은 문자열이어야 합니다.");
    };
    if !valid_profile_avatar(avatar) {
        return error(StatusCode::BAD_REQUEST,
            "사용자 아이콘은 1MB 이하의 PNG/JPEG/WebP 이미지여야 합니다.");
    }
    match state.store.put("owner_profile", &json!({
        "id":"global", "name":name, "avatar":avatar,
        "updatedAt":chrono::Utc::now().to_rfc3339()
    })) {
        Some(saved) => {
            state.bus.record("owner_profile", &saved);
            Json(saved).into_response()
        },
        None => error(StatusCode::INTERNAL_SERVER_ERROR,
            "사용자 프로필을 저장하지 못했습니다."),
    }
}
