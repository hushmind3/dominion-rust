//! H9 algorithm-patch artifacts.
//!
//! A patch is a versioned change to a policy revision: it records the base
//! revision it was written against, the target slot, and a parameter body.
//! Activation requires an isolated experiment with >=8 units and both semantic
//! evaluators; if the base revision moved, the patch is rejected as
//! incompatible instead of being applied to a different policy.
use crate::state::AppState;
use serde_json::{json, Value};

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("policy_patch");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    items
}

pub fn register(
    state: &AppState,
    slot: &str,
    target_policy_id: &str,
    body: &Value,
    rationale: &str,
) -> Result<Value, String> {
    if !body.is_object() || !body["parameters"].is_object() {
        return Err("patch body에는 parameters 객체가 필요합니다.".into());
    }
    let Some(policy) = state.store.get("adaptation_policy", target_policy_id) else {
        return Err("대상 정책을 찾을 수 없습니다.".into());
    };
    if policy["slot"].as_str() != Some(slot) {
        return Err("대상 정책의 slot과 일치하지 않습니다.".into());
    }
    if policy["status"].as_str() != Some("active") {
        return Err("활성 정책 revision에만 patch를 등록할 수 있습니다.".into());
    }
    let base_revision = policy["revision"].as_str().unwrap_or("");
    let id = format!(
        "policy-patch-{}",
        crate::engine::memory::stable_id(&[
            slot,
            target_policy_id,
            base_revision,
            &body.to_string()
        ])
    );
    if let Some(existing) = state.store.get("policy_patch", &id) {
        return Ok(existing);
    }
    let record = json!({
        "id": id,
        "slot": slot,
        "targetPolicyId": target_policy_id,
        "baseRevision": base_revision,
        "baseParameters": policy["parameters"].clone(),
        "body": body,
        "rationale": rationale,
        "status": "candidate",
        "evidenceGrade": "E0",
        "requiresExperiment": true,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state
        .store
        .put("policy_patch", &record)
        .ok_or("patch를 저장할 수 없습니다.")?;
    state.bus.record("policy_patch", &saved);
    state.store.audit(
        "policy.patch_registered",
        json!({"id": saved["id"], "target": target_policy_id, "baseRevision": base_revision}),
    );
    Ok(saved)
}

fn eligible(units: u64, verdict: &str, main: f64, independent: f64) -> bool {
    units >= crate::engine::policy::MIN_EXPERIMENT_UNITS
        && matches!(verdict, "improved" | "preliminary_support")
        && main >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA
        && independent >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA
}

fn merge_parameters(base: &Value, patch: &Value) -> Value {
    let mut merged = base.as_object().cloned().unwrap_or_default();
    if let Some(parameters) = patch.as_object() {
        for (key, value) in parameters {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

pub fn apply_experiment_result(
    state: &AppState,
    patch_id: &str,
    experiment: &Value,
) -> Result<Value, String> {
    let Some(mut patch) = state.store.get("policy_patch", patch_id) else {
        return Err("patch를 찾을 수 없습니다.".into());
    };
    if !matches!(
        patch["status"].as_str(),
        Some("candidate" | "active")
    ) {
        return Err("candidate/active patch만 실험 결과를 적용할 수 있습니다.".into());
    }
    let units = experiment["independentUnits"].as_u64().unwrap_or(0);
    let verdict = experiment["verdict"].as_str().unwrap_or("inconclusive");
    let delta = experiment["delta"].as_f64().unwrap_or(0.0);
    let semantic = experiment["semanticDelta"].as_f64().unwrap_or(0.0);
    let independent = experiment["independentSemanticDelta"]
        .as_f64()
        .unwrap_or(0.0);
    let target_id = patch["targetPolicyId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let base_revision = patch["baseRevision"]
        .as_str()
        .unwrap_or("")
        .to_string();
    patch["lastExperiment"] = json!({
        "experimentId": experiment["experimentId"],
        "verdict": verdict,
        "delta": delta,
        "semanticDelta": semantic,
        "independentSemanticDelta": independent,
        "independentUnits": units,
        "evaluatedAt": chrono::Utc::now().to_rfc3339(),
    });

    // Regression check: a patch written against another revision must not be
    // applied silently to the current one.
    let Some(mut policy) = state.store.get("adaptation_policy", &target_id) else {
        patch["status"] = json!("rejected");
        patch["reason"] = json!("대상 정책이 사라졌습니다.");
        let saved = state.store.put("policy_patch", &patch).ok_or("patch 저장 실패")?;
        return Ok(saved);
    };
    let base_moved = policy["revision"].as_str() != Some(base_revision.as_str())
        || policy["status"].as_str() != Some("active");
    if base_moved && patch["status"].as_str() == Some("candidate") {
        patch["status"] = json!("rejected");
        patch["reason"] = json!("base revision이 변경되어 patch를 적용할 수 없습니다.");
        patch["currentRevision"] = policy["revision"].clone();
        let saved = state.store.put("policy_patch", &patch).ok_or("patch 저장 실패")?;
        state.bus.record("policy_patch", &saved);
        state.store.audit(
            "policy.patch_base_mismatch",
            json!({"id": patch_id, "baseRevision": base_revision, "current": policy["revision"]}),
        );
        return Ok(saved);
    }

    if units >= crate::engine::policy::MIN_EXPERIMENT_UNITS && verdict == "regressed" {
        policy["parameters"] = patch["baseParameters"].clone();
        policy["revision"] = json!(crate::engine::memory::stable_id(&[
            policy["revision"].as_str().unwrap_or(""),
            patch_id,
            "rollback",
        ]));
        policy["status"] = json!("active");
        policy["rolledBackPatchId"] = patch["id"].clone();
        policy["evidenceGrade"] = json!("E2");
        if let Some(saved) = state.store.put("adaptation_policy", &policy) {
            state.bus.record("adaptation_policy", &saved);
        }
        patch["status"] = json!("rolled_back");
        patch["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
        patch["reason"] = json!("격리 실험에서 악화되어 patch를 롤백했습니다.");
    } else if eligible(units, verdict, semantic, independent) && delta > 0.0 {
        let merged = merge_parameters(&policy["parameters"], &patch["body"]["parameters"]);
        policy["baseRevisionBeforePatch"] = policy["revision"].clone();
        policy["parameters"] = merged;
        policy["revision"] = json!(crate::engine::memory::stable_id(&[
            policy["revision"].as_str().unwrap_or(""),
            patch_id,
            &policy["parameters"].to_string(),
        ]));
        policy["status"] = json!("active");
        policy["appliedPatchId"] = patch["id"].clone();
        policy["evidenceGrade"] = json!("E2");
        policy["patchedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(saved) = state.store.put("adaptation_policy", &policy) {
            state.bus.record("adaptation_policy", &saved);
        }
        for mut other in state.store.all("policy_patch") {
            if other["id"].as_str() != Some(patch_id)
                && other["slot"].as_str() == patch["slot"].as_str()
                && other["status"].as_str() == Some("active")
            {
                other["status"] = json!("superseded");
                other["supersededBy"] = patch["id"].clone();
                if let Some(saved) = state.store.put("policy_patch", &other) {
                    state.bus.record("policy_patch", &saved);
                }
            }
        }
        patch["status"] = json!("active");
        patch["evidenceGrade"] = json!("E2");
        patch["requiresExperiment"] = json!(false);
        patch["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        patch["appliedPolicyRevision"] = policy["revision"].clone();
    }

    let saved = state
        .store
        .put("policy_patch", &patch)
        .ok_or("patch 저장 실패")?;
    state.bus.record("policy_patch", &saved);
    state.store.audit(
        "policy.patch_experiment",
        json!({
            "id": patch_id,
            "verdict": verdict,
            "units": units,
            "status": saved["status"],
            "appliedPolicyRevision": saved["appliedPolicyRevision"],
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
        let dir = std::env::temp_dir().join(format!("dominion-patch-{}", uuid::Uuid::new_v4()));
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

    fn active_policy(state: &AppState) -> Value {
        let candidate = crate::engine::policy::register(
            state,
            "planning",
            "base-planning",
            &json!({"outline": true}),
            "base",
        )
        .unwrap();
        crate::engine::policy::force_activate(state, candidate["id"].as_str().unwrap()).unwrap()
    }

    fn experiment(units: u64, verdict: &str, main: f64, independent: f64) -> Value {
        json!({
            "experimentId":format!("patch-exp-{units}-{verdict}"),
            "verdict":verdict,
            "delta":0.4,
            "semanticDelta":main,
            "independentSemanticDelta":independent,
            "independentUnits":units,
        })
    }

    #[test]
    fn a_patch_needs_both_evaluators_and_eight_units() {
        let (state, dir) = state();
        let policy = active_policy(&state);
        let patch = register(
            &state,
            "planning",
            policy["id"].as_str().unwrap(),
            &json!({"parameters":{"minChars":120}}),
            "threshold patch",
        )
        .unwrap();
        assert_eq!(patch["status"], "candidate");
        let id = patch["id"].as_str().unwrap();

        let weak = apply_experiment_result(&state, id, &experiment(4, "preliminary_support", 0.4, 0.4)).unwrap();
        assert_eq!(weak["status"], "candidate");
        let no_independent =
            apply_experiment_result(&state, id, &experiment(8, "preliminary_support", 0.4, 0.0)).unwrap();
        assert_eq!(no_independent["status"], "candidate");
        let good =
            apply_experiment_result(&state, id, &experiment(8, "preliminary_support", 0.4, 0.3)).unwrap();
        assert_eq!(good["status"], "active");
        let current = state
            .store
            .get("adaptation_policy", policy["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(current["parameters"]["minChars"], 120);
        assert_eq!(current["appliedPatchId"], good["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_patch_against_a_stale_base_revision_is_rejected() {
        let (state, dir) = state();
        let policy = active_policy(&state);
        let patch = register(
            &state,
            "planning",
            policy["id"].as_str().unwrap(),
            &json!({"parameters":{"minChars":90}}),
            "stale base",
        )
        .unwrap();
        // A new activation moves the base revision.
        let mut current = state
            .store
            .get("adaptation_policy", policy["id"].as_str().unwrap())
            .unwrap();
        current["revision"] = json!("moved-revision");
        state.store.put("adaptation_policy", &current);
        let record = apply_experiment_result(
            &state,
            patch["id"].as_str().unwrap(),
            &experiment(8, "preliminary_support", 0.4, 0.4),
        )
        .unwrap();
        assert_eq!(record["status"], "rejected", "{record}");
        assert!(record["reason"]
            .as_str()
            .is_some_and(|text| text.contains("base revision")));
        let unchanged = state
            .store
            .get("adaptation_policy", policy["id"].as_str().unwrap())
            .unwrap();
        assert!(unchanged["parameters"]["minChars"].is_null());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_regressed_patch_restores_the_base_parameters() {
        let (state, dir) = state();
        let policy = active_policy(&state);
        let patch = register(
            &state,
            "planning",
            policy["id"].as_str().unwrap(),
            &json!({"parameters":{"minChars":150}}),
            "rollback patch",
        )
        .unwrap();
        apply_experiment_result(
            &state,
            patch["id"].as_str().unwrap(),
            &experiment(8, "preliminary_support", 0.4, 0.4),
        )
        .unwrap();
        let record = apply_experiment_result(
            &state,
            patch["id"].as_str().unwrap(),
            &experiment(8, "regressed", 0.0, 0.0),
        )
        .unwrap();
        assert_eq!(record["status"], "rolled_back", "{record}");
        let current = state
            .store
            .get("adaptation_policy", policy["id"].as_str().unwrap())
            .unwrap();
        assert!(current["parameters"]["minChars"].is_null());
        assert_eq!(current["parameters"]["outline"], true);
        assert_eq!(current["rolledBackPatchId"], record["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
