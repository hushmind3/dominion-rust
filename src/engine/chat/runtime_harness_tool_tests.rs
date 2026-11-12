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
