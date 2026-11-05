use crate::engine::memory::MemoryScope;
use crate::engine::{improve, procedure};
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};

/// Benchmark-only seed endpoint. It never exists in normal mode.
pub async fn seed_strategy(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let agent_id = body["agentId"].as_str().unwrap_or("");
    let text = body["text"].as_str().unwrap_or("");
    let rationale = body["rationale"]
        .as_str()
        .unwrap_or("benchmark strategy with a machine-checkable procedure");
    let mode = body["assistanceMode"].as_str().unwrap_or("model_guided");
    if agent_id.is_empty() || text.is_empty() {
        return (StatusCode::BAD_REQUEST, "agentId and text are required").into_response();
    }
    let verification = procedure::normalize(&body["verification"]);
    if verification["type"].as_str() != Some("procedure") {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid verification", "verification": verification}))).into_response();
    }
    let scope = MemoryScope {
        session_id: "benchmark".into(),
        agent_id: Some(agent_id.to_string()),
        partition_key: "local".into(),
    };
    let Some(record) = improve::propose(
        &state,
        &scope,
        "strategy",
        text,
        rationale,
        &["benchmark-seed".to_string()],
        None,
        "agent",
    ) else {
        return (StatusCode::BAD_REQUEST, "could not create strategy").into_response();
    };
    let id = record["id"].as_str().unwrap_or("");
    let Some(record) = improve::attach_verification(&state, id, &verification) else {
        return (StatusCode::BAD_REQUEST, "could not attach verification").into_response();
    };
    let record = improve::set_authority(&state, id, "standing").unwrap_or(record);
    let record = improve::set_assistance_mode(&state, id, mode).unwrap_or(record);
    Json(record).into_response()
}
