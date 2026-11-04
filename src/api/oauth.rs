use crate::engine::oauth;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn providers(State(state): State<AppState>) -> Json<Value> {
    Json(json!(oauth::status(&state)))
}

pub async fn save(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let client_id = body
        .get("clientId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let client_secret = body
        .get("clientSecret")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match oauth::save(&state, &id, client_id, client_secret) {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn start(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match oauth::start(&state, &id) {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn callback(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let code = query.get("code").cloned().unwrap_or_default();
    let state_token = query.get("state").cloned().unwrap_or_default();
    match oauth::callback(&state, &id, &code, &state_token).await {
        Ok(result) => Html(format!(
            "<!doctype html><meta charset=\"utf-8\"><title>DOMINION</title>\
             <body style=\"font-family:-apple-system,sans-serif;padding:40px;background:#F6F3EC;color:#24211D\">\
             <h2>연결 완료</h2><p>{} 계정이 연결되었습니다.</p>\
             <p>이 창을 닫고 DOMINION으로 돌아가세요.</p>\
             <script>setTimeout(()=>window.close(),1200)</script>",
            result
                .get("account")
                .map(|value| value.to_string().trim_matches('"').to_string())
                .unwrap_or_else(|| id.clone())
        ))
        .into_response(),
        Err(error) => Html(format!(
            "<!doctype html><meta charset=\"utf-8\"><title>DOMINION</title>\
             <body style=\"font-family:-apple-system,sans-serif;padding:40px;background:#F6F3EC;color:#8a1f1f\">\
             <h2>연결 실패</h2><pre style=\"white-space:pre-wrap\">{error}</pre>"
        ))
        .into_response(),
    }
}

pub async fn disconnect(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    oauth::disconnect(&state, &id);
    Json(json!({ "ok": true, "id": id, "connected": false }))
}
