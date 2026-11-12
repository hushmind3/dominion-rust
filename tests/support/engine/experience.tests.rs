#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-exp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn records_dedupes_and_retrieves_similar_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let first = record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m1".into()], &["workspace_write".into()], &["file_changed".into()],
        ).unwrap();
        record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m2".into()], &[], &[],
        ).unwrap();
        let stored = state.store.get("experience", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["occurrences"], 2);
        let cases = retrieve(&state, &scope, "README.md 파일 작성", 5);
        assert_eq!(cases.len(), 1);
        assert!(context_block(&state, &scope, "README.md 파일 작성")
            .unwrap()
            .contains("말로만"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scopes_do_not_leak_between_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record(&state, &a, "파일 작성", "success", "A의 절차", "", &[], &[], &[]).unwrap();
        assert!(retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_cases_reach_other_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record_shared(
            &state, &a, "파일 작성", "success", "공유 절차를 재사용한다", "", &[], &[], &[],
        )
        .unwrap();
        assert!(!retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_agents_supporting_a_method_share_it_with_the_group() {
        let (state, dir) = state();
        let attempt = |agent: &str, id: &str| json!({
            "id": id, "sessionId": "s", "agentId": agent, "taskOutcome": "succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        learn_verified_adaptations(&state, &attempt("agent-a", "run-a"));
        let a_scope = MemoryScope::for_session(&state.store, "s", Some("agent-a"));
        let b_scope = MemoryScope::for_session(&state.store, "s", Some("agent-b"));
        assert!(matching_rules(&state, &b_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).is_empty());
        learn_verified_adaptations(&state, &attempt("agent-b", "run-b"));
        let shared = matching_rules(&state, &b_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3);
        assert_eq!(shared.len(), 1, "{shared:?}");
        assert_eq!(shared[0]["scope"], "shared");
        let c_scope = MemoryScope::for_session(&state.store, "s", Some("agent-c"));
        assert_eq!(matching_rules(&state, &c_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).len(), 1);
        let _ = a_scope;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sharing_threshold_algorithm_controls_cross_agent_promotion() {
        let (state, dir) = state();
        crate::engine::algorithms::seed(&state);
        let three = state
            .store
            .all("algorithm")
            .into_iter()
            .find(|algorithm| algorithm["implementation"] == "three-agents")
            .unwrap();
        crate::engine::algorithms::activate(&state, three["id"].as_str().unwrap()).unwrap();
        let attempt = |agent: &str, id: &str| json!({
            "id": id, "sessionId": "s", "agentId": agent, "taskOutcome": "succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        for (agent, id) in [("agent-a","r1"), ("agent-b","r2")] {
            learn_verified_adaptations(&state, &attempt(agent, id));
        }
        let third = MemoryScope::for_session(&state.store, "s", Some("agent-c"));
        assert!(matching_rules(&state, &third, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).is_empty());
        learn_verified_adaptations(&state, &attempt("agent-c", "r3"));
        assert_eq!(matching_rules(&state, &third, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consolidation_archives_duplicate_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let base = state.store.put("experience", &json!({
            "id":"e1","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":5,"status":"active","lastSeenAt":"2026-01-01"
        })).unwrap();
        state.store.put("experience", &json!({
            "id":"e2","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":1,"status":"active","lastSeenAt":"2026-01-02"
        })).unwrap();
        consolidate(&state);
        assert_eq!(state.store.get("experience", base["id"].as_str().unwrap()).unwrap()["status"], "active");
        assert_eq!(state.store.get("experience", "e2").unwrap()["status"], "archived");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consolidation_is_independent_of_read_order() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let case = |id: &str, occurrences: u64, last: &str| json!({
            "id":id,"scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":occurrences,"status":"active","lastSeenAt":last
        });
        // Insert the weaker case first: the previous implementation kept it
        // active when the stronger case arrived later.
        state.store.put("experience", &case("weaker", 1, "2026-01-02")).unwrap();
        state.store.put("experience", &case("stronger", 5, "2026-01-01")).unwrap();
        consolidate(&state);
        assert_eq!(state.store.get("experience","stronger").unwrap()["status"], "active");
        assert_eq!(state.store.get("experience","weaker").unwrap()["status"], "archived");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verified_intervention_is_conditionally_reused_and_counterexamples_exclude_it() {
        let (state, dir) = state();
        let attempt = json!({
            "id":"verified-run","sessionId":"s","agentId":"a","taskOutcome":"succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        let created = learn_verified_adaptations(&state, &attempt);
        assert_eq!(created.len(), 1);
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let matched = matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0]["id"], created[0]["id"]);
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "permission denied", 3).is_empty());
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"different":"second.txt"}), "file not found", 3).is_empty());
        // The same observed outcome is not counted as independent evidence.
        let again = learn_verified_adaptations(&state, &attempt);
        assert_eq!(again[0]["supportingCases"].as_array().unwrap().len(), 1);
        state.store.put("session", &json!({
            "id":"another","agentId":"b",
            "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
        }));
        let foreign = MemoryScope::for_session(&state.store, "another", Some("b"));
        assert!(matching_rules(&state, &foreign, "workspace_read",
            &json!({"path":"second.txt"}), "file not found", 3).is_empty());
        record_rule_counterexamples(&state, &matched,
            &json!({"tool":"workspace_write",
                "inputs":{"path":"other.txt","content":"failed"}}),
            &json!({"error":"permission denied"}), "other-run");
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "file not found", 3).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_or_unknown_failures_never_become_reusable_rules() {
        let (state, dir) = state();
        let attempt = json!({
            "id":"incomplete","sessionId":"s","agentId":"a","taskOutcome":"answered_unverified",
            "adaptations":[{
                "observation":{"tool":"workspace_read","arguments":{"path":"a"},"actual":"file not found"},
                "selected":{"tool":"workspace_write","inputs":{"path":"a","content":"x"}},
                "result":{"success":true}
            }]
        });
        assert!(learn_verified_adaptations(&state, &attempt).is_empty());
        let mut passed = attempt.clone();
        passed["taskOutcome"] = json!("succeeded");
        passed["adaptations"][0]["observation"]["actual"] = json!("unexpected failure");
        assert!(learn_verified_adaptations(&state, &passed).is_empty());
        passed["adaptations"][0]["observation"]["actual"] = json!("file not found");
        passed["adaptations"][0]["result"] = json!({"outcomeUnknown":true});
        assert!(learn_verified_adaptations(&state, &passed).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_cases_cross_partitions_and_agents() {
        let (state, dir) = state();
        state.store.put(
            "session",
            &json!({
                "id":"d","agentId":"b",
                "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
            }),
        );
        let local = MemoryScope::for_session(&state.store, "s", Some("a"));
        let discord = MemoryScope::for_session(&state.store, "d", Some("b"));
        record_global(
            &state, &local, "파일 작성", "failure", "전역 교훈을 재사용한다", "", &[], &[], &[],
        )
        .unwrap();
        assert!(!retrieve(&state, &discord, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod hybrid_retrieval_tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    #[test]
    fn balanced_kernel_finds_korean_spacing_variants_without_an_llm() {
        let dir = std::env::temp_dir()
            .join(format!("dominion-hybrid-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {port:0, data_dir:dir.clone(),
            web_dir:dir.join("web"), workspace:dir.clone()};
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"hybrid-session","agentId":"agent-a"}));
        let scope = MemoryScope::for_session(&state.store, "hybrid-session", Some("agent-a"));
        state.store.put("experience", &json!({
            "id":"hybrid-1","status":"active","scope":"agent",
            "partitionKey":scope.partition_key,"agentId":"agent-a",
            "request":"데이터 베이스 백업", "signature":"데이터 베이스 백업",
            "lesson":"백업 파일을 검증한다","outcome":"success","occurrences":1
        }));
        state.store.put("experience", &json!({
            "id":"hybrid-2","status":"active","scope":"agent",
            "partitionKey":scope.partition_key,"agentId":"agent-a",
            "request":"비디오 렌더링", "signature":"비디오 렌더링",
            "lesson":"프레임 레이트 확인","outcome":"failure","occurrences":5
        }));
        let lexical = retrieve(&state, &scope, "데이터베이스", 5);
        assert!(lexical.is_empty(), "lexical must not pretend to be semantic");
        crate::engine::algorithms::seed(&state);
        for mut item in state.store.all("algorithm") {
            if item["kind"] == "memory.retrieval" {
                item["status"] = json!("disabled");
                state.store.put("algorithm", &item);
            }
        }
        state.store.put("algorithm", &json!({
            "id":"balanced-test","kind":"memory.retrieval",
            "status":"active","revision":"balanced-test",
            "implementation":"balanced","parameters":{"failureBoost":2.0}
        }));
        let matches = retrieve(&state, &scope, "데이터베이스", 5);
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert_eq!(matches[0]["id"], "hybrid-1");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn probe_unknown_actually_observes_before_applying_the_method() {
        let (state, dir) = state();
        crate::engine::algorithms::seed(&state);
        let probe = state
            .store
            .all("algorithm")
            .into_iter()
            .find(|algorithm| algorithm["implementation"] == "probe-unknown")
            .unwrap();
        crate::engine::algorithms::activate(&state, probe["id"].as_str().unwrap()).unwrap();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "experience_rule",
            &json!({
                "id":"rule-probe",
                "scope":"agent",
                "agentId":"a",
                "partitionKey":scope.partition_key,
                "status":"candidate",
                "applicabilityPredicates":[
                    {"type":"equals","from":"observation","path":"/tool","value":"workspace_read"},
                    {"type":"equals","from":"observation","path":"/failureClass","value":"not_found"},
                    {"type":"equals","from":"observation","path":"/inputShape","value":"alternate,path"},
                    {"type":"equals","from":"observation","path":"/pathState/alternate/exists","value":false}
                ],
                "intervention":{"tool":"workspace_write","inputShape":"content,path"},
                "supportingCases":["case-1"],
                "counterexamples":[]
            }),
        );
        let matched = matching_rules(
            &state,
            &scope,
            "workspace_read",
            &json!({"path":"target.txt","alternate":"target.txt"}),
            "no such file or directory",
            3,
        );
        assert_eq!(matched.len(), 1, "{matched:?}");
        assert_eq!(matched[0]["probeApplied"], true);
        assert_eq!(matched[0]["observation"]["pathState"]["alternate"]["exists"], false);

        std::fs::write(dir.join("target.txt"), "now exists").unwrap();
        let after = matching_rules(
            &state,
            &scope,
            "workspace_read",
            &json!({"path":"target.txt","alternate":"target.txt"}),
            "no such file or directory",
            3,
        );
        assert!(after.is_empty(), "probe did not use the real file state: {after:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
