#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_requires_independent_model_evidence_and_rolls_back_for_the_workload() {
        let (state, dir) = state();
        let candidate = register(&state, "mutation.preflight", "guard",
            &json!({"kernel":preflight::GUARD_KERNEL,"workload":preflight::GUARD_WORKLOAD}), "fixture").unwrap();
        let id = candidate["id"].as_str().unwrap();
        let settle = |cohort: Option<&str>, verdict: &str, delta: f64| apply_experiment_result(&state, id, &json!({
            "independentUnits":10,"verdict":verdict,"delta":delta,
            "preflightCohort":cohort,"experimentId":format!("{cohort:?}-{verdict}"),
        })).unwrap();
        settle(None, "preliminary_support", 1.0); // deterministic comparison
        settle(Some("model-a"), "preliminary_support", 1.0);
        settle(Some("model-a"), "preliminary_support", 1.0);
        assert!(portable_file_guard(&state).is_none());
        settle(Some("model-b"), "preliminary_support", 1.0);
        assert_eq!(portable_file_guard(&state).unwrap()["id"], id);
        settle(Some("model-c"), "regressed", -1.0);
        assert!(portable_file_guard(&state).is_none());
        settle(Some("model-b"), "preliminary_support", 1.0);
        assert!(portable_file_guard(&state).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn costly_guard_cannot_activate_even_with_a_positive_success_verdict() {
        let (state, dir) = state();
        let candidate = register(&state, "mutation.preflight", "guard",
            &json!({"kernel":preflight::GUARD_KERNEL,"workload":preflight::GUARD_WORKLOAD}), "fixture").unwrap();
        let id = candidate["id"].as_str().unwrap();
        for (cohort, rejection) in [("model-a", None), ("model-b", Some("token_cost_exceeded"))] {
            apply_experiment_result(&state, id, &json!({
                "independentUnits":10,"verdict":"preliminary_support","delta":1.0,
                "preflightCohort":cohort,"experimentId":cohort,"preflightRejection":rejection,
            })).unwrap();
        }
        assert!(portable_file_guard(&state).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

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
        assert!(created.len() >= 3, "search should propose multiple revisions");
        let planning = created
            .iter()
            .filter(|item| {
                item["request"]["searchId"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("policy-search"))
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(planning.len(), 3, "three planning revisions");
        let search_ids = planning
            .iter()
            .filter_map(|item| item["request"]["searchId"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(search_ids.len(), 1, "one search group per failure cluster");
        let mut variants = planning
            .iter()
            .map(|item| item["candidate"]["parameters"].clone())
            .collect::<Vec<_>>();
        variants.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        assert!(variants.iter().any(|value| value["outline"] == false));
        assert!(
            variants
                .iter()
                .any(|value| value["outline"] == true && value["minChars"].is_u64()),
            "min-length threshold variant missing: {variants:?}"
        );
        for item in &planning {
            assert_eq!(item["candidate"]["status"], "candidate");
            assert_eq!(item["request"]["kind"], "policy");
            assert_eq!(item["request"]["status"], "queued");
            assert_eq!(item["request"]["minIndependentUnits"], MIN_EXPERIMENT_UNITS);
            assert!(item["candidate"]["rationale"]
                .as_str()
                .is_some_and(|text| text.contains("근거 3건")));
        }
        // Idempotent: a second pass creates nothing new.
        assert!(propose_candidates(&state).is_empty());
        assert!(state.store.all("adaptation_policy").len() >= 6);
        assert!(state.store.all("experiment_request").len() >= 6);
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

        let small = json!({"experimentId":"small","verdict":"improved","delta":0.5,"independentUnits":4,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &small).unwrap();
        assert_eq!(record["status"], "candidate", "underpowered promotion");

        let powered = json!({"experimentId":"powered","verdict":"preliminary_support","delta":0.4,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let other = register(&state, "planning", "other", &json!({"outline": true}), "other").unwrap();
        let regressed = json!({"experimentId":"bad","verdict":"regressed","delta":-0.3,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
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

        let small = json!({"experimentId":"e1","verdict":"improved","delta":0.5,"independentUnits":2,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &small).unwrap();
        assert_eq!(record["status"], "candidate");

        let powered = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.2,"independentUnits":10,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
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
        let powered = json!({"experimentId":"e1","verdict":"improved","delta":0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, first["id"].as_str().unwrap(), &powered).unwrap();
        let second = register(&state, "planning", "b", &json!({"outline": false}), "b").unwrap();
        let powered2 = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.4,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, second["id"].as_str().unwrap(), &powered2).unwrap();

        let first = state.store.get("adaptation_policy", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(first["status"], "superseded");
        assert_eq!(active(&state, "planning").unwrap()["id"], second["id"]);
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let regressed = json!({"experimentId":"e3","verdict":"regressed","delta":-0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
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
        let no_semantic = json!({"experimentId":"no-semantic","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.0,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &no_semantic).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        // Semantic improvement alone without enough units is not enough.
        let few_units = json!({"experimentId":"few","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &few_units).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");

        // Both gates pass.
        let both = json!({"experimentId":"both","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
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
    fn planning_activation_rejects_an_independent_evaluator_disagreement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "independent-gate",
            &json!({"outline": true}),
            "independent gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        // Primary evaluator improves but the independent evaluator does not.
        let disagree = json!({"experimentId":"disagree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.0,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &disagree).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        let agree = json!({"experimentId":"agree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &agree).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["lastExperiment"]["independentSemanticDelta"], 0.2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outline_thresholds_control_planning() {
        assert_eq!(
            outline_decision(&json!({"outline": true}), 10),
            Some(true)
        );
        assert_eq!(
            outline_decision(&json!({"outline":false}), 10_000),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 50),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 150),
            Some(true)
        );
        assert_eq!(outline_decision(&json!({}), 10), None);
    }

    #[test]
    fn search_selection_keeps_the_best_independent_score_only() {
        let (state, dir) = state();
        let search = "search-1";
        let make = |id: &str, main: f64, independent: f64, status: &str| {
            let mut policy = register(
                &state,
                "planning",
                id,
                &json!({"outline":true}),
                "selection test",
            )
            .unwrap();
            policy["searchId"] = json!(search);
            policy["status"] = json!(status);
            policy["lastExperiment"] = json!({
                "independentUnits":10,
                "verdict":"preliminary_support",
                "semanticDelta":main,
                "independentSemanticDelta":independent,
            });
            state.store.put("adaptation_policy", &policy);
            policy
        };
        let best = make("best", 0.4, 0.35, "active");
        let weaker = make("weaker", 0.4, 0.10, "active");
        let rejected = make("rejected", 0.4, 0.0, "candidate");
        let selected = select_best(&state, search).unwrap();
        assert_eq!(selected["selectedPolicyId"], best["id"]);
        assert_eq!(selected["selectedScore"], 0.35);
        assert_eq!(
            state.store.get("adaptation_policy", best["id"].as_str().unwrap()).unwrap()["status"],
            "active"
        );
        assert_eq!(
            state.store.get("adaptation_policy", weaker["id"].as_str().unwrap()).unwrap()["status"],
            "superseded"
        );
        assert_eq!(
            state.store.get("adaptation_policy", rejected["id"].as_str().unwrap()).unwrap()["status"],
            "candidate"
        );
        let searches = state.store.all("policy_search");
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0]["selectedPolicyId"], best["id"]);
        assert_eq!(searches[0]["candidates"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recovery_limits_are_bounded_and_configurable() {
        assert_eq!(recovery_limits(&json!({})), (2, 2));
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":1,"maxProcedureRepairs":0})),
            (1, 0)
        );
        // A policy can tune but not remove safety bounds entirely.
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":99,"maxProcedureRepairs":99})),
            (4, 4)
        );
    }

    #[test]
    fn a_recovery_policy_uses_the_same_experiment_gate() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "recovery",
            "fewer-retries",
            &json!({"maxAcceptanceRetries":0,"maxProcedureRepairs":0}),
            "recovery gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        let underpowered = json!({"experimentId":"r1","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &underpowered).unwrap();
        assert_eq!(record["status"], "candidate");

        let staged = json!({"experimentId":"r2","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &staged).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(
            recovery_limits(&record["parameters"]),
            (0, 0)
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
