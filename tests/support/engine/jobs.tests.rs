#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-jobs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn verified_controller_changes_the_executed_repair_budget() {
        assert_eq!(rsi_edit_repair_limit(&json!({})),2);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":0}})),0);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":1}})),1);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":3}})),3);
        assert_eq!(rsi_edit_repair_limit(&json!({"rsiEditPolicy":{"repairLimit":999}})),3);
    }

    #[test]
    fn enqueue_creates_a_queued_job() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w1", "input": "hi" }),
        )
        .expect("job");
        assert_eq!(job["status"], "queued");
        assert_eq!(job["kind"], "workflow");
        assert_eq!(job["stepTotal"], 0);

        let queued = next_queued(&state).expect("worker sees it");
        assert_eq!(queued["id"], job["id"]);

        let mut running = queued.clone();
        running["status"] = json!("running");
        state.store.put("job", &running);
        assert!(next_queued(&state).is_none());
    }

    #[test]
    fn learning_jobs_are_priority_last_and_serialized() {
        let state = temp_state();
        let learn1 = enqueue(&state, "learn", json!({"sessionId":"s"})).unwrap();
        let learn2 = enqueue(&state, "learn", json!({"sessionId":"t"})).unwrap();
        let prompt = enqueue(&state, "prompt", json!({"prompt":"interactive"})).unwrap();

        let first = next_queued(&state).expect("prompt is picked first");
        assert_eq!(first["id"], prompt["id"]);
        state.store.patch_if(
            "job",
            prompt["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        let second = next_queued(&state).expect("one learning job runs");
        assert!(second["id"] == learn1["id"] || second["id"] == learn2["id"]);
        state.store.patch_if(
            "job",
            second["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        assert!(
            next_queued(&state).is_none(),
            "a second learning job must wait while one is running"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[test]
    fn rsi_stop_cancels_only_live_descendants_and_cannot_be_resurrected() {
        let state=temp_state();
        for (id,parent,status) in [("live","parent","running"),("queued","parent","queued"),
            ("done","parent","completed"),("other","sibling","running")] {
            state.store.put("job",&json!({"id":id,"kind":"rsi_generation","status":status,
                "input":{"rsiCampaignId":parent}}));
        }
        cancel_rsi_children(&state,"parent");
        cancel_rsi_children(&state,"parent");
        assert_eq!(state.store.get("job","live").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","queued").unwrap()["status"],"cancelled");
        assert_eq!(state.store.get("job","done").unwrap()["status"],"completed");
        assert_eq!(state.store.get("job","other").unwrap()["status"],"running");
        assert!(JobContext::new(state,"live".into()).checkpoint(json!({"status":"completed"})).is_err());
    }

    #[test]
    fn recovery_does_not_requeue_an_orphan_rsi_generation() {
        let state=temp_state();
        state.store.put("job",&json!({"id":"parent","kind":"rsi_evolve","status":"running",
            "rsiEvolutionInFlight":true,"attempts":1}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"parent"},"attempts":1}));
        recover(&state);
        assert_eq!(state.store.get("job","parent").unwrap()["status"],"needs_review");
        assert_eq!(state.store.get("job","child").unwrap()["status"],"cancelled");
    }

    #[test]
    fn recover_requeues_running_jobs() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w", "input": "" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "queued");
        assert_eq!(stored["recovered"], true);

        running["attempts"] = json!(3);
        running["status"] = json!("running");
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "needs_review");
    }

    #[test]
    fn recover_does_not_replay_an_execution_with_unknown_external_effects() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w", "input": "", "sessionId": "session-1" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        // No inFlight flag: the evidence must come from the operation record.
        state.store.put("job", &running);
        state.store.put(
            "execution",
            &json!({"id":"run-1","sessionId":"session-1","status":"interrupted"}),
        );
        state.store.put(
            "execution_operation",
            &json!({
                "id":"run-1:0:0:call","runId":"run-1","sessionId":"session-1",
                "tool":"shell_exec","status":"interrupted","outcomeUnknown":true
            }),
        );
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            stored["status"], "needs_review",
            "an unknown external result must not be replayed: {stored}"
        );
        assert!(stored["error"].as_str().is_some_and(|text| text.contains("확정")));
    }

    #[test]
    fn job_errors_are_recorded_as_failed() {
        let state = temp_state();
        let job = enqueue(&state, "room", json!({ "roomId": "missing", "goal": "x" })).unwrap();
        let id = job["id"].as_str().unwrap().to_string();
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_job(state.clone(), job));
        let stored = state.store.get("job", &id).unwrap();
        assert_eq!(stored["status"], "failed");
        assert!(stored["error"].as_str().unwrap().contains("대화방"));
    }

    #[test]
    fn uncertain_goal_recovery_updates_job_and_goal_without_replaying() {
        let state = temp_state();
        state
            .store
            .put("goal", &json!({"id":"g","status":"running","goal":"write"}));
        state.store.put(
            "job",
            &json!({"id":"j","kind":"goal","status":"running","attempts":1,
            "input":{"goalId":"g"},"goalState":{"inFlight":true}}),
        );
        recover(&state);
        assert_eq!(
            state.store.get("job", "j").unwrap()["status"],
            "needs_review"
        );
        assert_eq!(
            state.store.get("goal", "g").unwrap()["status"],
            "needs_review"
        );
        assert!(next_queued(&state).is_none());
    }

    #[tokio::test]
    async fn goal_review_state_is_preserved_on_the_parent_job() {
        let state = temp_state();
        state
            .store
            .put("goal", &json!({"id":"g","status":"running","goal":"실행"}));
        let job = state
            .store
            .put(
                "job",
                &json!({"id":"j","kind":"goal","status":"running","attempts":1,
            "input":{"goalId":"g"},"goalState":{"inFlight":true}}),
            )
            .unwrap();
        run_job(state.clone(), job).await;
        assert_eq!(
            state.store.get("job", "j").unwrap()["status"],
            "needs_review"
        );
        assert_eq!(
            state.store.get("goal", "g").unwrap()["status"],
            "needs_review"
        );
    }

    #[test]
    fn failed_learning_jobs_are_requeued_with_backoff() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({"id":"learn-failed","kind":"learn","status":"failed","attempts":1,"input":{},"error":"model unavailable"}),
        );
        assert_eq!(retry_failed_learning(&state), 1);
        let job = state.store.get("job", "learn-failed").unwrap();
        assert_eq!(job["status"], "queued");
        assert!(job["availableAt"].is_string());
        assert_eq!(job["lastError"], "model unavailable");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod rsi_native_preflight_tests {
    use super::*;
    #[test]
    fn opted_in_source_search_can_start_natively_without_spending_a_model_turn() {
        let input=json!({"rsiNativeInduction":true,
            "rsiSourceScopeExpanded":true,"rsiAutonomous":true,
            "rsiAutoFocus":true,"rsiInheritedCandidateId":null});
        assert!(rsi_native_preflight(&input));
        for key in ["rsiNativeInduction","rsiAutoFocus"] {
            let mut blocked=input.clone();
            blocked[key]=json!(false);
            assert!(!rsi_native_preflight(&blocked));
        }
        let mut initial=input.clone();
        initial["rsiSourceScopeExpanded"]=json!(false);
        initial["rsiAutonomous"]=json!(false);
        assert!(rsi_native_preflight(&initial));
        initial["rsiFailedPatchRecovery"]=json!({"candidateId":"compiler-failure"});
        assert!(!rsi_native_preflight(&initial));
        let mut inherited=input;
        inherited["rsiInheritedCandidateId"]=json!("neutral-patch");
        assert!(!rsi_native_preflight(&inherited));
    }
}
