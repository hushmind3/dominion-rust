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
        let stale_settling = stale["status"].as_str() == Some("settling")
            && stale["settleExpiresAtMs"]
                .as_i64()
                .unwrap_or(i64::MAX)
                <= now_ms;
        if stale_settling {
            if let Some(id) = stale["id"].as_str() {
                let _ = state.store.patch_if(
                    "experiment_request",
                    id,
                    "status",
                    &json!("settling"),
                    &json!({
                        "status": "queued",
                        "claimToken": Value::Null,
                        "settleToken": Value::Null,
                        "reason": "settlement expired",
                    }),
                );
            }
            continue;
        }
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
        let Some(request_id) = request["id"].as_str().map(str::to_string) else {
            continue;
        };
        let kind = request["kind"].as_str().unwrap_or("strategy");
        if kind == "policy" {
            let policy_id = request["policyId"].as_str().unwrap_or("");
            let Some(policy) = state.store.get("adaptation_policy", policy_id) else {
                let _ = state.store.patch_if(
                    "experiment_request",
                    &request_id,
                    "status",
                    &json!("queued"),
                    &json!({"status":"failed","reason":"정책 후보를 찾을 수 없습니다.",
                            "completedAt": chrono::Utc::now().to_rfc3339()}),
                );
                continue;
            };
            if !matches!(policy["status"].as_str(), Some("candidate" | "active")) {
                let _ = state.store.patch_if(
                    "experiment_request",
                    &request_id,
                    "status",
                    &json!("queued"),
                    &json!({"status":"failed","reason":"정책 후보가 활성 상태가 아닙니다.",
                            "completedAt": chrono::Utc::now().to_rfc3339()}),
                );
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
                return Json(json!({
                    "request": claimed,
                    "strategy": Value::Null,
                    "policy": {
                        "id": policy["id"],
                        "slot": policy["slot"],
                        "name": policy["name"],
                        "revision": policy["revision"],
                        "parameters": policy["parameters"],
                        "status": policy["status"],
                    }
                }))
                .into_response();
            }
            continue;
        }
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

/// Requeues every request claimed by a runner that exited or crashed.
pub fn release_runner_claims(state: &AppState, runner_id: &str) -> usize {
    let mut released = 0;
    for request in state.store.all("experiment_request") {
        if request["status"].as_str() != Some("claimed")
            || request["claimedBy"].as_str() != Some(runner_id)
        {
            continue;
        }
        let Some(id) = request["id"].as_str() else {
            continue;
        };
        if state
            .store
            .patch_if(
                "experiment_request",
                id,
                "status",
                &json!("claimed"),
                &json!({
                    "status": "queued",
                    "claimToken": Value::Null,
                    "reason": "runner exited without completing",
                }),
            )
            .is_some()
        {
            released += 1;
        }
    }
    released
}

pub async fn release_runner(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let runner_id = body["runnerId"].as_str().unwrap_or("");
    if runner_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "runnerId is required").into_response();
    }
    let released = release_runner_claims(&state, runner_id);
    Json(json!({"released": released})).into_response()
}

fn recompute_semantic_deltas(plans: &[Value], task: &str) -> (f64, f64) {
    use std::collections::BTreeMap;
    let mut pairs: BTreeMap<u64, (Option<f64>, Option<f64>, Option<f64>, Option<f64>)> =
        BTreeMap::new();
    for item in plans {
        let Some(pair) = item["pair"].as_u64() else {
            continue;
        };
        let plan = item["plan"].as_str().unwrap_or("");
        let entry = pairs.entry(pair).or_default();
        match item["arm"].as_str() {
            Some("baseline") => {
                entry.0 = Some(
                    crate::engine::plan_quality::score(plan, task)["total"]
                        .as_f64()
                        .unwrap_or(0.0),
                );
                entry.1 = Some(
                    crate::engine::plan_quality::score_independent(plan, task)["total"]
                        .as_f64()
                        .unwrap_or(0.0),
                );
            }
            Some("treatment") => {
                entry.2 = Some(
                    crate::engine::plan_quality::score(plan, task)["total"]
                        .as_f64()
                        .unwrap_or(0.0),
                );
                entry.3 = Some(
                    crate::engine::plan_quality::score_independent(plan, task)["total"]
                        .as_f64()
                        .unwrap_or(0.0),
                );
            }
            _ => {}
        }
    }
    let mut main = Vec::new();
    let mut independent = Vec::new();
    for (_, (b, bi, t, ti)) in pairs {
        if let (Some(b), Some(t)) = (b, t) {
            main.push(t - b);
        }
        if let (Some(b), Some(t)) = (bi, ti) {
            independent.push(t - b);
        }
    }
    let mean = |values: &[f64]| {
        if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f64>() / values.len() as f64
        }
    };
    (mean(&main), mean(&independent))
}

pub async fn complete(
    State(state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(request) = state.store.get("experiment_request", &request_id) else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"unknown request"}))).into_response();
    };
    let token = body["claimToken"].as_str().unwrap_or("").to_string();
    if request["status"].as_str() != Some("claimed")
        || request["claimToken"].as_str() != Some(token.as_str())
        || token.is_empty()
    {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"request is not claimed by this token"})),
        )
            .into_response();
    }
    let now_ms = chrono::Utc::now().timestamp_millis();
    if request["claimExpiresAtMs"].as_i64().unwrap_or(i64::MAX) < now_ms {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"claim expired; request must be reclaimed"})),
        )
            .into_response();
    }
    // Atomically take the settlement slot before any promotion write. A stale
    // or racing runner loses the CAS and cannot promote.
    let settle_token = uuid::Uuid::new_v4().to_string();
    let Some(request) = state.store.patch_if(
        "experiment_request",
        &request_id,
        "claimToken",
        &json!(token.as_str()),
        &json!({
            "status": "settling",
            "claimToken": Value::Null,
            "settleToken": settle_token,
            "settlingAt": chrono::Utc::now().to_rfc3339(),
            "settleExpiresAtMs": now_ms + 300_000,
        }),
    ) else {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"claim is no longer active"})),
        )
            .into_response();
    };
    let strategy_id = request["strategyId"].as_str().unwrap_or("");
    let policy_id = request["policyId"].as_str().unwrap_or("");
    let verdict = body["verdict"]
        .as_str()
        .unwrap_or("inconclusive")
        .to_string();
    let mut body = body;
    let verdict = verdict.as_str();
    let mut promotion = Value::Null;
    let mut promotion_error = Value::Null;
    let mut promotion_allowed = verdict != "not_evaluable";
    if request["kind"].as_str() == Some("policy") && promotion_allowed {
        use crate::engine::plan_quality;
        let version_matches = body["semanticEvaluator"]["id"].as_str()
            == Some(plan_quality::EVALUATOR_ID)
            && body["semanticEvaluator"]["version"].as_u64()
                == Some(plan_quality::EVALUATOR_VERSION as u64)
            && body["independentEvaluator"]["id"].as_str()
                == Some(plan_quality::INDEPENDENT_EVALUATOR_ID)
            && body["independentEvaluator"]["version"].as_u64()
                == Some(plan_quality::INDEPENDENT_EVALUATOR_VERSION as u64);
        let task = body["task"].as_str().unwrap_or("");
        let plans = body["plans"].as_array().cloned().unwrap_or_default();
        if !version_matches {
            promotion_allowed = false;
            promotion_error = json!("evaluator version mismatch");
        } else if plans.is_empty() {
            promotion_allowed = false;
            promotion_error = json!("plans missing; independent evaluation impossible");
        } else {
            let (main_delta, independent_delta) = recompute_semantic_deltas(&plans, task);
            if main_delta < plan_quality::MIN_SEMANTIC_DELTA
                || independent_delta < plan_quality::MIN_SEMANTIC_DELTA
            {
                promotion_allowed = false;
                promotion_error = json!(format!(
                    "semantic gate rejected (main={main_delta:.3}, independent={independent_delta:.3})"
                ));
            } else {
                body["semanticDelta"] = json!(main_delta);
                body["independentSemanticDelta"] = json!(independent_delta);
            }
        }
    }
    if promotion_allowed {
        let applied = if request["kind"].as_str() == Some("policy") {
            crate::engine::policy::apply_experiment_result(&state, policy_id, &body)
        } else {
            crate::engine::improve::apply_experiment_result(&state, strategy_id, &body)
        };
        match applied {
            Ok(record) => promotion = record,
            Err(error) => promotion_error = json!(error),
        }
    }
    let completed = state.store.patch_if(
        "experiment_request",
        &request_id,
        "settleToken",
        &json!(settle_token.as_str()),
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
            let selection = saved["searchId"]
                .as_str()
                .and_then(|search_id| crate::engine::policy::select_best(&state, search_id));
            state.store.audit(
                "experiment.request_completed",
                json!({
                    "requestId": request_id,
                    "strategyId": strategy_id,
                    "experimentId": body["experimentId"],
                    "verdict": verdict,
                }),
            );
            Json(json!({
                "request": saved,
                "promotion": promotion,
                "searchSelection": selection,
            }))
            .into_response()
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
    if let Some(policy_id) = body["policyId"].as_str().filter(|value| !value.is_empty()) {
        let Some(policy) = state.store.get("adaptation_policy", policy_id) else {
            return (StatusCode::BAD_REQUEST, "unknown policy").into_response();
        };
        let request_id = format!(
            "experiment-request-policy-{}",
            crate::engine::memory::stable_id(&[
                policy_id,
                policy["revision"].as_str().unwrap_or("")
            ])
        );
        if let Some(existing) = state.store.get("experiment_request", &request_id) {
            return Json(existing).into_response();
        }
        let request = json!({
            "id": request_id,
            "kind": "policy",
            "policyId": policy_id,
            "slot": policy["slot"],
            "revision": policy["revision"],
            "status": "queued",
            "fixture": body["fixture"].as_str().unwrap_or("planning_evaluation"),
            "searchId": body["searchId"],
            "minIndependentUnits": crate::engine::policy::MIN_EXPERIMENT_UNITS,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        return match state.store.put("experiment_request", &request) {
            Some(saved) => {
                state.bus.record("experiment_request", &saved);
                Json(saved).into_response()
            }
            None => (StatusCode::INTERNAL_SERVER_ERROR, "could not persist").into_response(),
        };
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
