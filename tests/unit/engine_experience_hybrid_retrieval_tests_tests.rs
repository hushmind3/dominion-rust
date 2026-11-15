//! Regression tests: src/engine/experience.rs (hybrid_retrieval_tests)
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
