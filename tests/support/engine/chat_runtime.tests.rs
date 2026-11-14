// Runtime unit/regression tests, kept out of the production execution source.
#[cfg(test)]
mod outcome_tests {
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
}

#[cfg(test)]
mod harness_tool_selection_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sub1_compiler_repair_tool_menu_advances_from_read_to_actual_edit() {
        let full=vec![
            json!({"function":{"name":"code_search"}}),
            json!({"function":{"name":"workspace_read_range"}}),
            json!({"function":{"name":"workspace_replace_range"}}),
            json!({"function":{"name":"workspace_replace"}}),
        ];
        let before=full.iter().filter(|tool|
            tool_name(tool)=="workspace_read_range").count();
        let after=full.iter().filter(|tool|
            matches!(tool_name(tool),"workspace_replace_range"|"workspace_replace"))
            .count();
        assert_eq!((before,after),(1,2));
        assert!(rsi_compiler_repair_tool("workspace_replace_range"));
        assert!(!rsi_compiler_repair_tool("code_search"));
    }

    #[test]
    fn sub1_target_window_grounds_small_model_in_actual_sibling_boundaries() {
        let raw="fn marker_at() -> bool { true }\n\nfn split_markers(text:&str) -> Vec<String> {\n    vec![text.to_owned()]\n}\n\nfn numbered_lines() {}\n";
        assert_eq!(rsi_target_function_window(
            "Improve split_markers() while preserving marker_at()",raw,40),
            Some(("split_markers".into(),42,45)));
        assert_eq!(rsi_target_function_window(
            "Improve missing_symbol()",raw,40),None);
        let nested="  fn split_markers() {}\n  fn marker_at() {}\n";
        assert_eq!(rsi_target_function_window(
            "Improve split_markers()",nested,1),None);
    }

    #[test]
    fn sub1_new_function_editor_still_gets_grounded_trial_after_older_method_learning() {
        assert!(rsi_untried_function_tool_allowed(
            "workspace_replace_function","workspace_replace_range",false,true));
        assert!(!rsi_untried_function_tool_allowed(
            "workspace_replace_function","code_search",false,true));
        assert!(!rsi_untried_function_tool_allowed(
            "workspace_replace_function","workspace_replace_range",false,false));
        assert!(!rsi_untried_function_tool_allowed(
            "workspace_write","workspace_replace_range",false,true));
        assert!(rsi_untried_function_tool_allowed(
            "workspace_read_range","workspace_replace_range",true,true));
        assert!(rsi_compiler_repair_tool("workspace_replace_function"));
    }

    #[test]
    fn sub1_learned_function_editor_receives_exclusive_source_tool_menu() {
        let method=json!({"preferredTool":"workspace_replace_function",
            "fallbackTool":"workspace_replace_range"});
        assert_eq!(rsi_method_edit_choice(&method,&[],"run"),
            Some(("workspace_replace_function".into(),false)));
        let failed=json!({"runId":"run","tool":"workspace_replace_function",
            "status":"failed","result":{"written":false}});
        assert_eq!(rsi_method_edit_choice(&method,&[failed],"run"),
            Some(("workspace_replace_range".into(),true)));
        assert!(rsi_method_tool_allowed(
            "workspace_replace_function","workspace_replace_function",true));
        assert!(!rsi_method_tool_allowed(
            "workspace_replace_range","workspace_replace_function",true));
        assert!(rsi_method_tool_allowed(
            "workspace_read_range","workspace_replace_function",true));
        let stale=json!({"tool":"workspace_replace_function","status":"failed",
            "result":{"error":"workspace_replace_function: file version changed; read again before editing"}});
        assert!(rsi_needs_fresh_source(&stale));
        assert!(rsi_action_program_tool_allowed(
            "workspace_read_range","workspace_replace_function",false));
    }

    #[test]
    fn sub1_compiler_repair_has_time_for_a_real_corrective_edit() {
        assert_eq!(rsi_generation_round_limit(24,true),6);
        assert_eq!(rsi_generation_round_limit(4,true),4);
        assert_eq!(rsi_generation_round_limit(6,true),6);
        assert_eq!(rsi_generation_round_limit(8,false),16);
        assert_eq!(rsi_generation_round_limit(20,false),20);
    }

    #[test]
    fn sub1_syntax_invalid_write_must_be_repaired_before_candidate_capture() {
        let invalid=json!({"runId":"execution","status":"completed",
            "tool":"workspace_replace_range","result":{
                "written":true,"syntaxValid":false},
            "toolReceipt":{"verified":true}});
        assert!(!rsi_confirmed_source_write(&[invalid.clone()],"execution"));
        let mut corrected=invalid;
        corrected["result"]["syntaxValid"]=json!(true);
        assert!(rsi_confirmed_source_write(&[corrected],"execution"));
    }

    #[test]
    fn short_or_malformed_model_sha_must_bind_only_to_a_real_read() {
        assert!(!rsi_source_sha_valid(None));
        assert!(!rsi_source_sha_valid(Some("")));
        assert!(!rsi_source_sha_valid(Some("b4a48b5d76e4d13a585778d1f0b66a71")),
            "The actual free model sent 32 hex digits, which used to bypass the binder");
        assert!(!rsi_source_sha_valid(Some(&"z".repeat(64))));
        assert!(rsi_source_sha_valid(Some(&"a".repeat(64))));
        let operations=vec![json!({"id":"native-read","sessionId":"real-session",
            "status":"completed","tool":"workspace_read_range",
            "finishedAt":"2026-11-12T00:00:03Z",
            "result":{"path":"src/engine/decompose.rs","sourceSha256":"f".repeat(64)}})];
        assert_eq!(rsi_observed_source_hash(&operations,"real-session",
            "src/engine/decompose.rs").unwrap().1,"f".repeat(64));
        assert!(rsi_observed_source_hash(&operations,"other-session",
            "src/engine/decompose.rs").is_none());
        assert!(rsi_observed_source_hash(&operations,"real-session",
            "src/engine/other.rs").is_none());
    }

    #[test]
    fn sub1_compile_repair_requires_read_after_the_actual_write() {
        let repair=json!({"rsiCompilerRepair":true});
        let write=json!({"id":"w1","tool":"workspace_replace_range",
            "status":"completed","finishedAt":"2026-11-12T00:00:02Z",
            "arguments":{"path":"src/engine/decompose.rs"},
            "result":{"written":true,"path":"src/engine/decompose.rs"},
            "toolReceipt":{"verified":true}});
        let stale=json!({"id":"r1","tool":"workspace_read_range",
            "status":"completed","finishedAt":"2026-11-12T00:00:01Z",
            "result":{"path":"src/engine/decompose.rs","sourceSha256":"a".repeat(64)}});
        let fresh=json!({"id":"r2","tool":"workspace_read_range",
            "status":"completed","finishedAt":"2026-11-12T00:00:03Z",
            "result":{"path":"src/engine/decompose.rs","sourceSha256":"b".repeat(64)}});
        let wrong=json!({"id":"r3","tool":"workspace_read_range",
            "status":"completed","finishedAt":"2026-11-12T00:00:04Z",
            "result":{"path":"src/engine/other.rs","sourceSha256":"c".repeat(64)}});
        assert!(rsi_compiler_requires_current_read(&repair,&[stale.clone(),write.clone()]));
        assert!(rsi_compiler_requires_current_read(&repair,&[stale,write.clone(),wrong]));
        assert!(!rsi_compiler_requires_current_read(&repair,&[write.clone(),fresh.clone()]));
        assert!(!rsi_compiler_requires_current_read(&json!({"rsiCompilerRepair":false}),&[write.clone()]));
        let inherited=json!({"rsiCompilerRepair":true,"rsiRestoredCandidateId":"candidate"});
        assert!(rsi_compiler_requires_current_read(&inherited,&[]));
        assert!(!rsi_compiler_requires_current_read(&inherited,&[fresh]));
    }

    #[test]
    fn tool_discovery_selects_core_or_query_relevant_tools() {
        let definitions = vec![
            json!({"type":"function","function":{"name":"workspace_write","description":"write"}}),
            json!({"type":"function","function":{"name":"web_search","description":"search the web"}}),
            json!({"type":"function","function":{"name":"github_api","description":"github"}}),
        ];
        let core = select_tool_definitions(
            definitions.clone(),
            "검색",
            &json!({"implementation":"core"}),
        );
        assert_eq!(core.len(), 2);
        assert!(core.iter().all(|tool| tool_name(tool) != "github_api"));
        let relevant = select_tool_definitions(
            definitions,
            "github issue",
            &json!({"implementation":"query-relevant","parameters":{"extra":2}}),
        );
        assert!(relevant.iter().any(|tool| tool_name(tool) == "workspace_write"));
        assert!(relevant.iter().any(|tool| tool_name(tool) == "github_api"));
    }

    #[test]
    fn room_tool_selection_is_zero_for_casual_and_exact_for_actions() {
        let definitions = vec![
            json!({"type":"function","function":{"name":"workspace_read","description":"Read a workspace file"}}),
            json!({"type":"function","function":{"name":"workspace_write","description":"Write a workspace file"}}),
            json!({"type":"function","function":{"name":"shell_exec","description":"Run a shell command"}}),
            json!({"type":"function","function":{"name":"web_search","description":"Search the web"}}),
            json!({"type":"function","function":{"name":"memory_store","description":"Store a memory"}}),
            json!({"type":"function","function":{"name":"memory_search","description":"Search memory"}}),
            json!({"type":"function","function":{"name":"mcp_github_search","description":"Search GitHub issues and pull requests"}}),
        ];
        let empty = std::collections::HashSet::new();
        assert!(select_room_tools(definitions.clone(), "안녕", &empty, false).is_empty());
        assert!(select_room_tools(definitions.clone(), "오늘 날씨는 어때 보여?", &empty, false).is_empty());
        assert!(select_room_tools(definitions.clone(), "책 읽는 건 좋아해?", &empty, false).is_empty());
        assert!(select_room_tools(
            definitions.clone(),
            "내가 첫 턴에 뭐라고 했는지 기억해?",
            &empty,
            false
        )
        .iter()
        .all(|tool| !matches!(tool_name(tool), "memory_store" | "memory_search")));
        let memory = select_room_tools(definitions.clone(), "메모리에 저장해", &empty, false);
        assert!(memory.iter().any(|tool| tool_name(tool) == "memory_store"), "{memory:?}");
        let files = select_room_tools(definitions.clone(), "파일을 읽고 저장해", &empty, false);
        let file_names = files.iter().map(tool_name).collect::<Vec<_>>();
        assert!(file_names.contains(&"workspace_read"), "{file_names:?}");
        assert!(file_names.contains(&"workspace_write"), "{file_names:?}");
        assert!(!file_names.contains(&"mcp_github_search"), "{file_names:?}");
        let mcp = select_room_tools(definitions.clone(), "github 이슈를 검색해", &empty, false);
        assert!(mcp.iter().any(|tool| tool_name(tool) == "mcp_github_search"));
        let mut recent = std::collections::HashSet::new();
        recent.insert("shell_exec".to_string());
        let continued = select_room_tools(definitions.clone(), "계속", &recent, false);
        assert!(continued.iter().any(|tool| tool_name(tool) == "shell_exec"));
        let accepted = select_room_tools(definitions, "작업", &empty, true);
        assert!(!accepted.is_empty());
    }

    #[test]
    fn room_tool_intent_uses_full_english_words_not_accidental_substrings() {
        let tools = vec![
            json!({"function":{"name":"workspace_read","description":"Read file"}}),
            json!({"function":{"name":"shell_exec","description":"Run shell"}}),
            json!({"function":{"name":"web_search","description":"Search web"}}),
        ];
        let none = std::collections::HashSet::new();
        let casual = "The latest profile thread on brunch and capitalization";
        assert!(!room_tool_request(casual));
        assert!(select_room_tools(tools.clone(), casual, &none, false).is_empty());

        assert!(room_tool_request("Please read files and run tests"));
        let selected = select_room_tools(tools, "read files and run tests", &none, false);
        assert!(selected.iter().any(|tool| tool_name(tool) == "workspace_read"));
        assert!(selected.iter().any(|tool| tool_name(tool) == "shell_exec"));
    }

    #[test]
    fn room_tool_history_is_used_only_for_explicit_continuation() {
        let definitions = vec![
            json!({"function":{"name":"workspace_write","description":"Write workspace content"}}),
            json!({"function":{"name":"shell_exec","description":"Run shell commands"}}),
        ];
        let recent = std::collections::HashSet::from(["shell_exec".to_string()]);
        let new_action = select_room_tools(definitions.clone(), "수정해", &recent, false);
        assert!(new_action.iter().any(|tool| tool_name(tool) == "workspace_write"));
        assert!(new_action.iter().all(|tool| tool_name(tool) != "shell_exec"));
        let continuation = select_room_tools(definitions, "계속", &recent, false);
        assert!(continuation.iter().any(|tool| tool_name(tool) == "shell_exec"));
    }

    #[test]
    fn room_code_edit_exposes_surgical_editing_and_source_discovery() {
        let definitions = vec![
            json!({"function":{"name":"code_search"}}),
            json!({"function":{"name":"workspace_read_range"}}),
            json!({"function":{"name":"workspace_replace_range"}}),
            json!({"function":{"name":"workspace_replace_function"}}),
            json!({"function":{"name":"web_search"}}),
        ];
        let selected = select_room_tools(
            definitions, "이 코드 버그를 수정해", &std::collections::HashSet::new(), false,
        );
        let names = selected.iter().map(tool_name).collect::<Vec<_>>();
        assert!(names.contains(&"code_search"), "{names:?}");
        assert!(names.contains(&"workspace_replace_range"), "{names:?}");
        assert!(names.contains(&"workspace_replace_function"), "{names:?}");
        assert!(!names.contains(&"web_search"), "{names:?}");
        let english = vec![
            json!({"function":{"name":"code_search"}}),
            json!({"function":{"name":"workspace_replace_function"}}),
            json!({"function":{"name":"web_search"}}),
        ];
        let repaired = select_room_tools(
            english, "Fix the bug in this code",
            &std::collections::HashSet::new(), false,
        );
        assert!(repaired.iter().any(|tool| tool_name(tool) == "code_search"));
        assert!(repaired.iter().any(|tool| tool_name(tool) == "workspace_replace_function"));
        assert!(repaired.iter().all(|tool| tool_name(tool) != "web_search"));
    }

    #[test]
    fn path_arguments_are_normalized_only_when_the_kernel_is_active() {
        let raw = json!({"path":"  ./sub//file.txt  ","content":"x"});
        let untouched = normalize_tool_arguments(raw.clone(), "none");
        assert_eq!(untouched["path"], "  ./sub//file.txt  ");
        let normalized = normalize_tool_arguments(raw, "normalize-paths");
        assert_eq!(normalized["path"], "sub/file.txt");
        assert_eq!(normalized["content"], "x");
    }

    #[test]
    fn read_first_ordering_puts_observation_before_mutation() {
        assert!(tool_is_read_only("workspace_read"));
        assert!(tool_is_read_only("web_search"));
        assert!(!tool_is_read_only("workspace_write"));
        assert!(!tool_is_read_only("shell_exec"));
        let mut calls = vec![
            ToolCall { id: "1".into(), name: "workspace_write".into(), arguments: "{}".into() },
            ToolCall { id: "2".into(), name: "workspace_read".into(), arguments: "{}".into() },
        ];
        calls.sort_by_key(|call| !tool_is_read_only(&call.name));
        assert_eq!(calls[0].name, "workspace_read");
        assert_eq!(calls[1].name, "workspace_write");
    }

    #[test]
    fn tool_schemas_fit_the_selected_provider_budget() {
        let tools = (0..30)
            .map(|index| json!({"type":"function","function":{"name":format!("tool{index}"),"description":"x".repeat(100)}}))
            .collect::<Vec<_>>();
        let kept = trim_tool_definitions(tools, 2_000);
        assert!(!kept.is_empty());
        assert!(kept.len() < 30);
        let total = kept
            .iter()
            .map(|tool| serde_json::to_string(tool).unwrap().len())
            .sum::<usize>();
        assert!(total <= 2_000 || kept.len() == 1);
    }

    #[test]
    fn measured_quality_descendant_reads_then_edits_not_researches() {
        assert!(rsi_quality_revision_tool("workspace_read_range",false));
        assert!(!rsi_quality_revision_tool("code_search",false));
        assert!(!rsi_quality_revision_tool("workspace_replace_function",false));
        assert!(rsi_quality_revision_tool("workspace_replace_function",true));
        assert!(rsi_quality_revision_tool("workspace_replace_range",true));
        assert!(rsi_quality_revision_tool("workspace_replace",true));
        assert!(!rsi_quality_revision_tool("code_search",true));
    }

    #[test]
    fn rsi_checkpoint_allows_an_edit_or_a_no_change_finish() {
        assert!(!rsi_should_checkpoint(5, false));
        assert!(rsi_should_checkpoint(6, false));
        assert!(!rsi_should_checkpoint(6, true));
        for name in ["workspace_replace", "workspace_replace_range", "workspace_write", "workspace_read_range", "rsi_method_propose", "algorithm_propose"] {
            assert!(rsi_checkpoint_tool(name,false), "{name} must stay available at the checkpoint");
        }
        for name in ["code_search", "shell_exec", "file_write", "web_search"] {
            assert!(!rsi_checkpoint_tool(name,false), "{name} must not stay available");
        }
        assert!(tool_discovery_applies(false, false));
        assert!(!tool_discovery_applies(true, false));
        assert!(!tool_discovery_applies(false, true), "RSI must keep its full allowed toolset");
    }

    #[test]
    fn rsi_uses_the_objective_and_checks_budget_before_another_call() {
        let session=json!({"rsiRun":true,"rsiObjective":"작은 능력 하나 개선"});
        assert_eq!(task_input(&session,"quoted source and index"),"작은 능력 하나 개선");
        assert_eq!(task_input(&json!({}),"owner input"),"owner input");
        assert_eq!(rsi_output_allowance(240_000,9_000).unwrap(),1000);
        assert!(rsi_output_allowance(249_000,1000).is_err());
        assert_eq!(rsi_output_allowance(0,1000).unwrap(),8192);
    }

    #[test]
    fn long_running_rsi_uses_campaign_bounded_worker_time_not_chat_limit() {
        let normal=json!({"kind":"conversation"});
        let room=json!({"rsiRun":true,"roomId":"room1"});
        let rsi=json!({"rsiRun":true,"rsiCampaignId":"rsi-123"});
        let ad_hoc=json!({"rsiRun":true});
        assert_eq!(elapsed_limit_for_session(&normal),MAX_ELAPSED);
        assert_eq!(elapsed_limit_for_session(&room),MAX_ELAPSED);
        assert_eq!(elapsed_limit_for_session(&ad_hoc),MAX_ELAPSED);
        assert_eq!(elapsed_limit_for_session(&rsi).as_secs(),840);
    }

    #[test]
    fn grounded_explicit_repairs_enter_edit_stage_after_first_read() {
        assert!(rsi_initial_focused_tool("workspace_read_range"));
        assert!(rsi_initial_focused_tool("workspace_replace"));
        assert!(rsi_initial_focused_tool("workspace_replace_range"));
        assert!(rsi_initial_focused_tool("workspace_write"));
        assert!(!rsi_initial_focused_tool("code_search"));
        assert!(!rsi_initial_focused_tool("algorithm_propose"));
        assert_eq!(rsi_checkpoint_stage(1,1,false,true,false),Some(true));
        assert_eq!(rsi_checkpoint_stage(1,0,false,true,true),Some(true));
        assert_eq!(rsi_checkpoint_stage(1,0,false,true,false),None);
        assert_eq!(rsi_checkpoint_stage(1,1,false,false,false),None);
        assert_eq!(rsi_checkpoint_stage(6,0,false,false,false),Some(false));
        assert_eq!(rsi_checkpoint_stage_with_limit(3,0,false,false,false,3),Some(false),
            "A learned research method must affect the native runtime");
        assert_eq!(rsi_checkpoint_stage_with_limit(3,0,false,false,false,6),None);
        assert_eq!(rsi_checkpoint_stage(3,3,true,true,true),None);
        assert!(rsi_checkpoint_tool("workspace_replace",true));
        assert!(rsi_checkpoint_tool("workspace_replace_range",true));
        assert!(!rsi_checkpoint_tool("workspace_read_range",true));
        assert!(rsi_checkpoint_tool("workspace_read_range",false));
    }

    #[test]
    fn composed_action_program_controls_search_edit_and_recovery() {
        let plan=json!({"sequence":["code_search","workspace_read_range",
            "workspace_replace_range"],
            "onFailure":[{"onFailure":"workspace_replace",
                "tryTool":"workspace_replace_range"}]});
        let first=rsi_action_program_next(&plan,&[],"run");
        assert_eq!(first,Some(("code_search".into(),
            "induced_cross_episode_sequence".into())));
        let searched=json!({"runId":"run","tool":"code_search","status":"completed",
            "opSeq":1,"result":{"matches":[]}});
        assert_eq!(rsi_action_program_next(&plan,&[searched.clone()],"run")
            .unwrap().0,"workspace_read_range");
        let read=json!({"runId":"run","tool":"workspace_read_range","status":"completed",
            "opSeq":2,"result":{"content":"fn x(){}"}});
        assert_eq!(rsi_action_program_next(&plan,&[searched,read.clone()],"run")
            .unwrap().0,"workspace_replace_range");
        assert_eq!(rsi_action_program_next(&plan,&[read],"run")
            .unwrap().0,"workspace_replace_range",
            "Skip optional discovery already bypassed by genuine source evidence");
        let rejected=json!({"runId":"run","tool":"workspace_replace","status":"failed",
            "opSeq":3,"result":{"error":"oldText mismatch"}});
        assert_eq!(rsi_action_program_next(&plan,&[rejected],"run")
            .unwrap().1,"observed_source_precondition_recovery",
            "An actual stale oldText must refresh source before fallback");
        assert!(rsi_action_program_tool_allowed("workspace_read_range",
            "workspace_replace_range",false),
            "An edit without a read must still be able to obtain its source SHA");
        assert!(!rsi_action_program_tool_allowed("workspace_read_range",
            "workspace_replace_range",true),
            "After a real read, only the learned edit is offered");
        assert!(!rsi_action_program_tool_allowed("code_search",
            "workspace_replace_range",true),
            "Native policy must not restart discovery at the mutation step");
        assert!(rsi_action_program_tool_allowed("workspace_replace_range",
            "workspace_replace_range",true));
        assert!(!rsi_action_program_tool_allowed("web_search","workspace_replace_range",false));
        assert!(rsi_action_program_next(&plan,&[],"unrelated").is_some());
    }

    #[test]
    fn model_cannot_execute_tools_omitted_by_its_native_rsi_program() {
        let definitions=vec![
            json!({"type":"function","function":{"name":"workspace_replace_range"}}),
            json!({"type":"function","function":{"name":"workspace_replace"}})
        ];
        assert!(rsi_tool_offered_this_round(&definitions,"workspace_replace_range"));
        assert!(rsi_tool_offered_this_round(&definitions,"workspace_replace"));
        assert!(!rsi_tool_offered_this_round(&definitions,"workspace_read_range"));
        assert!(!rsi_tool_offered_this_round(&definitions,"code_search"));
        assert!(!rsi_tool_offered_this_round(&[],"workspace_replace"),
            "An empty forced-final tool menu cannot execute a model tool call");
    }

    #[test]
    fn stale_source_hash_induces_a_refresh_before_another_edit() {
        let sequence=json!({"sequence":["workspace_read_range",
            "workspace_replace_range"],"onFailure":[],"recoveryPaths":[]});
        let failure=json!({"runId":"r","tool":"workspace_replace_range",
            "status":"failed","opSeq":2,
            "result":{"error":"workspace_replace_range: 소스가 읽은 뒤 변경되었습니다"}});
        assert!(rsi_needs_fresh_source(&failure));
        let next=rsi_action_program_next(&sequence,&[failure.clone()],"r").unwrap();
        assert_eq!(next,("workspace_read_range".into(),
            "observed_source_precondition_recovery".into()));
        assert!(rsi_checkpoint_with_recovery("workspace_read_range",true,Some(&next)));
        let repaired=json!({"runId":"r","tool":"workspace_read_range",
            "status":"completed","opSeq":3,"result":{"sourceSha256":"new-hash"}});
        assert_eq!(rsi_action_program_next(&sequence,&[failure,repaired],"r")
            .unwrap().0,"workspace_replace_range");
        let declined=json!({"runId":"r","tool":"workspace_replace_range",
            "status":"failed","result":{"error":"Denied by the user"}});
        assert!(!rsi_needs_fresh_source(&declined),
            "A denied approval is not a source-hash counterexample");
    }

    #[test]
    fn program_cursor_survives_multiple_model_turns_in_one_rsi_generation() {
        let plan=json!({"sequence":["code_search","workspace_read_range",
            "workspace_replace_range"],"onFailure":[],"recoveryPaths":[]});
        let read=json!({"sessionId":"gen-session","runId":"old-turn",
            "opSeq":2,"startedAt":"2026-11-11T08:00:01Z",
            "id":"old-turn:2","tool":"workspace_read_range",
            "status":"completed","result":{"sourceSha256":"exact-source"}});
        assert_eq!(rsi_program_next_in_session(&plan,&[read.clone()],
            "new-turn","gen-session").unwrap().0,"workspace_replace_range",
            "Read proof from the preceding model turn must advance the program");
        assert_eq!(rsi_program_next_in_session(&plan,&[read.clone()],
            "new-turn","different-session").unwrap().0,"code_search",
            "Another generation may not inherit a different session's tool cursor");
        let written=json!({"sessionId":"gen-session","runId":"new-turn",
            "opSeq":1,"startedAt":"2026-11-11T08:00:02Z",
            "id":"new-turn:1","tool":"workspace_replace_range",
            "status":"completed","result":{"written":true},
            "toolReceipt":{"verified":true}});
        assert!(rsi_program_next_in_session(&plan,&[read,written],
            "third-turn","gen-session").is_none(),
            "A verified edit hands the work to candidate checking, not another read");
    }

    #[test]
    fn old_absolute_rsi_read_paths_become_scoped_relative_edit_arguments() {
        let root=std::env::temp_dir().join(format!("rsi-normalize-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        let file=root.join("src/engine/edit.rs");
        std::fs::write(&file,"fn example() {}\n").unwrap();
        assert_eq!(rsi_relative_edit_path(&root,&file.to_string_lossy()),
            Some("src/engine/edit.rs".into()));
        assert_eq!(rsi_relative_edit_path(&root,"./src/engine/edit.rs"),
            Some("src/engine/edit.rs".into()));
        assert_eq!(rsi_relative_edit_path(&root,"src/engine/new.rs"),
            Some("src/engine/new.rs".into()));
        assert!(rsi_relative_edit_path(&root,"../outside.rs").is_none());
        let outside=std::env::temp_dir().join(format!("rsi-outside-{}.rs",uuid::Uuid::new_v4()));
        std::fs::write(&outside,"// outside\n").unwrap();
        assert!(rsi_relative_edit_path(&root,&outside.to_string_lossy()).is_none());
        let _=std::fs::remove_file(outside);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn compiler_repair_keeps_read_and_edit_but_never_resumes_discovery() {
        assert!(rsi_compiler_repair_tool("workspace_read_range"));
        assert!(rsi_compiler_repair_tool("workspace_replace"));
        assert!(rsi_compiler_repair_tool("workspace_replace_range"));
        assert!(!rsi_compiler_repair_tool("code_search"));
        assert!(!rsi_compiler_repair_tool("web_search"));
    }

    #[test]
    fn edit_only_checkpoint_preserves_only_the_learned_source_refresh() {
        let recovery=("workspace_read_range".into(),"learned_multi_step_recovery".into());
        assert!(rsi_checkpoint_with_recovery("workspace_read_range",true,Some(&recovery)));
        assert!(rsi_checkpoint_with_recovery("workspace_replace",true,Some(&recovery)));
        assert!(!rsi_checkpoint_with_recovery("code_search",true,Some(&recovery)));
        assert!(!rsi_checkpoint_with_recovery("workspace_read_range",true,None));
        let arbitrary=("workspace_read_range".into(),"induced_cross_episode_sequence".into());
        assert!(!rsi_checkpoint_with_recovery("workspace_read_range",true,Some(&arbitrary)));
    }

    #[test]
    fn induced_recovery_executes_verified_multi_step_path_and_stops_after_edit() {
        let program=json!({"sequence":["code_search","workspace_read_range",
            "workspace_replace"],
            "recoveryPaths":[{"onFailure":"workspace_replace",
                "steps":["workspace_read_range","workspace_replace_range"],
                "observedRecoveries":2,"weightedEvidence":4}],
            "onFailure":[{"onFailure":"workspace_replace",
                "tryTool":"workspace_replace_range"}]});
        let failure=json!({"runId":"run","opSeq":1,"tool":"workspace_replace",
            "status":"completed","result":{"written":false}});
        assert_eq!(rsi_action_program_next(&program,&[failure.clone()],"run"),
            Some(("workspace_read_range".into(),"learned_multi_step_recovery".into())));
        let read=json!({"runId":"run","opSeq":2,"tool":"workspace_read_range",
            "status":"completed","result":{"content":"actual source"}});
        assert_eq!(rsi_action_program_next(&program,&[failure.clone(),read.clone()],"run"),
            Some(("workspace_replace_range".into(),"learned_multi_step_recovery".into())));
        let written=json!({"runId":"run","opSeq":3,"tool":"workspace_replace_range",
            "status":"completed","result":{"written":true},
            "toolReceipt":{"verified":true}});
        assert_eq!(rsi_action_program_next(&program,
            &[failure.clone(),read.clone(),written],"run"),None,
            "A verified edit must hand off to candidate evaluation");
        let failed_read=json!({"runId":"run","opSeq":2,"tool":"workspace_read_range",
            "status":"failed","result":{"error":"source unavailable"}});
        assert_ne!(rsi_action_program_next(&program,
            &[failure,failed_read],"run").unwrap().0,"workspace_replace_range",
            "An unsuccessful prerequisite cannot advance a repair program");
    }

    #[test]
    fn local_model_json_patch_is_interpreted_only_with_full_revision_evidence() {
        let mut allowed=std::collections::HashSet::new();
        allowed.insert("workspace_replace_range".to_string());
        allowed.insert("workspace_replace".to_string());
        let sha="a".repeat(64);
        let args=json!({"path":"src/engine/memory.rs",
            "startLine":10,"endLine":11,"newText":"fn changed() {}",
            "expectedSha256":sha});
        let parsed=crate::engine::chat::protocol::parse_rsi_unwrapped_edit(
            &format!("```json\n{}\n```",args),&allowed).unwrap();
        assert_eq!(parsed.0,"workspace_replace_range");
        assert_eq!(serde_json::from_str::<Value>(&parsed.1).unwrap()["startLine"],10);
        assert!(crate::engine::chat::protocol::parse_rsi_unwrapped_edit(
            "I suggest changing your code next.",&allowed).is_none());
        assert!(crate::engine::chat::protocol::parse_rsi_unwrapped_edit(
            &json!({"path":"src/engine/memory.rs","startLine":1,"endLine":1,
                "newText":"bad"}).to_string(),&allowed).is_none(),
            "No read source hash, no range write");
        let unversioned=json!({"path":"src/engine/memory.rs",
            "startLine":1,"endLine":1,"newText":"fn changed() {}"}).to_string();
        let (tool,bound)=crate::engine::chat::protocol::parse_rsi_unwrapped_edit_observed(
            &unversioned,&allowed,|path|
                (path=="src/engine/memory.rs").then(||sha.clone())).unwrap();
        assert_eq!(tool,"workspace_replace_range");
        assert_eq!(serde_json::from_str::<Value>(&bound).unwrap()["expectedSha256"],sha);
        assert!(crate::engine::chat::protocol::parse_rsi_unwrapped_edit_observed(
            &unversioned,&allowed,|_|None).is_none());
        assert!(crate::engine::chat::protocol::parse_rsi_unwrapped_edit_observed(
            &unversioned,&allowed,|_|Some("bad".into())).is_none());
        assert!(crate::engine::chat::protocol::parse_rsi_unwrapped_edit(
            &json!({"path":"../escape.rs","oldText":"a","newText":"b"})
                .to_string(),&allowed).is_none());
        let block=crate::engine::chat::protocol::parse_rsi_unwrapped_edit(
            &json!({"path":"src/engine/memory.rs","oldText":"old","newText":"new"})
                .to_string(),&allowed).unwrap();
        assert_eq!(block.0,"workspace_replace");
    }

    #[test]
    fn learned_method_filters_competing_edit_tools_after_evidence() {
        assert!(rsi_method_tool_allowed("workspace_replace","workspace_replace_range",false));
        assert!(rsi_method_tool_allowed("workspace_replace_range","workspace_replace_range",true));
        assert!(!rsi_method_tool_allowed("workspace_replace","workspace_replace_range",true));
        assert!(rsi_method_tool_allowed("workspace_read_range","workspace_replace_range",true));
        assert!(rsi_method_tool_allowed("workspace_write","workspace_replace_range",true));
        assert!(rsi_method_tool_allowed("workspace_replace","workspace_replace",true));
    }

    #[test]
    fn learned_method_controls_next_edit_and_counterexample_recovery() {
        let method=json!({"preferredTool":"workspace_replace",
            "fallbackTool":"workspace_replace_range"});
        assert_eq!(rsi_method_edit_choice(&method,&[],"run"),
            Some(("workspace_replace".into(),false)));
        let unsuccessful=json!({"runId":"run","tool":"workspace_replace",
            "status":"failed","result":{"error":"oldText mismatch"}});
        assert_eq!(rsi_method_edit_choice(&method,&[unsuccessful.clone()],"run"),
            Some(("workspace_replace_range".into(),true)));
        assert_eq!(rsi_method_edit_choice(&method,&[unsuccessful],"other"),
            Some(("workspace_replace".into(),false)));
        assert!(rsi_method_edit_choice(&json!({}),&[],"run").is_none());
    }

    #[test]
    fn real_source_write_is_a_candidate_handoff_not_a_phantom_tool_call() {
        let phantom=json!({"runId":"r","status":"completed",
            "tool":"workspace_replace_range","result":{"error":"staleVersion"}});
        let failed=json!({"runId":"r","status":"failed",
            "tool":"workspace_replace","result":{"written":true}});
        assert!(!rsi_confirmed_source_write(&[phantom,failed],"r"));
        let read=json!({"runId":"r","status":"completed",
            "tool":"workspace_read_range","result":{"written":true}});
        assert!(!rsi_confirmed_source_write(&[read],"r"));
        let unverified=json!({"runId":"r","status":"completed",
            "tool":"workspace_replace_range","result":{"written":true},
            "toolReceipt":{"verified":false}});
        assert!(!rsi_confirmed_source_write(&[unverified],"r"));
        let valid=json!({"runId":"r","status":"completed",
            "tool":"workspace_replace_range","result":{"written":true},
            "toolReceipt":{"verified":true}});
        assert!(!rsi_confirmed_source_write(&[valid.clone()],"other-run"));
        assert!(rsi_confirmed_source_write(&[valid],"r"));
    }

    #[test]
    fn a_long_prompt_gets_a_bounded_unique_contract_intent() {
        let input = "가".repeat(10_000);
        let intent = bounded_contract_intent(&input);
        assert!(intent.chars().count() <= 4096);
        assert!(intent.ends_with(']'));
        assert_ne!(intent, bounded_contract_intent(&format!("{input}x")));
        assert_eq!(bounded_contract_intent("short"), "short");
    }
}
