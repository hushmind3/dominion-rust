use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;
    use serde_json::json;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-outcome-{}", uuid::Uuid::new_v4()));
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

    fn execution(status: &str, acceptance: bool, acceptance_text: Value) -> Value {
        json!({
            "status": status,
            "acceptanceChecks":[{"type":"file_equals","path":"test.txt","content":"ok"}],
            "acceptancePassed": acceptance,
            "acceptanceText": acceptance_text,
            "strategyCompliance": []
        })
    }

    #[test]
    fn finalization_hash_ignores_mutable_counters_and_timings() {
        let base = json!({
            "id":"e1","sessionId":"s","agentId":"a","request":"do it",
            "status":"completed","taskOutcome":"succeeded",
            "acceptancePassed":true,"acceptanceText":"check",
            "appliedImprovementIds":["i1"],
            "strategyCompliance":[],"finalMessageId":"m1",
            "tokens":10,"toolCalls":1,"iterations":2,
            "finishedAt":"2026-11-05T10:00:00Z","firstTokenMs":120
        });
        let operations = vec![json!({
            "id":"o1","runId":"e1","tool":"workspace_write",
            "arguments":{"path":"a.txt"},"origin":"model",
            "status":"completed","result":{"ok":true},
            "startedAt":"2026-11-05T09:59:00Z"
        })];
        let first = finalization_payload_hash(&base, &operations);
        let mut retried = base.clone();
        retried["tokens"] = json!(999);
        retried["iterations"] = json!(7);
        retried["finishedAt"] = json!("2026-11-05T10:05:00Z");
        retried["firstTokenMs"] = json!(900);
        let mut reordered = operations.clone();
        reordered.insert(
            0,
            json!({"id":"o0","runId":"e1","tool":"workspace_read","status":"completed"}),
        );
        let mut reversed = reordered.clone();
        reversed.reverse();
        assert_eq!(first, finalization_payload_hash(&retried, &operations));
        assert_eq!(
            finalization_payload_hash(&base, &reordered),
            finalization_payload_hash(&base, &reversed)
        );
        retried["taskOutcome"] = json!("failed");
        assert_ne!(first, finalization_payload_hash(&retried, &operations));
    }

    #[tokio::test]
    async fn program_resumes_from_a_durable_checkpoint_without_repeating_completed_tools() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        let execution_id = "exec-resume";
        state.store.put(
            "execution",
            &json!({
                "id": execution_id,
                "status": "running",
                "taskId": execution_id,
                "sessionId": "s1",
                "request": "fallback.txt 파일을 만들어줘",
                "programCheckpoint": {
                    "schemaVersion":1,
                    "programRevision":"p-resume",
                    "status":"running",
                    "frameStack":[],
                    "nextIndex":1,
                    "resolvedInputs":{"try":{"exitCode":7}},
                    "completedOperationIds":["try"],
                    "pendingOperationIds":[]
                }
            }),
        );
        let session = json!({"id":"s1","roomId":null});
        let request = TurnRequest {
            session_id: Some("s1".into()),
            input: "fallback.txt 파일을 만들어줘".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: None,
            model: None,
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let program = json!({
            "revision":"p-resume",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
                {"type":"branch","id":"check",
                 "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
                 "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
                 "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
            ]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::budget::BudgetLedger::new(
            MAX_ROUNDS,
            MAX_TOOL_CALLS,
            MAX_ELAPSED,
        );
        let mut op_seq = 0u64;
        let generation = state.cancellation_generation("s1");
        run_program(
            &state, &tx, &session, &request, "s1", execution_id, &program,
            &metrics, &mut budget, &mut op_seq, generation,
        )
        .await
        .expect("resume program");
        assert_eq!(
            std::fs::read_to_string(dir.join("fallback.txt")).unwrap_or_default(),
            "FALLBACK"
        );
        let operations = state
            .store
            .all("execution_operation")
            .into_iter()
            .filter(|operation| operation["runId"] == execution_id)
            .collect::<Vec<_>>();
        assert_eq!(operations.len(), 1, "completed try node was re-executed: {operations:?}");
        assert_eq!(operations[0]["tool"], "workspace_write");
        let execution = state.store.get("execution", execution_id).unwrap();
        assert_eq!(execution["programCheckpoint"]["status"], "completed");
        assert!(execution["programCheckpoint"]["completedOperationIds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "try"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_partially_completed_loop_resumes_with_distinct_iteration_ids() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        state.store.put("execution", &json!({
            "id":"loop-resume","taskId":"loop-resume","sessionId":"s1",
            "status":"running","request":"반복 작성",
            "programCheckpoint":{
                "schemaVersion":1,"programRevision":"p-loop","status":"running",
                "frameStack":[],"nextIndex":0,
                "resolvedInputs":{"condition":true,"write":{"path":"repeat.txt"}},
                "completedOperationIds":["repeat:0:write"],
                "pendingOperationIds":[],"loopProgress":{}
            }
        }));
        let session = json!({"id":"s1"});
        let request = TurnRequest {
            session_id: Some("s1".into()), input: "반복 작성".into(),
            attachment_ids: Vec::new(), agent_id: None, provider_id: None,
            model: None, tools_enabled: true, tools_execution_enabled: true,
            thinking: None, reasoning_effort: None, reasoning_language: None,
            auto_learning: false, system_override: None, task_id: None,
            parent_task_id: None, task_revision: None, verification_owner: None,
        };
        let program = json!({
            "revision":"p-loop",
            "nodes":[{
                "type":"loop","id":"repeat","maxIterations":2,
                "predicate":{"type":"equals","from":"condition","value":true},
                "body":[{"type":"call_tool","id":"write",
                    "tool":"workspace_write",
                    "inputs":{"path":"repeat.txt","content":"REPEAT"}}]
            }]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::budget::BudgetLedger::new(
            MAX_ROUNDS, MAX_TOOL_CALLS, MAX_ELAPSED,
        );
        let mut op_seq = 0;
        run_program(&state, &tx, &session, &request, "s1", "loop-resume",
            &program, &metrics, &mut budget, &mut op_seq,
            state.cancellation_generation("s1")).await.unwrap();
        let execution = state.store.get("execution", "loop-resume").unwrap();
        let checkpoint = &execution["programCheckpoint"];
        assert_eq!(checkpoint["loopProgress"]["repeat"], 2);
        assert_eq!(checkpoint["status"], "completed");
        assert!(checkpoint["resolvedInputs"]["write"]["path"].as_str()
            .is_some_and(|path| path.ends_with("repeat.txt")));
        let completed = checkpoint["completedOperationIds"].as_array().unwrap();
        assert!(completed.iter().any(|x| x == "repeat:0:write"));
        assert!(completed.iter().any(|x| x == "repeat:1:write"));
        let ops = state.store.all("execution_operation");
        assert_eq!(ops.len(), 1, "resumed iteration zero was replayed: {ops:?}");
        assert_eq!(std::fs::read_to_string(dir.join("repeat.txt")).unwrap(), "REPEAT");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_failed_program_revision_does_not_mark_the_program_completed() {
        let (state, dir) = state();
        state.store.put("policy", &json!({"id":"default","autoApproveAll":true}));
        let execution_id = "exec-revision-failure";
        state.store.put(
            "execution",
            &json!({
                "id": execution_id, "status": "running",
                "taskId": execution_id, "sessionId": "s1",
                "request": "실패 프로그램"
            }),
        );
        let session = json!({"id":"s1"});
        let request = TurnRequest {
            session_id: Some("s1".into()),
            input: "실패 프로그램".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: None,
            model: None,
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let program = json!({
            "revision":"p-revision-failure",
            "nodes":[
                {"type":"call_tool","id":"try","tool":"shell_exec",
                 "inputs":{"command":"exit 7"},"saveAs":"try","onFailure":"revise"}
            ]
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let metrics = std::sync::Mutex::new(Metrics::default());
        let mut budget = super::super::budget::BudgetLedger::new(
            MAX_ROUNDS, MAX_TOOL_CALLS, MAX_ELAPSED,
        );
        let mut op_seq = 0u64;
        let generation = state.cancellation_generation("s1");
        run_program(
            &state, &tx, &session, &request, "s1", execution_id, &program,
            &metrics, &mut budget, &mut op_seq, generation,
        )
        .await
        .expect("program runs and reports the unresolved failure");
        let execution = state.store.get("execution", execution_id).unwrap();
        assert_eq!(
            execution["programCheckpoint"]["status"], "needs_review",
            "{execution}"
        );
        assert!(!execution["programCheckpoint"]["programFailure"].is_null());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_records_the_whole_applied_bundle_and_real_context() {
        let (state, dir) = state();
        let task_id = "task-snapshot";
        crate::engine::episode::start_root(
            &state,
            crate::engine::episode::RootSpec {
                task_id,
                intent: "bench 작업",
                family: "chat",
                source: "owner",
                source_message_id: None,
                scope: json!({"partition":"local","sessionId":"session-1"}),
                mandatory: Vec::new(),
            },
        );
        let request = TurnRequest {
            session_id: Some("session-1".into()),
            input: "bench 작업".into(),
            attachment_ids: Vec::new(),
            agent_id: None,
            provider_id: Some("p".into()),
            model: Some("m".into()),
            tools_enabled: true,
            tools_execution_enabled: true,
            thinking: None,
            reasoning_effort: None,
            reasoning_language: None,
            auto_learning: false,
            system_override: None,
            task_id: None,
            parent_task_id: None,
            task_revision: None,
            verification_owner: None,
        };
        let strategies = vec![
            json!({
                "id":"s1","revision":"r1","kind":"strategy","authority":"adaptive",
                "text":"first","assistanceMode":"harness_assisted",
                "verification":{"type":"procedure","appliesTo":{"requestContains":["bench"]}}
            }),
            json!({
                "id":"s2","revision":"r2","kind":"strategy","authority":"adaptive",
                "text":"second","assistanceMode":"model_guided",
                "verification":{"type":"none","appliesTo":{}}
            }),
            // Legacy record without a revision must still be recorded as an
            // applied arm instead of silently disappearing from the bundle.
            json!({"id":"s3","kind":"tool_policy","authority":"adaptive","text":"legacy"}),
        ];
        let context_items = vec![
            json!({"kind":"system_prompt","digest":"d1"}),
            json!({"kind":"history","count":0}),
        ];
        record_selection_snapshot(
            &state,
            "exec-1",
            task_id,
            "session-1",
            &request,
            &strategies,
            context_items,
            "watermark-1".into(),
            json!({"providerId":"p","model":"m","toolMode":"native"}),
        )
        .unwrap();

        let snapshots = state.store.all("selection_snapshot");
        assert_eq!(snapshots.len(), 1);
        let snapshot = &snapshots[0];
        let arms = snapshot["policyBundle"]["arms"].as_array().unwrap();
        assert_eq!(arms.len(), 3, "every applied arm must be recorded");
        assert_eq!(arms[0]["armId"], "s1");
        assert_eq!(arms[1]["armId"], "s2");
        assert_eq!(arms[2]["armId"], "s3");
        assert!(!arms[2]["revision"].as_str().unwrap_or("").is_empty());
        assert_eq!(arms[0]["role"], "policy");
        assert_eq!(arms[1]["role"], "advice");
        assert_eq!(snapshot["policyBundle"]["selection"], "deterministic");
        assert_eq!(snapshot["policyBundle"]["probability"], 1.0);
        assert_eq!(snapshot["decisions"][0]["selected"], "bundle:exec-1");
        assert_eq!(
            snapshot["decisions"][0]["context"]["appliedArmIds"],
            json!(["s1", "s2", "s3"])
        );
        assert_eq!(snapshot["memoryWatermark"], "watermark-1");
        assert_eq!(snapshot["contextItems"].as_array().unwrap().len(), 2);
        assert_eq!(snapshot["completeness"]["reproducible"], true);
        assert_eq!(snapshot["model"]["model"], "m");
        assert!(!snapshot["snapshotHash"].as_str().unwrap_or("").is_empty());

        let decisions = state.store.all("strategy_decision");
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0]["selected"], "bundle:exec-1");
        assert_eq!(decisions[0]["candidates"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compliance_summary_prefers_violation_over_followed() {
        use crate::engine::adaptation::types::Compliance;
        let mixed = vec![
            json!({
                "strategyId":"a","applicable":true,
                "result":{"triggered":true,"followed":true}
            }),
            json!({
                "strategyId":"b","applicable":true,
                "result":{"triggered":true,"followed":false}
            }),
        ];
        assert_eq!(compliance_summary(&mixed), Compliance::Violated);
        assert_eq!(compliance_summary(&mixed[..1]), Compliance::Followed);

        let violated_and_unknown = vec![
            json!({
                "strategyId":"a","applicable":true,
                "result":{"verified":true,"triggered":true,"followed":false}
            }),
            json!({
                "strategyId":"b","applicable":true,
                "result":{"verified":false,"triggered":true,"status":"unknown"}
            }),
        ];
        assert_eq!(
            compliance_summary(&violated_and_unknown),
            Compliance::Violated,
            "unknown must not mask a confirmed violation"
        );
        let detail = compliance_detail(&violated_and_unknown);
        assert_eq!(detail["summary"], "violated");
        assert_eq!(detail["violated"], json!(["a"]));
        assert_eq!(detail["unknown"], json!(["b"]));
        assert_eq!(detail["total"], 2);

        assert_eq!(
            compliance_summary(&[json!({
                "strategyId":"c","applicable":true,
                "result":{"triggered":true,"status":"unknown"}
            })]),
            Compliance::Unknown
        );
        assert_eq!(
            compliance_summary(&[json!({
                "strategyId":"d","applicable":true,
                "result":{"triggered":false}
            })]),
            Compliance::NotTriggered
        );
        assert_eq!(compliance_summary(&[]), Compliance::NotApplicable);
    }

    #[test]
    fn completed_is_not_automatically_succeeded() {
        assert_eq!(
            determine_task_outcome(&execution("completed", false, Value::Null)),
            TaskOutcome::AnsweredUnverified
        );
        assert_eq!(
            determine_task_outcome(&execution("completed", false, json!("check"))),
            TaskOutcome::Failed
        );
        assert_eq!(
            determine_task_outcome(&execution("completed", true, json!("check"))),
            TaskOutcome::Succeeded
        );
    }

    #[test]
    fn standing_violation_forces_failure() {
        let mut value = execution("completed", true, json!("check"));
        value["strategyCompliance"] = json!([{
            "strategyId":"s1","authority":"standing","applicable":true,
            "result":{"verified":true,"triggered":true,"followed":false}
        }]);
        assert_eq!(determine_task_outcome(&value), TaskOutcome::Failed);
    }

    #[test]
    fn simple_deterministic_tasks_skip_the_planner() {
        let checks = Some((
            "create report".to_string(),
            json!([{"type":"file_changed","path":"report.txt"}]),
        ));
        assert!(!needs_plan("report.txt 파일을 만들어줘", &checks));
        assert!(needs_plan("파일을 만들고 그리고 내용도 확인해", &checks));
        assert!(needs_plan("report.txt 파일을 만들어줘", &None));
        assert!(needs_plan(&"가".repeat(300), &checks));
    }

    #[test]
    fn adaptive_strategy_violation_still_allows_task_success() {
        let mut value = execution("completed", true, json!("check"));
        value["strategyCompliance"] = json!([{
            "strategyId":"s1","authority":"adaptive","applicable":true,
            "result":{"verified":true,"triggered":true,"followed":false}
        }]);
        assert_eq!(determine_task_outcome(&value), TaskOutcome::Succeeded);
    }

    #[test]
    fn success_experience_is_idempotent() {
        let (state, dir) = state();
        let execution = json!({
            "id":"exec-once","sessionId":"s","agentId":null,
            "acceptanceText":"create a verified file",
            "finalMessageId":"m1","finalAnswer":"verified",
            "toolsUsed":["workspace_write"]
        });
        assert!(record_success_experience(
            &state,
            &execution,
            "exec-once",
            TaskOutcome::Succeeded
        )
        .is_some());
        assert!(record_success_experience(
            &state,
            &execution,
            "exec-once",
            TaskOutcome::Succeeded
        )
        .is_none());
        let experiences = state.store.all("experience");
        assert_eq!(experiences.len(), 1);
        assert_eq!(experiences[0]["occurrences"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelled_and_failed_are_not_success() {
        assert_eq!(
            determine_task_outcome(&execution("cancelled", true, json!("check"))),
            TaskOutcome::Cancelled
        );
        assert_eq!(
            determine_task_outcome(&execution("failed", true, json!("check"))),
            TaskOutcome::Failed
        );
    }
