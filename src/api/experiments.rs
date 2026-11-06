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
            let Some(id) = stale["id"].as_str() else {
                continue;
            };
            let intent = &stale["promotionIntent"];
            let target = if intent["kind"].as_str() == Some("policy") {
                intent["targetId"]
                    .as_str()
                    .and_then(|id| state.store.get("adaptation_policy", id))
            } else {
                intent["targetId"]
                    .as_str()
                    .and_then(|id| state.store.get("improvement", id))
            };
            let applied = target.as_ref().is_some_and(|target| {
                target["lastExperiment"]["experimentId"]
                    == intent["experimentId"]
            });
            if applied {
                // The promotion write landed before the crash; finish the
                // request from the intent instead of applying it again.
                let target = target.unwrap_or(Value::Null);
                let _ = state.store.patch_if(
                    "experiment_request",
                    id,
                    "status",
                    &json!("settling"),
                    &json!({
                        "status": "completed",
                        "completedAt": chrono::Utc::now().to_rfc3339(),
                        "promotionStatus": target["status"],
                        "promotionEvidence": target["evidenceGrade"],
                        "promotionError": "recovered after restart; promotion not reapplied",
                        "evidenceVerification": {"verified": true, "recovered": true},
                    }),
                );
                state.store.audit(
                    "experiment.settlement_recovered",
                    json!({"requestId": id, "experimentId": intent["experimentId"]}),
                );
            } else {
                let _ = state.store.patch_if(
                    "experiment_request",
                    id,
                    "status",
                    &json!("settling"),
                    &json!({
                        "status": "queued",
                        "claimToken": Value::Null,
                        "settleToken": Value::Null,
                        "reason": "settlement expired before promotion",
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
            let scoped_preflight = request["evaluationScope"].is_string()
                && request["slot"] == "mutation.preflight";
            if request["slot"] == "mutation.preflight"
                && (policy["parameters"]["kernel"] != crate::engine::preflight_fixture::GUARD_KERNEL
                    || policy["parameters"]["workload"] != crate::engine::preflight_fixture::GUARD_WORKLOAD)
            {
                if let Some(saved) = state.store.patch_if(
                    "experiment_request", &request_id, "status", &json!("queued"),
                    &json!({"status":"superseded","reason":"This candidate predates the model-mediated guard evaluation contract."}),
                ) {
                    state.bus.record("experiment_request", &saved);
                }
                continue;
            }
            if !matches!(policy["status"].as_str(), Some("candidate" | "active"))
                && !(scoped_preflight && matches!(policy["status"].as_str(), Some("superseded" | "rolled_back")))
            {
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

fn trial_success(trial: &Value) -> bool {
    let requires_compliance = trial["requiresCompliance"].as_bool().unwrap_or(true);
    trial["status"].as_str() == Some("completed")
        && trial["auditOk"].as_bool() == Some(true)
        && trial["reportOk"].as_bool() == Some(true)
        && trial["taskOutcome"].as_str() == Some("succeeded")
        && (!requires_compliance || trial["compliance"].as_bool() == Some(true))
        && trial["errorClass"].as_str().unwrap_or("").is_empty()
}

/// Recomputes ITT success numbers from submitted raw trials and rejects a
/// completion whose claimed verdict/delta/units do not match the evidence.
pub fn verify_evidence(
    state: &AppState,
    body: &Value,
    request: &Value,
) -> Result<(f64, u64, String, u64, u64), String> {
    use sha2::Digest;
    use std::collections::BTreeMap;
    let manifest = body["manifest"]
        .as_object()
        .ok_or("frozen manifest is missing")?;
    let pairs = manifest
        .get("pairs")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or("manifest.pairs is required")?;
    let manifest_value = Value::Object(manifest.clone());
    let expected_hash = format!(
        "{:x}",
        sha2::Sha256::digest(manifest_value.to_string().as_bytes())
    );
    if body["manifestHash"].as_str() != Some(expected_hash.as_str()) {
        return Err("manifest hash mismatch".into());
    }
    // The runner's manifest must match the copy the server froze before any
    // trial started; a self-consistent runner manifest is not enough.
    if let Some(frozen) = request["frozenManifest"].as_object() {
        if let Some(minimum_pairs) = frozen
            .get("totalBudget")
            .and_then(|budget| budget.get("pairs"))
            .and_then(Value::as_u64)
        {
            let submitted = manifest
                .get("pairs")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if submitted < minimum_pairs {
                return Err(format!(
                    "submitted pairs {submitted} are below the frozen minimum {minimum_pairs}"
                ));
            }
        }
        if let (Some(fixture), Some(fixtures)) = (
            manifest.get("fixture").and_then(Value::as_str),
            frozen.get("fixtures").and_then(Value::as_array),
        ) {
            if !fixtures
                .iter()
                .any(|item| item.as_str() == Some(fixture))
            {
                return Err("submitted fixture differs from the server-frozen contract".into());
            }
        }
    }
    let trials = body["trials"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("raw trials are missing")?;
    let preflight = request["slot"] == "mutation.preflight";
    if preflight {
        verify_preflight_trials(body, request, trials, pairs)?;
    }
    let mut by_pair: BTreeMap<u64, (Option<&Value>, Option<&Value>)> = BTreeMap::new();
    for trial in trials {
        let pair = trial["pair"]
            .as_u64()
            .filter(|pair| *pair < pairs)
            .ok_or("trial pair out of range")?;
        if let Some(revision) = trial["revision"].as_str() {
            if request["revision"].as_str().is_some_and(|rev| rev != revision) {
                return Err(format!("trial revision {revision} does not match the request"));
            }
        }
        match trial["arm"].as_str() {
            Some("baseline") => {
                let slot = by_pair.entry(pair).or_default();
                if slot.0.is_some() {
                    return Err(format!("duplicate baseline trial for pair {pair}"));
                }
                slot.0 = Some(trial);
            }
            Some("treatment") => {
                let slot = by_pair.entry(pair).or_default();
                if slot.1.is_some() {
                    return Err(format!("duplicate treatment trial for pair {pair}"));
                }
                slot.1 = Some(trial);
            }
            _ => return Err("trial arm must be baseline or treatment".into()),
        }
    }
    let mut deltas = Vec::new();
    let mut timeouts = 0u64;
    for pair in 0..pairs {
        let Some((baseline, treatment)) = by_pair.get(&pair) else {
            return Err(format!("trial pair {pair} is incomplete"));
        };
        let (Some(baseline), Some(treatment)) = (baseline, treatment) else {
            return Err(format!("trial pair {pair} is missing an arm"));
        };
        for trial in [baseline, treatment] {
            if trial["errorClass"].as_str() == Some("timeout") {
                timeouts += 1;
            }
            if trial_success(trial) {
                // A claimed success must point at a real execution whose own
                // outcome is success. Isolated benchmark runners keep their
                // own data root, so for those the structured artifact and
                // operation evidence is checked instead of a parent execution.
                let execution = trial["executionId"]
                    .as_str()
                    .and_then(|id| state.store.get("execution", id));
                if let Some(execution) = execution {
                    if execution["taskOutcome"].as_str() != Some("succeeded")
                        || execution["failureClass"]
                            .as_str()
                            .is_some_and(|class| !class.is_empty())
                    {
                        return Err(format!(
                            "trial claims success but execution {} did not succeed",
                            trial["executionId"]
                        ));
                    }
                } else if request["externalRunner"].as_bool() == Some(true) {
                    let artifact_ok = ["audit", "report"].iter().any(|key| {
                        let artifact = &trial["artifacts"][key];
                        artifact["exists"].as_bool() == Some(true)
                            && artifact["sha256"]
                                .as_str()
                                .is_some_and(|hash| !hash.is_empty())
                    });
                    let operation_ok = trial["operations"]
                        .as_array()
                        .is_some_and(|operations| {
                            operations
                                .iter()
                                .any(|operation| operation["status"].as_str() == Some("completed"))
                        });
                    if !artifact_ok || !operation_ok {
                        return Err(format!(
                            "successful trial for pair {} has no verifiable artifact/operation evidence",
                            trial["pair"]
                        ));
                    }
                } else {
                    return Err(format!(
                        "successful trial for pair {} has no server execution evidence",
                        trial["pair"]
                    ));
                }
            }
        }
        let b = if trial_success(baseline) { 1.0 } else { 0.0 };
        let t = if trial_success(treatment) { 1.0 } else { 0.0 };
        deltas.push(t - b);
    }
    let n = deltas.len() as u64;
    let mean = deltas.iter().sum::<f64>() / n as f64;
    let alpha = 0.05;
    let halfwidth = (2.0 * (2.0f64 / alpha).ln() / n as f64).sqrt();
    let lower = (mean - halfwidth).max(-1.0);
    let upper = (mean + halfwidth).min(1.0);
    let verdict = experiment_verdict(mean, lower, upper, n);
    let claimed_delta = body["delta"].as_f64().unwrap_or(f64::NAN);
    let claimed_units = body["independentUnits"].as_u64().unwrap_or(0);
    let claimed_verdict = body["verdict"].as_str().unwrap_or("");
    if (claimed_delta - mean).abs() > 0.000_001
        || claimed_units != n
        || claimed_verdict != verdict
    {
        return Err(format!(
            "claimed result does not match raw trials (delta {} vs {mean:.6}, units {claimed_units} vs {n}, verdict {claimed_verdict} vs {verdict})",
            body["delta"]
        ));
    }
    Ok((mean, n, verdict.to_string(), timeouts, pairs))
}

fn verify_preflight_trials(
    body: &Value,
    request: &Value,
    trials: &[Value],
    pairs: u64,
) -> Result<(), String> {
    use crate::engine::preflight_fixture as fixture;
    let manifest = &body["manifest"];
    let split_hash = fixture::digest(json!(fixture::CASES).to_string().as_bytes());
    let timeout = manifest["budget"]["clientTimeoutSecs"].as_u64().unwrap_or(0);
    if manifest["preflightMode"] != fixture::GUARD_MODE
        || manifest["budget"] != fixture::budget(timeout)
        || !(30..=900).contains(&timeout)
        || manifest["toolCatalogHash"] != fixture::digest(fixture::TOOLS.join(",").as_bytes())
        || manifest["model"]["maxOutputTokens"] != fixture::MAX_OUTPUT_TOKENS
        || manifest["model"]["thinkingRequested"] != false
        || manifest["model"]["toolMode"] != "native"
        || manifest["fixture"] != fixture::ID
        || body["manifest"]["taskSplitManifestHash"] != split_hash
        || body["manifest"]["oracleVersion"] != fixture::ORACLE
        || body["oracleVersion"] != fixture::ORACLE
        || pairs > fixture::CASES.len() as u64
    {
        return Err("preflight holdout or oracle version mismatch".into());
    }
    if let Some(model) = request["evaluationModel"].as_str() {
        if body["manifest"]["model"]["model"] != model
            || body["manifest"]["model"]["preflightScope"] != request["evaluationScope"]
        {
            return Err("preflight experiment ran outside its queued model scope".into());
        }
    }
    for trial in trials {
        let pair = trial["pair"]
            .as_u64()
            .filter(|pair| *pair < pairs)
            .ok_or("preflight trial pair out of range")? as usize;
        let arm = match trial["arm"].as_str() {
            Some("baseline") => 0,
            Some("treatment") => 1,
            _ => return Err("invalid preflight arm".into()),
        };
        if matches!(trial["errorClass"].as_str(), Some("provider_rate_limited" | "provider_route_changed")) {
            return Err("preflight provider failure invalidated the comparison".into());
        }
        if trial["status"] == "invalid" {
            return Err("preflight experiment contained invalid trials".into());
        }
        let artifact = &trial["artifacts"]["report"];
        let target = format!("bench/{}", fixture::CASES[pair].0);
        let hash = artifact["sha256"].as_str().unwrap_or_default();
        let operations = trial["operations"]
            .as_array()
            .ok_or("preflight operations missing")?;
        let (written_hash, checked) = fixture::write_evidence(pair, operations);
        if operations.iter().any(|operation| operation["origin"] == "exact-replacement")
            || (trial["status"] == "completed"
                && (trial["modelCalls"].as_u64().unwrap_or(0) == 0
                    || trial["meteredCalls"].as_u64().unwrap_or(0) == 0))
        {
            return Err(format!("preflight guard comparison did not run the model for pair {pair}"));
        }
        let artifact_ok = artifact["exists"] == true
            && artifact["relativePath"] == target
            && fixture::file_ok(pair, hash);
        if trial["reportOk"] != artifact_ok
            || trial["auditOk"] != (written_hash.as_deref() == Some(hash))
            || trial["requiresCompliance"] != false
        {
            return Err(format!(
                "preflight file/operation oracle mismatch for pair {pair}"
            ));
        }
        for operation in operations
            .iter()
            .filter(|operation| operation["preflightRevision"].is_string())
        {
            if arm == 0 || operation["preflightRevision"] != request["revision"] {
                return Err(format!(
                    "unexpected preflight kernel revision for pair {pair}"
                ));
            }
        }
        // A treatment that overrode the guard cannot promote even if the final bytes match.
        if arm == 1
            && written_hash.is_some()
            && (!checked
                || operations.iter().any(|operation| {
                    operation["target"] == target
                        && operation["status"] == "completed"
                        && matches!(
                            operation["tool"].as_str(),
                            Some("workspace_write" | "file_write")
                        )
                        && operation["preflightRevision"] != request["revision"]
                }))
        {
            return Err(format!("unchecked treatment write for pair {pair}"));
        }
    }
    Ok(())
}

/// A powered paired experiment is only `preliminary_support` when the whole
/// interval is above zero, and `regressed` when it is entirely below zero.
/// The latter is what the strategy/policy rollback paths wait for.
fn experiment_verdict(mean: f64, lower: f64, upper: f64, units: u64) -> &'static str {
    if units < 10 {
        "inconclusive"
    } else if lower > 0.0 {
        "preliminary_support"
    } else if upper < 0.0 {
        "regressed"
    } else if mean == 0.0 {
        "inconclusive"
    } else {
        "inconclusive"
    }
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
            "promotionIntent": {
                "kind": request["kind"],
                "targetId": if request["kind"].as_str() == Some("policy") {
                    request["policyId"].clone()
                } else {
                    request["strategyId"].clone()
                },
                "experimentId": body["experimentId"],
            },
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
    let mut verdict = body["verdict"]
        .as_str()
        .unwrap_or("inconclusive")
        .to_string();
    let mut body = body;
    let mut promotion = Value::Null;
    let mut promotion_error = Value::Null;
    let mut promotion_allowed = verdict != "not_evaluable";
    let mut evidence_summary = Value::Null;
    if promotion_allowed {
        match verify_evidence(&state, &body, &request) {
            Ok((delta, units, recomputed_verdict, timeouts, planned)) => {
                body["delta"] = json!(delta);
                body["independentUnits"] = json!(units);
                body["verdict"] = json!(recomputed_verdict);
                verdict = recomputed_verdict;
                evidence_summary = json!({
                    "verified": true,
                    "plannedUnits": planned,
                    "units": units,
                    "timeouts": timeouts,
                });
            }
            Err(error) => {
                promotion_allowed = false;
                promotion_error = json!(format!("evidence rejected: {error}"));
                evidence_summary = json!({"verified": false, "error": error});
            }
        }
    }
    if request["kind"].as_str() == Some("policy")
        && promotion_allowed
        && verdict != "regressed"
        && request["slot"] != "mutation.preflight"
    {
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
    if request["slot"] == "mutation.preflight" {
        use crate::engine::preflight_fixture as fixture;
        let cohort = fixture::evaluation_cohort(&body["manifest"]["model"]);
        body["preflightCohort"] = if promotion_allowed { json!(cohort) } else { Value::Null };
        if promotion_allowed {
            let trials = body["trials"].as_array().map(Vec::as_slice).unwrap_or_default();
            let (ratio, timeout_increased) = fixture::guard_cost(trials);
            body["preflightRejection"] = match (ratio, timeout_increased) {
                (_, true) => json!("timeouts_increased"),
                (Some(ratio), false) if ratio > fixture::MAX_COST_RATIO => json!("token_cost_exceeded"),
                (None, false) => json!("missing_token_usage"),
                _ => Value::Null,
            };
            evidence_summary["tokenRatio"] = json!(ratio);
            evidence_summary["guardRejection"] = body["preflightRejection"].clone();
        }
    }
    // Independent claims can complete concurrently and must not overwrite
    // one another's evidence or activations.
    let _settlement = (request["kind"] == "policy")
        .then(|| state.policy_settlement.lock().unwrap());
    let mut promotion_records: Vec<(String, Value)> = Vec::new();
    if promotion_allowed {
        let target = if request["kind"].as_str() == Some("policy") {
            state.store.get("adaptation_policy", policy_id)
        } else {
            state.store.get("improvement", strategy_id)
        };
        let already_applied = target.as_ref().is_some_and(|target| {
            target["lastExperiment"]["experimentId"].as_str()
                == body["experimentId"].as_str()
        });
        if already_applied {
            promotion = target.unwrap_or(Value::Null);
            state.store.audit(
                "experiment.promotion_recovered",
                json!({"requestId": request_id, "experimentId": body["experimentId"]}),
            );
        } else {
            let computed = if request["kind"].as_str() == Some("policy") {
                crate::engine::policy::experiment_result_records(&state, policy_id, &body)
            } else {
                crate::engine::improve::experiment_result_record(&state, strategy_id, &body)
                    .map(|record| vec![("improvement".to_string(), record)])
            };
            match computed {
                Ok(records) => {
                    promotion = records
                        .iter()
                        .find(|(_, record)| {
                            record["id"].as_str()
                                == Some(if request["kind"].as_str() == Some("policy") {
                                    policy_id
                                } else {
                                    strategy_id
                                })
                        })
                        .map(|(_, record)| record.clone())
                        .unwrap_or(Value::Null);
                    promotion_records = records;
                }
                Err(error) => promotion_error = json!(error),
            }
        }
    }
    let mut completed_record = request.clone();
    completed_record["status"] = json!("completed");
    completed_record["completedAt"] = json!(chrono::Utc::now().to_rfc3339());
    completed_record["experimentId"] = body["experimentId"].clone();
    completed_record["verdict"] = json!(verdict);
    completed_record["delta"] = body["delta"].clone();
    completed_record["independentUnits"] = body["independentUnits"].clone();
    completed_record["promotionStatus"] = promotion["status"].clone();
    completed_record["promotionEvidence"] = promotion["evidenceGrade"].clone();
    completed_record["promotionError"] = promotion_error.clone();
    completed_record["evidenceVerification"] = evidence_summary.clone();
    completed_record["promotionAppliedAt"] = if promotion.is_null() {
        Value::Null
    } else {
        json!(chrono::Utc::now().to_rfc3339())
    };
    completed_record["settleToken"] = Value::Null;
    promotion_records.push(("experiment_request".to_string(), completed_record.clone()));
    // Promotion, supersede and request finalization commit together. A crash
    // cannot leave a half-applied policy or a completed request without it.
    let completed = match state.store.transaction_if_status(
        "experiment_request",
        &request_id,
        "settling",
        &promotion_records,
    ) {
        Ok(true) => {
            for (kind, record) in &promotion_records {
                if kind != "experiment_request" {
                    state.bus.record(kind, record);
                }
            }
            Some(completed_record)
        }
        Ok(false) => None,
        Err(error) => {
            state.store.audit(
                "experiment.settlement_transaction_failed",
                json!({"requestId": request_id, "error": error}),
            );
            None
        }
    };
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
            "externalRunner": true,
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
        "externalRunner": true,
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

pub async fn optimizers(State(state): State<AppState>) -> Response {
    Json(crate::engine::policy::optimizer_report(&state)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_verdicts_include_regression_and_require_power() {
        assert_eq!(experiment_verdict(0.8, 0.2, 1.0, 10), "preliminary_support");
        assert_eq!(experiment_verdict(-0.8, -1.0, -0.2, 10), "regressed");
        assert_eq!(experiment_verdict(-0.8, -1.0, -0.2, 9), "inconclusive");
        assert_eq!(experiment_verdict(0.05, -0.5, 0.6, 10), "inconclusive");
    }

    #[test]
    fn preflight_promotion_evidence_requires_the_checked_revision_and_file_bytes() {
        use crate::engine::preflight_fixture as fixture;
        let (name, seed, old, new) = fixture::CASES[0];
        let path = format!("bench/{name}");
        let observed = fixture::digest(format!("{seed}\n").as_bytes());
        let written = fixture::digest(format!("{}\n", seed.replacen(old, new, 1)).as_bytes());
        let body = json!({
            "oracleVersion": fixture::ORACLE,
            "manifest":{"fixture":fixture::ID,"oracleVersion":fixture::ORACLE,
                "preflightMode":fixture::GUARD_MODE,"budget":fixture::budget(60),
                "toolCatalogHash":fixture::digest(fixture::TOOLS.join(",").as_bytes()),
                "model":{"maxOutputTokens":fixture::MAX_OUTPUT_TOKENS,"thinkingRequested":false,"toolMode":"native"},
                "taskSplitManifestHash":fixture::digest(json!(fixture::CASES).to_string().as_bytes())},
        });
        let request = json!({"revision":"candidate-revision"});
        let mut trial = json!({
            "arm":"treatment","pair":0,"status":"completed","reportOk":true,"auditOk":true,
            "modelCalls":2,"meteredCalls":2,
            "requiresCompliance":false,"artifacts":{"report":{"relativePath":path,"exists":true,"sha256":written}},
            "operations":[
                {"tool":"workspace_read","target":path,"opSeq":1,"status":"completed","observedSha256":observed},
                {"tool":"workspace_write","target":path,"opSeq":2,"status":"completed","observedSha256":observed,
                    "writtenSha256":written,"preflightChecked":true,"preflightRevision":"candidate-revision"},
            ],
        });
        assert!(verify_preflight_trials(&body, &request, &[trial.clone()], 1).is_ok());
        trial["modelCalls"] = json!(0);
        assert!(verify_preflight_trials(&body, &request, &[trial.clone()], 1).is_err());
        trial["modelCalls"] = json!(2);
        trial["operations"][1]["origin"] = json!("exact-replacement");
        assert!(verify_preflight_trials(&body, &request, &[trial.clone()], 1).is_err());
        trial["operations"][1]["origin"] = json!("model");
        trial["operations"][1]["preflightRevision"] = Value::Null;
        assert!(verify_preflight_trials(&body, &request, &[trial.clone()], 1).is_err());
        trial["operations"][1]["preflightRevision"] = json!("candidate-revision");
        trial["artifacts"]["report"]["sha256"] = json!("unrelated-file");
        assert!(verify_preflight_trials(&body, &request, &[trial], 1).is_err());
    }
}
