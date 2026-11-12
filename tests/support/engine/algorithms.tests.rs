#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-alg-{}", uuid::Uuid::new_v4()));
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
    fn genuine_multistep_goals_use_the_improved_kernel_in_live_selection() {
        let (state,dir)=state();
        let input="설정을 읽고 결과를 저장한 다음 검증해";
        let initial=choose_for_agent(&state,"task.decomposition",Some("agent"),None).unwrap();
        assert_eq!(initial["implementation"],"single-step");
        let chosen=choose_decomposition_for_agent(
            &state,Some("agent"),Some("real-execution"),input).unwrap();
        assert_ne!(chosen["implementation"],"single-step");
        let actual=crate::engine::decompose::decompose(input,&chosen);
        assert_eq!(actual["subtasks"].as_array().unwrap().len(),3);
        assert!(actual["qualityScore"].as_f64().unwrap()>0.75);
        let id=format!("algorithm-usage-{}",
            crate::engine::memory::stable_id(&["real-execution","task.decomposition"]));
        let usage=state.store.get("algorithm_usage",&id).unwrap();
        assert_eq!(usage["revision"],chosen["revision"],
            "feedback must reach the kernel that actually ran");
        let simple=choose_decomposition_for_agent(
            &state,Some("other-agent"),Some("simple-execution"),"파일을 열어").unwrap();
        assert_eq!(simple["implementation"],"single-step",
            "single actions should still use the learned/default kernel");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn activation_requires_evaluation_evidence_unless_forced() {
        let (state, dir) = state();
        seed(&state);
        let record = register(
            &state,
            &json!({
                "kind": "memory.rerank",
                "name": "bm25-candidate",
                "variant": "bm25",
                "implementation": "bm25"
            }),
        )
        .unwrap();
        let id = record["id"].as_str().unwrap();
        assert!(activate_checked(&state, id, false).is_err());
        state.store.put(
            "algorithm_eval",
            &json!({
                "id": "eval-1",
                "dimensions": [
                    {"kind": "memory.rerank", "winner": "bm25", "recommendation": "promote"}
                ]
            }),
        );
        assert!(activate_checked(&state, id, false).is_ok());
        let other = register(
            &state,
            &json!({
                "kind": "memory.rerank",
                "name": "none-manual",
                "variant": "none",
                "implementation": "none"
            }),
        )
        .unwrap();
        assert!(activate_checked(&state, other["id"].as_str().unwrap(), true).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_executable_kernels_register_and_activation_is_kind_isolated() {
        let (state, dir) = state();
        seed(&state);
        let adaptation_before = active(&state, "adaptation.selection").unwrap();
        let strategy_before = active(&state, "strategy.selection").unwrap();
        assert!(register(
            &state,
            &json!({"kind":"adaptation.selection","name":"fake","implementation":"does-not-exist"})
        )
        .is_err());
        let record = register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"cheap-probe",
                "implementation":"probe-first",
                "parameters":{"discriminationWeight":2.0}
            }),
        )
        .unwrap();
        activate(&state, record["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            active(&state, "adaptation.selection").unwrap()["id"],
            record["id"]
        );
        assert_ne!(
            active(&state, "adaptation.selection").unwrap()["id"],
            adaptation_before["id"]
        );
        assert_eq!(
            active(&state, "strategy.selection").unwrap()["id"],
            strategy_before["id"],
            "activation leaked into another kind"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_generated_algorithms_never_enter_live_ucb_until_promoted() {
        let (state, dir) = state();
        seed(&state);
        let candidate = register(&state, &json!({
            "kind":"memory.importance", "name":"unsafe-generated-candidate",
            "implementation":"fixed", "source":"generated",
            "parameters":{"usesWeight":999.0}
        })).unwrap();
        assert_eq!(candidate["status"], "available");
        for mut item in state.store.all("algorithm") {
            if item["kind"] == "memory.importance"
                && item["id"] != candidate["id"]
            {
                item["status"] = json!("disabled");
                state.store.put("algorithm", &item);
            }
        }
        assert!(choose_for_agent(
            &state, "memory.importance", Some("agent-a"), None
        ).is_none(), "unreviewed candidate reached production");
        let activated = activate(&state, candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(choose_for_agent(
            &state, "memory.importance", Some("agent-a"), None
        ).unwrap()["id"], activated["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generation_uses_evidence_and_returns_executable_kernels() {
        let (state, dir) = state();
        let generated = generate(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "evidence":["timeout timeout permission denied"]
            }),
        );
        let created = generated["created"].as_array().unwrap();
        assert!(!created.is_empty());
        for algorithm in created {
            let kind = algorithm["kind"].as_str().unwrap();
            let implementation = algorithm["implementation"].as_str().unwrap();
            assert!(kernel_exists(kind, implementation), "{algorithm}");
        }
        assert!(created
            .iter()
            .any(|algorithm| algorithm["implementation"] == "probe-first"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expression_kernel_adds_new_selection_logic_without_rebuild() {
        let (state, dir) = state();
        let mut variables = std::collections::BTreeMap::new();
        variables.insert("discrimination".to_string(), 2.0);
        variables.insert("cost".to_string(), 0.5);
        assert_eq!(
            eval_expression("discrimination * 2 - cost", &variables).unwrap(),
            3.5
        );
        let record = register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"custom-formula",
                "implementation":"expression",
                "parameters":{"expression":"discrimination * 2 - cost"}
            }),
        )
        .unwrap();
        assert_eq!(record["implementation"], "expression");
        assert!(register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"broken-formula",
                "implementation":"expression",
                "parameters":{"expression":"discrimination +"}
            })
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn selfimprove_expression_uses_search_features_and_rejects_unknown_names() {
        let (state, dir) = state();
        let record = register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"search-formula",
                "implementation":"expression",
                "parameters":{"expression":"accepted * 2 - tokenCost + novelty - regressions"}
            }),
        )
        .unwrap();
        assert_eq!(record["implementation"], "expression");
        assert!(register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"wrong-features",
                "implementation":"expression",
                "parameters":{"expression":"discrimination * 2"}
            })
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generated_source_targeting_formula_is_validated_and_versioned() {
        let (state,dir)=state();
        let valid=json!({"kind":"selfimprove.search","name":"evolvable-target-v1",
            "implementation":"expression",
            "parameters":{"expression":"objectiveMatch * 5 + exploration",
                "targetExpression":"pathEvidence * 3 + symbolEvidence - repeatPenalty - sourceCost"}});
        let registered=register(&state,&valid).unwrap();
        assert_eq!(registered["parameters"]["targetExpression"],
            "pathEvidence * 3 + symbolEvidence - repeatPenalty - sourceCost");
        assert_eq!(registered["status"],"available");
        let body_aware = register(&state, &json!({
            "kind":"selfimprove.search", "name":"body-evidence-target-v1",
            "implementation":"expression",
            "parameters":{"expression":"objectiveMatch + exploration",
                "targetExpression":"bodyEvidence * 2 + pathEvidence - repeatPenalty"}
        })).unwrap();
        assert_eq!(body_aware["status"], "available");
        assert!(register(&state,&json!({"kind":"selfimprove.search","name":"unknown-feature",
            "implementation":"expression","parameters":{"expression":"novelty",
                "targetExpression":"unguardedVariable + pathEvidence"}})).is_err());
        assert!(register(&state,&json!({"kind":"selfimprove.search","name":"invalid-score",
            "implementation":"expression","parameters":{"expression":"novelty",
                "targetExpression":"pathEvidence / (novelty - novelty)"}})).is_err());
        // A model-authored targeting rule cannot be silently attached to a
        // different algorithm or a display-only kernel.
        assert!(register(&state,&json!({"kind":"memory.rerank","name":"wrong-kind",
            "implementation":"bm25","parameters":{"targetExpression":"sourceCost"}})).is_err());
        assert!(eval_expression(&"(".repeat(36),&expression_sample("selfimprove.target")).is_err());
        assert!(eval_expression(&"a".repeat(513),&expression_sample("selfimprove.target")).is_err());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn execution_reads_implementation_not_display_variant() {
        let (state, dir) = state();
        seed(&state);
        let record = register(
            &state,
            &json!({
                "kind":"tool.selection",
                "name":"display-only-name",
                "variant":"display-only-name",
                "implementation":"block-write-without-read",
                "parameters":{}
            }),
        )
        .unwrap();
        activate(&state, record["id"].as_str().unwrap()).unwrap();
        assert_eq!(variant(&state, "tool.selection"), "block-write-without-read");
        assert_ne!(variant(&state, "tool.selection"), "display-only-name");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_algorithm_outcomes_become_local_and_global_statistics() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "adaptation.selection", Some("agent-a"), Some("exec-1"),
        )
        .expect("algorithm");
        assert!(state.store.all("algorithm_usage").iter().any(|usage| {
            usage["executionId"] == "exec-1"
                && usage["agentId"] == "agent-a"
                && usage["revision"] == chosen["revision"]
        }));
        finalize_execution(
            &state,
            &json!({"id":"exec-1","agentId":"agent-a","taskOutcome":"succeeded"}),
        );
        let stats = state.store.all("algorithm_stat");
        let local = stats.iter().find(|stat| {
            stat["agentId"] == "agent-a"
                && stat["revision"] == chosen["revision"]
        }).expect("local stat");
        assert_eq!(local["uses"], 1);
        assert_eq!(local["successes"], 1);
        let global = stats.iter().find(|stat| {
            stat["agentId"] == "global"
                && stat["revision"] == chosen["revision"]
        }).expect("global stat");
        assert_eq!(global["uses"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_domain_signal_overrides_whole_task_success() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "verification.predicate", Some("agent-a"), Some("exec-signal"),
        )
        .expect("algorithm");
        mark_signal(&state, "exec-signal", "verification.predicate", false);
        finalize_execution(
            &state,
            &json!({"id":"exec-signal","agentId":"agent-a","taskOutcome":"succeeded"}),
        );
        let stat = state
            .store
            .all("algorithm_stat")
            .into_iter()
            .find(|stat| {
                stat["agentId"] == "agent-a"
                    && stat["revision"] == chosen["revision"]
            })
            .unwrap();
        assert_eq!(stat["successes"], 0, "{stat}");
        assert_eq!(stat["failures"], 1, "{stat}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_conversations_never_become_algorithm_failures() {
        let (state, dir) = state();
        seed(&state);
        for index in 0..10 {
            let execution_id = format!("exec-chat-{index}");
            let chosen = choose_for_agent(
                &state,
                "context.selection",
                Some("agent-a"),
                Some(&execution_id),
            )
            .expect("algorithm");
            finalize_execution(
                &state,
                &json!({
                    "id": execution_id,
                    "agentId": "agent-a",
                    "taskOutcome": "answered_unverified"
                }),
            );
            assert!(
                !state
                    .store
                    .all("algorithm_stat")
                    .iter()
                    .any(|stat| stat["revision"] == chosen["revision"]),
                "unverified chat created a failure statistic"
            );
        }
        let unrated = state
            .store
            .all("algorithm_usage")
            .into_iter()
            .filter(|usage| usage["status"] == "unrated")
            .count();
        assert_eq!(unrated, 10, "every unverified usage must be unrated");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rebuild_drops_polluted_failures_and_keeps_rated_evidence() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state,
            "context.selection",
            Some("agent-a"),
            Some("exec-rated"),
        )
        .expect("algorithm");
        mark_signal(&state, "exec-rated", "context.selection", true);
        finalize_execution(
            &state,
            &json!({
                "id":"exec-rated","agentId":"agent-a","taskOutcome":"succeeded"
            }),
        );
        let chat = choose_for_agent(
            &state,
            "context.selection",
            Some("agent-a"),
            Some("exec-chat"),
        )
        .expect("algorithm");
        assert_eq!(chat["revision"], chosen["revision"]);
        finalize_execution(
            &state,
            &json!({
                "id":"exec-chat","agentId":"agent-a","taskOutcome":"answered_unverified"
            }),
        );
        // Simulate the old polluted record: 9 fake failures on top of the
        // genuine signal.
        state.store.put(
            "algorithm_stat",
            &json!({
                "id": stat_id("agent-a", "context.selection", chosen["revision"].as_str().unwrap()),
                "agentId":"agent-a","kind":"context.selection",
                "revision":chosen["revision"],"uses":10,"successes":1,"failures":9,
            }),
        );
        let report = rebuild_stats_from_usage(&state).unwrap();
        assert_eq!(report["unrated"], 1, "{report}");
        let rated = report["rated"].as_u64().unwrap();
        assert_eq!(rated, 1, "{report}");
        let stats = state.store.all("algorithm_stat");
        assert_eq!(stats.len(), 2, "local + global only: {stats:?}");
        for stat in &stats {
            assert_eq!(stat["uses"], 1, "{stat}");
            assert_eq!(stat["successes"], 1, "{stat}");
            assert_eq!(stat["failures"], 0, "{stat}");
            assert_eq!(stat["statVersion"], 2, "{stat}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_execution_is_counted_once_and_signals_aggregate() {
        let (state, dir) = state();
        seed(&state);
        let chosen = choose_for_agent(
            &state, "adaptation.selection", None, Some("exec-global"),
        )
        .expect("algorithm");
        mark_signal(&state, "exec-global", "adaptation.selection", false);
        mark_signal(&state, "exec-global", "adaptation.selection", false);
        mark_signal(&state, "exec-global", "adaptation.selection", true);
        finalize_execution(
            &state,
            &json!({"id":"exec-global","taskOutcome":"succeeded"}),
        );
        let stats = state
            .store
            .all("algorithm_stat")
            .into_iter()
            .filter(|stat| stat["revision"] == chosen["revision"])
            .collect::<Vec<_>>();
        assert_eq!(stats.len(), 1, "global was counted twice: {stats:?}");
        assert_eq!(stats[0]["uses"], 1, "{stats:?}");
        assert_eq!(stats[0]["successes"], 0, "last signal overwrote failures");
        assert_eq!(stats[0]["failures"], 1, "{stats:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn list_exposes_per_algorithm_observation_stats_without_double_counting() {
        let (state, dir) = state();
        seed(&state);
        let algorithm = active(&state, "strategy.selection").unwrap();
        let revision = algorithm["revision"].as_str().unwrap().to_string();
        state.store.put(
            "algorithm_stat",
            &json!({
                "id":"agent-stat","agentId":"agent-a","kind":"strategy.selection",
                "revision":revision,"uses":4,"successes":3,"failures":1,
                "successRate":0.75,"lastUsedAt":"2026-01-01T00:00:00Z"
            }),
        );
        state.store.put(
            "algorithm_stat",
            &json!({
                "id":"global-stat","agentId":"global","kind":"strategy.selection",
                "revision":revision,"uses":4,"successes":3,"failures":1,
                "successRate":0.75,"lastUsedAt":"2026-01-01T00:00:00Z"
            }),
        );
        let report = list(&state);
        let row = report["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|kind| kind["algorithms"].as_array().unwrap().clone())
            .find(|row| row["id"] == algorithm["id"])
            .expect("algorithm row");
        assert_eq!(row["stats"]["uses"], 4, "{row}");
        assert_eq!(row["stats"]["successes"], 3, "{row}");
        assert_eq!(row["stats"]["agents"], 1, "{row}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn active_optimizer_is_found_by_status_not_a_fixed_id() {
        let (state, dir) = state();
        state.store.put(
            "optimizer_policy",
            &json!({
                "id":"optimizer-custom",
                "revision":"optimizer-custom-v2",
                "status":"active",
                "failureClustering":"custom"
            }),
        );
        let active = crate::engine::policy::active_optimizer(&state);
        assert_eq!(active["revision"], "optimizer-custom-v2");
        let _ = std::fs::remove_dir_all(dir);
    }
}
