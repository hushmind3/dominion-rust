//! Extracted isolated regression tests for src/engine/principle.rs.
use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-principle-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone(),
        };
        (
            AppState::new(
                config.clone(),
                Store::open(&config.db_path()).unwrap(),
                Vault::open(&config.key_path()),
            ),
            dir,
        )
    }

    fn execution(model: &str, agent: &str, failed: &str, selected: &str, error: &str) -> Value {
        json!({
            "id": format!("run-{agent}"),
            "taskOutcome": "succeeded",
            "model": model,
            "agentId": agent,
            "adaptations": [{
                "observation": {"tool": failed, "actual": error},
                "selected": {"tool": selected, "inputs": {"path": "safe"}},
                "result": {"success": true}
            }]
        })
    }

    #[test]
    fn verified_github_failure_creates_a_global_principle() {
        let (state, dir) = state();
        assert_eq!(
            learn_from_execution(
                &state,
                &execution(
                    "model-a",
                    "agent-a",
                    "github_api_update",
                    "github_api_get",
                    "409 conflict: sha mismatch"
                )
            ),
            1
        );
        let principles = state.store.all("experience_principle");
        assert_eq!(principles.len(), 1);
        assert_eq!(principles[0]["kind"], "verify-before-mutate");
        assert_eq!(principles[0]["scope"], "global");
        assert!(principles[0]["sourceDomains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "source_control"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn direct_recovery_requires_the_same_resource_to_be_read_and_retried() {
        let (state, dir) = state();
        let execution = json!({"id":"direct","taskOutcome":"succeeded","model":"real-tool-loop"});
        for (seq, tool, method, path, status, result) in [
            (1, "github_api", "PUT", "/target", "failed", json!({"error":"409 conflict"})),
            (2, "github_api", "GET", "/other", "completed", json!({"status":200})),
            (4, "github_api", "PUT", "/target", "completed", json!({"status":200})),
        ] {
            state.store.put("execution_operation", &json!({
                "id":format!("operation-{seq}"),"runId":"direct","opSeq":seq,"tool":tool,
                "arguments":{"method":method,"path":path},"status":status,"result":result,
            })).unwrap();
        }
        assert_eq!(learn_from_execution(&state, &execution), 0);
        state.store.put("execution_operation", &json!({
            "id":"operation-3","runId":"direct","opSeq":3,"tool":"github_api",
            "arguments":{"method":"GET","path":"/target"},"status":"completed","result":{"status":200},
        })).unwrap();
        assert_eq!(learn_from_execution(&state, &execution), 1);
        assert_eq!(state.store.all("experience_principle")[0]["supportingModels"], json!(["real-tool-loop"]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn principle_transfers_to_another_domain_and_model() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        for (index, (agent, model, tool, run)) in [
            ("agent-b", "model-b", "sql_update", "run-b"),
            ("agent-c", "model-c", "http_post", "run-c"),
            ("agent-d", "model-d", "workspace_write", "run-d"),
        ]
        .into_iter()
        .enumerate()
        {
            let guidance = context_block(
                &state,
                Some(agent),
                model,
                tool,
                &json!({}),
                "409 conflict: stale state",
                Some(run),
            )
            .unwrap_or_else(|| panic!("cross-domain principle missing for {tool}"));
            assert!(guidance.contains("현재 상태와 버전"), "{guidance}");
            let execution = json!({"id":run,"model":model,"agentId":agent});
            assert_eq!(finalize_usage(&state, &execution, true), 1);
            assert_eq!(state.store.all("principle_usage").len(), index + 1);
        }
        // A second verified source run promotes the principle into the
        // existing strategy-candidate pipeline.
        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2",
                "taskOutcome":"succeeded",
                "model":"model-a2",
                "agentId":"agent-a2",
                "sessionId":"s-source-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        let candidate = state.store.all("improvement").into_iter()
            .find(|record| record["kind"].as_str() == Some("strategy"))
            .expect("principle strategy candidate");
        assert_eq!(candidate["scope"], "global", "{candidate}");
        assert!(candidate["text"].as_str().unwrap_or("").contains("현재 상태와 버전"), "{candidate}");
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["strategyCandidateId"], candidate["id"], "{principle}");
        for model in ["model-b", "model-c", "model-d"] {
            assert!(principle["supportingModels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == model), "{principle}");
        }
        assert_eq!(principle["supportingCases"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn synthesized_principle_is_typed_not_a_prompt_sentence() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["schemaVersion"], 2);
        assert!(principle["conditions"].as_array().unwrap().len() >= 2);
        assert!(principle["exclusions"].as_array().unwrap().len() >= 5);
        assert_eq!(principle["failureLayer"], "state");
        assert_eq!(principle["procedure"][0]["operator"], "observe");
        assert_eq!(principle["procedure"][3]["operator"], "verify");
        assert_eq!(principle["verification"]["modelClaimAccepted"], false);
        assert_eq!(principle["autoApply"], false);
        assert_eq!(principle["cost"]["modelCalls"], 0);
        assert!(principle["provenance"]["capabilities"].is_object());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn composition_requires_independent_cases_and_stays_unpromoted() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        assert!(context_block(
            &state,
            Some("agent-b"),
            "model-b",
            "workspace_write",
            &json!({}),
            "409 conflict",
            Some("run-b"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-b"}), true);
        assert!(
            state.store.all("strategy_composition").is_empty(),
            "one source and one supporting case are not enough"
        );

        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2","taskOutcome":"succeeded","model":"model-a2",
                "agentId":"agent-a2","sessionId":"s-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        assert!(context_block(
            &state,
            Some("agent-c"),
            "model-c",
            "file_write",
            &json!({}),
            "409 conflict: stale state",
            Some("run-c"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-c"}), true);

        let composition = state
            .store
            .all("strategy_composition")
            .into_iter()
            .next()
            .expect("composition from independent evidence");
        assert_eq!(composition["promotionEligible"], false);
        assert_eq!(composition["experiment"]["status"], "not_run");
        assert_eq!(composition["experiment"]["gates"]["tokenRatioMax"], 1.25);
        let domains = composition["supportedDomains"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["domain"].as_str())
            .collect::<Vec<_>>();
        assert!(domains.contains(&"filesystem"));
        assert!(domains.contains(&"source_control"));
        assert!(domains.contains(&"system") == false);
        let principle = state.store.all("experience_principle")[0].clone();
        assert_eq!(principle["strategyCompositionId"], composition["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unresolved_effects_are_not_learned_as_success() {
        let (state, dir) = state();
        let mut execution = execution(
            "model-a",
            "agent-a",
            "github_api_update",
            "github_api_get",
            "409 conflict: sha mismatch",
        );
        execution["effectSummary"] = json!({"unverifiedEffects": 1});
        assert_eq!(learn_from_execution(&state, &execution), 0);
        assert!(state.store.all("experience_principle").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn observations_do_not_receive_mutation_principles() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "workspace_write",
                "workspace_read",
                "no such file",
            ),
        );
        assert!(context_block(
            &state,
            None,
            "model-b",
            "workspace_read",
            &json!({}),
            "no such file",
            Some("run-c"),
        )
        .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
