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
    fn grounded_neutral_descendant_can_edit_despite_search_only_controller(){
        let catalog=vec![
            json!({"function":{"name":"code_search"}}),
            json!({"function":{"name":"workspace_read_range"}}),
            json!({"function":{"name":"workspace_replace_range"}}),
            json!({"function":{"name":"workspace_replace"}}),
        ];
        let mut contrast=catalog[..2].to_vec();
        assert!(rsi_offer_grounded_edit_fallback(
            &mut contrast,&catalog,true,true,false,false));
        assert!(rsi_tool_advertised("workspace_replace_range",&contrast));
        assert!(rsi_tool_advertised("workspace_replace",&contrast));
        assert_eq!(contrast.len(),4);
        assert!(!rsi_offer_grounded_edit_fallback(
            &mut contrast,&catalog,true,true,false,false));
        let mut reground=catalog[..2].to_vec();
        assert!(!rsi_offer_grounded_edit_fallback(
            &mut reground,&catalog,true,true,true,false));
        assert!(!rsi_tool_advertised("workspace_replace_range",&reground));
        assert!(!rsi_offer_grounded_edit_fallback(
            &mut reground,&catalog,true,false,false,false));
    }

    #[test]
    fn learned_rsi_method_changes_the_real_edit_checkpoint() {
        let fast=json!({"method":"focused-range","checkpointAfter":2,
            "preferredTool":"workspace_replace_range"});
        let slow=json!({"method":"exact-context","checkpointAfter":4,
            "preferredTool":"workspace_replace"});
        assert_eq!(rsi_checkpoint_stage_with_method(2,1,false,true,true,&fast),Some(true));
        assert_eq!(rsi_checkpoint_stage_with_method(2,1,false,true,true,&slow),None);
        assert_eq!(rsi_checkpoint_stage_with_method(4,1,false,true,true,&slow),Some(true));
        assert_eq!(rsi_checkpoint_stage_with_method(5,0,false,true,false,&fast),None);
        assert_eq!(rsi_checkpoint_stage_with_method(6,0,false,true,false,&fast),Some(false));
        assert_eq!(rsi_checkpoint_stage_with_method(5,1,true,true,true,&fast),None);
    }

    #[test]
    fn rsi_checkpoint_allows_an_edit_or_a_no_change_finish() {
        assert!(!rsi_should_checkpoint(5, false));
        assert!(rsi_should_checkpoint(6, false));
        assert!(!rsi_should_checkpoint(6, true));
        // Explicit source goals must transition to writing after genuine
        // source evidence, without mistaking mere search for grounding.
        assert_eq!(rsi_checkpoint_stage(2, 1, false, true, true), None);
        assert_eq!(rsi_checkpoint_stage(3, 1, false, true, true), Some(true));
        assert_eq!(rsi_checkpoint_stage(3, 0, false, true, false), None);
        assert_eq!(rsi_checkpoint_stage(6, 0, false, true, false), Some(false));
        assert_eq!(rsi_checkpoint_stage(7, 1, false, true, false), Some(true));
        assert_eq!(rsi_checkpoint_stage(8, 2, true, true, true), None);
        assert_eq!(rsi_checkpoint_stage(6, 2, false, false, true), Some(false));
        // If a targeted model stops after one successful source read, the
        // runtime requests a single edit-only continuation instead of
        // silently ending the generation as research-only.
        for name in ["workspace_replace", "workspace_replace_range", "workspace_write", "workspace_read_range", "rsi_method_propose", "algorithm_propose"] {
            assert!(rsi_checkpoint_tool(name, false), "{name} must stay available at the general checkpoint");
        }
        for name in ["workspace_replace", "workspace_replace_range", "workspace_write"] {
            assert!(rsi_checkpoint_tool(name, true), "{name} must remain in the edit stage");
        }
        for name in ["code_search", "shell_exec", "file_write", "web_search", "workspace_read_range", "algorithm_propose"] {
            assert!(!rsi_checkpoint_tool(name, true), "{name} must not reopen in the edit stage");
        }
        assert!(tool_discovery_applies(false, false));
        assert!(!tool_discovery_applies(true, false));
        assert!(!tool_discovery_applies(false, true), "RSI must keep its full allowed toolset before the checkpoint");
        assert!(rsi_textual_edit_fallback(true, true, true));
        assert!(!rsi_textual_edit_fallback(true, false, true));
        assert!(!rsi_textual_edit_fallback(true, true, false));
        assert!(!rsi_textual_edit_fallback(false, true, true));
        let known = std::collections::HashSet::from(["workspace_replace".to_string()]);
        let (calls, cleaned) = super::super::protocol::parse_text_tool_calls_known(
            r#"<tool_call>{"name":"workspace_replace","arguments":{"path":"src/engine/example.rs","oldText":"old","newText":"new"}}</tool_call>"#, &known);
        assert_eq!(calls.len(), 1, "native textual tools must be converted to executable calls");
        assert_eq!(calls[0].0, "workspace_replace");
        assert!(cleaned.is_empty());
    }

    #[test]
    fn rsi_rejects_unadvertised_native_tools_after_the_edit_checkpoint() {
        let menu = vec![json!({"function":{"name":"workspace_replace_range"}})];
        assert!(rsi_tool_advertised("workspace_replace_range", &menu));
        assert!(!rsi_tool_advertised("workspace_read_range", &menu));
        assert!(!rsi_tool_advertised("workspace_read_range<|channel|>commentary", &menu));
        assert_eq!(rsi_canonical_tool_name(
            "workspace_replace_range<|channel|>commentary",&menu),
            Some("workspace_replace_range"));
        assert_eq!(rsi_canonical_tool_name(
            "workspace_replace_range<|channel|>analysis",&menu),
            Some("workspace_replace_range"));
        assert_eq!(rsi_canonical_tool_name("workspace_replace_range",&menu),
            Some("workspace_replace_range"));
        assert_eq!(rsi_canonical_tool_name(
            "workspace_read_range<|channel|>commentary",&menu),None);
        assert_eq!(rsi_canonical_tool_name(
            "workspace_replace_range<|channel|>final",&menu),None);
        assert_eq!(rsi_canonical_tool_name("shell_exec<|channel|>commentary",&menu),None);
    }

    #[test]
    fn rsi_checkpoint_never_confuses_unchanged_edit_calls_with_a_real_patch() {
        let base=json!({"status":"completed","tool":"workspace_replace_range",
            "result":{"written":true},"toolReceipt":{"verified":true}});
        assert!(rsi_verified_write_operation(&base));
        let mut unchanged=base.clone();
        unchanged["result"]["written"]=json!(false);
        assert!(!rsi_verified_write_operation(&unchanged));
        let mut unknown=base.clone();
        unknown["toolReceipt"]["verified"]=json!(false);
        assert!(!rsi_verified_write_operation(&unknown));
        let mut read=base;
        read["tool"]=json!("workspace_read_range");
        assert!(!rsi_verified_write_operation(&read));
    }

    #[test]
    fn focused_rsi_routes_a_verified_edit_to_compilation_without_another_model_call() {
        let focused = json!({"rsiRun":true,"rsiExplicitFocus":true,
            "rsiCampaignId":"c","rsiFocusFiles":["src/engine/decompose.rs"]});
        let args = json!({"path":"src/engine/decompose.rs"});
        let written = json!({"written":true,"postSha256":"a".repeat(64)});
        assert!(rsi_patch_ready_for_verification(&focused, "workspace_replace_range", &args, &written, true));
        assert!(!rsi_patch_ready_for_verification(&focused, "workspace_replace_range", &args, &written, false));
        assert!(!rsi_patch_ready_for_verification(&focused, "workspace_read_range", &args, &written, true));
        assert!(!rsi_patch_ready_for_verification(&json!({"rsiRun":true}), "workspace_replace_range", &args, &written, true));
        assert!(!rsi_patch_ready_for_verification(&focused, "workspace_replace_range", &args, &json!({"written":false}), true));
        assert!(!rsi_patch_ready_for_verification(&focused, "workspace_replace_range",
            &json!({"path":"src/engine/other.rs"}), &written, true));
        let autonomous=json!({"rsiRun":true,"rsiAutoFocus":true,"rsiCampaignId":"c",
            "rsiFocusFiles":["src/engine/decompose.rs"]});
        assert!(rsi_patch_ready_for_verification(&autonomous,"workspace_replace_range",
            &args,&written,true));
        let multi = json!({"rsiRun":true,"rsiExplicitFocus":true,"rsiCampaignId":"c",
            "rsiFocusFiles":["src/engine/decompose.rs","src/engine/other.rs"]});
        assert!(rsi_patch_ready_for_verification(&multi, "workspace_replace_range", &args, &written, true));
        assert!(!rsi_patch_ready_for_verification(&multi, "workspace_replace_range",
            &json!({"path":"src/unselected.rs"}), &written, true));
    }

    #[test]
    fn rsi_range_edit_can_reuse_only_a_matching_successful_source_observation() {
        let hash = "a".repeat(64);
        let reads = vec![json!({"status":"completed","opSeq":3,
            "tool":"workspace_read_range",
            "arguments":{"path":"src/engine/decompose.rs"},
            "result":{"startLine":10,"endLine":30,"sourceSha256":hash}})];
        let edit = json!({"path":"src/engine/decompose.rs",
            "startLine":"12","endLine":"15","newText":"fixed\\n"});
        let corrected = rsi_ground_range_edit_args(edit.clone(), &reads);
        assert_eq!(corrected["expectedSha256"], hash);
        assert_eq!(corrected["startLine"], 12);
        assert_eq!(corrected["endLine"], 15);
        let wrong_path = json!({"path":"src/engine/other.rs",
            "startLine":12,"endLine":15,"newText":"fixed"});
        assert!(rsi_ground_range_edit_args(wrong_path, &reads)["expectedSha256"].is_null());
        let outside = json!({"path":"src/engine/decompose.rs",
            "startLine":12,"endLine":34,"newText":"fixed"});
        assert!(rsi_ground_range_edit_args(outside, &reads)["expectedSha256"].is_null());
        let existing = json!({"path":"src/engine/decompose.rs","startLine":12,"endLine":15,
            "newText":"fixed","expectedSha256":"f".repeat(64)});
        assert_eq!(rsi_ground_range_edit_args(existing, &reads)["expectedSha256"], "f".repeat(64));
        let malformed=json!({"path":"src/engine/decompose.rs","startLine":12,"endLine":15,
            "newText":"fixed","expectedSha256":"the hash from read"});
        assert_eq!(rsi_ground_range_edit_args(malformed,&reads)["expectedSha256"],hash,
            "only the exact witnessed source can repair an unusable hash");
        let stale_valid=json!({"path":"src/engine/decompose.rs","startLine":12,"endLine":15,
            "newText":"fixed","expectedSha256":"b".repeat(64)});
        assert_eq!(rsi_ground_range_edit_args(stale_valid,&reads)["expectedSha256"],"b".repeat(64),
            "never bypass a supplied syntactically valid source version");
        let failed = vec![json!({"status":"failed","opSeq":4,"tool":"workspace_read_range",
            "arguments":{"path":"src/engine/decompose.rs"},
            "result":{"startLine":10,"endLine":30,"sourceSha256":hash}})];
        assert!(rsi_ground_range_edit_args(edit, &failed)["expectedSha256"].is_null());
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
    fn a_long_prompt_gets_a_bounded_unique_contract_intent() {
        let input = "가".repeat(10_000);
        let intent = bounded_contract_intent(&input);
        assert!(intent.chars().count() <= 4096);
        assert!(intent.ends_with(']'));
        assert_ne!(intent, bounded_contract_intent(&format!("{input}x")));
        assert_eq!(bounded_contract_intent("short"), "short");
    }
}
