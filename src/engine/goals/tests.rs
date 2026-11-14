//! Goal harness regression evidence, split from production runtime.
    use super::*;
    fn state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-goals-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }
    #[test]
    fn read_only_and_failed_commands_cannot_complete_actions() {
        let (state, dir) = state();
        for evidence in [
            json!({"tool":"workspace_read","result":{"content":"ok"}}),
            json!({"tool":"shell_exec","arguments":{"command":"ls"},"result":{"exitCode":0}}),
            json!({"tool":"shell_exec","result":{"exitCode":1}}),
            json!({"tool":"mcp__x__write","result":{"isError":true}}),
        ] {
            assert!(
                !acceptance_gaps(&state, "create a report", &json!([]), &[evidence]).is_empty()
            );
        }
        assert!(acceptance_gaps(
            &state,
            "create a report",
            &json!([]),
            &[json!({"tool":"workspace_write","result":{"ok":true}})]
        )
        .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn inferred_file_changes_and_deletion_are_verified() {
        let (state, dir) = state();
        std::fs::write(dir.join("a.txt"), "old").unwrap();
        let checks = baseline_checks(&state, infer_criteria("a.txt를 수정해줘"));
        assert_eq!(checks[0]["path"], "a.txt");
        let evidence = vec![json!({"tool":"workspace_write","result":{"ok":true}})];
        assert!(!acceptance_gaps(&state, "a.txt 수정", &checks, &evidence).is_empty());
        std::fs::write(dir.join("a.txt"), "new").unwrap();
        assert!(acceptance_gaps(&state, "a.txt 수정", &checks, &evidence).is_empty());
        let deletion = baseline_checks(&state, infer_criteria("delete a.txt"));
        std::fs::remove_file(dir.join("a.txt")).unwrap();
        assert!(acceptance_gaps(&state, "delete a.txt", &deletion, &evidence).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn explicit_checks_require_real_results() {
        let (state, dir) = state();
        let checks = json!([{"type":"command_succeeded","command":"cargo test"}]);
        assert!(validate_criteria(&checks).is_ok());
        assert!(!acceptance_gaps(&state,"run tests",&checks,&[json!({"tool":"shell_exec","arguments":{"command":"echo cargo test"},"result":{"exitCode":0}})]).is_empty());
        assert!(acceptance_gaps(&state,"run tests",&checks,&[json!({"tool":"terminal_exec","arguments":{"command":"cargo","args":["test"]},"result":{"exitCode":0}})]).is_empty());
        assert!(validate_criteria(&json!([{"type":"anything"}])).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn malformed_or_conflicting_reviews_never_complete() {
        assert!(parse_review("```json\n{\"complete\":true,\"gaps\":[]}\n```").0);
        for text in [
            "{\"complete\":true}",
            "{\"complete\":true,\"gaps\":[1]}",
            "{\"complete\":true,\"gaps\":[\"missing proof\"]}",
            "prefix {\"complete\":true,\"gaps\":[]}",
        ] {
            assert!(!parse_review(text).0);
        }
    }
    #[test]
    fn unfinished_external_effects_are_scoped_to_the_exact_executor_session(){
        let (state,dir)=state();
        state.store.put("execution",&json!({"id":"run-other",
            "sessionId":"room-session","status":"completed"})).unwrap();
        state.store.put("execution_operation",&json!({"id":"op-other",
            "runId":"run-other","status":"interrupted","outcomeUnknown":true})).unwrap();
        state.store.put("execution",&json!({"id":"run-goal",
            "sessionId":"goal-session","status":"completed"})).unwrap();
        state.store.put("execution_operation",&json!({"id":"op-ok",
            "runId":"run-goal","status":"completed","outcomeUnknown":false})).unwrap();
        assert!(!state.store.session_has_uncertain_effects("goal-session").unwrap(),
            "other agents' remote effects must not block an independent goal");
        assert!(state.store.session_has_uncertain_effects("room-session").unwrap());
        // A model can return successful text while the tool's effect remains
        // unknown. The server must detect it independently from turn_error.
        state.store.put("execution_operation",&json!({"id":"op-unknown",
            "runId":"run-goal","status":"completed","outcomeUnknown":true})).unwrap();
        assert!(state.store.session_has_uncertain_effects("goal-session").unwrap());
        state.store.patch_if("execution_operation","op-unknown","outcomeUnknown",
            &json!(true),&json!({"outcomeUnknown":false})).unwrap();
        assert!(!state.store.session_has_uncertain_effects("goal-session").unwrap());
        state.store.put("execution_operation",&json!({"id":"op-running",
            "runId":"run-goal","status":"running","outcomeUnknown":false})).unwrap();
        assert!(state.store.session_has_uncertain_effects("goal-session").unwrap());
        state.store.patch_if("execution_operation","op-running","status",
            &json!("running"),&json!({"status":"completed"})).unwrap();
        assert!(!state.store.session_has_uncertain_effects("goal-session").unwrap());
        // Delegated children inherit the owner's recovery boundary. A parent
        // cannot declare completion while a grandchild tool effect is unknown.
        state.store.put("session",&json!({"id":"child-session",
            "parentSessionId":"goal-session","delegated":true})).unwrap();
        state.store.put("session",&json!({"id":"grandchild-session",
            "parentSessionId":"child-session","delegated":true})).unwrap();
        state.store.put("execution",&json!({"id":"run-grandchild",
            "sessionId":"grandchild-session","status":"completed"})).unwrap();
        state.store.put("execution_operation",&json!({"id":"op-grandchild",
            "runId":"run-grandchild","status":"interrupted","outcomeUnknown":true})).unwrap();
        assert!(state.store.session_has_uncertain_effects("goal-session").unwrap());
        assert!(state.store.session_has_uncertain_effects("child-session").unwrap());
        assert!(!state.store.session_has_uncertain_effects("room-session-2").unwrap());
        state.store.patch_if("execution_operation","op-grandchild","outcomeUnknown",
            &json!(true),&json!({"outcomeUnknown":false,"status":"completed"})).unwrap();
        assert!(!state.store.session_has_uncertain_effects("goal-session").unwrap());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn interrupted_inflight_goal_never_replays() {
        let (state, dir) = state();
        state.store.put(
            "goal",
            &json!({"id":"g","goal":"create report","status":"running"}),
        );
        let context = JobContext::new(state.clone(), "j".into());
        let result = run(
            &context,
            &json!({"input":{"goalId":"g"},"goalState":{"inFlight":true}}),
        )
        .await
        .unwrap();
        assert_eq!(result["status"], "needs_review");
        assert_eq!(state.store.count("execution"), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    async fn mock_provider(
        tool: &'static str,
    ) -> (
        u16,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::{
            extract::State,
            response::{IntoResponse, Response},
            routing::post,
            Json, Router,
        };
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        #[derive(Clone)]
        struct Mock {
            tool: &'static str,
            calls: Arc<AtomicUsize>,
        }
        async fn handle(State(mock): State<Mock>, Json(body): Json<Value>) -> Response {
            mock.calls.fetch_add(1, Ordering::SeqCst);
            if body["stream"] != true {
                let is_review = body
                    .pointer("/messages/0/content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .contains("strict reviewer");
                return Json(json!({"choices":[{"message":{"content":if is_review {"{\"complete\":true,\"gaps\":[]}"} else {"1. Execute the requested work. 2. Verify the file."}}}]})).into_response();
            }
            let has_result = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool");
            let delta = if has_result {
                json!({"content":"Everything is complete; all requested work succeeded."})
            } else {
                let arguments = if mock.tool == "workspace_write" {
                    json!({"path":"report.txt","content":"verified content"})
                } else {
                    json!({"path":"input.txt"})
                };
                json!({"tool_calls":[{"index":0,"id":"real-call","type":"function","function":{"name":mock.tool,"arguments":arguments.to_string()}}]})
            };
            let stream = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]})
            );
            ([("content-type", "text/event-stream")], stream).into_response()
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .route("/v1/chat/completions", post(handle))
            .with_state(Mock {
                tool,
                calls: calls.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (port, calls, server)
    }
    async fn execute_mock_goal(
        tool: &'static str,
        text: &str,
    ) -> (Value, AppState, PathBuf, usize) {
        use std::sync::atomic::Ordering;
        let (state, dir) = state();
        std::fs::write(dir.join("input.txt"), "a real readable file").unwrap();
        state
            .store
            .put("policy", &json!({"id":"default","autoApproveAll":true}));
        let (port, calls, server) = mock_provider(tool).await;
        state.store.put("provider",&json!({"id":"mock","name":"mock","baseUrl":format!("http://127.0.0.1:{port}/v1"),"model":"test","enabled":true,"toolMode":"native"}));
        state.store.put("goal",&json!({"id":"g","goal":text,"maxIterations":1,"executor":{"providerId":"mock","model":"test"}}));
        let job = state
            .store
            .put(
                "job",
                &json!({"id":"j","kind":"goal","status":"running","input":{"goalId":"g"}}),
            )
            .unwrap();
        let context = JobContext::new(state.clone(), "j".into());
        let result = run(&context, &job).await.unwrap();
        let count = calls.load(Ordering::SeqCst);
        server.abort();
        (result, state, dir, count)
    }
    #[tokio::test]
    async fn lying_executor_and_reviewer_cannot_complete_read_only_goal_run() {
        let (result, state, dir, calls) =
            execute_mock_goal("workspace_read", "create a report").await;
        assert_eq!(calls, 3, "planner + executor tool and completion; deterministic failure avoids paid reviewer");
        assert_eq!(result["status"], "needs_review");
        assert_eq!(
            state.store.get("goal", "g").unwrap()["status"],
            "needs_review"
        );
        assert_eq!(result["evidence"][0]["tool"], "workspace_read");
        assert!(!result["gaps"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn successful_goal_has_actual_file_evidence_and_durable_checkpoint() {
        let (result, state, dir, calls) =
            execute_mock_goal("workspace_write", "write report.txt").await;
        assert_eq!(calls, 4);
        assert_eq!(result["status"], "completed");
        assert_eq!(
            std::fs::read_to_string(dir.join("report.txt")).unwrap(),
            "verified content"
        );
        assert_eq!(result["evidence"][0]["arguments"]["path"], "report.txt");
        assert_eq!(
            state.store.get("job", "j").unwrap()["goalState"]["inFlight"],
            false
        );
        assert_eq!(
            state.store.get("goal", "g").unwrap()["acceptanceChecks"][0]["type"],
            "file_changed"
        );
        let goal = state.store.get("goal", "g").unwrap();
        let task_id = goal["taskId"].as_str().unwrap();
        let episode = state.store.get("task_episode", task_id).unwrap();
        assert_eq!(episode["contract"]["family"], "goal");
        assert_eq!(episode["contract"]["source"], "owner");
        assert_eq!(episode["contract"]["intent"], "write report.txt");
        assert_eq!(episode["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(episode["attempts"][0]["verificationOwner"], "parent");
        assert_eq!(episode["rootOutcome"], "succeeded");
        assert_eq!(episode["rootFinal"], true);
        let execution = state.store.all("execution")[0].clone();
        assert_eq!(execution["taskId"], task_id);
        assert_eq!(execution["parentTaskId"], task_id);
        // The parent goal owns final verification; the child attempt must not
        // claim an independent verified-success sample.
        assert_eq!(execution["taskOutcome"], "answered_unverified");
        assert_eq!(state.store.count("success_experience_marker"), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn relative_acceptance_paths_cannot_escape_workspace() {
        let (state, dir) = state();
        assert!(
            validate_criteria(&json!([{"type":"file_exists","path":"../outside.txt"}])).is_err()
        );
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir()
                .join(format!("dominion-goal-outside-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&outside).unwrap();
            std::fs::write(outside.join("secret.txt"), "private").unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
            assert!(!acceptance_gaps(
                &state,
                "explain file",
                &json!([{"type":"file_contains","path":"link/secret.txt","contains":"private"}]),
                &[]
            )
            .is_empty());
            std::fs::remove_dir_all(outside).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn missing_job_checkpoint_prevents_any_model_execution() {
        let (state, dir) = state();
        state.store.put(
            "goal",
            &json!({"id":"g","goal":"create report","plan":"execute"}),
        );
        let context = JobContext::new(state.clone(), "missing-job".into());
        assert!(run(&context, &json!({"input":{"goalId":"g"}}))
            .await
            .is_err());
        assert_eq!(state.store.count("execution"), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn restart_after_terminal_goal_save_never_restarts_execution() {
        for status in ["completed", "cancelled", "failed", "needs_review"] {
            let (state, dir) = state();
            state.store.put("goal", &json!({"id":"g","goal":"create report","status":status,"iterations":2,"sessionId":"s","gaps":["saved terminal decision"]}));
            let context = JobContext::new(state.clone(), "job-not-finalized".into());
            // Cancellation can be persisted while a turn's outcome marker is
            // still inFlight. The terminal decision still forbids replay.
            let result = run(&context, &json!({"status":"running","input":{"goalId":"g"},"goalState":{"inFlight":status=="cancelled"}})).await.unwrap();
            assert_eq!(result["status"], status);
            assert_eq!(result["iterations"], 2);
            assert_eq!(state.store.count("execution"), 0);
            assert_eq!(state.store.count("session"), 0);
            assert_eq!(state.store.get("goal", "g").unwrap()["status"], status);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
