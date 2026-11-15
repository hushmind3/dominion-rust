//! Regression tests: src/engine/experience.rs (probe_tests)
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
