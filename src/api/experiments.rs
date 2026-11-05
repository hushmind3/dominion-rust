//! Durable claim/complete lifecycle for queued experiment requests.
//!
//! The isolated benchmark runner claims one request, runs paired trials in its
//! own data root, then reports the result here. Claims are CAS-based and
//! released on failure so a crashed runner cannot strand a request forever.
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};

const MAX_ATTEMPTS: u64 = 3;

pub async fn claim(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let runner = body["runnerId"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("experiment-runner");
    let ttl_seconds = body["ttlSeconds"].as_u64().unwrap_or(900).clamp(30, 3_600);
    // Requeue claims whose runner crashed past the TTL. The CAS keeps two
    // reclaimers from both winning the same request.
    let now_ms = chrono::Utc::now().timestamp_millis();
    for stale in state.store.all("experiment_request") {
        if stale["status"].as_str() == Some("claimed")
            && stale["claimExpiresAtMs"].as_i64().unwrap_or(i64::MAX) <= now_ms
        {
            if let Some(id) = stale["id"].as_str() {
                let _ = state.store.patch_if(
                    "experiment_request",
                    id,
                    "status",
                    &json!("claimed"),
                    &json!({
                        "status": "queued",
                        "claimToken": Value::Null,
                        "reason": "runner claim expired",
                    }),
                );
            }
        }
    }
    let mut requests = state.store.all("experiment_request");
    requests.sort_by(|a, b| {
        a["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(b["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    for request in requests {
        if request["status"].as_str() != Some("queued") {
            continue;
        }
        // Policy experiments need a planning-quality evaluator; keep them
        // queued instead of failing them through the procedure runner.
        if request["kind"].as_str() == Some("policy") {
            continue;
        }
        let Some(request_id) = request["id"].as_str().map(str::to_string) else {
            continue;
        };
        let strategy_id = request["strategyId"].as_str().unwrap_or("");
        let strategy = state.store.get("improvement", strategy_id);
        let unavailable = strategy.as_ref().and_then(|strategy| {
            if strategy["status"].as_str() != Some("adopted")
                && strategy["status"].as_str() != Some("validated")
            {
                Some("전략이 더 이상 활성 상태가 아닙니다.")
            } else if strategy["verification"]["type"].as_str() != Some("procedure") {
                Some("기계 검증 가능한 procedure가 없습니다.")
            } else {
                None
            }
        });
        if let Some(reason) = unavailable {
            let failed = state.store.patch_if(
                "experiment_request",
                &request_id,
                "status",
                &json!("queued"),
                &json!({
                    "status": "failed",
                    "reason": reason,
                    "completedAt": chrono::Utc::now().to_rfc3339(),
                }),
            );
            if let Some(saved) = failed {
                state.bus.record("experiment_request", &saved);
            }
            continue;
        }
        let attempts = request["attempts"].as_u64().unwrap_or(0) + 1;
        let token = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let claimed = state.store.patch_if(
            "experiment_request",
            &request_id,
            "status",
            &json!("queued"),
            &json!({
                "status": "claimed",
                "claimToken": token,
                "claimedBy": runner,
                "claimedAt": chrono::Utc::now().to_rfc3339(),
                "claimExpiresAtMs": now + (ttl_seconds as i64) * 1000,
                "attempts": attempts,
            }),
        );
        if let Some(claimed) = claimed {
            state.bus.record("experiment_request", &claimed);
            let strategy = strategy.unwrap_or(Value::Null);
            return Json(json!({
                "request": claimed,
                "strategy": {
                    "id": strategy["id"],
                    "revision": strategy["revision"],
                    "text": strategy["text"],
                    "verification": strategy["verification"],
                    "assistanceMode": strategy["assistanceMode"],
                    "authority": strategy["authority"],
                    "status": strategy["status"],
                }
            }))
            .into_response();
        }
    }
    (StatusCode::NO_CONTENT, Json(json!({"status":"no_queued_request"}))).into_response()
}

pub async fn complete(
    State(state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(request) = state.store.get("experiment_request", &request_id) else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"unknown request"}))).into_response();
    };
    let token = body["claimToken"].as_str().unwrap_or("");
    if request["status"].as_str() != Some("claimed")
        || request["claimToken"].as_str() != Some(token)
        || token.is_empty()
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"request is not claimed by this token"})),
        )
            .into_response();
    }
    let strategy_id = request["strategyId"].as_str().unwrap_or("");
    let verdict = body["verdict"].as_str().unwrap_or("inconclusive");
    let mut promotion = Value::Null;
    let mut promotion_error = Value::Null;
    if verdict != "not_evaluable" {
        match crate::engine::improve::apply_experiment_result(&state, strategy_id, &body) {
            Ok(record) => promotion = record,
            Err(error) => promotion_error = json!(error),
        }
    }
    let completed = state.store.patch_if(
        "experiment_request",
        &request_id,
        "claimToken",
        &json!(token),
        &json!({
            "status": "completed",
            "completedAt": chrono::Utc::now().to_rfc3339(),
            "experimentId": body["experimentId"],
            "verdict": verdict,
            "delta": body["delta"],
            "independentUnits": body["independentUnits"],
            "promotionStatus": promotion["status"],
            "promotionEvidence": promotion["evidenceGrade"],
            "promotionError": promotion_error,
        }),
    );
    match completed {
        Some(saved) => {
            state.bus.record("experiment_request", &saved);
            state.store.audit(
                "experiment.request_completed",
                json!({
                    "requestId": request_id,
                    "strategyId": strategy_id,
                    "experimentId": body["experimentId"],
                    "verdict": verdict,
                }),
            );
            Json(json!({"request": saved, "promotion": promotion})).into_response()
        }
        None => (
            StatusCode::CONFLICT,
            Json(json!({"error":"request changed during completion"})),
        )
            .into_response(),
    }
}

pub async fn release(
    State(state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let token = body["claimToken"].as_str().unwrap_or("");
    let Some(request) = state.store.get("experiment_request", &request_id) else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"unknown request"}))).into_response();
    };
    if request["claimToken"].as_str() != Some(token) || token.is_empty() {
        return (StatusCode::CONFLICT, Json(json!({"error":"wrong claim token"}))).into_response();
    }
    let attempts = request["attempts"].as_u64().unwrap_or(0);
    let status = if attempts >= MAX_ATTEMPTS {
        "failed"
    } else {
        "queued"
    };
    let released = state.store.patch_if(
        "experiment_request",
        &request_id,
        "claimToken",
        &json!(token),
        &json!({
            "status": status,
            "reason": body["reason"],
            "claimToken": Value::Null,
            "releasedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    match released {
        Some(saved) => {
            state.bus.record("experiment_request", &saved);
            Json(json!({"request": saved})).into_response()
        }
        None => (StatusCode::CONFLICT, Json(json!({"error":"claim changed"}))).into_response(),
    }
}

/// Benchmark-only helper used by tests and manual scheduling tools.
pub async fn enqueue(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if std::env::var("DOMINION_BENCHMARK").as_deref() != Ok("1") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let strategy_id = body["strategyId"].as_str().unwrap_or("");
    let Some(strategy) = state.store.get("improvement", strategy_id) else {
        return (StatusCode::BAD_REQUEST, "unknown strategy").into_response();
    };
    if strategy["verification"]["type"].as_str() != Some("procedure") {
        return (StatusCode::BAD_REQUEST, "strategy needs a procedure").into_response();
    }
    let revision = strategy["revision"].as_str().unwrap_or("");
    let request_id = format!(
        "experiment-request-{}",
        crate::engine::memory::stable_id(&[strategy_id, revision])
    );
    if let Some(existing) = state.store.get("experiment_request", &request_id) {
        return Json(existing).into_response();
    }
    let request = json!({
        "id": request_id,
        "strategyId": strategy_id,
        "revision": revision,
        "status": "queued",
        "fixture": body["fixture"].as_str().unwrap_or("audit_report"),
        "minIndependentUnits": crate::engine::improve::MIN_EXPERIMENT_UNITS,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    match state.store.put("experiment_request", &request) {
        Some(saved) => {
            state.bus.record("experiment_request", &saved);
            Json(saved).into_response()
        }
        None => (StatusCode::INTERNAL_SERVER_ERROR, "could not persist").into_response(),
    }
}
