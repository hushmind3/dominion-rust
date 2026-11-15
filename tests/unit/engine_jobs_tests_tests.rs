//! Regression tests: src/engine/jobs.rs (tests)
use super::*;
    #[test]
    fn automation_workflow_due_tick_has_one_durable_job_and_no_duplicate() {
        let state=temp_state();
        state.store.put("workflow",&json!({"id":"flow-1","name":"team",
            "steps":[{"id":"a","name":"writer","agentId":"coder","prompt":"write"}]})).unwrap();
        let past="2026-01-01T09:00:00Z";
        state.store.put("automation",&json!({"id":"auto-flow","kind":"workflow",
            "input":{"workflowId":"flow-1","input":"document the change"},
            "enabled":true,"intervalMinutes":60,"nextAt":past})).unwrap();
        schedule_automations(&state);
        let first=state.store.all("job");
        assert_eq!(first.len(),1);
        assert_eq!(first[0]["kind"],"workflow");
        assert_eq!(first[0]["input"]["workflowId"],"flow-1");
        assert_eq!(first[0]["input"]["automationId"],"auto-flow");
        let record=state.store.get("automation","auto-flow").unwrap();
        assert_eq!(record["lastJobId"],first[0]["id"]);
        assert_ne!(record["nextAt"],past);
        schedule_automations(&state);
        assert_eq!(state.store.all("job").len(),1);
    }
    #[test]
    fn next_at_cas_failure_does_not_create_scheduled_job() {
        let state=temp_state();
        state.store.put("automation",&json!({"id":"cas","nextAt":"t0"})).unwrap();
        let changed=json!({"id":"cas","nextAt":"t1"});
        let job=json!({"id":"stale-job","kind":"workflow","status":"queued"});
        assert!(!state.store.transaction_if_field("automation","cas","nextAt",
            &json!("different"),&[("automation".into(),changed),
                                  ("job".into(),job)]).unwrap());
        assert!(state.store.get("job","stale-job").is_none());
        assert_eq!(state.store.get("automation","cas").unwrap()["nextAt"],"t0");
    }

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
    fn crashed_workflow_local_reads_resume_but_mutations_stop() {
        let state=temp_state();
        let saved_session=state.store.put("session",&json!({
            "id":"safe-wf-session","title":"read-only workflow"
        })).unwrap();
        assert_eq!(saved_session["id"],"safe-wf-session");
        let job=enqueue(&state,"workflow",json!({"workflowId":"safe-wf","input":"test"})).unwrap();
        let mut running=job.clone();
        running["status"]=json!("running");
        running["attempts"]=json!(1);
        running["sessionId"]=json!("safe-wf-session");
        running["workflowState"]=json!({
            "sessionId":"safe-wf-session",
            "inFlight":true,
            "activeStepIndex":0,
            "stepStartedAt":"2026-11-14T10:00:00Z",
            "outputs":[],
            "definition":{"steps":[{"id":"s1","prompt":"read"}]}
        });
        state.store.put("job",&running);
        state.store.put("execution",&json!({
            "id":"safe-run","sessionId":"safe-wf-session",
            "startedAt":"2026-11-14T10:00:01Z","status":"running"
        }));
        state.store.put("execution_operation",&json!({
            "id":"safe-op","sessionId":"safe-wf-session","runId":"safe-run",
            "startedAt":"2026-11-14T10:00:02Z",
            "tool":"workspace_read","status":"completed",
            "arguments":{"path":"src/lib.rs"}
        }));
        recover(&state);
        let queued=state.store.get("job",job["id"].as_str().unwrap()).unwrap();
        assert_eq!(queued["status"],"queued","read-only job may retry after crash: {queued}");
        assert_eq!(queued["replayReason"],"local_read_only_step_retry");

        let mut unsafe_job=queued.clone();
        unsafe_job["status"]=json!("running");
        unsafe_job["recovered"]=json!(false);
        state.store.put("job",&unsafe_job);
        state.store.put("execution_operation",&json!({
            "id":"unsafe-op","sessionId":"safe-wf-session","runId":"safe-run",
            "startedAt":"2026-11-14T10:00:03Z",
            "tool":"shell_exec","status":"running",
            "arguments":{"command":"touch src/main.rs"}
        }));
        recover(&state);
        let paused=state.store.get("job",job["id"].as_str().unwrap()).unwrap();
        assert_eq!(paused["status"],"needs_review","a possible external effect must never replay");
    }

    #[test]
    fn legacy_inflight_workflow_without_checkpoint_cannot_autoreplay(){
        let state=temp_state();
        let job=enqueue(&state,"workflow",json!({"workflowId":"old"})).unwrap();
        let mut running=job.clone();
        running["status"]=json!("running");
        running["workflowState"]=json!({"inFlight":true,"sessionId":"unknown","outputs":[]});
        state.store.put("job",&running);
        recover(&state);
        let paused=state.store.get("job",job["id"].as_str().unwrap()).unwrap();
        assert_eq!(paused["status"],"needs_review");
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
