//! Split from src/engine/rooms.rs; regression coverage only.
    use super::*;
    use serde_json::json;

    #[test]
    fn participant_model_pool_is_deduplicated_and_generic() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("agent", &json!({"id":"a","providerId":"p1","model":"m1"}));
        state.store.put("agent", &json!({"id":"b","providerId":"p1","model":"m1"}));
        state.store.put("agent", &json!({"id":"c","providerId":"p2","model":"m2"}));
        let pool = model_pool(&state, &json!({"agentIds":["a","b","c"]}));
        assert_eq!(pool, vec![("p1".into(),"m1".into(),None),("p2".into(),"m2".into(),None)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explicit_room_models_override_agent_defaults() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-explicit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("provider", &json!({"id":"cloud","kind":"openai","baseUrl":"https://api.example.com/v1"}));
        state.store.put("provider", &json!({"id":"local","kind":"ollama","baseUrl":"http://127.0.0.1:11434/v1"}));
        state.store.put("agent", &json!({"id":"a","providerId":"cloud","model":"agent-default"}));
        let room = json!({
            "agentIds":["a"],
            "models":[
                {"providerId":"cloud","model":"picked-fast","reasoningEffort":"low"},
                {"providerId":"cloud","model":"picked-fast"},
                {"providerId":"local","model":"picked-local"}
            ]
        });
        let pool = model_pool(&state, &room);
        assert_eq!(
            pool,
            vec![
                ("cloud".into(), "picked-fast".into(), Some("low".into())),
                ("local".into(), "picked-local".into(), None)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn participant_model_pool_keeps_local_and_cloud_routes() {
        let dir = std::env::temp_dir().join(format!("dominion-pool-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config,
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put("provider", &json!({"id":"local","kind":"mlx","local":true,"baseUrl":"http://127.0.0.1:1234/v1"}));
        state.store.put("provider", &json!({"id":"cloud","kind":"openai","baseUrl":"https://api.example.com/v1"}));
        state.store.put("agent", &json!({"id":"a","providerId":"local","model":"mlx-7b"}));
        state.store.put("agent", &json!({"id":"b","providerId":"cloud","model":"free-model"}));
        let pool = model_pool(&state, &json!({"agentIds":["a","b"]}));
        assert_eq!(
            pool,
            vec![
                ("local".into(), "mlx-7b".into(), None),
                ("cloud".into(), "free-model".into(), None)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }
