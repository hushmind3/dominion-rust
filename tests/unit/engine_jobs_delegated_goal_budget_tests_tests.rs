//! Regression tests: src/engine/jobs.rs (delegated_goal_budget_tests)
use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    #[tokio::test]
    async fn exhausted_root_budget_blocks_child_before_any_provider_call() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-child-budget-{}",uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("goal",&json!({"id":"goal","sessionId":"owner",
            "status":"running","tokenBudget":20})).unwrap();
        state.store.put("session",&json!({"id":"owner","goalId":"goal"})).unwrap();
        state.store.put("session",&json!({"id":"old-child",
            "parentSessionId":"owner","delegated":true})).unwrap();
        state.store.put("session",&json!({"id":"new-child",
            "parentSessionId":"owner","delegated":true})).unwrap();
        for (session,tokens) in [("owner",8),("old-child",15)] {
            state.store.audit("model.usage",json!({
                "sessionId":session,"totalTokens":tokens
            }));
        }
        // No model/provider is configured: a model request would error.
        // The native guard must instead return a deterministic needs_review.
        let job=json!({"id":"child-job","kind":"prompt","status":"running",
            "input":{"prompt":"fix the code","sessionId":"new-child",
                "delegation":true,"parentSessionId":"owner"}});
        let context=JobContext::new(state.clone(),"child-job".into());
        let result=run_prompt(&context,&job).await.unwrap();
        assert_eq!(result["status"],"needs_review");
        assert_eq!(result["observedTokens"],23);
        assert_eq!(result["tokenBudget"],20);
        assert_eq!(state.store.session_usage("new-child")["calls"],0);
        drop(context);
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
