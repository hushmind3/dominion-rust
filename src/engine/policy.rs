//! H9 versioned policy slots.
//!
//! A policy slot names a replaceable algorithm/parameter set (for example the
//! planning strategy). New revisions are candidates; only an isolated paired
//! experiment can activate them, and activation supersedes the previous active
//! revision. This is deliberately separate from one-off strategy text: the
//! artifact is versioned and rollbackable.
use crate::state::AppState;
use serde_json::{json, Value};

pub const SLOTS: &[&str] = &["planning"];
pub const MIN_EXPERIMENT_UNITS: u64 = 8;

/// Minimum distinct recorded failures before a candidate is proposed.
pub const PROPOSER_MIN_EVIDENCE: usize = 3;

/// Derives planning-policy candidates from failure clusters. Proposals are
/// candidates only: the outer gate in `apply_experiment_result` requires an
/// independent >=8-unit experiment before activation.
pub fn propose_candidates(state: &AppState) -> Vec<Value> {
    use std::collections::BTreeMap;
    let mut clusters: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let mut executions = state.store.all("execution");
    executions.sort_by(|a, b| {
        b["startedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["startedAt"].as_str().unwrap_or(""))
    });
    for execution in executions.iter().take(500) {
        let Some(class) = execution["failureClass"].as_str() else {
            continue;
        };
        if matches!(class, "cancelled") {
            continue;
        }
        clusters.entry(class).or_default().push(json!({
            "executionId": execution["id"],
            "taskOutcome": execution["taskOutcome"],
            "recoveryHint": execution["recoveryHint"],
        }));
    }
    let mut created = Vec::new();
    for (class, evidence) in clusters {
        if evidence.len() < PROPOSER_MIN_EVIDENCE {
            continue;
        }
        let (name, parameters, rationale) = match class {
            "verification_failed" => (
                "plan-before-acting",
                json!({"outline": true}),
                "검증 실패가 반복되어 행동 전 계획을 세우는 정책을 제안합니다.",
            ),
            "limit_exceeded" | "model_error" => (
                "skip-extra-planning",
                json!({"outline": false}),
                "예산/모델 실패가 반복되어 추가 계획 호출을 생략하는 정책을 제안합니다.",
            ),
            _ => continue,
        };
        let evidence_ids = evidence
            .iter()
            .filter_map(|item| item["executionId"].as_str())
            .take(8)
            .collect::<Vec<_>>();
        let rationale = format!(
            "{rationale} (근거 {n}건: {ids})",
            n = evidence.len(),
            ids = evidence_ids.join(", ")
        );
        let Ok(candidate) = register(state, "planning", name, &parameters, &rationale) else {
            continue;
        };
        let policy_id = candidate["id"].as_str().unwrap_or("");
        let revision = candidate["revision"].as_str().unwrap_or("");
        let request_id = format!(
            "experiment-request-policy-{}",
            crate::engine::memory::stable_id(&[policy_id, revision])
        );
        if state.store.get("experiment_request", &request_id).is_none() {
            let request = json!({
                "id": request_id,
                "kind": "policy",
                "policyId": policy_id,
                "slot": "planning",
                "revision": revision,
                "status": "queued",
                "fixture": "planning_evaluation",
                "minIndependentUnits": MIN_EXPERIMENT_UNITS,
                "evidence": evidence.iter().take(8).cloned().collect::<Vec<_>>(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
            });
            if let Some(saved) = state.store.put("experiment_request", &request) {
                state.bus.record("experiment_request", &saved);
                state.store.audit(
                    "policy.candidate_queued",
                    json!({"policyId": policy_id, "requestId": saved["id"], "evidence": evidence.len()}),
                );
                created.push(json!({"candidate": candidate, "request": saved}));
            }
        }
    }
    created
}

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("adaptation_policy");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    items
}

pub fn active(state: &AppState, slot: &str) -> Option<Value> {
    state
        .store
        .all("adaptation_policy")
        .into_iter()
        .find(|policy| {
            policy["slot"].as_str() == Some(slot) && policy["status"].as_str() == Some("active")
        })
}

#[allow(dead_code)] // used by tests and by future slots that only need parameters
pub fn active_parameters(state: &AppState, slot: &str) -> Value {
    active(state, slot)
        .map(|policy| policy["parameters"].clone())
        .unwrap_or_else(|| json!({}))
}

/// Registers a candidate revision. Identical name+parameters is idempotent.
pub fn register(
    state: &AppState,
    slot: &str,
    name: &str,
    parameters: &Value,
    rationale: &str,
) -> Result<Value, String> {
    if !SLOTS.contains(&slot) {
        return Err(format!("지원하지 않는 정책 슬롯입니다: {slot}"));
    }
    if name.trim().is_empty() {
        return Err("정책 이름이 필요합니다.".into());
    }
    if !parameters.is_object() {
        return Err("정책 parameters는 객체여야 합니다.".into());
    }
    let id = format!(
        "policy-{}",
        crate::engine::memory::stable_id(&[slot, name, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("adaptation_policy", &id) {
        return Ok(existing);
    }
    let revision = crate::engine::memory::stable_id(&[&id, &parameters.to_string()]);
    let record = json!({
        "id": id,
        "slot": slot,
        "name": name,
        "revision": revision,
        "status": "candidate",
        "evidenceGrade": "E0",
        "parameters": parameters,
        "rationale": rationale,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state
        .store
        .put("adaptation_policy", &record)
        .ok_or("정책 후보를 저장할 수 없습니다.")?;
    state.bus.record("adaptation_policy", &saved);
    state.store.audit(
        "policy.candidate",
        json!({"id": saved["id"], "slot": slot, "name": name}),
    );
    Ok(saved)
}

/// Benchmark-sandbox only: force a policy active after an isolated engine is
/// disposable. Production activation still goes through the experiment gate.
pub fn force_activate(state: &AppState, policy_id: &str) -> Option<Value> {
    let mut policy = state.store.get("adaptation_policy", policy_id)?;
    let slot = policy["slot"].as_str().unwrap_or("").to_string();
    for mut other in state.store.all("adaptation_policy") {
        if other["id"].as_str() != Some(policy_id)
            && other["slot"].as_str() == Some(slot.as_str())
            && other["status"].as_str() == Some("active")
        {
            other["status"] = json!("superseded");
            other["supersededBy"] = policy["id"].clone();
            if let Some(saved) = state.store.put("adaptation_policy", &other) {
                state.bus.record("adaptation_policy", &saved);
            }
        }
    }
    policy["status"] = json!("active");
    policy["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("adaptation_policy", &policy)?;
    state.bus.record("adaptation_policy", &saved);
    Some(saved)
}

pub fn deactivate_slot(state: &AppState, slot: &str) -> usize {
    let mut count = 0;
    for mut policy in state.store.all("adaptation_policy") {
        if policy["slot"].as_str() == Some(slot)
            && policy["status"].as_str() == Some("active")
        {
            policy["status"] = json!("inactive");
            if let Some(saved) = state.store.put("adaptation_policy", &policy) {
                state.bus.record("adaptation_policy", &saved);
                count += 1;
            }
        }
    }
    count
}

/// Activates or rolls back a candidate based on an isolated experiment.
pub fn apply_experiment_result(
    state: &AppState,
    policy_id: &str,
    experiment: &Value,
) -> Result<Value, String> {
    let Some(mut policy) = state.store.get("adaptation_policy", policy_id) else {
        return Err("정책 후보를 찾을 수 없습니다.".into());
    };
    let units = experiment["independentUnits"].as_u64().unwrap_or(0);
    if units == 0 {
        return Err("독립 실험 표본이 없습니다.".into());
    }
    let verdict = experiment["verdict"].as_str().unwrap_or("inconclusive");
    let delta = experiment["delta"].as_f64().unwrap_or(0.0);
    let semantic_delta = experiment["semanticDelta"].as_f64();
    let semantic_metric = experiment["semanticMetric"]
        .as_str()
        .unwrap_or(crate::engine::plan_quality::METRIC_VERSION);
    let experiment_id = experiment["experimentId"].as_str().unwrap_or("");
    let revision = crate::engine::memory::stable_id(&[
        experiment_id,
        experiment["oracleVersion"].as_str().unwrap_or(""),
        &units.to_string(),
        &format!("{delta:.6}"),
    ]);
    policy["lastExperiment"] = json!({
        "experimentId": experiment_id,
        "verdict": verdict,
        "delta": delta,
        "independentUnits": units,
        "semanticDelta": semantic_delta,
        "semanticMetric": semantic_metric,
        "oracleVersion": experiment["oracleVersion"],
        "evaluatedAt": chrono::Utc::now().to_rfc3339(),
    });

    if units >= MIN_EXPERIMENT_UNITS && verdict == "regressed" {
        policy["status"] = json!("rolled_back");
        policy["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
        policy["experimentRevision"] = json!(revision);
    } else if units >= MIN_EXPERIMENT_UNITS
        && matches!(verdict, "improved" | "preliminary_support")
        && delta > 0.0
    {
        let slot = policy["slot"].as_str().unwrap_or("").to_string();
        // Planning policies must also improve the semantic plan metric. A
        // file-success win with no better plan is not enough for activation.
        let semantic_ok = if slot == "planning" {
            semantic_delta
                .is_some_and(|value| value >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA)
        } else {
            true
        };
        if !semantic_ok {
            policy["requiresExperiment"] = json!(true);
            policy["semanticVerified"] = json!(false);
            policy["reason"] = json!(format!(
                "의미 계획 개선폭이 기준({})보다 낮아 활성화하지 않았습니다.",
                crate::engine::plan_quality::MIN_SEMANTIC_DELTA
            ));
            let saved = state
                .store
                .put("adaptation_policy", &policy)
                .ok_or("정책 결과를 저장할 수 없습니다.")?;
            state.bus.record("adaptation_policy", &saved);
            state.store.audit(
                "policy.semantic_gate_rejected",
                json!({"id": policy_id, "semanticDelta": semantic_delta, "units": units}),
            );
            return Ok(saved);
        }
        policy["semanticVerified"] = json!(true);
        for mut other in state.store.all("adaptation_policy") {
            if other["id"].as_str() != Some(policy_id)
                && other["slot"].as_str() == Some(slot.as_str())
                && other["status"].as_str() == Some("active")
            {
                other["status"] = json!("superseded");
                other["supersededBy"] = policy["id"].clone();
                other["supersededAt"] = json!(chrono::Utc::now().to_rfc3339());
                if let Some(saved) = state.store.put("adaptation_policy", &other) {
                    state.bus.record("adaptation_policy", &saved);
                }
            }
        }
        policy["status"] = json!("active");
        policy["evidenceGrade"] = json!("E2");
        policy["experimentRevision"] = json!(revision);
        policy["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    }
    let saved = state
        .store
        .put("adaptation_policy", &policy)
        .ok_or("정책 결과를 저장할 수 없습니다.")?;
    state.bus.record("adaptation_policy", &saved);
    state.store.audit(
        "policy.experiment_result",
        json!({
            "id": policy_id,
            "experimentId": experiment_id,
            "verdict": verdict,
            "units": units,
            "status": saved["status"],
        }),
    );
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn failure(state: &AppState, id: &str, class: &str) {
        state.store.put(
            "execution",
            &json!({
                "id":id,"status":"failed","taskOutcome":"failed",
                "failureClass":class,"recoveryHint":"hint",
                "sessionId":"s","startedAt":format!("2026-11-05T10:00:0{id}Z")
            }),
        );
    }

    #[test]
    fn proposer_rejects_clusters_below_the_evidence_threshold() {
        let (state, dir) = state();
        failure(&state, "1", "verification_failed");
        failure(&state, "2", "verification_failed");
        assert!(propose_candidates(&state).is_empty());
        assert!(list(&state).is_empty());
        assert!(state.store.all("experiment_request").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn proposer_creates_one_candidate_and_policy_experiment_request() {
        let (state, dir) = state();
        for id in 1..=3 {
            failure(&state, &id.to_string(), "verification_failed");
        }
        let created = propose_candidates(&state);
        assert_eq!(created.len(), 1);
        let candidate = &created[0]["candidate"];
        assert_eq!(candidate["status"], "candidate");
        assert_eq!(candidate["parameters"]["outline"], true);
        assert!(candidate["rationale"]
            .as_str()
            .is_some_and(|text| text.contains("근거 3건")));
        let request = &created[0]["request"];
        assert_eq!(request["kind"], "policy");
        assert_eq!(request["status"], "queued");
        assert_eq!(request["minIndependentUnits"], MIN_EXPERIMENT_UNITS);
        // Idempotent: a second pass creates nothing new.
        assert!(propose_candidates(&state).is_empty());
        assert_eq!(state.store.all("adaptation_policy").len(), 1);
        assert_eq!(state.store.all("experiment_request").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn proposed_candidate_stays_inactive_until_eight_units_and_rolls_back() {
        let (state, dir) = state();
        for id in 1..=3 {
            failure(&state, &id.to_string(), "limit_exceeded");
        }
        let created = propose_candidates(&state);
        let policy_id = created[0]["candidate"]["id"].as_str().unwrap().to_string();
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"small","verdict":"improved","delta":0.5,"independentUnits":4,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &small).unwrap();
        assert_eq!(record["status"], "candidate", "underpowered promotion");

        let powered = json!({"experimentId":"powered","verdict":"preliminary_support","delta":0.4,"independentUnits":8,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let other = register(&state, "planning", "other", &json!({"outline": true}), "other").unwrap();
        let regressed = json!({"experimentId":"bad","verdict":"regressed","delta":-0.3,"independentUnits":8,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, other["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert_eq!(active(&state, "planning").unwrap()["id"], policy_id.as_str());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn candidates_need_a_powered_experiment_before_activation() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "outline-off",
            &json!({"outline": false}),
            "skip planning for short tasks",
        )
        .unwrap();
        assert_eq!(candidate["status"], "candidate");
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"e1","verdict":"improved","delta":0.5,"independentUnits":2,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &small).unwrap();
        assert_eq!(record["status"], "candidate");

        let powered = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.2,"independentUnits":10,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["evidenceGrade"], "E2");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn activation_supersedes_the_previous_revision_and_regression_rolls_back() {
        let (state, dir) = state();
        let first = register(&state, "planning", "a", &json!({"outline": true}), "a").unwrap();
        let powered = json!({"experimentId":"e1","verdict":"improved","delta":0.3,"independentUnits":9,"semanticDelta":0.25,"oracleVersion":"v1"});
        apply_experiment_result(&state, first["id"].as_str().unwrap(), &powered).unwrap();
        let second = register(&state, "planning", "b", &json!({"outline": false}), "b").unwrap();
        let powered2 = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.4,"independentUnits":9,"semanticDelta":0.25,"oracleVersion":"v1"});
        apply_experiment_result(&state, second["id"].as_str().unwrap(), &powered2).unwrap();

        let first = state.store.get("adaptation_policy", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(first["status"], "superseded");
        assert_eq!(active(&state, "planning").unwrap()["id"], second["id"]);
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let regressed = json!({"experimentId":"e3","verdict":"regressed","delta":-0.3,"independentUnits":9,"semanticDelta":0.25,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, second["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert!(active(&state, "planning").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn planning_activation_requires_both_units_and_semantic_improvement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "semantic-gate",
            &json!({"outline": true}),
            "semantic gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();

        // Enough units, file success, but no semantic plan improvement.
        let no_semantic = json!({"experimentId":"no-semantic","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.0,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &no_semantic).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        // Semantic improvement alone without enough units is not enough.
        let few_units = json!({"experimentId":"few","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &few_units).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");

        // Both gates pass.
        let both = json!({"experimentId":"both","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &both).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["semanticVerified"], true);
        assert_eq!(record["lastExperiment"]["semanticDelta"], 0.2);
        assert_eq!(
            record["lastExperiment"]["semanticMetric"],
            crate::engine::plan_quality::METRIC_VERSION
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_slots_and_non_object_parameters_are_rejected() {
        let (state, dir) = state();
        assert!(register(&state, "shell", "x", &json!({}), "").is_err());
        assert!(register(&state, "planning", "x", &json!([]), "").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
