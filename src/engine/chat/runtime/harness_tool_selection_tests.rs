//! Unit tests for native chat runtime: harness_tool_selection_tests.
use super::*;
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
    fn focused_editor_only_advertises_available_read_tool() {
        let direct = rsi_edit_checkpoint_guidance(false);
        assert!(direct.contains("workspace_replace"));
        assert!(direct.contains("sourceSha256"));
        assert!(direct.contains("제공되지 않습니다"));
        assert!(!direct.contains("workspace_read_range로 현재"));
        let repair = rsi_edit_checkpoint_guidance(true);
        assert!(repair.contains("workspace_read_range로 현재"));
        assert!(repair.contains("expectedSha256"));
        assert!(!repair.contains("제공되지 않습니다"));
    }

    #[test]
    fn coupled_system_requires_all_verified_source_effects_before_handoff() {
        let session=json!({"rsiSystemEvolution":true,"rsiSystemDesign":{
            "component":"system","targetFiles":[
                "src/engine/memory.rs","src/engine/learning.rs"]
        }});
        let recorded=|path:&str|json!({"tool":"workspace_replace",
            "arguments":{"path":path},"status":"completed",
            "result":{"written":true},"toolReceipt":{"verified":true}});
        let first=recorded("src/engine/memory.rs");
        assert!(!rsi_complete_system_bundle(&session,&[first.clone()]),
            "a one-file patch must never stand in for a coupled redesign");
        let mut unverified=recorded("src/engine/learning.rs");
        unverified["toolReceipt"]["verified"]=json!(false);
        assert!(!rsi_complete_system_bundle(
            &session,&[first.clone(),unverified]));
        assert!(rsi_complete_system_bundle(&session,&[
            first,recorded("src/engine/learning.rs")]));
        assert!(rsi_complete_system_bundle(&json!({"rsiRun":true}),&[]),
            "single-file legacy mode retains its original handoff");
    }

    #[test]
    fn focused_rsi_verification_handoff_requires_real_verified_edit() {
        let session=json!({"rsiRun":true,"rsiExplicitFocus":true,
            "rsiCampaignId":"campaign","rsiFocusFiles":["src/engine/memory.rs"]});
        let args=json!({"path":"src/engine/memory.rs"});
        let operation=json!({"status":"completed","tool":"workspace_replace",
            "result":{"written":true},"toolReceipt":{"verified":true}});
        assert!(rsi_patch_ready_for_verification(&session,"workspace_replace",&args,&operation));
        let mut unverified=operation.clone();
        unverified["toolReceipt"]["verified"]=json!(false);
        assert!(!rsi_patch_ready_for_verification(&session,"workspace_replace",&args,&unverified));
        let mut no_edit=operation.clone();
        no_edit["result"]["written"]=json!(false);
        assert!(!rsi_patch_ready_for_verification(&session,"workspace_replace",&args,&no_edit));
        assert!(!rsi_patch_ready_for_verification(&session,"workspace_replace",
            &json!({"path":"src/engine/other.rs"}),&operation));
        assert!(!rsi_patch_ready_for_verification(&session,"workspace_read_range",&args,&operation));
        let indexed=json!({"rsiRun":true,"rsiCampaignId":"campaign",
            "rsiExplicitFocus":false,"rsiCodeIndexId":"indexed-revision",
            "rsiFocusContextReady":true,
            "rsiFocusFiles":["src/engine/context.rs","src/engine/memory.rs"]});
        assert!(rsi_patch_ready_for_verification(
            &indexed,"workspace_replace",&args,&operation),
            "autonomous indexed source edits must not be lost to another model round");
        let mut not_indexed=indexed.clone();
        not_indexed["rsiCodeIndexId"]=Value::Null;
        assert!(!rsi_patch_ready_for_verification(
            &not_indexed,"workspace_replace",&args,&operation));
        not_indexed=indexed.clone();
        not_indexed["rsiFocusContextReady"]=json!(false);
        assert!(!rsi_patch_ready_for_verification(
            &not_indexed,"workspace_replace",&args,&operation));
        not_indexed["rsiRecoveredPatch"]=json!({
            "status":"restored_unverified",
            "paths":["src/engine/memory.rs"]
        });
        assert!(rsi_patch_ready_for_verification(
            &not_indexed,"workspace_replace",&args,&operation),
            "a newly indexed, actually corrected descendant may advance");
        not_indexed=indexed.clone();
        not_indexed["rsiFocusFiles"]=json!(["src/engine/context.rs"]);
        assert!(!rsi_patch_ready_for_verification(
            &not_indexed,"workspace_replace",&args,&operation));
    }

    #[test]
    fn focused_rsi_structural_errors_use_one_bounded_repair_read_not_a_second_model() {
        let focused=json!({"rsiRun":true,"rsiExplicitFocus":true,
            "rsiFocusFiles":["src/engine/memory.rs"]});
        let args=json!({"path":"src/engine/memory.rs",
            "startLine":120,"endLine":1,"newText":"fn fixed() {}"});
        let invalid=json!({"error":"endLine은 startLine 이상이어야 합니다."});
        assert_eq!(rsi_direct_source_repair(
            &focused,"workspace_replace",&args,&invalid),Some("invalid_source_span"));
        assert_eq!(rsi_direct_source_repair(
            &focused,"workspace_replace",&args,
            &json!({"error":"changed source","staleVersion":true})),
            Some("source_revision_mismatch"));
        assert_eq!(rsi_direct_source_repair(
            &focused,"workspace_write",&args,
            &json!({"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."})),
            Some("existing_file_write"));
        let mut not_focused=focused.clone();
        not_focused["rsiExplicitFocus"]=json!(false);
        assert_eq!(rsi_direct_source_repair(
            &not_focused,"workspace_replace",&args,&invalid),None);
        let mut other_path=args.clone();
        other_path["path"]=json!("src/engine/other.rs");
        assert_eq!(rsi_direct_source_repair(
            &focused,"workspace_replace",&other_path,&invalid),None);
        assert_eq!(rsi_direct_source_repair(
            &focused,"workspace_replace",&args,
            &json!({"written":true})),None);
        assert_eq!(rsi_direct_source_repair(
            &focused,"code_search",&args,&invalid),None);
    }

    #[test]
    fn adopted_inherited_method_write_hands_off_without_model_or_fake_gain() {
        let session=json!({"rsiRun":true,"rsiCampaignId":"camp",
            "rsiExplicitFocus":true,"rsiFocusFiles":["src/engine/memory.rs"]});
        let method=json!({"id":"learned-1","revision":"rev-1",
            "assistanceMode":"harness_assisted",
            "rsiMethodExperienceId":"experience-1",
            "verification":{"nodes":[{"id":"edit","tool":"workspace_replace",
                "arguments":{"path":"src/engine/memory.rs"}}]}});
        let successful=json!({"id":"op-1","origin":"procedure",
            "procedureStrategyId":"learned-1","procedureRevision":"rev-1",
            "procedureNodeId":"edit",
            "tool":"workspace_replace","status":"completed",
            "arguments":{"path":"src/engine/memory.rs"},
            "result":{"written":true},"toolReceipt":{"verified":true}});
        let proof=rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[successful.clone()]).unwrap();
        assert_eq!(proof["verifiedWrite"],true);
        assert!(proof["independentQualityGain"].is_null());
        assert_eq!(proof["operationId"],"op-1");
        // Autonomous generations need no model-selected file to hand an
        // already selected and executed source procedure to the evaluator.
        let autonomous=json!({"rsiRun":true,"rsiCampaignId":"camp"});
        assert!(rsi_inherited_patch_for_evaluation(
            &autonomous,&[method.clone()],&[successful.clone()]).is_some());
        let mut wrong=successful.clone();
        wrong["toolReceipt"]["verified"]=json!(false);
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        wrong=successful.clone();
        wrong["procedureNodeId"]=json!("other-node");
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        wrong=successful.clone();
        wrong["procedureRevision"]=json!("different");
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        wrong=successful.clone();
        wrong["origin"]=json!("model");
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        wrong=successful.clone();
        wrong["arguments"]["path"]=json!("src/engine/context.rs");
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        wrong=successful.clone();
        wrong["result"]["written"]=json!(false);
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[method.clone()],&[wrong]).is_none());
        let mut unlearned=method.clone();
        unlearned["rsiMethodExperienceId"]=Value::Null;
        assert!(rsi_inherited_patch_for_evaluation(
            &session,&[unlearned],&[successful]).is_none());
    }

    #[test]
    fn rsi_checkpoint_allows_an_edit_or_a_no_change_finish() {
        assert!(!rsi_should_checkpoint(5, false));
        assert!(rsi_should_checkpoint(6, false));
        assert!(!rsi_should_checkpoint(6, true));
        for name in ["workspace_replace", "workspace_write", "workspace_read_range", "rsi_method_propose", "algorithm_propose"] {
            assert!(rsi_checkpoint_tool(name, false), "{name} must stay available at the checkpoint");
        }
        for name in ["code_search", "shell_exec", "file_write", "web_search"] {
            assert!(!rsi_checkpoint_tool(name, false), "{name} must not stay available");
        }
        assert_eq!(rsi_checkpoint_stage(2, 1, false), None);
        assert_eq!(rsi_checkpoint_stage(3, 1, false), Some(true));
        assert_eq!(rsi_checkpoint_stage(6, 0, false), Some(false));
        assert_eq!(rsi_checkpoint_stage(7, 1, false), Some(true));
        assert_eq!(rsi_checkpoint_stage(8, 2, true), None);
        assert!(rsi_should_retry_read_only(true, true, false, false));
        assert!(!rsi_should_retry_read_only(false, true, false, false));
        assert!(!rsi_should_retry_read_only(true, false, false, false));
        assert!(!rsi_should_retry_read_only(true, true, true, false));
        assert!(!rsi_should_retry_read_only(true, true, false, true));
        assert!(rsi_checkpoint_tool("workspace_replace", true));
        assert!(!rsi_checkpoint_tool("workspace_read_range", true));
        assert!(!rsi_checkpoint_tool("code_search", true));
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
    fn a_long_prompt_gets_a_bounded_unique_contract_intent() {
        let input = "가".repeat(10_000);
        let intent = bounded_contract_intent(&input);
        assert!(intent.chars().count() <= 4096);
        assert!(intent.ends_with(']'));
        assert_ne!(intent, bounded_contract_intent(&format!("{input}x")));
        assert_eq!(bounded_contract_intent("short"), "short");
    }

