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

/// Benchmark-only promotion endpoint. Runs or reports an isolated experiment;
/// small, inconclusive samples are recorded but never promote.
pub async fn record_experiment(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let strategy_id = body["strategyId"].as_str().unwrap_or("");
    if strategy_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "strategyId is required").into_response();
    }
    match improve::apply_experiment_result(&state, strategy_id, &body) {
        Ok(record) => Json(record).into_response(),
        Err(error) => {
            (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
        }
    }
}

/// Benchmark-only policy candidate registration (H9).
pub async fn register_policy(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    match crate::engine::policy::register(
        &state,
        body["slot"].as_str().unwrap_or(""),
        body["name"].as_str().unwrap_or(""),
        &body["parameters"],
        body["rationale"].as_str().unwrap_or(""),
    ) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

/// Benchmark-only policy activation/rollback from a paired experiment (H9).
pub async fn policy_experiment(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let policy_id = body["policyId"].as_str().unwrap_or("");
    if policy_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "policyId is required").into_response();
    }
    match crate::engine::policy::apply_experiment_result(&state, policy_id, &body) {
        Ok(record) => Json(record).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
    }
}

/// Benchmark-sandbox only: activate a policy candidate (or clear the slot) in
/// a disposable isolated engine so paired policy trials can run.
pub async fn policy_state(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let slot = body["slot"].as_str().unwrap_or("planning");
    if body["parameters"].is_object() {
        let name = body["name"].as_str().unwrap_or("policy-eval");
        match crate::engine::policy::register(
            &state,
            slot,
            name,
            &body["parameters"],
            "benchmark policy evaluation",
        ) {
            Ok(candidate) => match crate::engine::policy::force_activate(
                &state,
                candidate["id"].as_str().unwrap_or(""),
            ) {
                Some(active) => Json(json!({"active": active})).into_response(),
                None => (StatusCode::INTERNAL_SERVER_ERROR, "could not activate").into_response(),
            },
            Err(error) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
            }
        }
    } else {
        let cleared = crate::engine::policy::deactivate_slot(&state, slot);
        Json(json!({"cleared": cleared})).into_response()
    }
}

/// Benchmark-only: register a group of policy candidates that share one search
/// id and enqueue one experiment request per revision.
pub async fn policy_search(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let slot = body["slot"].as_str().unwrap_or("planning");
    let search_id = body["searchId"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("policy-search-{}", uuid::Uuid::new_v4()));
    let Some(candidates) = body["candidates"].as_array() else {
        return (StatusCode::BAD_REQUEST, "candidates are required").into_response();
    };
    let mut created = Vec::new();
    for candidate in candidates.iter().take(6) {
        let name = candidate["name"].as_str().unwrap_or("");
        let parameters = &candidate["parameters"];
        match crate::engine::policy::register(
            &state,
            slot,
            name,
            parameters,
            "benchmark policy search",
        ) {
            Ok(policy) => {
                let policy_id = policy["id"].as_str().unwrap_or("");
                let mut tagged = policy.clone();
                tagged["searchId"] = json!(search_id);
                if let Some(saved) = state.store.put("adaptation_policy", &tagged) {
                    state.bus.record("adaptation_policy", &saved);
                }
                if let Some((request, policy, _)) =
                    crate::engine::policy::enqueue_policy_request(
                        &state,
                        policy_id,
                        "planning_evaluation",
                        Some(&search_id),
                    )
                {
                    created.push(json!({"candidate": policy, "request": request}));
                }
            }
            Err(error) => {
                return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response()
            }
        }
    }
    Json(json!({"searchId": search_id, "created": created})).into_response()
}
