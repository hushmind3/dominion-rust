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

        let small = json!({"experimentId":"e1","verdict":"improved","delta":0.5,"independentUnits":2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &small).unwrap();
        assert_eq!(record["status"], "candidate");

        let powered = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.2,"independentUnits":10,"oracleVersion":"v1"});
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
        let powered = json!({"experimentId":"e1","verdict":"improved","delta":0.3,"independentUnits":9,"oracleVersion":"v1"});
        apply_experiment_result(&state, first["id"].as_str().unwrap(), &powered).unwrap();
        let second = register(&state, "planning", "b", &json!({"outline": false}), "b").unwrap();
        let powered2 = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.4,"independentUnits":9,"oracleVersion":"v1"});
        apply_experiment_result(&state, second["id"].as_str().unwrap(), &powered2).unwrap();

        let first = state.store.get("adaptation_policy", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(first["status"], "superseded");
        assert_eq!(active(&state, "planning").unwrap()["id"], second["id"]);
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let regressed = json!({"experimentId":"e3","verdict":"regressed","delta":-0.3,"independentUnits":9,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, second["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert!(active(&state, "planning").is_none());
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
