#[cfg(test)]
mod native_plan_transfer_regression {
    use super::*;
    #[test]
    fn system_descendant_filters_non_source_and_stale_indexed_paths_before_planning() {
        let root=std::env::temp_dir().join(format!(
            "sub1-verified-descendant-focus-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
        std::fs::write(root.join("src/engine/memory.rs"),
            "pub fn recall() -> u32 { 7 }\n").unwrap();
        std::fs::write(root.join(".github/workflows/memory.yml"),
            "name: pretend-target\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let index_id=index["id"].as_str().unwrap();
        let valid="src/engine/memory.rs";
        let rows=vec![
            json!({"path":".github/workflows/memory.yml"}),
            json!({"path":"src/engine/../secrets.rs"}),
            json!({"path":"src/engine/missing.rs"}),
            json!({"path":valid,"score":1.0})
        ];
        let actual=verified_descendant_focus(&state,index_id,&root,rows.clone());
        assert_eq!(actual.len(),1);
        assert_eq!(actual[0]["path"],valid);
        std::fs::write(root.join(valid),"pub fn recall() -> u32 { 8 }\n").unwrap();
        assert!(verified_descendant_focus(&state,index_id,&root,rows).is_empty(),
            "stale indexed content must not be fed to model-source editor");
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn adopted_native_descendant_goal_reaches_fresh_executable_without_losing_owner_request() {
        let original=json!({"rsiObjective":"memory","rsiAutonomous":false,
            "rsiNativeInduction":true,"rsiExpandAfterGain":true,
            "selectionPolicy":"automatic",
            "rsiFocusFiles":["src/engine/room_state.rs"]});
        let campaign=json!({"planObjectiveOverride":"improver"});
        let planned=native_plan_source_input(&original,&campaign);
        assert_eq!(selection_objective(&planned),"improver");
        assert_eq!(planned["rsiFocusFiles"],json!([]));
        assert_eq!(original["rsiObjective"],"memory");
        let denied=native_plan_source_input(
            &json!({"rsiObjective":"memory","rsiNativeInduction":false,
                "rsiExpandAfterGain":true,"selectionPolicy":"automatic"}),
            &campaign);
        assert_eq!(denied["rsiObjective"],"memory");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-evolution-test-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root.join("work"),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, root)
    }
    fn parent(state: &AppState) -> JobContext {
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"running","input":{"rsiNativeApproved":true}})).unwrap();
        JobContext::new(state.clone(), "campaign".into())
    }
    #[test]
    fn research_is_used_before_deferring_source_context_under_budget() {
        let base="OWNER GOAL: repair memory recall";
        let experience="\nREUSABLE EXPERIENCE";
        let research="\nVERIFIED RESEARCH LOCATION";
        let source="\n".to_string()+&"SOURCE EXCERPT ".repeat(1200);
        let enough=crate::engine::chat::estimate_text_tokens(
            &format!("{base}\n\n{experience}{research}"))+1280;
        let (prompt,receipt)=fit_parent_generation_context(
            base,experience,research,&source,Some(enough)).unwrap();
        assert_eq!(receipt["mode"],"source_deferred");
        assert!(prompt.contains("OWNER GOAL") && prompt.contains("REUSABLE EXPERIENCE")
            && prompt.contains("VERIFIED RESEARCH LOCATION"));
        assert!(!prompt.contains("SOURCE EXCERPT"));
        assert!(fit_parent_generation_context(&"x".repeat(30000),"","","",Some(100)).is_none());
    }

    #[test]
    fn descendant_worker_preserves_implementation_directive_and_valid_feedback() {
        let directive=json!({"mode":"implement_research","sourceGenerationId":"r1",
            "evidenceHashes":["verified-hash"]});
        let history=vec![json!({"domain":"memory","accepted":false,"tokens":2200,
            "evaluation":{"verdict":"inconclusive","metrics":{
                "measurements":[{"holdout":true,"details":"long".repeat(10000)}]}}})];
        let (prompt,receipt)=fit_worker_generation_context(
            "OWNER REQUEST","memory","memory source",&history,&directive,Some(2200)).unwrap();
        assert!(prompt.contains("OWNER REQUEST") && prompt.contains("implement_research"));
        assert!(prompt.contains("verified-hash"));
        assert_ne!(receipt["mode"],"measured");
        assert!(prompt_fits_allowance(&prompt,Some(2200),256));
        assert!(fit_worker_generation_context(&"H".repeat(30000),
            "memory","memory",&history,&directive,Some(100)).is_err());
    }

    #[test]
    fn sub1_fair_generation_quota_preserves_real_descendant_token_budget(){
        let mut campaign=json!({"limits":{"tokenBudget":90000,"maxAttempts":3},
            "tokens":0,"attempts":0});
        assert_eq!(generation_token_quota(&campaign),Some(30000));
        campaign["tokens"]=json!(27800);
        campaign["attempts"]=json!(1);
        assert_eq!(generation_token_quota(&campaign),Some(31100));
        campaign["tokens"]=json!(58900);
        campaign["attempts"]=json!(2);
        assert_eq!(generation_token_quota(&campaign),Some(31100));
        let local=json!({"result":{"rsi":{"modelError":
            "rsi_generation_quota_reached: reserve tokens for descendant"}}});
        assert!(generation_model_error(&local).is_none());
        let network=json!({"result":{"rsi":{"modelError":"HTTP 502 timeout"}}});
        assert_eq!(generation_model_error(&network),Some("HTTP 502 timeout"));
    }

    #[test]
    fn runtime_budget_stops_are_not_no_progress_or_another_generation(){
        let worker=json!({"status":"completed","result":{"status":"needs_review","reason":"model_error",
            "rsi":{"modelError":"rsi_token_budget: remaining context cannot fit"}}});
        assert_eq!(generation_stop_reason(&worker,None),Some("token_budget"));
        assert_eq!(generation_stop_reason(&json!({}),Some("time_budget")),Some("time_budget"));
        assert_eq!(generation_stop_reason(&json!({"result":{"rsi":{"modelError":"authentication failed"}}}),None),None);
        assert_eq!(generation_stop_reason(&json!({"result":{"rsi":{"diffBytes":0}}}),None),None);
        let record=json!({"limits":{"tokenBudget":30000},"tokens":27446});
        assert!(!generation_fits_remaining_budget(&record,&json!({"prompt":"한".repeat(3000)})),"do not build or launch a worker when even its existing prompt cannot fit");
        assert!(generation_fits_remaining_budget(&json!({"limits":{}}),&json!({"prompt":"한".repeat(3000)})));
        let budget=json!({"limits":{"tokenBudget":30000},"tokens":27000});
        let base=json!({"prompt":"small source"});
        assert!(generation_fits_remaining_budget(&budget,&base));
        let expanded=json!({"prompt":format!("small source{}", "research context ".repeat(3000))});
        assert!(!generation_fits_remaining_budget(&budget,&expanded),
            "check the complete composed worker prompt before creating the next job");
    }

    #[test]
    fn method_only_research_survives_worker_presave_and_is_novel_only_once(){
        let (state,root)=state();let input=json!({"rsiSourceDir":root,"rsiObjective":"memory lookup"});
        state.store.put("execution_operation",&json!({"id":"proposal-op","sessionId":"s","tool":"rsi_method_propose","status":"completed",
            "result":{"method":{"verification":{"type":"procedure","nodes":[{"id":"read","tool":"code_search","arguments":{"query":"memory"}}],"after":[{"kind":"tool_succeeded","tool":"code_search"}]}}}}));
        let artifacts=research_artifacts(&state,"s");assert_eq!(artifacts.len(),1);
        state.store.put("experience",&json!({"id":"current","partitionKey":research_scope(&input).partition_key,
            "status":"active","generationJobId":"current-worker","researchArtifacts":artifacts}));
        let hashes=artifacts.iter().map(|a|a["hash"].clone()).collect::<Vec<_>>();
        assert_eq!(new_research_findings(&state,&input,"current-worker",&hashes,&[]),1,"worker pre-save must not erase its own progress");
        assert!(!should_stop_no_progress(1,true,true));
        assert_eq!(new_research_findings(&state,&input,"next-worker",&hashes,&[]),0,"re-proposing the same program is not new research");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn capability_usage_is_read_from_real_operation_records() {
        let (state, root) = state();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op1","sessionId":"s1","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op2","sessionId":"s1","tool":"workspace_read_range","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op3","sessionId":"s2","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op4","sessionId":"s1","tool":"workspace_read","status":"failed"}),
            )
            .unwrap();
        let usage = session_tool_usage(&state, "s1");
        assert_eq!(usage.len(), 2, "{usage:?}");
        assert!(usage.iter().any(|row| row["tool"] == "code_search"));
        assert!(usage
            .iter()
            .any(|row| row["tool"] == "workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn research_digest_lists_ranges_already_read() {
        let (state, root) = state();
        state
            .store
            .put("job", &json!({"id":"digest-job","sessionId":"digest-session"}));
        state
            .store
            .put("execution", &json!({"id":"digest-run","sessionId":"digest-session"}));
        state.store.put(
            "execution_operation",
            &json!({"id":"digest-op","runId":"digest-run","tool":"workspace_read_range","status":"completed",
                "arguments":{"path":"src/engine/memory.rs","startLine":10,"endLine":20}}),
        );
        let record = json!({"history":[{"jobId":"digest-job"}]});
        let text = research_digest(&state, &json!({}), &record);
        assert!(text.contains("src/engine/memory.rs 10-20"));
        assert!(text.contains("다시 읽지 마세요"));
        assert_eq!(research_digest(&state, &json!({}), &json!({"history":[]})), "");
        // A continuation must also see the previous campaign's reads.
        state.store.put("rsi_campaign", &json!({"id":"digest-prior","history":[{"jobId":"digest-job"}]}));
        let continued = research_digest(&state, &json!({"rsiContinueFrom":"digest-prior"}), &json!({"history":[]}));
        assert!(continued.contains("src/engine/memory.rs 10-20"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn explicit_owner_patch_starts_with_execution_without_wasting_a_discovery_generation() {
        let patch=json!({"rsiAutonomous":false,
            "rsiObjective":"Fix the failing Korean text splitting algorithm",
            "rsiFocusFiles":["src/engine/decompose.rs"]});
        let directive=generation_improvement_directive(&patch,&[]);
        assert_eq!(directive["mode"],"implement_research");
        assert_eq!(directive["reason"],"explicit_owner_patch_request_with_source");
        let auto=json!({"rsiAutonomous":true,
            "rsiObjective":"Improve DOMINION autonomously",
            "rsiFocusFiles":["src/engine/decompose.rs"]});
        assert_eq!(generation_improvement_directive(&auto,&[])["mode"],"discover");
        let inspect=json!({"rsiAutonomous":false,
            "rsiObjective":"Inspect current memory retrieval performance",
            "rsiFocusFiles":["src/engine/memory.rs"]});
        assert_eq!(generation_improvement_directive(&inspect,&[])["mode"],"discover");
        let failure=json!({"id":"failed","improvementDirective":{"mode":"implement_research"},
            "improvementActionObservation":{"status":"not_followed"},
            "newFindings":0,"candidateId":null,"accepted":false});
        assert_eq!(generation_improvement_directive(&patch,&[failure])["mode"],"change_approach",
            "past execution failure must override first-generation shortcut");
    }

    #[test]
    fn descendant_replan_executes_new_focus_with_inherited_research() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/memory_recall.rs"),
            "fn memory_recall() { /* stored memory retrieval */ }\n").unwrap();
        std::fs::write(root.join("src/engine/room_ui.rs"),
            "fn room_ui() { /* room conversation UI */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let strategy=json!({"implementation":"failure-clusters"});
        let mut job=json!({"input":{
            "rsiAutonomous":true,
            "rsiObjective":"Improve DOMINION autonomously",
            "rsiTargetDomain":"memory",
            "rsiTargetPlan":generation_target_plan(
                "memory","algorithm_active",&strategy,"revision-1"),
            "rsiSourceDir":root,
            "rsiIndexId":index["id"],
            "rsiAutoFocus":true,
            "rsiFocusFiles":["src/engine/memory_recall.rs"],
            "rsiPromptBase":"OWNER REQUEST: improve DOMINION",
            "rsiPromptInherited":"\nINHERITED SUCCESSFUL EXPERIENCE\n",
            "rsiPromptResearch":"\nRESEARCH EVIDENCE TO IMPLEMENT\n",
            "rsiRemainingCampaignTokens":30000,
            "prompt":"old memory implementation instructions"
        }});
        assert!(reconcile_descendant_target(&state,&mut job,&[],
            "ui","algorithm_active",&strategy,"revision-1").unwrap());
        assert_eq!(job["input"]["rsiTargetPlan"]["domain"],"ui");
        assert_eq!(job["input"]["rsiTargetDomain"],"ui");
        assert_eq!(job["input"]["rsiFocusFiles"][0],"src/engine/room_ui.rs");
        let prompt=job["input"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("OWNER REQUEST")
            && prompt.contains("INHERITED SUCCESSFUL EXPERIENCE")
            && prompt.contains("RESEARCH EVIDENCE TO IMPLEMENT")
            && prompt.contains("room_ui.rs"));
        assert_eq!(job["rsiTargetReplan"]["coordinatorPlan"]["domain"],"memory");
        assert_eq!(job["rsiTargetReplan"]["descendantPlan"]["domain"],"ui");
        let unchanged=job["input"]["prompt"].clone();
        assert!(!reconcile_descendant_target(&state,&mut job,&[],
            "ui","algorithm_active",&strategy,"revision-1").unwrap());
        assert_eq!(job["input"]["prompt"],unchanged,
            "unchanged descendants should never duplicate inherited context");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn owner_goal_constrains_domain_even_when_another_domain_is_new_or_profitable() {
        let algorithm=json!({"implementation":"failure-clusters"});
        let history=vec![
            json!({"domain":"memory","accepted":false,"candidateId":null,"tokens":80_000,"newFindings":0}),
            json!({"domain":"memory","accepted":false,"candidateId":null,"tokens":70_000,"newFindings":0}),
            json!({"domain":"learning","accepted":true,"candidateId":"c","tokens":3_000,"newFindings":1})
        ];
        assert_eq!(select_domain(&algorithm,&history,"기억 검색 최적화"),"memory");
        assert_eq!(select_domain(&algorithm,&history,"대화 문맥 검색"),"context");
        assert_eq!(select_domain(&algorithm,&history,"브라우저 검색 도구"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"텔레그램 메신저 연결"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"로컬 모델 다운로드"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"학습 기록 검색"),"learning");
        // With no owner scope, productive low-cost work should beat repeated
        // barren token spend when comparing the two domains directly.
        let productive=search_features(&history,"learning","");
        let expensive=search_features(&history,"memory","");
        assert!(score_domain(&algorithm,&productive)>score_domain(&algorithm,&expensive));
        assert!(expensive["barrenRate"]>0.9);
    }

    #[test]
    fn generated_target_expression_actually_changes_file_selection() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/memory_first.rs"),
            "fn memory_dispatch() {}\n").unwrap();
        std::fs::write(root.join("src/engine/memory_second.rs"),
            format!("fn memory_dispatch() {{}}\n{}", "// non-goal padding\n".repeat(45))).unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiObjective":"memory dispatch",
            "rsiTargetDomain":"memory"});
        let baseline=choose_focus_files(&state,&input,&[],None);
        assert_eq!(baseline[0]["path"],"src/engine/memory_first.rs");
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"runtime-target-formula","implementation":"expression",
            "parameters":{"expression":"objectiveMatch","targetExpression":"sourceCost"}
        })).unwrap();
        assert_eq!(candidate["status"],"available");
        let trial=choose_focus_files(&state,&input,&[],Some(&candidate));
        assert_eq!(trial[0]["path"],"src/engine/memory_second.rs",
            "a model-authored formula must change an actual selection, not just be stored");
        assert_eq!(trial[0]["scoring"],"expression");
        // A malformed runtime candidate does not fail a generation or make
        // non-finite ordering decisions. It is surfaced as policyError.
        let mut corrupt=candidate.clone();
        corrupt["parameters"]["targetExpression"]=json!("sourceCost / (novelty - novelty)");
        let safe=choose_focus_files(&state,&input,&[],Some(&corrupt));
        assert_eq!(safe[0]["path"],"src/engine/memory_first.rs");
        assert!(safe[0]["policyError"].as_str().is_some());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_focus_uses_chosen_capability_instead_of_generic_owner_text() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/memory_recall.rs"),
            "fn memory_recall() { /* retrieve stored memory */ }\n").unwrap();
        std::fs::write(root.join("src/engine/room_ui.rs"),
            "fn room_ui() { /* UI conversation render */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let owner="기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다";
        let automatic=json!({"rsiIndexId":index["id"],"rsiObjective":owner,
            "rsiAutonomous":true,"rsiTargetDomain":"memory"});
        assert_eq!(source_focus_objective(&automatic),"memory");
        let chosen=choose_focus_files(&state,&automatic,&[],None);
        assert_eq!(chosen[0]["path"],"src/engine/memory_recall.rs",
            "the selected memory capability must drive the source index");
        let explicit=json!({"rsiIndexId":index["id"],"rsiObjective":"room UI",
            "rsiAutonomous":false,"rsiTargetDomain":"ui"});
        assert_eq!(source_focus_objective(&explicit),"room UI");
        assert_eq!(choose_focus_files(&state,&explicit,&[],None)[0]["path"],
            "src/engine/room_ui.rs");
        assert_eq!(source_focus_objective(&json!({"rsiObjective":owner,
            "rsiAutonomous":true,"rsiTargetDomain":"unknown"})),owner,
            "unknown domains must not erase the owner's objective");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn auto_targeting_uses_current_index_and_never_requires_a_fixed_filename() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::write(root.join("src/engine/custom_retrieval.rs"),
            "fn memory_search_rank() { /* BM25 memory recall */ }\n").unwrap();
        std::fs::write(root.join("web/src/RoomPanel.tsx"),
            "export function RoomPanel() { return 'room chat ui'; }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],
            "rsiObjective":"기억 검색 BM25 순위 품질","rsiTargetDomain":"memory"});
        let candidates=choose_focus_files(&state,&input,&[],None);
        assert_eq!(candidates[0]["path"],"src/engine/custom_retrieval.rs");
        assert!(candidates[0]["score"].as_f64().unwrap()>0.0);
        let ui=choose_focus_files(&state,&json!({"rsiIndexId":index["id"],
            "rsiObjective":"room panel UI","rsiTargetDomain":"ui"}),&[],None);
        assert_eq!(ui[0]["path"],"web/src/RoomPanel.tsx");
        std::fs::write(root.join("src/engine/telegram.rs"),
            "fn connect_telegram() {}\n").unwrap();
        std::fs::write(root.join("src/engine/models.rs"),
            "fn download_model() {}\n").unwrap();
        let extended=crate::engine::code_index::refresh(&state,&root).unwrap();
        let messenger=choose_focus_files(&state,&json!({"rsiIndexId":extended["id"],
            "rsiObjective":"텔레그램 연결","rsiTargetDomain":"tools"}),&[],None);
        assert_eq!(messenger[0]["path"],"src/engine/telegram.rs");
        let models=choose_focus_files(&state,&json!({"rsiIndexId":extended["id"],
            "rsiObjective":"로컬 모델 다운로드","rsiTargetDomain":"tools"}),&[],None);
        assert_eq!(models[0]["path"],"src/engine/models.rs");
        let no_match=choose_focus_files(&state,&json!({"rsiIndexId":index["id"],
            "rsiObjective":"quantumqzx uniquezzz","rsiTargetDomain":""}),&[],None);
        assert!(no_match.is_empty(),"an unknown goal must trigger discovery, not a forced source");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_interruption_is_not_mislearned_as_ignored_edit_instruction() {
        let stalled=json!({"improvementDirective":{"mode":"implement_research"},
            "error":"최근 응답이 너무 느렸던 모델이라 잠시 건너뜁니다.",
            "toolsUsed":[{"tool":"workspace_read_range","calls":1}],
            "candidateId":null,"accepted":false,"newFindings":1});
        let observation=improvement_action_observation(&stalled,0);
        assert_eq!(observation["status"],"provider_interrupted");
        let mut recorded=stalled;
        recorded["improvementActionObservation"]=observation;
        assert_eq!(improvement_directive(&[recorded])["mode"],"implement_research");
        let mut normally_finished=json!({"improvementDirective":{"mode":"implement_research"},
            "error":null,"toolsUsed":[],"candidateId":null,"accepted":false,
            "newFindings":0});
        normally_finished["improvementActionObservation"]=
            improvement_action_observation(&normally_finished,0);
        assert_eq!(normally_finished["improvementActionObservation"]["status"],"not_followed");
        assert_eq!(improvement_directive(&[normally_finished])["mode"],"change_approach");
    }

    #[test]
    fn first_generation_of_next_campaign_continues_real_stored_research() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"memory retrieval improvement",
            "rsiKnowledgeSource":root,"rsiFocusFiles":["src/engine/memory.rs"]});
        let partition=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&[
            input["rsiObjective"].as_str().unwrap()]);
        state.store.put("experience",&json!({"id":"prior-evidence",
            "status":"active","partitionKey":partition,"researchArtifacts":[
                {"tool":"code_search","hash":"h1"}]})).unwrap();
        state.store.put("rsi_generation",&json!({"id":"previous-discovery",
            "campaignId":"previous","jobId":"old-worker",
            "domain":"memory","accepted":false,"candidateId":null,
            "newFindings":1,"knowledgeId":"prior-evidence",
            "knowledgePartition":partition,"objectiveId":objective,
            "createdAt":"2026-11-11T00:00:00Z"})).unwrap();
        let inherited=improvement_history(&state,&input,&[],"new-campaign");
        assert_eq!(inherited.len(),1);
        assert_eq!(improvement_directive(&inherited)["mode"],"implement_research");
        let commit="a".repeat(40);
        state.store.put("job",&json!({"id":"new-campaign","kind":"rsi_evolve",
            "status":"running","cancelRequested":false,"input":input})).unwrap();
        let campaign=json!({"id":"new-campaign","headCommit":commit,
            "history":[],"planAttempt":1,"planIndexId":"index-revision"});
        let plan=native_generation_plan(&state,&campaign,
            &state.store.get("job","new-campaign").unwrap(),&commit).unwrap();
        assert_eq!(plan["historyCount"],0,
            "Inherited research is not falsely counted as a new campaign generation");
        assert_eq!(plan["improvementDirective"]["mode"],"implement_research");
        assert_eq!(plan["improvementDirective"]["sourceGenerationId"],"previous-discovery");
        let unrelated=json!({"rsiObjective":"unrelated browser task",
            "rsiKnowledgeSource":root});
        assert!(improvement_history(&state,&unrelated,&[],"new-campaign").is_empty(),
            "Research from another objective must not control this campaign");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cross_campaign_handoff_uses_latest_failed_implementation_not_stale_research() {
        let (state,root)=state();
        let source=root.join("repository");
        std::fs::create_dir_all(&source).unwrap();
        let mut input=json!({"rsiObjective":"memory retrieval",
            "rsiKnowledgeSource":source,"rsiBaseCommit":"revision-one"});
        let partition=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&["memory retrieval"]);
        state.store.put("experience",&json!({"id":"found-source",
            "status":"active","partitionKey":partition,
            "researchArtifacts":[{"tool":"code_search","hash":"h1"}]})).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"discovered","campaignId":"old","jobId":"research-worker",
            "generatorCommit":"revision-one","knowledgePartition":partition,
            "objectiveId":objective,"newFindings":1,"knowledgeId":"found-source",
            "accepted":false,"candidateId":null,
            "completedAt":"2026-11-11T00:01:00.100Z"
        })).unwrap();
        let initial=improvement_history(&state,&input,&[],"new");
        assert_eq!(improvement_directive(&initial)["mode"],"implement_research");
        state.store.put("rsi_generation",&json!({
            "id":"failed-implementation","campaignId":"old","jobId":"edit-worker",
            "generatorCommit":"revision-one","knowledgePartition":partition,
            "objectiveId":objective,"newFindings":0,
            "accepted":false,"candidateId":null,
            "improvementDirective":{"mode":"implement_research",
                "sourceGenerationId":"discovered"},
            "improvementActionObservation":{"status":"not_followed"},
            "completedAt":"2026-11-11T00:02:00.200Z"
        })).unwrap();
        let inherited=improvement_history(&state,&input,&[],"new");
        assert_eq!(inherited.len(),1);
        assert_eq!(inherited[0]["id"],"failed-implementation");
        assert_eq!(improvement_directive(&inherited)["mode"],"change_approach",
            "Do not spend a new campaign repeating a failed implementation");
        // The failure is scoped to the unchanged source revision only.
        input["rsiBaseCommit"]=json!("revision-two");
        assert!(improvement_history(&state,&input,&[],"new").is_empty());
        // Another objective can never borrow this episode.
        input["rsiBaseCommit"]=json!("revision-one");
        input["rsiObjective"]=json!("browser automation");
        assert!(improvement_history(&state,&input,&[],"new").is_empty());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn completion_order_handles_legacy_sqlite_utc_and_new_rfc3339() {
        let old=json!({"createdAt":"2026-11-11 11:00:00"});
        let earlier_new=json!({"completedAt":"2026-11-11T10:59:59.999Z"});
        let later_new=json!({"completedAt":"2026-11-11T11:00:00.001Z"});
        assert!(generation_completion_millis(&earlier_new)<generation_completion_millis(&old));
        assert!(generation_completion_millis(&later_new)>generation_completion_millis(&old));
        assert_eq!(generation_completion_millis(&json!({"createdAt":"bad"})),i64::MIN);
    }

    #[test]
    fn newer_candidate_prevents_old_research_from_restarting() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"memory retrieval",
            "rsiKnowledgeSource":root,"rsiBaseCommit":"revision-one"});
        let partition=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&["memory retrieval"]);
        state.store.put("experience",&json!({"id":"old-research","status":"active",
            "partitionKey":partition})).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"old","campaignId":"first","knowledgePartition":partition,
            "objectiveId":objective,"newFindings":1,"knowledgeId":"old-research",
            "accepted":false,"candidateId":null,"completedAt":"2026-11-11T00:00:00.100Z"
        })).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"candidate","campaignId":"second","knowledgePartition":partition,
            "objectiveId":objective,"newFindings":0,"accepted":false,
            "candidateId":"not-adopted","completedAt":"2026-11-11T00:00:00.200Z"
        })).unwrap();
        assert!(improvement_history(&state,&input,&[],"third").is_empty(),
            "A newer evaluated candidate supersedes an older research-only episode");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn research_generations_become_implementation_then_switch_hypothesis_on_failure() {
        let research=json!({"id":"gen-1","jobId":"j1","domain":"tools",
            "accepted":false,"candidateId":null,"newFindings":2,
            "knowledgeArtifactHashes":["evidence-1"],"focusFiles":["src/tools.rs"]});
        let directive=improvement_directive(&[research.clone()]);
        assert_eq!(directive["mode"],"implement_research");
        assert_eq!(directive["sourceGenerationId"],"gen-1");
        assert_eq!(directive["evidenceHashes"],json!(["evidence-1"]));
        assert!(improvement_work_contract(&directive).contains("구현 단계"));
        assert_eq!(research_continuation(&[research.clone()]),Some("tools".to_string()));
        let implementation_failed=json!({"id":"gen-2","jobId":"j2",
            "domain":"tools","accepted":false,"candidateId":null,"newFindings":1,
            "improvementDirective":directive});
        let revised=improvement_directive(&[research.clone(),implementation_failed.clone()]);
        assert_eq!(revised["mode"],"change_approach");
        assert_eq!(research_continuation(&[research,implementation_failed]),None,
            "A second research-only generation must not silently trigger a third");
        let regression=json!({"id":"gen-r","jobId":"jr","domain":"tools",
            "accepted":false,"candidateId":"patch-1",
            "evaluation":{"verdict":"regressed"}});
        let repair=improvement_directive(&[regression.clone()]);
        assert_eq!(repair["mode"],"repair_counterexample");
        let mut repeated=regression;
        repeated["improvementDirective"]=repair;
        assert_eq!(improvement_directive(&[repeated])["mode"],"change_approach");
        let adopted=json!({"id":"gen-good","jobId":"jg","accepted":true,
            "pairedRevalidated":true,"tokens":4000,"newFindings":1,
            "improvementDirective":{"mode":"implement_research"}});
        assert_eq!(improvement_directive(&[adopted.clone()])["mode"],
            "extend_verified_capability");
        let stats=improvement_action_results(&[adopted]);
        assert_eq!(stats[0]["verifiedAdoptions"],1);
        assert_eq!(stats[0]["adoptionsPer100kTokens"],25.0);
    }

    #[test]
    fn rejected_patch_revisits_its_changed_source_not_an_unrelated_file() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["alpha","beta","gamma"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),
                "fn search_dispatch() { /* same relevant implementation */ }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools",
            "rsiGeneratorCommit":"base-sha"});
        state.store.put("rsi_candidate",&json!({"id":"failed-patch","status":"inconclusive",
            "baseCommit":"base-sha","paths":["src/gamma.rs"]})).unwrap();
        let history=vec![json!({"id":"prior","jobId":"failed-worker",
            "domain":"tools","candidateId":"failed-patch",
            "accepted":false,"evaluation":{"verdict":"inconclusive"}})];
        assert_eq!(improvement_directive(&history)["mode"],"revise_candidate");
        let chosen=choose_focus_files(&state,&input,&history,None);
        assert_eq!(chosen[0]["path"],"src/gamma.rs",
            "The next generation must investigate its actual failed patch first");
        let mut changed=input;
        changed["rsiGeneratorCommit"]=json!("new-source-commit");
        let invalidated=choose_focus_files(&state,&changed,&history,None);
        assert_eq!(invalidated[0]["path"],"src/alpha.rs",
            "Do not attribute failure to unrelated revised source state");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_implementation_gets_one_real_corrective_generation_before_no_progress() {
        let discovered=json!({"id":"g1","candidateId":null,
            "newFindings":2,"accepted":false,
            "improvementDirective":{"mode":"discover"}});
        let ignored=json!({"id":"g2","candidateId":null,
            "newFindings":0,"accepted":false,
            "improvementDirective":{"mode":"implement_research"},
            "improvementActionObservation":{"status":"not_followed"}});
        assert!(should_stop_no_progress(2,false,false),
            "The old logic would have ended the campaign");
        assert!(corrective_retry_allowed(&[discovered,ignored.clone()],2),
            "The corrective change_approach action must get a scheduled turn");
        assert_eq!(improvement_directive(&[ignored.clone()])["mode"],"change_approach");
        let failed_retry=json!({"id":"g3","candidateId":null,
            "newFindings":0,"accepted":false,
            "improvementDirective":{"mode":"change_approach"}});
        assert!(!corrective_retry_allowed(&[ignored,failed_retry],3),
            "Corrective retries must be bounded, not spin forever");
    }

    #[test]
    fn learned_plan_is_measured_by_actual_edits_not_prompt_delivery() {
        let delivered=json!({"mode":"implement_research",
            "sourceGenerationId":"prior",
            "previousFocusFiles":["src/prior.rs"]});
        let mut no_action=json!({"id":"followup","jobId":"worker",
            "accepted":false,"candidateId":null,"newFindings":3,
            "improvementDirective":delivered,
            "focusFiles":["src/prior.rs"],"toolsUsed":[{"tool":"code_search","calls":8}]});
        let ignored=improvement_action_observation(&no_action,0);
        assert_eq!(ignored["status"],"not_followed",
            "Eight more searches do not mean the model implemented its learned strategy");
        no_action["improvementActionObservation"]=ignored;
        assert_eq!(improvement_directive(&[no_action.clone()])["mode"],"change_approach");
        let attempted=improvement_action_observation(&json!({
            "accepted":false,"candidateId":null,
            "improvementDirective":no_action["improvementDirective"],
            "writeReceipts":[{"tool":"workspace_replace","written":true}],
            "newFindings":0}),8);
        assert_eq!(attempted["followed"],true);
        assert_eq!(attempted["status"],"attempted");
        let selected=improvement_action_observation(&json!({
            "accepted":true,"pairedRevalidated":true,"candidateId":"paired-1",
            "improvementDirective":{"mode":"implement_research"},
            "toolsUsed":[]}),0);
        assert_eq!(selected["status"],"verified_adoption");
        let unproven=improvement_action_observation(&json!({
            "accepted":true,"pairedRevalidated":false,"candidateId":"paired-1",
            "improvementDirective":{"mode":"implement_research"}}),0);
        assert_eq!(unproven["status"],"reported_adoption_unverified");
    }

    #[test]
    fn repeated_unproductive_action_program_evolves_its_own_algorithm() {
        let source=json!({"id":"source","newFindings":2,
            "actionTrace":[{"tool":"code_search","executed":true},
                {"tool":"workspace_read_range","executed":true}]});
        let original=synthesize_action_program(&[source]);
        let revision=original["revision"].clone();
        let failed=|job:&str|json!({"id":job,"jobId":job,
            "executionMethod":{"actionProgram":{"revision":revision}},
            "methodActions":[{"inducedProgramDecision":{"chosenTool":"code_search"}}],
            "attemptedCandidateId":null,"writeReceipts":[],"accepted":false});
        let one=evolve_stalled_action_program(original.clone(),&[failed("j1")],
            "workspace_replace_range");
        assert_eq!(one["revision"],revision,"A single failure must not overfit");
        let changed=evolve_stalled_action_program(original.clone(),
            &[failed("j1"),failed("j2")],"workspace_replace_range");
        assert_ne!(changed["revision"],revision);
        assert_eq!(changed["mutationOperator"],"append_source_mutation");
        assert_eq!(changed["sequence"],json!(["code_search","workspace_read_range","workspace_replace_range"]));
        assert_eq!(changed["failureEpisodes"],2);
        let duplicate=evolve_stalled_action_program(original,
            &[failed("j1"),failed("j1")],"workspace_replace_range");
        assert_eq!(duplicate["revision"],revision,
            "Replaying a generation is not a second independent trial");
    }

    #[test]
    fn unrelated_episodes_compose_a_new_native_harness_method() {
        let exploratory=json!({
            "id":"research","newFindings":3,"accepted":false,
            "actionTrace":[
                {"tool":"code_search","executed":true},
                {"tool":"workspace_read_range","executed":true}
            ]
        });
        let implementation=json!({
            "id":"candidate","accepted":false,
            "attemptedCandidateId":"real-candidate",
            "candidateCapture":{"checksPassed":true},
            "actionTrace":[
                {"tool":"workspace_replace","executed":false},
                {"tool":"workspace_replace_range","executed":true}
            ]
        });
        let plan=synthesize_action_program(&[exploratory.clone(),implementation.clone()]);
        assert_eq!(plan["sequence"],
            json!(["code_search","workspace_read_range","workspace_replace_range"]));
        assert_eq!(plan["onFailure"][0]["onFailure"],"workspace_replace");
        assert_eq!(plan["onFailure"][0]["tryTool"],"workspace_replace_range");
        assert_eq!(plan["status"],"trial_unverified",
            "Method synthesis does not claim a frozen quality gain");
        let altered=synthesize_action_program(&[exploratory]);
        assert_ne!(altered["revision"],plan["revision"],
            "New execution observations must revise the harness algorithm");
        let empty=synthesize_action_program(&[]);
        assert_eq!(empty["sequence"],json!([]));
    }

    #[test]
    fn a_failed_descendant_mutates_the_improvement_algorithm_again() {
        let parent=synthesize_action_program(&[json!({"id":"research",
            "newFindings":2,"actionTrace":[
                {"tool":"code_search","executed":true},
                {"tool":"workspace_read_range","executed":true}]})]);
        let row=|id:&str,policy:&Value|json!({"id":id,"jobId":id,
            "createdAt":id,"executionMethod":{"actionProgram":policy},
            "methodActions":[{"inducedProgramDecision":{
                "programRevision":policy["revision"],"chosenTool":"code_search"}}],
            "writeReceipts":[],"attemptedCandidateId":null});
        let first=evolve_stalled_action_program(parent.clone(),
            &[row("a",&parent),row("b",&parent)],"workspace_replace");
        assert_eq!(first["mutatedFrom"],parent["revision"]);
        assert_eq!(first["mutationOperator"],"append_source_mutation");
        let adopted=evolve_stalled_action_program(parent.clone(),
            &[row("a",&parent),row("b",&parent),row("c",&first)],
            "workspace_replace");
        assert_eq!(adopted["revision"],first["revision"],
            "One unsuccessful descendant trial does not invent a second gain");
        let second=evolve_stalled_action_program(parent.clone(),
            &[row("a",&parent),row("b",&parent),row("c",&first),row("d",&first)],
            "workspace_replace");
        assert_eq!(second["mutatedFrom"],first["revision"]);
        assert_ne!(second["revision"],first["revision"]);
        assert_ne!(second["mutationOperator"],first["mutationOperator"]);
        assert_ne!(second["sequence"],first["sequence"]);
        // A duplicated failed receipt must not count twice for the child.
        let replay=evolve_stalled_action_program(parent,
            &[row("a",&first),row("a",&first)],"workspace_replace");
        assert_ne!(replay["revision"],second["revision"]);
    }

    #[test]
    fn one_grounded_read_only_generation_produces_a_real_next_edit_trial() {
        let read_only=json!({"revision":"read-only-one",
            "sequence":["workspace_read_range"],"onFailure":[],
            "recoveryPaths":[],"evidence":{"episodes":1}});
        let executable=complete_read_only_action_program(
            read_only.clone(),"workspace_replace_range");
        assert_eq!(executable["sequence"],
            json!(["workspace_read_range","workspace_replace_range"]));
        assert_eq!(executable["mutatedFrom"],"read-only-one");
        assert_eq!(executable["mutationOperator"],
            "complete_grounded_read_only_experience");
        assert_eq!(executable["status"],
            "trial_from_successful_read_unverified_edit");
        assert_ne!(executable["revision"],read_only["revision"]);
        assert_eq!(complete_read_only_action_program(
            executable.clone(),"workspace_replace_range")["revision"],
            executable["revision"],"No duplicate mutation after edit stage exists");
        assert_eq!(complete_read_only_action_program(
            json!({"revision":"none","sequence":["code_search"]}),
            "workspace_replace")["revision"],"none",
            "No source observation means no invented model-written patch");
    }

    #[test]
    fn verified_successes_induce_a_shorter_unverified_descendant_program() {
        let parent=json!({"revision":"research-then-edit",
            "sequence":["code_search","workspace_read_range","workspace_replace_range"],
            "onFailure":[],"recoveryPaths":[]});
        let observed=|job:&str,passed:bool|{
            let execution=format!("exec-{job}");
            json!({
                "jobId":job,"domain":"memory","sourceContextChars":1100,
                "parentContextFit":{"sourceExcerptDeferred":false},
                "executionMethod":{"actionProgram":parent},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":"research-then-edit",
                        "chosenTool":"workspace_replace_range","actionCount":1}}],
                "actionTrace":[
                    {"runId":execution,"opSeq":1,"tool":"workspace_read_range",
                        "executed":true,"sourceChanged":false,"status":"completed"},
                    {"runId":execution,"opSeq":2,"tool":"workspace_replace_range",
                        "executed":true,"sourceChanged":true,"status":"completed"}],
                "accepted":passed,"pairedRevalidated":passed,
                "evaluation":{"candidateStatus":"verified",
                    "measurementScope":"frozen_native_cases_only",
                    "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}}
            })
        };
        let single=compress_successful_action_program(parent.clone(),
            &[observed("one",true)]);
        assert_eq!(single["revision"],parent["revision"]);
        let compressed=compress_successful_action_program(parent.clone(),
            &[observed("one",true),observed("two",true)]);
        assert_eq!(compressed["mutatedFrom"],parent["revision"]);
        assert_eq!(compressed["mutationOperator"],"success_derived_discovery_ablation");
        assert_eq!(compressed["sequence"],
            json!(["workspace_read_range","workspace_replace_range"]));
        assert_eq!(compressed["status"],"trial_success_compression_unverified");
        let no_gain=compress_successful_action_program(parent.clone(),
            &[observed("one",true),observed("two",false)]);
        assert_eq!(no_gain["revision"],parent["revision"]);
        let mut missing_source=observed("two",true);
        missing_source["sourceContextChars"]=json!(0);
        let no_context=compress_successful_action_program(parent.clone(),
            &[observed("one",true),missing_source]);
        assert_eq!(no_context["revision"],parent["revision"]);
        let mut search_used=observed("two",true);
        search_used["actionTrace"].as_array_mut().unwrap().insert(0,json!({
            "runId":"exec-two","opSeq":0,"tool":"code_search",
            "executed":true,"sourceChanged":false,"status":"completed"}));
        let search_required=compress_successful_action_program(parent.clone(),
            &[observed("one",true),search_used]);
        assert_eq!(search_required["revision"],parent["revision"]);
    }

    #[test]
    fn invalid_compiled_candidates_improve_the_method_not_only_the_prompt() {
        let original=json!({"revision":"compiler-policy",
            "sequence":["code_search","workspace_replace"],
            "onFailure":[],"recoveryPaths":[]});
        let episode=|id:&str,kind:&str| {
            let execution=format!("exec-{id}");
            json!({"id":id,"jobId":id,
                "executionMethod":{"actionProgram":original},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":"compiler-policy",
                        "chosenTool":"workspace_replace","actionCount":0}}],
                "actionTrace":[{"runId":execution,"opSeq":1,
                    "tool":"workspace_replace","status":"completed",
                    "sourceChanged":true,"executed":true}],
                "attemptedCandidateId":format!("candidate-{id}"),
                "candidateCapture":{"checksPassed":kind!="compile"},
                "evaluation":{"verdict":if kind=="regression" {"regressed"}else{"inconclusive"}},
                "accepted":false})
        };
        let once=evolve_stalled_action_program(original.clone(),
            &[episode("bad-1","compile")],"workspace_replace");
        assert_eq!(once["revision"],original["revision"],
            "One bad candidate is not enough to rewrite its execution algorithm");
        let altered=evolve_stalled_action_program(original.clone(),
            &[episode("bad-1","compile"),episode("bad-2","compile")],
            "workspace_replace");
        assert_eq!(altered["mutationOperator"],"repair_failed_compilation_method");
        assert_eq!(altered["sequence"],json!([
            "code_search","workspace_read_range","workspace_replace_range"]));
        assert_eq!(altered["compileFailureEpisodes"],2);
        assert_ne!(altered["revision"],original["revision"]);
        let regression=evolve_stalled_action_program(original.clone(),
            &[episode("holdout-1","regression"),episode("holdout-2","regression")],
            "workspace_replace");
        assert_eq!(regression["mutationOperator"],"repair_regressed_quality_method");
        assert_eq!(regression["sequence"],json!(["code_search","workspace_replace_range"]));
        assert_eq!(regression["qualityRegressionEpisodes"],2);
        let phantom=episode("phantom","compile");
        let mut phantom2=episode("phantom-2","compile");
        phantom2["actionTrace"]=json!([]);
        let no_credited_failure=evolve_stalled_action_program(original,
            &[phantom,phantom2],"workspace_replace");
        assert_eq!(no_credited_failure["revision"],"compiler-policy",
            "A non-executed policy must not accumulate compile failure feedback");
    }

    #[test]
    fn verified_no_gain_candidates_change_the_next_generation_method() {
        let program=json!({"revision":"tested-source-method",
            "sequence":["code_search","workspace_replace"],
            "onFailure":[],"recoveryPaths":[]});
        let episode=|job:&str,observed:bool| {
            let execution=format!("run-{job}");
            json!({"id":job,"jobId":job,
                "executionMethod":{"actionProgram":program},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":"tested-source-method",
                        "chosenTool":"workspace_replace","actionCount":0}}],
                "actionTrace":if observed {json!([{"runId":execution,"opSeq":1,
                    "tool":"workspace_replace","status":"completed",
                    "executed":true,"sourceChanged":true}])} else {json!([])},
                "attemptedCandidateId":format!("candidate-{job}"),
                "candidateCapture":{"checksPassed":true},
                "evaluation":{"verdict":"inconclusive","metrics":{
                    "pairedCases":13,"improvedHoldouts":0,"regressions":0}},
                "accepted":false})
        };
        let once=evolve_stalled_action_program(program.clone(),
            &[episode("one",true)],"workspace_replace");
        assert_eq!(once["revision"],program["revision"],
            "One measured no-gain patch is not enough to alter the controller");
        let changed=evolve_stalled_action_program(program.clone(),
            &[episode("one",true),episode("two",true)],"workspace_replace");
        assert_eq!(changed["mutationOperator"],"repair_zero_gain_candidate_method");
        assert_eq!(changed["qualityNoGainEpisodes"],2);
        assert_eq!(changed["sequence"],json!([
            "code_search","workspace_read_range","workspace_replace_range"]));
        assert_ne!(changed["revision"],program["revision"]);
        let phantom=evolve_stalled_action_program(program.clone(),
            &[episode("one",true),episode("two",false)],"workspace_replace");
        assert_eq!(phantom["qualityNoGainEpisodes"],Value::Null,
            "A displayed method choice without an executed tool is not a trial");
    }

    #[test]
    fn adopted_meta_operator_changes_a_new_domains_executable_program() {
        let parent=json!({"revision":"root-method",
            "sequence":["code_search","workspace_read_range","workspace_replace_range"],
            "onFailure":[],"recoveryPaths":[]});
        let generation=|job:&str,domain:&str,accepted:bool|{
            let execution=format!("exec-{job}");
            json!({"id":job,"jobId":job,"domain":domain,
                "accepted":accepted,"pairedRevalidated":accepted,
                "evaluation":{"candidateStatus":"verified",
                    "measurementScope":"frozen_native_cases_only",
                    "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}},
                "executionMethod":{"actionProgram":{
                    "revision":"source-revision",
                    "mutationOperator":"success_derived_discovery_ablation"}},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":"source-revision",
                        "chosenTool":"workspace_replace_range","actionCount":0}}],
                "actionTrace":[{"runId":execution,"opSeq":1,
                    "tool":"workspace_replace_range","status":"completed"}]})
        };
        let one=generation("job-a","memory",true);
        let two=generation("job-b","tools",true);
        let transferred=transfer_verified_mutation_operator(parent.clone(),
            &[one.clone(),two.clone()]);
        assert_eq!(transferred["sequence"],
            json!(["workspace_read_range","workspace_replace_range"]));
        assert_eq!(transferred["metaTransfer"]["operator"],
            "omit_redundant_discovery");
        assert_eq!(transferred["mutatedFrom"],"root-method");
        assert_ne!(transferred["revision"],parent["revision"]);
        assert_eq!(transferred["status"],"trial_cross_domain_meta_operator");
        assert_eq!(transfer_verified_mutation_operator(parent.clone(),
            &[one.clone()])["revision"],"root-method");
        assert_eq!(transfer_verified_mutation_operator(parent.clone(),
            &[one.clone(),generation("job-b","tools",false)])["revision"],
            "root-method","Unverified outcomes cannot train the meta-operator");
        assert_eq!(transfer_verified_mutation_operator(parent,
            &[one,generation("job-c","memory",true)])["revision"],"root-method",
            "The same domain twice is not cross-domain transfer");
    }

    #[test]
    fn independent_matched_source_program_trials_promote_only_measured_quality() {
        let root=json!({"revision":"root","sequence":["workspace_read_range",
            "workspace_replace"],"status":"trial_unverified"});
        let child=json!({"revision":"child","mutatedFrom":"root",
            "sequence":["workspace_read_range","workspace_replace_range"],
            "status":"trial_unverified"});
        let run=|id:&str,program:&Value,verified:bool,commit:&str|{
            let execution=format!("exec-{id}");
            json!({
                "id":id,"jobId":id,
                "objectiveId":"objective","domain":"memory",
                "generatorCommit":commit,"generatorModel":"qwen-local",
                "generatorProviderId":"local","focusFiles":["src/engine/memory.rs"],
                "executionMethod":{"actionProgram":program},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":program["revision"],
                        "chosenTool":"workspace_read_range","actionCount":0}}],
                "actionTrace":[{"runId":execution,"opSeq":1,
                    "tool":"workspace_read_range","status":"completed","executed":true}],
                "accepted":verified,"pairedRevalidated":verified,
                "evaluation":{"candidateStatus":"verified",
                    "measurementScope":"frozen_native_cases_only",
                    "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}},
                "error":"","tokens":1000,"elapsedMs":1000
            })
        };
        let source="a".repeat(40);
        let mut rows=vec![
            run("baseline-1",&root,false,&source),
            run("baseline-2",&root,false,&source),
            run("challenger-1",&child,true,&source),
            run("challenger-2",&child,true,&source)
        ];
        let (promoted,comparison)=select_evaluated_action_program(&rows,child.clone());
        assert_eq!(promoted["revision"],"child");
        assert_eq!(comparison["status"],"quality_promoted");
        assert_eq!(comparison["independentWins"],2);
        rows[3]["actionTrace"]=json!([]); // A selected but unused tool is no trial.
        let (unproven,receipt)=select_evaluated_action_program(&rows,child.clone());
        assert_eq!(unproven["revision"],"child");
        assert_eq!(receipt["status"],"trial_unverified");
        rows[3]=run("challenger-2",&child,true,&"b".repeat(40));
        let (_,mismatch)=select_evaluated_action_program(&rows,child.clone());
        assert_eq!(mismatch["status"],"trial_unverified",
            "Different source commits cannot be paired as a controlled trial");
        rows[3]=run("challenger-2",&child,true,&source);
        rows[3]["jobId"]=json!("challenger-1");
        let (_,duplicate)=select_evaluated_action_program(&rows,child);
        assert_eq!(duplicate["status"],"trial_unverified",
            "Duplicate worker identity cannot establish two independent wins");
    }

    #[test]
    fn a_measured_regressing_descendant_returns_to_its_executed_parent() {
        let parent=json!({"revision":"parent","sequence":["workspace_replace"]});
        let child=json!({"revision":"child","mutatedFrom":"parent",
            "sequence":["workspace_replace_range"]});
        let run=|id:&str,program:&Value,accepted:bool| {
            let execution=format!("exec-{id}");
            json!({"id":id,"jobId":id,
                "objectiveId":"same","domain":"memory","generatorCommit":"a".repeat(40),
                "generatorModel":"one-model","generatorProviderId":"provider",
                "focusFiles":["src/engine/memory.rs"],
                "executionMethod":{"actionProgram":program},
                "methodActions":[{"executionId":execution,
                    "inducedProgramDecision":{"programRevision":program["revision"],
                        "chosenTool":"workspace_read_range","actionCount":0}}],
                "actionTrace":[{"runId":execution,"opSeq":1,
                    "tool":"workspace_read_range","status":"completed"}],
                "accepted":accepted,"pairedRevalidated":accepted,
                "evaluation":{"candidateStatus":"verified",
                    "measurementScope":"frozen_native_cases_only",
                    "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}},
                "error":""})
        };
        let history=vec![
            run("p1",&parent,true),run("p2",&parent,true),
            run("c1",&child,false),run("c2",&child,false)
        ];
        let (selected,receipt)=select_evaluated_action_program(&history,child);
        assert_eq!(selected["revision"],"parent");
        assert_eq!(selected["status"],"quality_rejected_child_fallback");
        assert_eq!(receipt["status"],"quality_rejected");
    }

    #[test]
    fn failed_edit_can_synthesize_a_multi_step_repair_from_real_receipts() {
        let failed=json!({"id":"repair-episode","accepted":false,
            "candidateCapture":{"checksPassed":true},
            "actionTrace":[
                {"tool":"code_search","executed":true,"sourceChanged":false},
                {"tool":"workspace_replace","executed":false,"sourceChanged":false},
                {"tool":"workspace_read_range","executed":true,"sourceChanged":false},
                {"tool":"workspace_replace_range","executed":true,"sourceChanged":true}
            ]});
        let plan=synthesize_action_program(&[failed.clone()]);
        assert_eq!(plan["recoveryPaths"][0]["onFailure"],"workspace_replace");
        assert_eq!(plan["recoveryPaths"][0]["steps"],
            json!(["workspace_read_range","workspace_replace_range"]));
        assert_eq!(plan["recoveryPaths"][0]["weightedEvidence"],3);
        let no_write=json!({"id":"read-only","actionTrace":[
            {"tool":"workspace_replace","executed":false},
            {"tool":"workspace_read_range","executed":true}]});
        assert!(synthesize_action_program(&[no_write])["recoveryPaths"]
            .as_array().unwrap().is_empty());
        let stalled=|id:&str|json!({"id":id,"jobId":id,
            "executionMethod":{"actionProgram":{"revision":plan["revision"]}},
            "methodActions":[{"inducedProgramDecision":{"chosenTool":"code_search"}}],
            "attemptedCandidateId":null,"writeReceipts":[]});
        let improved=evolve_stalled_action_program(plan.clone(),
            &[stalled("fail-a"),stalled("fail-b")],"workspace_replace");
        assert_eq!(improved["mutationOperator"],"promote_observed_recovery_path");
        assert_eq!(improved["sequence"],json!(["workspace_read_range","workspace_replace_range"]));
        assert_ne!(improved["revision"],plan["revision"]);
    }

    #[test]
    fn real_edit_failures_induce_an_executable_next_generation_method() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"장기 기억 검색 개선",
            "rsiKnowledgeSource":root,"rsiTargetDomain":"memory"});
        let partition=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&["장기 기억 검색 개선"]);
        let failed=json!({"id":"g1","knowledgePartition":partition,
            "objectiveId":objective,"domain":"memory",
            "writeReceipts":[{"tool":"workspace_replace","written":false,
                "status":"failed","error":"oldText mismatch"}]});
        let learned=learn_execution_method(&state,&input,&[failed.clone()]);
        assert_eq!(learned["preferredTool"],"workspace_replace_range");
        assert_eq!(learned["reason"],"counterexample_exploration");
        assert_eq!(learned["status"],"trial_unverified");
        assert_eq!(learned["evidence"]["workspace_replace"]["verifiedAdoptions"],0);
        assert_eq!(learned["program"][2]["tool"],"workspace_replace_range");
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-g2-worker","jobId":"g2-worker",
            "candidateId":"proposed-g2","checksPassed":true
        })).unwrap();
        let succeeding=json!({"id":"g2","jobId":"g2-worker","knowledgePartition":partition,
            "objectiveId":objective,"domain":"memory","accepted":false,
            "attemptedCandidateId":"proposed-g2",
            "candidateCapture":{"checksPassed":true},
            "writeReceipts":[{"tool":"workspace_replace_range","written":true,
                "status":"completed"}]});
        let next=learn_execution_method(&state,&input,&[failed,succeeding.clone()]);
        assert_eq!(next["preferredTool"],"workspace_replace_range");
        assert_eq!(next["evidence"]["workspace_replace_range"]["checkedCandidates"],1);
        assert_eq!(next["evidence"]["workspace_replace_range"]["verifiedAdoptions"],0);
        assert_ne!(learned["revision"],next["revision"]);
        let mut forged=succeeding.clone();
        forged["id"]=json!("forged");
        forged["jobId"]=json!("unknown-worker");
        let untrusted=learn_execution_method(&state,&input,&[forged]);
        assert_eq!(untrusted["evidence"]["workspace_replace_range"]["checkedCandidates"],0,
            "A generation claim without an original rsi_run receipt must not train the policy");
        let other=json!({"rsiObjective":"브라우저 자동화 개선",
            "rsiKnowledgeSource":root,"rsiTargetDomain":"tools"});
        assert_eq!(learn_execution_method(&state,&other,&[])["status"],"unmeasured_default");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn actual_function_editor_receipts_train_separate_model_tool_selection() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"Improve named source function",
            "rsiKnowledgeSource":root,"rsiTargetDomain":"tools"});
        let partition=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&["Improve named source function"]);
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-scoped-function-worker","jobId":"scoped-function-worker",
            "candidateId":"scoped-candidate","checksPassed":true
        })).unwrap();
        let generation=json!({"id":"scoped-function-gen","jobId":"scoped-function-worker",
            "knowledgePartition":partition,"objectiveId":objective,"domain":"tools",
            "accepted":false,"attemptedCandidateId":"scoped-candidate",
            "writeReceipts":[{"tool":"workspace_replace_function","written":true,
                "status":"completed"}]});
        let learned=learn_execution_method(&state,&input,&[generation]);
        assert_eq!(learned["preferredTool"],"workspace_replace_function");
        assert_eq!(learned["fallbackTool"],"workspace_replace_range");
        assert_eq!(learned["evidence"]["workspace_replace_function"]["checkedCandidates"],1);
        assert_eq!(learned["evidence"]["workspace_replace_range"]["checkedCandidates"],0,
            "A function-scoped source change must not be attributed to line range editing");
        assert_eq!(learned["status"],"trial_unverified",
            "Compiler success is not independently adopted improvement");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejected_write_is_not_a_completed_improvement_action() {
        let candidate=json!({
            "accepted":false,"candidateId":null,"newFindings":0,
            "improvementDirective":{"mode":"implement_research"},
            "toolsUsed":[{"tool":"workspace_replace","calls":1}],
            "writeReceipts":[{"tool":"workspace_replace","status":"completed",
                "written":false,"error":"oldText not found"}]
        });
        let observed=improvement_action_observation(&candidate,0);
        assert_eq!(observed["completedWrites"],0);
        assert_eq!(observed["rejectedWrites"],1);
        assert_eq!(observed["status"],"not_followed");
    }

    #[test]
    fn compilation_failure_remains_candidate_evidence_for_next_generation() {
        let failed=json!({"candidateId":"rsi-real-patch","checksPassed":false,
            "checks":[{"label":"cargo check","passed":false,
                "stderr":"error[E0308]: mismatched types"}]});
        let receipt=candidate_capture_feedback(&failed).unwrap();
        assert!(receipt.contains("rsi-real-patch"));
        assert!(receipt.contains("E0308"));
        let previous=json!({"id":"g1","accepted":false,"candidateId":null,
            "attemptedCandidateId":"rsi-real-patch","newFindings":0,
            "improvementDirective":{"mode":"implement_research"},
            "improvementActionObservation":{"status":"attempted"}});
        assert_eq!(improvement_directive(&[previous])["mode"],"revise_candidate");
        assert!(candidate_capture_feedback(&json!({"candidateId":"rsi-ok",
            "checksPassed":true})).is_none());
    }

    #[test]
    fn newly_researched_current_source_becomes_first_implementation_target() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["alpha","beta","gamma"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),
                "fn search_dispatch() { /* same relevant implementation */ }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/alpha.rs");
        let path="src/gamma.rs";
        let hash=crate::engine::code_index::indexed_source_hash(&state,
            index["id"].as_str().unwrap(),path).unwrap();
        state.store.put("experience",&json!({"id":"research-saved",
            "status":"active","generationJobId":"first-worker",
            "partitionKey":research_scope(&input).partition_key,
            "researchArtifacts":[{"tool":"workspace_read_range",
                "path":path,"sourceSha256":hash}]})).unwrap();
        let research=json!({"id":"first","jobId":"first-worker",
            "domain":"tools","candidateId":null,"accepted":false,
            "newFindings":1});
        let chosen=choose_focus_files(&state,&input,&[research.clone()],None);
        assert_eq!(chosen[0]["path"],path,
            "Actual research must change the next source choice rather than just the prompt");
        std::fs::write(root.join(path),
            "fn search_dispatch() { /* new revision invalidates prior evidence */ }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let refreshed=choose_focus_files(&state,&input,&[research],None);
        assert_ne!(refreshed[0]["path"],path,
            "Stale research must not be preferred after the source changes");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_unproductive_targeting_downranks_the_failed_source() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        for name in ["search_first.rs","search_second.rs"] {
            std::fs::write(root.join("src/engine").join(name),
                "fn search_dispatch() { /* same relevant implementation */ }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let first=choose_focus_files(&state,&input,&[],None);
        assert_eq!(first[0]["path"],"src/engine/search_first.rs");
        let history=vec![json!({"accepted":false,"candidateId":null,"newFindings":0,
            "focusFiles":["src/engine/search_first.rs"]})];
        let next=choose_focus_files(&state,&input,&history,None);
        assert_eq!(next[0]["path"],"src/engine/search_second.rs");
        // A verified candidate or a new finding is not a barren attempt.
        let useful=vec![json!({"accepted":false,"candidateId":"patch","newFindings":1,
            "focusFiles":["src/engine/search_first.rs"]})];
        assert_eq!(choose_focus_files(&state,&input,&useful,None)[0]["path"],
            "src/engine/search_first.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_success_becomes_revision_scoped_target_learning_not_a_patch_counter() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let beta=root.join("src/beta.rs");
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),
                "fn search_dispatch() { println!(\"search\"); }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/alpha.rs");
        let selected_beta=crate::engine::code_index::rank_focus_files(
            &state,index["id"].as_str().unwrap(),"search dispatch","tools",
            &BTreeMap::new(),None,2).into_iter()
            .find(|v|v["path"]=="src/beta.rs").unwrap();
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"verified",
            "paths":["src/beta.rs"],
            "evaluation":{"metrics":{"eligible":true}}
        })).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"prior-success","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":true,"candidateId":"verified-patch","newFindings":0,
            "focusFiles":["src/beta.rs"],"focusSelection":{"candidates":[selected_beta.clone()]},
            "createdAt":"2026-11-10T00:00:00Z"
        })).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/beta.rs");
        assert_eq!(learned[0]["evidence"]["priorProductive"],1.0);
        std::fs::write(&beta,"fn search_dispatch() { println!(\"new revision\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let stale=choose_focus_files(&state,&input,&[],None);
        assert_eq!(stale[0]["path"],"src/alpha.rs");
        assert_eq!(stale[0]["evidence"]["priorProductive"],0.0);
        // A falsely declared acceptance must not contribute feedback.
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"rejected",
            "paths":["src/beta.rs"],"evaluation":{"metrics":{"eligible":false}}
        })).unwrap();
        std::fs::write(&beta,"fn search_dispatch() { println!(\"search\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let unverified=choose_focus_files(&state,&input,&[],None);
        assert_eq!(unverified[0]["path"],"src/alpha.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cross_campaign_target_failures_are_reused_only_at_same_source_revision() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let first=root.join("src/search_first.rs");
        let second=root.join("src/search_second.rs");
        std::fs::write(&first,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        std::fs::write(&second,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/search_first.rs");
        let prior=json!({"id":"persisted-prior","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":false,"candidateId":null,"newFindings":0,
            "focusFiles":["src/search_first.rs"],
            "focusSelection":{"candidates":initial},
            "createdAt":"2026-11-10T00:00:00Z"});
        state.store.put("rsi_generation",&prior).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/search_second.rs",
            "the later campaign should not re-spend its tokens on the same unproductive target");
        // Same-length edit is a different revision, so old failure evidence
        // can no longer drive the new source choice.
        std::fs::write(&first,"fn search_dispatch() { /* scan bravo */ }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let refreshed=choose_focus_files(&state,&input,&[],None);
        assert_eq!(refreshed[0]["path"],"src/search_first.rs",
            "stale failure evidence must not penalize changed code");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn automatically_found_source_does_not_redefine_the_users_objective() {
        let input=json!({"rsiObjective":"기억 검색","rsiAutoFocus":true,
            "rsiFocusFiles":["src/engine/tools.rs"]});
        assert_eq!(selection_objective(&input),"기억 검색");
    }

    #[test]
    fn focus_file_context_inlines_the_target() {
        let (state, root) = state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/x.rs"), "fn a() {}\n").unwrap();
        let meta = crate::engine::code_index::refresh(&state, &root).unwrap();
        let text = focus_file_context(&state, &json!({"labDir":root}), &json!({"rsiFocusFiles":["src/engine/x.rs"],"rsiIndexId":meta["id"],"rsiObjective":"a"}));
        assert!(text.contains("fn a() {}"));
        assert!(text.contains("대상 파일 전체"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sub1_inherited_paired_failures_require_matching_frozen_evidence() {
        let (state,root)=state();
        let patch_sha="a".repeat(64);
        let proof="b".repeat(64);
        state.store.put("rsi_candidate",&json!({
            "id":"candidate-one","patchSha256":patch_sha,"baseCommit":"base",
            "oracleVersion":"rsi-worktree-v2","pairedComparisonComplete":true,
            "evaluation":{"id":"eval-one","evidenceSha256":proof}
        }));
        let pair=json!({
            "id":"rsi-pair-candidate-one-eval-one","candidateId":"candidate-one",
            "evaluationId":"eval-one","evidenceSha256":proof,
            "report":{"reportSha256":proof,"patchSha256":patch_sha,
                "baseCommit":"base","oracleVersion":"rsi-worktree-v2",
                "cases":[{"id":"planning-quality",
                    "behavioralCounterexamples":{"candidate":[{
                        "input":"설계를 읽고 결과를 저장한 다음 검증해",
                        "expectedSegments":3,"observedSegments":2,
                        "steps":["설계를 읽고 결과를 저장한","검증해"]
                    }]}}]}
        });
        state.store.put("rsi_pair",&pair);
        let examples=paired_behavioral_feedback(&state,"candidate-one");
        assert_eq!(examples.len(),1);
        assert!(examples[0].contains("expected=3"));
        let mut altered=pair;
        altered["evidenceSha256"]=json!("0".repeat(64));
        state.store.put("rsi_pair",&altered);
        assert!(paired_behavioral_feedback(&state,"candidate-one").is_empty());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn generation_context_reuses_implementation_evidence_not_tool_counters() {
        let (state, root) = state();
        let input = json!({"rsiObjective":"코드 탐색 능력을 다음 세대에 계승"});
        state.store.put("rsi_candidate",&json!({"id":"candidate","hypothesis":"검증된 검색 전략과 적용 조건","paths":["src/engine/memory.rs"]}));
        let record = json!({"history":[{
            "attempt":1,"accepted":true,"acceptedCommit":"accepted-source-revision","candidateId":"candidate",
            "toolsUsed":[{"tool":"code_search","calls":2},{"tool":"workspace_read_range","calls":1}]
        }]});
        let text = generation_context(&state, &input, &record);
        assert!(text.contains("code_search"), "{text}");
        assert!(text.contains("workspace_read_range"), "{text}");
        assert!(text.contains("1세대"), "{text}");
        assert!(text.contains("검증된 검색 전략과 적용 조건"), "{text}");
        assert!(text.contains("src/engine/memory.rs"), "{text}");
        assert!(text.contains("accepted-source-revision"), "{text}");
        assert!(!text.contains("\"calls\""),"tool frequency is not an inherited implementation: {text}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_verified_patch_is_a_candidate_even_when_the_model_hit_the_budget() {
        let result = json!({
            "status": "needs_review",
            "reason": "model_error",
            "rsi": {
                "candidateId": "rsi-test",
                "checksPassed": true,
                "modelError": "토큰 예산(250000)을 초과했습니다."
            }
        });
        assert_eq!(verified_candidate_id(&result), Some("rsi-test"));
        assert_eq!(
            verified_candidate_id(&json!({"rsi":{"candidateId":"rsi-test","checksPassed":false}})),
            None
        );
    }

    #[test]
    fn research_continues_for_new_findings_not_for_new_query_spellings() {
        assert!(!should_stop_no_progress(1, true, true));
        assert!(should_stop_no_progress(1, true, false));
        assert!(should_stop_no_progress(1, false, true));
        assert!(!should_stop_no_progress(2, true, true));
        assert!(!should_stop_no_progress(3, true, true));
        assert!(should_stop_no_progress(3, true, false));
    }

    #[test]
    fn truncated_research_rehydrates_its_complete_verified_range_without_reading_the_file_into_the_prompt(){
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let mut source=(0..80).map(|i|format!("// source line {i} with enough content to exceed a short excerpt\n")).collect::<String>();
        source.push_str("fn COMPLETE_RANGE_TAIL() {}\n");
        source.push_str("fn OUTSIDE_REQUESTED_RANGE() {}\n");
        std::fs::write(root.join("src/reuse.rs"),&source).unwrap();
        let input=json!({"rsiSourceDir":root,"rsiObjective":"memory complete source"});
        state.store.put("session",&json!({"id":"next"}));
        state.store.put("experience",&json!({"id":"bounded-research","scope":"shared","status":"active",
            "partitionKey":research_scope(&input).partition_key,"request":"memory complete source","signature":"memory complete source",
            "lesson":"memory complete source","outcome":"research","confidence":0.9,"researchArtifacts":[{
                "tool":"workspace_read_range","hash":"artifact","path":"src/reuse.rs","startLine":1,"endLine":81,
                "sourceSha256":format!("{:x}",Sha256::digest(source.as_bytes())),"truncated":true,
                "content":source.chars().take(2200).collect::<String>()}]}));
        let context=reuse_research(&state,&input,&root,"next");
        assert!(context.contains("COMPLETE_RANGE_TAIL"));assert!(!context.contains("OUTSIDE_REQUESTED_RANGE"));
        let receipt=&state.store.get("session","next").unwrap()["rsiKnowledgeReuse"][0];
        assert_eq!(receipt["rangeHydrated"],true);assert_eq!(receipt["mode"],"hash_verified_source_cache");
        assert_eq!(state.store.get("experience","bounded-research").unwrap()["researchArtifacts"][0]["content"].as_str().unwrap().chars().count(),2200);
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn research_survives_restart_and_executes_hash_checked_reuse_in_the_next_session() {
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let source="pub fn remember() { /* REUSABLE_RESEARCH_SOURCE */ }\n";
        std::fs::write(root.join("src/reuse.rs"),source).unwrap();
        state.store.put("session",&json!({"id":"research-1","rsiRun":true}));
        let args=json!({"path":"src/reuse.rs","startLine":1,"endLine":1});
        let ctx=tools::ToolContext{state:&state,store:&state.store,workspace:&root,session_id:"research-1",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let observed=tools::execute(&ctx,"workspace_read_range",&args,"run-1").await.unwrap();
        let old_hash=observed["sourceSha256"].clone();
        state.store.put("execution_operation",&json!({"id":"source-observation","sessionId":"research-1","tool":"workspace_read_range","arguments":args,"result":observed,"status":"completed"}));
        let input=json!({"rsiSourceDir":root,"rsiObjective":"장기 기억 검색 개선"});
        let first=json!({"jobId":"generation-1","accepted":false,"domain":"memory"});
        let id=remember_research(&state,&input,&first,"research-1").unwrap();
        assert_eq!(state.store.get("experience",&id).unwrap()["outcome"],"research");
        let cfg=state.config.clone();drop(state);
        let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),crate::vault::Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"research-2","rsiRun":true}));
        let context=reuse_research(&state,&input,&root,"research-2");
        assert!(context.contains("REUSABLE_RESEARCH_SOURCE"));
        let receipt=state.store.get("session","research-2").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(receipt["experienceId"],id);assert_eq!(receipt["sourceOperationId"],"source-observation");
        assert_eq!(receipt["mode"],"hash_verified_source_cache");
        assert_eq!(receipt["sourceValidity"],"hash_verified_current_source");
        assert_eq!(receipt["hypothesisValidity"],"prior_hypothesis_unverified");
        assert_eq!(receipt["entrypoint"],"rsi_evolve::reuse_research");
        // A real next-session mutation is linked to its source provenance.
        let change=json!({"path":"src/reuse.rs","oldText":"REUSABLE_RESEARCH_SOURCE","newText":"IMPROVED_RESEARCH_SOURCE"});
        let ctx=tools::ToolContext{state:&state,store:&state.store,workspace:&root,session_id:"research-2",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let written=tools::execute(&ctx,"workspace_replace",&change,"run-2").await.unwrap();
        state.store.put("execution_operation",&json!({"id":"real-next-change","sessionId":"research-2","tool":"workspace_replace","arguments":change,"result":written,"status":"completed","toolReceipt":{"verified":true}}));
        state.store.put("rsi_candidate",&json!({"id":"c","status":"verified","hypothesis":"Apply the observed implementation","paths":["src/reuse.rs"],"evaluation":{"metrics":{"eligible":true}}}));
        let second=json!({"jobId":"generation-2","accepted":true,"candidateId":"c","acceptedCommit":"a".repeat(40),"domain":"memory"});
        let next_id=remember_research(&state,&input,&second,"research-2").unwrap();
        let saved=state.store.get("experience",&next_id).unwrap();
        assert_eq!(saved["implementation_ref"]["commit"],"a".repeat(40));
        assert_eq!(saved["knowledgeApplications"][0]["actionOperationIds"][0],"real-next-change");
        assert_eq!(saved["knowledgeApplications"][0]["status"],"related_verified_change");
        assert_eq!(saved["laterGain"],"unmeasured","related actions alone cannot prove a causal gain");
        state.store.put("session",&json!({"id":"research-3"}));
        let refreshed=reuse_research(&state,&input,&root,"research-3");
        assert!(refreshed.contains("IMPROVED_RESEARCH_SOURCE"),
            "a changed source range must be refreshed, not discarded: {refreshed}");
        let refresh_receipt=state.store.get("session","research-3").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(refresh_receipt["mode"],"refreshed_source_cache");
        assert_eq!(refresh_receipt["sourceValidity"],"refreshed_range_current_source");
        assert_eq!(refresh_receipt["hypothesisValidity"],"unverified_after_source_change");
        assert_eq!(refresh_receipt["experienceId"],id);
        assert_eq!(refresh_receipt["refreshed"],true);
        let refreshed_record=state.store.get("experience",&id).unwrap();
        assert_eq!(refreshed_record["researchArtifacts"][0]["refreshKind"],"line_range_revalidation");
        assert_eq!(refreshed_record["researchArtifacts"][0]["previousSourceSha256"],old_hash);
        assert_ne!(refreshed_record["researchArtifacts"][0]["sourceSha256"],old_hash);
        let foreign=json!({"rsiSourceDir":"/different/project","rsiObjective":"장기 기억 검색 개선"});
        assert!(reuse_research(&state,&foreign,&root,"research-3").is_empty());
        drop(state);let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_source_keeps_research_record_and_reports_unavailable_range() {
        let (state, root) = state();
        let workspace = root.join("work");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file = workspace.join("src/reuse.rs");
        std::fs::write(&file, "old source\n").unwrap();
        let old_hash = format!("{:x}", Sha256::digest(std::fs::read(&file).unwrap()));
        let input = json!({"rsiSourceDir":workspace,"rsiObjective":"memory research refresh"});
        let scope = research_scope(&input);
        state.store.put("experience", &json!({
            "id":"experience-missing","scope":"shared","partitionKey":scope.partition_key,
            "signature":"memory research refresh","request":"memory research refresh",
            "lesson":"test range refresh","outcome":"research","status":"active","occurrences":1,
            "researchArtifacts":[{
                "tool":"workspace_read_range","path":"src/reuse.rs","startLine":1,"endLine":1,
                "sourceSha256":old_hash,"content":"    1 old source\n","hash":"artifact-missing",
                "operationId":"op-1"
            }]
        })).unwrap();
        state.store.put("session", &json!({"id":"s-missing"})).unwrap();
        std::fs::remove_file(&file).unwrap();
        let prompt = reuse_research(&state, &input, &workspace, "s-missing");
        assert!(prompt.is_empty(), "missing source must not be invented: {prompt}");
        let receipt = state.store.get("session", "s-missing").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(receipt["mode"], "source_range_unavailable");
        assert_eq!(receipt["reason"], "source_missing");
        assert_eq!(receipt["sourceValidity"], "source_unavailable");
        assert_eq!(receipt["hypothesisValidity"], "prior_hypothesis_unverified");
        assert!(
            state.store.get("experience", "experience-missing").is_some(),
            "the research record survives an unavailable source"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_searches_and_changed_queries_do_not_manufacture_findings() {
        let (state,root)=state();
        state.store.put("execution_operation",&json!({"id":"empty","sessionId":"s","tool":"code_search","arguments":{"query":"anything"},"result":{"matchCount":0,"matches":[]},"status":"completed"}));
        assert!(research_artifacts(&state,"s").is_empty());
        let result=json!({"matchCount":1,"matches":[{"path":"src/reuse.rs","name":"remember","line":1}]});
        for (id,query) in [("found-1","remember"),("found-2","fn remember")] {
            state.store.put("execution_operation",&json!({"id":id,"sessionId":"s","tool":"code_search","arguments":{"query":query},"result":result,"status":"completed"}));
        }
        assert_eq!(research_artifacts(&state,"s").len(),1);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn resource_limits_reject_unbounded_or_malformed_requests() {
        assert!(limits(&json!({})).unwrap()["tokenBudget"].is_null());
        assert!(limits(&json!({})).unwrap()["timeBudgetSeconds"].is_null());
        assert!(limits(&json!({"tokenBudget":0,"timeBudgetSeconds":0})).unwrap()["tokenBudget"].is_null());
        for body in [
            json!({"maxAttempts":0}),
            json!({"timeBudgetSeconds":999999}),
            json!({"tokenBudget":"unlimited"}),
            json!({"tokenBudget":999}),
        ] {
            assert!(limits(&body).is_err());
        }
    }
    #[tokio::test]
    async fn manual_selection_waits_for_owner_and_is_revision_bound() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record = json!({"id":"campaign","status":"running","selectionPolicy":"manual","waitingMs":0,
            "comparison":{"attempt":2,"evaluationId":"evaluation-2","canSelectCandidate":true}});
        let owner = async {
            loop {
                if state
                    .store
                    .get("rsi_campaign", "campaign")
                    .is_some_and(|r| r["phase"] == "waiting_for_choice")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":1})
            )
            .is_err());
            let chosen = choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":2}),
            )
            .unwrap();
            assert_eq!(
                choose(
                    &state,
                    "campaign",
                    &json!({"choice":"candidate","attempt":2})
                )
                .unwrap(),
                chosen
            );
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"baseline","attempt":2})
            )
            .is_err());
        };
        let (selected, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(select(&context, &mut record), owner)
        })
        .await
        .unwrap();
        assert_eq!(selected.unwrap(), "candidate");
        assert_eq!(record["comparison"]["selected"], "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn automatic_choice_keeps_the_baseline_when_superiority_is_unproven() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record =
            json!({"selectionPolicy":"automatic","comparison":{"canSelectCandidate":false}});
        assert_eq!(select(&context, &mut record).await.unwrap(), "baseline");
        record["comparison"]["canSelectCandidate"] = json!(true);
        assert_eq!(select(&context, &mut record).await.unwrap(), "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn automatic_write_permission_is_scoped_to_the_active_private_worktree() {
        let (state, root) = state();
        let _context = parent(&state);
        state.store.put(
            "job",
            &json!({"id":"generation","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"campaign"},"rsiWorkspace":state.config.workspace}),
        );
        state.store.put("session",&json!({"id":"session","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"generation"}));
        let args = json!({"path":"src/engine/algorithm.rs"});
        assert!(auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        assert!(!auto_write_allowed(&state, "session", "shell_exec", &args));
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &json!({"path":".git/config"})
        ));
        let mut outside = state.clone();
        outside.config.workspace = root.join("outside");
        assert!(!auto_write_allowed(
            &outside,
            "session",
            "workspace_write",
            &args
        ));
        state.store.patch_if(
            "job",
            "campaign",
            "status",
            &json!("running"),
            &json!({"cancelRequested":true}),
        );
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn restart_never_blindly_replays_a_partial_evolution() {
        let (state, root) = state();
        let context = parent(&state);
        context
            .checkpoint(json!({"rsiEvolutionInFlight":true}))
            .unwrap();
        jobs::recover(&state);
        assert_eq!(
            state.store.get("job", "campaign").unwrap()["status"],
            "needs_review"
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn exhausted_usage_prevents_a_new_model_call_and_caps_the_remaining_output() {
        let (state, root) = state();
        let context = parent(&state);
        context.checkpoint(json!({"input":{"limits":{"tokenBudget":2000}},"evolution":{"activeBaseTokens":500}})).unwrap();
        state
            .store
            .put("session", &json!({"id":"s","rsiCampaignId":"campaign"}));
        assert_eq!(
            remaining_output_tokens(&state, "s", 1000).unwrap(),
            Some(500)
        );
        assert!(remaining_output_tokens(&state, "s", 1400).is_err());
        assert_eq!(
            remaining_output_tokens(&state, "ordinary", 99999).unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_paired_runner_finds_a_quality_gain_when_every_guard_already_passes() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(
            source.join("Cargo.toml"),
            "[package]\nname=\"rsi-test-subject\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let before = "pub fn accepts(n:i32)->bool { n >= 0 }\nfn main(){}\n";
        let after = "pub fn accepts(n:i32)->bool { n > 0 }\nfn main(){}\n";
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "baseline",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), after).unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&source).args(["diff", "HEAD"]);
        let patch = command(&context, cmd, Duration::from_secs(5))
            .await
            .unwrap()
            .stdout;
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        let fixtures = root.join("frozen");
        std::fs::create_dir_all(&fixtures).unwrap();
        let oracle = fixtures.join("oracle.rs");
        let mut code = String::new();
        for (index, value) in [-4, -3, -2, -1, 1, 2, 3].into_iter().enumerate() {
            code.push_str(&format!(
                "#[test] fn case_{index}(){{assert_eq!(crate::accepts({value}),{value}>0);}}\n"
            ));
        }
        code.push_str("#[test] fn case_7(){let quality=(-4..=4).filter(|n|crate::accepts(*n)==(*n>0)).count() as f64/9.0;println!(\"DOMINION_RSI_METRIC {{\\\"quality\\\":{quality}}}\");}\n");
        std::fs::write(&oracle, &code).unwrap();
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i==7{"quality"}else{"guard"},"holdout":i==0||i==7,
            "filter":format!("rsi_frozen_oracle::case_{i}"),"metrics":if i==7{json!([{"key":"quality","direction":"higher","minRelativeGain":0.05}])}else{json!([])}})).collect::<Vec<_>>();
        let plan = json!({"oracleVersion":"independent-boundary-v1","cases":cases,"oracleFixture":oracle,
            "oracleSha256":format!("{:x}",Sha256::digest(code.as_bytes()))});
        let patch_file = fixtures.join("candidate.patch");
        std::fs::write(&patch_file, &patch).unwrap();
        let plan_file = fixtures.join("plan.json");
        std::fs::write(&plan_file, plan.to_string()).unwrap();
        std::fs::write(
            fixtures.join("rsi-lab.sh"),
            include_str!("../../../scripts/rsi-lab.sh"),
        )
        .unwrap();
        std::fs::write(
            fixtures.join("rsi-measure.mjs"),
            include_str!("../../../scripts/rsi-measure.mjs"),
        )
        .unwrap();
        let report_file = fixtures.join("report.json");
        let mut cmd = Command::new("bash");
        cmd.current_dir(&source)
            .arg(fixtures.join("rsi-lab.sh"))
            .arg(&base)
            .arg(&patch_file)
            .arg(&plan_file)
            .arg(&report_file);
        let output = command(&context, cmd, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(&report_file).unwrap()).unwrap();
        assert_eq!(report["baselineBuildOk"], true);
        assert_eq!(report["candidateBuildOk"], true);
        assert!(
            report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["baselinePass"] == true && r["candidatePass"] == true),
            "{report}"
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["candidate"], 1.0);
        assert!(
            report["cases"][7]["metrics"]["quality"]["baseline"]
                .as_f64()
                .unwrap()
                < 1.0
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["samples"], 6);
        assert_eq!(std::fs::read_to_string(&oracle).unwrap(), code);
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_and_adoption_preserve_the_users_checkout_and_commit() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/main.rs"), "base\n").unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "base",
            ],
        )
        .await
        .unwrap();
        let original = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), "owner edit\n").unwrap();
        std::fs::write(source.join("src/new.rs"), "new code\n").unwrap();
        std::fs::write(source.join("notes.txt"), "owner research\n").unwrap();
        std::fs::create_dir_all(source.join("scripts/research")).unwrap();
        std::fs::write(
            source.join("scripts/research/probe.py"),
            "owner research script\n",
        )
        .unwrap();
        std::fs::write(
            source.join("scripts/new-build.mjs"),
            "// new build helper\n",
        )
        .unwrap();
        let lab = root.join("lab");
        git(
            &context,
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                lab.to_str().unwrap(),
                &original,
            ],
        )
        .await
        .unwrap();
        let (snapshot, fingerprint) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(snapshot);
        assert_eq!(fingerprint.len(), 64);
        assert!(!lab.join("notes.txt").exists());
        assert!(lab.join("src/new.rs").exists());
        assert!(!lab.join("scripts/research/probe.py").exists());
        assert!(lab.join("scripts/new-build.mjs").exists());
        let base = git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap();
        std::fs::write(lab.join("src/main.rs"), "measured improvement\n").unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&lab).args(["diff", "HEAD"]);
        let patch = String::from_utf8(
            command(&context, cmd, Duration::from_secs(5))
                .await
                .unwrap()
                .stdout,
        )
        .unwrap();
        std::fs::write(lab.join("src/main.rs"), "owner edit\n").unwrap();
        let mut candidate = json!({"status":"verified","baseCommit":base,"patch":patch,
            "patchSha256":format!("{:x}",Sha256::digest(patch.as_bytes())),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":false}}});
        assert!(adopt(&context, &lab, &candidate, &base).await.is_err());
        candidate["evaluation"]["metrics"]["eligible"] = json!(true);
        assert!(adopt(&context, &lab, &candidate, &base).await.is_err(),
            "A manually altered eligibility flag cannot authorize adoption without a raw paired receipt");
        assert_eq!(
            git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap(),
            base
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            "owner edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "owner research\n"
        );
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn accepted_source_is_rebuilt_and_really_executes_the_next_generation() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(
            source.join("src/main.rs"),
            include_str!("../../fixtures/rsi-generation-worker.rs"),
        )
        .unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source)
            .args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "controlled worker",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context
            .checkpoint(
                json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "limits":{"maxAttempts":2,"tokenBudget":1000,"timeBudgetSeconds":240}}}),
            )
            .unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(240), run(&context, &job))
            .await
            .unwrap()
            .unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 2);
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        assert_eq!(evolution["history"][0]["domain"], "original_executable");
        assert_eq!(
            evolution["history"][1]["domain"], "improved_executable",
            "a label change alone must not count as recursion"
        );
        assert_eq!(
            evolution["history"][1]["generatorCommit"],
            evolution["history"][0]["acceptedCommit"]
        );
        assert_ne!(
            evolution["history"][1]["generatorBinarySha256"],
            evolution["history"][0]["generatorBinarySha256"]
        );
        let inherited=&evolution["history"][1]["inheritedImplementations"][0];
        assert_eq!(inherited["status"],"available_in_binary");
        assert_eq!(inherited["implementation_ref"]["commit"],evolution["history"][0]["acceptedCommit"]);
        assert_eq!(inherited["binarySha256"],evolution["history"][1]["generatorBinarySha256"]);
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            evolution["headCommit"]
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert!(std::fs::read_to_string(source.join("src/main.rs"))
            .unwrap()
            .contains("n >= 0"));
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn goal_capability_loop_inherits_research_adopts_and_applies_next_decision() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(source.join("src/main.rs"), include_str!("../../fixtures/rsi-generation-worker.rs")).unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source).args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30)).await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(&context, &source, &["-c","user.name=Test","-c","user.email=test@localhost","commit","-qm","controlled worker"]).await.unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context.checkpoint(json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "rsiObjective":"memory capability improvement",
            "rsiFocusFiles":["src/engine/memory.rs"],
            "rsiFixtureMode":"research_then_adopt",
            "limits":{"maxAttempts":4,"tokenBudget":1000,"timeBudgetSeconds":300}}})).unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(300), run(&context, &job)).await.unwrap().unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 4, "{result}");
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        let history = evolution["history"].as_array().unwrap();
        assert_eq!(history.len(), 4);
        assert!(history[0]["candidateId"].is_null());
        assert!(history[0]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[0]["knowledgeId"].as_str().is_some());
        assert_eq!(history[0]["improvementDirective"]["mode"],"discover");
        assert_eq!(history[1]["improvementDirective"]["mode"],"implement_research",
            "A new discovery must produce an implementation directive, not another research-only plan");
        assert_eq!(history[1]["improvementDirective"]["sourceJobId"],history[0]["jobId"]);
        assert_eq!(history[1]["researchDelivered"], true, "next generation did not receive stored research");
        assert_eq!(history[1]["accepted"], true, "{result}");
        assert_eq!(history[1]["decisionInput"]["nextDomain"], "memory",
            "the user goal did not drive the next capability selection");
        let accepted_commit = history[1]["acceptedCommit"].as_str().unwrap();
        assert_eq!(history[2]["generatorCommit"], accepted_commit);
        assert_ne!(history[2]["generatorBinarySha256"], history[1]["generatorBinarySha256"]);
        assert_eq!(history[2]["workerMarker"], "improved_execution");
        assert_eq!(history[2]["improvementDirective"]["mode"],"extend_verified_capability");
        assert_eq!(history[2]["boundaryResult"], false, "the rebuilt function must actually execute");
        assert!(history[2]["candidateId"].is_string());
        assert_eq!(history[2]["accepted"], false, "a comment-only candidate must not be adopted");
        assert!(history[2]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[2]["knowledgeId"].as_str().is_some());
        assert_ne!(history[2]["knowledgeId"], history[0]["knowledgeId"]);
        let inherited = &history[2]["inheritedImplementations"][0];
        assert_eq!(inherited["status"], "available_in_binary");
        assert_eq!(inherited["implementation_ref"]["commit"], accepted_commit);
        assert_eq!(history[2]["decisionInput"]["basis"], "accepted_implementation");
        assert_eq!(history[3]["decisionInput"]["basis"], "research_findings");
        assert_eq!(history[3]["improvementDirective"]["mode"],"revise_candidate");
        assert!(evolution["improvementActionResults"].as_array().is_some(),
            "Every campaign must retain measured strategy outcomes");
        assert_eq!(history[3]["decisionInput"]["observed"]["accepted"], false);
        assert!(history[3]["decisionInput"]["observed"]["tokens"].is_number());
        assert!(history[3]["decisionInput"]["observed"]["elapsedMs"].is_number());
        assert_eq!(history[3]["domain"], history[3]["decisionInput"]["nextDomain"]);
        assert_eq!(history[3]["searchStrategy"]["decisionSource"], "parent_next_decision",
            "the fourth generation did not execute the parent's next decision");
        assert_eq!(history[3]["workerMarker"], "decision_use");
        assert_eq!(evolution["nextDecision"]["basis"], "no_progress");
        assert_eq!(evolution["nextDecision"]["fromGeneration"], 4);
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            accepted_commit
        );
        assert_eq!(git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap(), base);
        for row in history {
            let stored=state.store.get("rsi_generation",row["id"].as_str().unwrap()).unwrap();
            for field in ["newFindings","knowledgeId","knowledgeArtifactHashes","methodExecutions","accepted"] {
                assert_eq!(stored[field],row[field],"next workers must read the completed {field}");
            }
        }
        // The same accepted chain can reach the real test branch through an
        // explicit product action. Untracked research and main are preserved.
        git(&context,&source,&["update-ref","refs/heads/main",&base]).await.unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap_err().contains("작업 브랜치"),"main must never be an application target");
        git(&context,&source,&["checkout","-b","dominion-sub1"]).await.unwrap();
        std::fs::write(source.join("my-research.txt"),"keep my unrelated research").unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        context.checkpoint(json!({"status":"completed"})).unwrap();
        let original=state.store.all("rsi_candidate").into_iter()
            .find(|c|c["experimentAdoption"]["commit"]==accepted_commit).unwrap();
        let mut tampered=original.clone();tampered["patch"]=json!("unverified replacement");
        state.store.put("rsi_candidate",&tampered);
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_err());
        state.store.put("rsi_candidate",&original);
        let mut stopped=state.store.get("rsi_campaign","campaign").unwrap();
        stopped["status"]=json!("cancelled");stopped["acceptedGenerations"]=json!(0);
        state.store.put("rsi_campaign",&stopped);
        // A later stopped campaign may carry an earlier verified selection.
        // No new model generation is needed merely to publish that selection.
        let applied=rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap();
        assert_eq!(applied["commit"],accepted_commit);
        assert_eq!(applied["branch"],"dominion-sub1");
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_ok(),"retry must not apply twice");
        assert_eq!(std::fs::read_to_string(source.join("my-research.txt")).unwrap(),"keep my unrelated research");
        let rev=|reference:&str|String::from_utf8(std::process::Command::new("git").current_dir(&source)
            .args(["rev-parse",reference]).output().unwrap().stdout).unwrap().trim().to_string();
        assert_eq!(rev("HEAD"),accepted_commit);assert_eq!(rev("main"),base);
        context.checkpoint(json!({"status":"running"})).unwrap();
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(&context, &source, &["worktree", "remove", "--force", lab.to_str().unwrap()]).await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    fn generation(domain: &str, accepted: bool) -> Value {
        json!({
            "domain": domain,
            "accepted": accepted,
            "tokens": 10_000,
            "candidateId": if accepted { json!("candidate") } else { Value::Null },
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn active_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_active",
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn trial_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
        candidate: &str,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_trial",
            "searchStrategy": {"id": candidate, "mode": "trial"},
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn search_expression(name: &str, expression: &str) -> Value {
        json!({
            "id": format!("algorithm-{name}"),
            "kind": "selfimprove.search",
            "name": name,
            "variant": name,
            "implementation": "expression",
            "revision": format!("revision-{name}"),
            "parameters": {"expression": expression}
        })
    }

    #[test]
    fn capability_also_requires_measured_cross_domain_transfer() {
        let holdout = |id: &str, domain: &str, improved: bool| json!({
            "id":id,"domain":domain,"baselinePass":!improved,"candidatePass":true,
            "metricGains":0,"metricRegressions":0,"improved":improved
        });
        let verified = |memory_gain: bool, context_gain: bool| json!({
            "accepted":true,"pairedRevalidated":true,"domain":"memory",
            "evaluation":{"candidateStatus":"verified",
                "measurementScope":"frozen_native_cases_only",
                "evidenceSha256":"a".repeat(64),
                "metrics":{"eligible":true,"holdoutCases":2,"regressions":0,
                    "holdoutDomains":["context","memory"],
                    "improvedHoldouts":u64::from(memory_gain)+u64::from(context_gain),
                    "metaGains":0,
                    "holdoutEvidence":[holdout("a","memory",memory_gain),
                        holdout("b","context",context_gain)]}}
        });
        let only_source_gain = verified(true,false);
        assert!(!meta_transfer_proven(&[only_source_gain.clone()]),
            "Two measured domains with only source-domain gain are not transfer");
        let transferred = verified(false,true);
        assert!(meta_transfer_proven(&[transferred.clone()]));
        assert!(!capability_demonstrated(&[
            transferred.clone(),only_source_gain.clone(),only_source_gain.clone()]),
            "Repeated evidence without a new compiled generator chain is not recursion");
        let broken = json!({"accepted":false,"evaluation":{"verdict":"regressed"}});
        assert!(!capability_demonstrated(&[
            transferred,broken,only_source_gain.clone(),
            only_source_gain.clone(),only_source_gain.clone()]),
            "Transfer from an interrupted streak must not prove capability");
        let mut forged = verified(false,true);
        forged["evaluation"]["metrics"]["improvedHoldouts"] = json!(2);
        assert!(!meta_transfer_proven(&[forged]),
            "Summary counts must match individual measured holdouts");
        let mut unproven = verified(false,true);
        unproven["evaluation"]["evidenceSha256"] = Value::Null;
        assert!(!meta_transfer_proven(&[unproven]),
            "A claimed improvement without a paired receipt is not proof");

        let improver_gain = json!({"accepted":true,"pairedRevalidated":true,"domain":"memory",
            "evaluation":{"candidateStatus":"verified",
                "measurementScope":"frozen_native_cases_only",
                "evidenceSha256":"b".repeat(64),
                "metrics":{"eligible":true,"holdoutCases":2,"regressions":0,
                    "holdoutDomains":["improver","memory"],"improvedHoldouts":1,
                    "metaGains":1,"metaHoldoutGains":1,"holdoutEvidence":[
                        holdout("a","memory",false),
                        {"id":"b","domain":"improver","baselinePass":true,
                         "candidatePass":true,"metricGains":1,"metricRegressions":0,
                         "improved":true}]}}});
        assert!(meta_transfer_proven(&[improver_gain]),
            "An actual improver holdout gain supports recursive improvement");
    }

    #[test]
    fn recursive_capability_requires_real_lineage_and_child_domain_gain() {
        let make=|domain:&str,from:&str,to:&str,target:&str|json!({
            "pairedRevalidated":true,"accepted":true,
            "domain":domain,"candidateTarget":target,
            "generatorCommit":from,"acceptedCommit":to,
            "nativePlan":{"source":"compiled_generator_plan_v1"},
            "evaluation":{"candidateStatus":"verified",
                "measurementScope":"frozen_native_cases_only",
                "metrics":{"eligible":true,"holdoutCases":2,"regressions":0,
                    "improvedHoldouts":1,"holdoutEvidence":[
                        {"id":"held","domain":domain,"improved":true}]}}
        });
        let a="a".repeat(40);
        let b="b".repeat(40);
        let c="c".repeat(40);
        let d="d".repeat(40);
        let parent=make("improver",&a,&b,"improver");
        let child=make("context",&b,&c,"harness");
        let grandchild=make("context",&c,&d,"algorithm");
        assert!(capability_demonstrated(&[parent.clone(),child.clone(),grandchild.clone()]));
        let mut copied=child.clone();
        copied["generatorCommit"]=json!(a);
        assert!(!capability_demonstrated(&[parent.clone(),copied,grandchild.clone()]),
            "A copied success without inherited executable lineage is not recursion");
        let mut no_child_gain=child.clone();
        no_child_gain["evaluation"]["metrics"]["holdoutEvidence"]=json!([{
            "id":"held","domain":"improver","improved":true}]);
        assert!(!capability_demonstrated(&[parent.clone(),no_child_gain,grandchild.clone()]),
            "The child must improve a measured case in its new domain");
        let mut no_native_plan=child.clone();
        no_native_plan["nativePlan"]=Value::Null;
        assert!(!capability_demonstrated(&[parent.clone(),no_native_plan,grandchild.clone()]));
        let same_domain=make("improver",&b,&c,"harness");
        assert!(!capability_demonstrated(&[parent,same_domain,grandchild]));
    }

    #[test]
    fn forged_generation_flag_cannot_replace_missing_original_paired_receipt() {
        let (state,root)=state();
        let forged=json!({"accepted":true,"pairedRevalidated":true,
            "campaignId":"fake","candidateId":"missing-candidate",
            "candidateTarget":"improver",
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40),
            "evaluation":{"candidateStatus":"verified","measurementScope":"frozen_native_cases_only",
                "evidenceSha256":"c".repeat(64),
                "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}}});
        assert!(generation_is_verified(&forged),"fixture pretends to be verified");
        let audited=revalidated_history(&state,&[forged.clone(),forged.clone(),forged]);
        assert!(audited.iter().all(|generation|generation["pairedRevalidated"]==false));
        assert_eq!(verified_capability_streak(&audited),(0,0));
        assert!(!capability_demonstrated(&audited));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_capability_requires_measured_holdout_evidence() {
        let verified=json!({"accepted":true,"pairedRevalidated":true,"evaluation":{
            "candidateStatus":"verified","measurementScope":"frozen_native_cases_only",
            "metrics":{"eligible":true,"holdoutCases":2,"regressions":0}}});
        let regressed=json!({"accepted":true,"evaluation":{"metrics":{"eligible":true,"holdoutCases":2,"regressions":1}}});
        assert_eq!(verified_capability_streak(&[verified.clone(),verified.clone(),verified.clone()]),(3,3));
        assert_eq!(verified_capability_streak(&[verified.clone(),regressed.clone()]),(0,1));
        let research_only=json!({"accepted":false,"newFindings":5,"evaluation":null});
        assert_eq!(verified_capability_streak(&[
            verified.clone(),research_only.clone(),verified.clone()
        ]),(1,1),"Research-only generations interrupt consecutive verified adoption");
        assert_eq!(verified_capability_streak(&[
            verified.clone(),research_only.clone(),verified.clone(),verified.clone()
        ]),(2,2),"Research-only generations must not be skipped in max streak");
        assert_eq!(verified_capability_streak(&[
            verified.clone(),research_only,verified.clone(),verified.clone(),verified.clone()
        ]),(3,3),"New independent uninterrupted streaks still count");
        assert_eq!(verified_capability_streak(&[json!({"accepted":true})]),(0,0));
        assert!(target_plan_mismatch("rsi_target_plan_mismatch: mismatch"));
        assert!(!target_plan_mismatch("rsi_token_budget"));
    }

    #[test]
    fn empty_model_error_does_not_abort_a_campaign() {
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":""}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":"  "}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{}}})), None);
        assert_eq!(
            generation_model_error(&json!({"result":{"rsi":{"modelError":"rsi_token_budget: stop"}}})),
            Some("rsi_token_budget: stop")
        );
    }

    #[tokio::test]
    async fn source_snapshot_fingerprint_tracks_the_commit() {
        let (state, root) = state();
        state.store.put("job", &json!({"id":"snapshot-test","status":"running","cancelRequested":false}));
        let source = root.join("source-repo");
        std::fs::create_dir_all(&source).unwrap();
        let run_git = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&source)
                .args(["-c","user.name=test","-c","user.email=test@localhost","-c","commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(run_git(&["init","-q"]).status.success());
        std::fs::write(source.join("value.txt"), "one").unwrap();
        assert!(run_git(&["add","."]).status.success());
        assert!(run_git(&["commit","-q","-m","one"]).status.success());
        let lab = root.join("lab");
        std::fs::create_dir_all(&lab).unwrap();
        std::process::Command::new("git").current_dir(&lab).args(["init","-q"]).output().unwrap();
        let context = crate::engine::jobs::JobContext::new(state.clone(), "snapshot-test".into());
        let (_, first) = snapshot_source(&context, &source, &lab).await.unwrap();
        std::fs::write(source.join("value.txt"), "two").unwrap();
        assert!(run_git(&["commit","-qam","two"]).status.success());
        let (_, second) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(lineage_fingerprint_valid(&first), "a clean checkout must not hash to the empty digest");
        assert_ne!(first, second, "a new commit must change the lineage fingerprint");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lineage_fingerprint_identifies_the_source_commit() {
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let first = lineage_fingerprint("a".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        let second = lineage_fingerprint("c".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        assert_eq!(first.len(), 64);
        assert_ne!(first, empty_hash, "a clean checkout still has a commit and tree");
        assert_ne!(first, second, "different commits must not share a fingerprint");
        assert!(lineage_fingerprint_valid(&first));
        assert!(!lineage_fingerprint_valid(empty_hash));
        assert!(!lineage_fingerprint_valid(""));
    }

    #[test]
    fn autonomous_default_objective_does_not_constrain_the_domain() {
        let generic=json!({"rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다."});
        assert_eq!(selection_objective(&generic),"");
        assert_eq!(select_domain(&json!({"implementation":"failure-clusters"}),&[],""),"memory",
            "an autonomous goal must consider the full declared domain order");
        let explicit=json!({"rsiAutonomous":false,"rsiObjective":"기억 검색 정확도"});
        assert_eq!(selection_objective(&explicit),"기억 검색 정확도");
        assert_eq!(select_domain(&json!({"implementation":"failure-clusters"}),&[],&selection_objective(&explicit)),"memory");
    }

    #[test]
    fn korean_goals_select_the_requested_capability_without_focus_files() {
        let algorithm=json!({"implementation":"failure-clusters"});
        for (goal,domain) in [
            ("오래된 사실을 기억하고 회상해","memory"),
            ("대화 토큰 비용을 줄여","context"),
            ("외부 지식을 학습해","learning"),
            ("브라우저 검색 도구를 연결해","tools"),
            ("버튼과 화면을 단순하게 해","ui"),
            ("자기개선 알고리즘을 발전시켜","improver"),
        ] {assert_eq!(select_domain(&algorithm,&[],goal),domain,"{goal}");}
        assert!(!objective_matches("ui","build the service"));
    }

    #[tokio::test]
    async fn learned_method_uses_real_tools_and_failed_postconditions_change_reuse() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        // This local provider only substitutes model decisions. The production
        // runtime must read the research, register a DAG, bind results, execute
        // tools, verify effects and update the following decision itself.
        async fn provider(Json(body):Json<Value>)->axum::response::Response {
            let messages=body["messages"].as_array().unwrap();
            let last=messages.last().unwrap();
            let request=messages.iter().rev().find(|m|m["role"]=="user")
                .and_then(|m|m["content"].as_str()).unwrap_or("");
            let mut call=None;
            if request.contains("학습 조사") {
                if last["role"]=="tool" {
                    let result:Value=serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
                    if let Some(content)=result["content"].as_str() {
                        // workspace_read_range returns raw source, never a
                        // line-number-prefixed snippet. Parse what the real
                        // tool now delivers to the learning loop.
                        let knowledge:Value=serde_json::from_str(content.trim()).unwrap();
                        call=Some(("rsi_method_propose",json!({"name":"연구에서 합성한 자료 처리","domain":"context",
                            "description":knowledge["principle"],
                            "verification":{"type":"procedure","appliesTo":{"requestContains":["대화"]},
                                "nodes":[
                                    {"id":"source","tool":"workspace_read_range","arguments":{"path":knowledge["input"],"startLine":1,"endLine":2}},
                                    {"id":"derive","tool":"workspace_write","arguments":{"path":knowledge["output"],"content":"$source.content"}}
                                ],"edges":[{"from":"source","to":"derive"}],
                                "after":[{"kind":"file_contains","path":knowledge["output"],"text":knowledge["needle"]}]}})));
                    }
                } else {
                    call=Some(("workspace_read_range",json!({"path":"src/recipe.json","startLine":1,"endLine":1})));
                }
            }
            let (delta,finish)=if let Some((name,args))=call {
                (json!({"tool_calls":[{"index":0,"id":format!("call-{name}"),"type":"function",
                    "function":{"name":name,"arguments":args.to_string()}}]}),"tool_calls")
            }else{(json!({"content":"작업 완료"}),"stop")};
            let events=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],events).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {axum::serve(listener,Router::new().route("/chat/completions",post(provider))).await.unwrap()});
        let (state,root)=state();
        let _parent=parent(&state);
        std::fs::create_dir_all(state.config.workspace.join("src")).unwrap();
        let marker=format!("learned-{}",uuid::Uuid::new_v4());
        std::fs::write(state.config.workspace.join("src/input.txt"),&marker).unwrap();
        std::fs::write(state.config.workspace.join("src/recipe.json"),json!({
            "input":"src/input.txt","output":"src/derived.txt","needle":marker,
            "principle":"읽은 결과를 다음 도구의 입력으로 연결한다"}).to_string()).unwrap();
        state.store.put("provider",&json!({"id":"local-test","kind":"openai","baseUrl":format!("http://{address}"),
            "model":"test","toolMode":"native","enabled":true}));
        let input=json!({"rsiObjective":"대화 자료 처리 능력","rsiDomain":"context",
            "rsiKnowledgeSource":state.config.workspace,"rsiSourceDir":state.config.workspace});
        for (index,session_id) in ["learn","use","counterexample","after-failure"].into_iter().enumerate() {
            state.store.put("job",&json!({"id":session_id,"kind":"rsi_generation","status":"running",
                "rsiWorkspace":state.config.workspace,"input":{"rsiCampaignId":"campaign"}}));
            let mut session=json!({"id":session_id,"providerId":"local-test","rsiRun":true,
                "rsiObjective":input["rsiObjective"],"rsiCampaignId":"campaign","rsiGenerationJobId":session_id});
            configure_method_session(&state,&input,&mut session);
            state.store.put("session",&session);
            if index==2 {std::fs::write(state.config.workspace.join("src/input.txt"),"changed premise").unwrap();}
            let request=crate::engine::chat::TurnRequest::from_value(&json!({"sessionId":session_id,
                "input":if index==0{"대화 방법을 학습 조사해"}else{"대화 자료를 처리해"},
                "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false})).unwrap();
            tokio::time::timeout(Duration::from_secs(10),crate::engine::run_quiet(&state,&request))
                .await.unwrap().unwrap();
            collect_method_executions(&state,session_id,&state.config.workspace);
            let generation=json!({"jobId":session_id,"accepted":false,"domain":"context","newFindings":1,"tokens":20,"elapsedMs":1});
            if index==0 {
                let id=remember_research(&state,&input,&generation,session_id).expect("research and executable proposal must persist");
                let record=state.store.get("experience",&id).unwrap();
                assert_eq!(record["executableMethods"].as_array().unwrap().len(),1);
                assert!(!record["researchArtifacts"].as_array().unwrap().is_empty());
                assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
            }else {
                let receipts=record_method_feedback(&state,&generation,session_id);
                if index<3 {
                    assert_eq!(receipts.len(),1,"actual engine procedure invocation missing");
                    assert_eq!(receipts[0]["operationIds"].as_array().unwrap().len(),2);
                    assert_eq!(receipts[0]["result"]["followed"],index==1,"{receipts:?}");
                }else{assert!(receipts.is_empty(),"failed method must not run again");}
                if index==1 {
                    assert!(std::fs::read_to_string(state.config.workspace.join("src/derived.txt")).unwrap().contains(&marker));
                    assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
                    // A different repository and an explicit no-reuse decision cannot inject it.
                    let mut probe=session.clone();probe["id"]=json!("not-yet-executed");
                    assert_eq!(method_strategies(&state,&probe).len(),1);
                    let mut foreign=probe.clone();foreign["rsiResearchPartition"]=json!("other");
                    assert!(method_strategies(&state,&foreign).is_empty());
                    foreign=probe.clone();foreign["rsiDecisionInput"]["reuseDecision"]["action"]=json!("research_only");
                    assert!(method_strategies(&state,&foreign).is_empty());
                }else{assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"research_only");}
            }
        }
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_method_unknown_verification_is_held_not_failed() {
        let (state,root)=state();
        let workspace=&state.config.workspace;
        std::fs::create_dir_all(workspace.join("unreadable.txt")).unwrap();
        std::fs::write(workspace.join("known.txt"),"ok").unwrap();
        let evaluate=|after:Value| crate::engine::procedure::evaluate(
            &json!({"id":"method","verification":{"type":"procedure","after":after}}),&[],workspace);
        let unknown=evaluate(json!([{"kind":"file_equals","path":"unreadable.txt","text":"ok"}]));
        assert_eq!(unknown["verified"],true);
        assert_eq!(unknown["unknown"],true);
        let mut status_only=unknown.clone();status_only["unknown"]=Value::Null;
        let violated=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        let mixed=evaluate(json!([
            {"kind":"file_equals","path":"unreadable.txt","text":"ok"},
            {"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        assert_eq!(mixed["unknown"],true);
        let passed=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"ok"}]));
        for (index,(result,status,verdict,postcondition)) in [
            (unknown,"held","inconclusive",Value::Null),
            (status_only,"held","inconclusive",Value::Null),
            (violated,"suspended","violated",json!(false)),
            (mixed,"suspended","violated",json!(false)),
            (passed,"executable","passed",json!(true)),
        ].into_iter().enumerate() {
            let id=format!("experience-{index}");
            state.store.put("experience",&json!({"id":id,"executableMethods":[
                {"id":"method","revision":"r1","status":"candidate","successfulApplications":0}]}));
            let session=format!("s-{index}");
            state.store.put("session",&json!({"id":session,"rsiMethodExecutions":[{
                "methodId":"method","revision":"r1","experienceId":id,
                "executionId":format!("exec-{index}"),"result":result}]}));
            let generation=json!({"jobId":"child","accepted":true,"newFindings":1});
            record_method_feedback(&state,&generation,&session);
            record_method_feedback(&state,&generation,&session);
            let record=state.store.get("experience",&id).unwrap();
            let method=&record["executableMethods"][0];
            assert_eq!(method["status"],status);
            assert_eq!(method["applications"].as_array().unwrap().len(),1);
            assert_eq!(method["applications"][0]["verificationVerdict"],verdict);
            assert_eq!(method["applications"][0]["postconditionsPassed"],postcondition);
            assert_eq!(method["successfulApplications"],usize::from(verdict=="passed"));
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_method_proposals_require_running_rsi_and_executable_verification() {
        let (state,root)=state();
        let _context=parent(&state);
        state.store.put("session",&json!({"id":"s","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"child"}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running","input":{"rsiCampaignId":"campaign"}}));
        let valid=json!({"name":"읽기 절차","domain":"context","verification":{
            "type":"procedure","appliesTo":{"requestContains":["대화"]},
            "nodes":[{"id":"read","tool":"workspace_read_range","arguments":{"path":"src/a.rs","startLine":1}}],
            "after":[{"kind":"tool_called","tool":"workspace_read_range","path":"src/a.rs"}]}});
        assert!(propose_method(&state,"missing",&valid).is_err());
        let mut invalid=valid.clone();
        invalid["verification"]["nodes"][0]["tool"]=json!("shell_exec");
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["nodes"][0]["tool"]=json!("rsi_method_propose");
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["after"]=json!([]);
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["after"]=json!([{"kind":"file_contains","path":"src/a.rs"}]);
        assert!(propose_method(&state,"s",&invalid).is_err());
        let first=propose_method(&state,"s",&valid).unwrap();
        assert_eq!(first["method"]["status"],"candidate");
        propose_method(&state,"s",&valid).unwrap();
        assert_eq!(state.store.get("session","s").unwrap()["rsiMethodProposals"].as_array().unwrap().len(),1);
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"cancelled"}));
        assert!(propose_method(&state,"s",&valid).is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_selection_follows_the_user_goal_before_unrelated_domains() {
        let baseline = json!({"implementation":"failure-clusters","parameters":{}});
        assert_eq!(select_domain(&baseline, &[], "improver"), "improver");
        assert_eq!(select_domain(&baseline, &[], "memory"), "memory");
        assert_eq!(select_domain(&baseline, &[], "unrelated capability"), "memory");
    }

    #[test]
    fn replay_reports_match_rate_only_and_never_contains_promotion() {
        let baseline = json!({"id":"baseline","revision":"b","implementation":"failure-clusters","parameters":{}});
        let candidate = search_expression("accepted-first", "accepted * 10 + novelty * 0.1");
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let report = evaluate_search_algorithm(&candidate, &baseline, &history, "unrelated");
        assert_eq!(report["metric"], "past_successful_domain_match_rate");
        assert_eq!(report["samples"], 2);
        assert_eq!(report["candidateMatches"], 2);
        assert!(report["candidateMatchRate"].as_f64().unwrap()
            > report["activeMatchRate"].as_f64().unwrap());
        assert!(report.get("promote").is_none());
        assert!(report.get("recommendation").is_none());

        let sparse = &history[..2];
        let report = evaluate_search_algorithm(&candidate, &baseline, sparse, "context");
        assert_eq!(report["samples"], 1);
        assert_eq!(report["evaluable"], false);
    }

    #[test]
    fn replay_match_queues_a_trial_without_activating_it() {
        let (state, root) = state();
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let queued = queue_search_trial(
            &state,
            &candidate,
            &active_before,
            &history,
            "context",
            "campaign",
            1,
        );
        assert_eq!(queued["status"], "pending_trial");
        assert_eq!(queued["replay"]["samples"], 2);
        let saved = state.store.get("algorithm", candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["status"], "pending_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"],
            "replay match must not replace the global strategy"
        );
        assert!(state.store.all("algorithm_eval").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_campaigns_cannot_overwrite_another_generators_binary() {
        let root=PathBuf::from("/tmp/dominion-generator-build-test");
        let first=generator_target_dir(&root,"campaign-one");
        let second=generator_target_dir(&root,"campaign-two");
        assert_ne!(first,second);
        assert!(first.starts_with(root.join("rsi-build/generator")));
        assert!(second.starts_with(root.join("rsi-build/generator")));
        assert_eq!(first,generator_target_dir(&root,"campaign-one"));
        assert!(!generator_target_dir(&root,"../main").to_string_lossy().contains("../main"));
    }

    #[test]
    fn new_generator_plans_its_own_domain_without_running_a_model() {
        let (state,root)=state();
        let commit="a".repeat(40);
        state.store.put("job",&json!({"id":"native-plan","kind":"rsi_evolve",
            "status":"running","cancelRequested":false,
            "input":{"rsiObjective":"Improve memory retrieval","rsiFocusFiles":["src/engine/memory.rs"]}}));
        let mut campaign=json!({"id":"native-plan","headCommit":commit,"history":[],
            "planAttempt":1,"planSelectedDomain":null,
            "nextDecision":{"nextDomain":"tools","basis":"parent_recommendation"},
            "planTrialAlgorithmId":null,"planIndexId":"index-revision"});
        state.store.put("rsi_campaign",&campaign);
        let job=state.store.get("job","native-plan").unwrap();
        let plan=native_generation_plan(&state,&campaign,&job,&commit).unwrap();
        assert_eq!(plan["domain"],"memory");
        assert_ne!(plan["domain"],campaign["nextDecision"]["nextDomain"],
            "The improved generator must be free to revise its parent's recommendation");
        assert_eq!(plan["source"],"compiled_generator_plan_v1");
        assert_eq!(plan["targetPlan"]["generatorCommit"],commit);
        assert_eq!(plan["rankedFiles"],json!([]));
        assert!(native_generation_plan(&state,&campaign,&job,&"b".repeat(40)).is_err(),
            "a different executable cannot claim a generation plan");
        campaign["planAttempt"]=json!(2);
        assert!(native_generation_plan(&state,&campaign,&job,&commit).is_err(),
            "a stale generation counter cannot silently reuse a plan");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn coordinator_and_worker_share_continuation_plan_even_when_a_trial_is_pending() {
        let (state, root)=state();
        let proposed=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"continuation-safety-trial",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration",
                "targetExpression":"bodyEvidence + pathEvidence"}
        })).unwrap();
        let id=proposed["id"].as_str().unwrap();
        let history=vec![
            active_generation("prior",1,"memory",false,"commit-1",10000,1000),
            json!({"attempt":2,"domain":"memory","candidateId":null,
                "newFindings":1,"accepted":false,"generatorCommit":"commit-1"})
        ];
        let continuation=research_continuation(&history);
        assert_eq!(continuation.as_deref(),Some("memory"));
        let (parent_domain,parent_mode,parent_strategy)=select_generation_strategy(
            &state,&history,"context",continuation.clone(),Some(id.into()),
            Some("context".into()),"commit-1");
        let (worker_domain,worker_mode,worker_strategy)=select_generation_strategy(
            &state,&history,"context",research_continuation(&history),Some(id.into()),
            Some("context".into()),"commit-1");
        let plan=generation_target_plan(&parent_domain,parent_mode,&parent_strategy,"commit-1");
        assert_eq!(plan,generation_target_plan(
            &worker_domain,worker_mode,&worker_strategy,"commit-1"));
        assert_eq!(plan["domain"],"memory");
        assert_eq!(plan["selection"],"research_continuation");
        assert!(plan["strategy"].is_null(),"trial formula was not run");
        assert_ne!(plan,generation_target_plan(
            "context","algorithm_trial",&proposed,"commit-1"));
        let mut failed=history.clone();
        failed.push(json!({"attempt":3,"domain":"memory","candidateId":null,
            "newFindings":0,"accepted":false}));
        assert!(research_continuation(&failed).is_none(),
            "stale research must not force continuation");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_runtime_domain_policy_preserves_owner_scope_and_never_counts_as_trial() {
        let (state,root)=state();
        let expression=json!({"implementation":"expression",
            "parameters":{"expression":"1 / attempts"}});
        let history=vec![];
        assert_eq!(select_domain(&expression,&history,"브라우저 도구"),"tools",
            "zero-attempt division must not redirect a tools goal to memory");
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"runtime-div-zero",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"1 / attempts"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        let (domain,mode,strategy)=select_generation_strategy(
            &state,&history,"브라우저 도구",None,Some(id.into()),None,"rev");
        assert_eq!(domain,"tools");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(strategy["trialFallback"],
            "candidate_expression_invalid_for_runtime_features");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_target_formula_cannot_count_as_improved_search_or_activate_itself() {
        let (state,root)=state();
        let before=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"invalid-live-target",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration",
                "targetExpression":"1 / pathEvidence"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        let generation=json!({"jobId":"live-trial","attempt":2,
            "domainSelection":"algorithm_trial","domain":"memory",
            "focusSelection":{"source":"indexed_evidence",
                "policyRevision":candidate["revision"],
                "candidates":[{"scoring":"corpus-evidence",
                    "policyError":"division_by_zero"}]},
            "accepted":true,"tokens":1000,"elapsedMs":100,
            "evaluation":{"verdict":"improved"}});
        let outcome=record_trial_outcome(&state,id,&generation,&[]);
        assert_eq!(outcome["reason"],"target_formula_not_executed");
        let saved=state.store.get("algorithm",id).unwrap();
        assert_eq!(saved["rsiTrial"]["status"],"trial_inconclusive");
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            before["id"],"a fallback score must not be credited to an AI-authored formula");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn generation_selection_distinguishes_continuation_trial_and_active() {
        let (state, root) = state();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        crate::engine::algorithms::activate(&state, candidate["id"].as_str().unwrap()).unwrap();
        let id = candidate["id"].as_str().unwrap();

        let (domain, mode, strategy) = select_generation_strategy(
            &state,
            &[],
            "context",
            Some("memory".to_string()),
            Some(id.to_string()),
            None,
            "commit-1",
        );
        assert_eq!(domain, "memory");
        assert_eq!(mode, "research_continuation");
        assert!(strategy.is_null(), "continuation did not run the formula");

        let (_, mode, strategy) =
            select_generation_strategy(&state, &[], "context", None, Some(id.to_string()), None, "commit-1");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["trialFallback"], "unused_comparable_baseline_missing");

        let baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let (_, mode, _) = select_generation_strategy(
            &state, &[baseline.clone()], "context", None, Some(id.to_string()), None, "commit-2");
        assert_eq!(mode, "algorithm_active", "another generator's result is not a control");
        let (domain, mode, strategy) = select_generation_strategy(
            &state, &[baseline], "context", None, Some(id.to_string()), None, "commit-1");
        assert_eq!(domain, "context");
        assert_eq!(mode, "algorithm_trial");
        assert_eq!(strategy["id"], id);
        assert_eq!(strategy["mode"], "trial");

        let (domain, mode, strategy) = select_generation_strategy(
            &state, &[], "context", None, None, Some("learning".to_string()), "commit-1");
        assert_eq!(domain, "learning");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["decisionSource"], "parent_next_decision");

        let (_, mode, strategy) =
            select_generation_strategy(&state, &[], "context", None, None, None, "commit-1");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["mode"], "active");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trial_verdict_requires_success_before_supporting_cost_savings() {
        let mut baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let mut trial = trial_generation("trial", 2, "context", false, "commit-1", 5_000, 500, "candidate");
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive",
            "cheaper non-results are not successful improvement");
        baseline["error"] = json!("failed");
        trial["error"] = json!("failed");
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive");
        // Even an inconsistent accepted flag cannot turn execution errors into support.
        baseline["accepted"] = json!(true);
        trial["accepted"] = json!(true);
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive");
        baseline["error"] = Value::Null;
        assert_eq!(trial_verdict(&trial, &baseline).0, "rejected");
        trial["error"] = Value::Null;
        assert_eq!(trial_verdict(&trial, &baseline).0, "supported");
        trial["evaluation"]["verdict"] = json!("regressed");
        assert_eq!(trial_verdict(&trial, &baseline).0, "rejected");
    }

    #[test]
    fn trial_outcome_needs_a_comparable_baseline_and_two_supported_trials() {
        let (state2, root2) = state();
        let (state, root) = state();
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        queue_search_trial(
            &state,
            &candidate,
            &active_before,
            &[],
            "context",
            "campaign",
            1,
        );

        let baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let first = trial_generation("trial-1", 2, "context", true, "commit-1", 9_000, 900, id);
        let outcome = record_trial_outcome(&state, id, &first, &[baseline.clone()]);
        assert_eq!(outcome["verdict"], "supported");
        assert_eq!(outcome["status"], "pending_second_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"],
            "one trial must not activate the candidate"
        );

        let repeated = record_trial_outcome(&state, id, &first, &[baseline.clone()]);
        assert_eq!(repeated["alreadyRecorded"], true);
        assert_eq!(repeated["status"], "pending_second_trial");
        let (_, mode, strategy) = select_generation_strategy(
            &state, &[baseline.clone(), first.clone()], "context", None, Some(id.into()), None, "commit-1");
        assert_eq!(mode, "algorithm_active", "a second trial must not reuse the first control");
        assert_eq!(strategy["trialFallback"], "unused_comparable_baseline_missing");
        let saved = state.store.get("algorithm", id).unwrap();
        assert_eq!(saved["rsiTrial"]["trials"].as_array().unwrap().len(), 1);
        assert!(comparable_trial_baseline(&[baseline.clone()], &first, &saved).is_none());

        let baseline2 = active_generation("base-2", 3, "context", false, "commit-1", 10_000, 1_000);
        let (_, mode, _) = select_generation_strategy(
            &state, &[baseline.clone(), first.clone(), baseline2.clone()], "context", None, Some(id.into()), None, "commit-1");
        assert_eq!(mode, "algorithm_trial", "a fresh control permits the pending trial");
        let second = trial_generation("trial-2", 4, "context", true, "commit-1", 9_000, 900, id);
        let outcome = record_trial_outcome(&state, id, &second, &[baseline.clone(), first.clone(), baseline2]);
        assert_eq!(outcome["verdict"], "supported");
        assert_eq!(outcome["status"], "adopted");
        let saved = state.store.get("algorithm", id).unwrap();
        assert_eq!(saved["rsiTrial"]["trials"].as_array().unwrap().len(), 2);
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            id
        );

        // A trial with no active-strategy baseline under the same commit and
        // domain stays inconclusive and never activates.
        let active2 = crate::engine::algorithms::active(&state2, "selfimprove.search").unwrap();
        let candidate2 = crate::engine::algorithms::register(
            &state2,
            &json!({
                "kind":"selfimprove.search","name":"objective-first-2",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let id2 = candidate2["id"].as_str().unwrap();
        queue_search_trial(&state2, &candidate2, &active2, &[], "context", "campaign", 1);
        let unmatched = trial_generation("trial-x", 2, "context", true, "commit-9", 1, 1, id2);
        let outcome = record_trial_outcome(&state2, id2, &unmatched, &[]);
        assert_eq!(outcome["status"], "inconclusive");
        let repeated = record_trial_outcome(&state2, id2, &unmatched, &[]);
        assert_eq!(repeated["alreadyRecorded"], true);
        assert_eq!(state2.store.get("algorithm", id2).unwrap()["rsiTrial"]["trials"].as_array().unwrap().len(), 1);
        assert_eq!(
            crate::engine::algorithms::active(&state2, "selfimprove.search").unwrap()["id"],
            active2["id"]
        );

        // A research continuation is not a trial even when a candidate was queued.
        let mut continuation = trial_generation("trial-c", 2, "context", true, "commit-1", 1, 1, id);
        continuation["domainSelection"] = json!("research_continuation");
        let outcome = record_trial_outcome(&state2, id2, &continuation, &[]);
        assert_eq!(outcome["status"], "not_used");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(root2);
    }

    #[test]
    fn proposed_algorithm_is_queued_not_activated() {
        let (state, root) = state();
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true,"rsiCampaignId":"campaign"}))
            .unwrap();
        state
            .store
            .put("job", &json!({"id":"campaign","kind":"rsi_evolve","status":"running"}))
            .unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op","sessionId":"s","tool":"algorithm_propose","status":"completed",
                    "result":{"algorithmId":candidate["id"]}}),
            )
            .unwrap();
        let proposals = proposed_algorithms(&state, "s");
        assert_eq!(proposals.len(), 1);
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let queued = queue_search_trial(
            &state,
            &proposals[0],
            &active_before,
            &history,
            "context",
            "campaign",
            1,
        );
        assert_eq!(queued["status"], "pending_trial");
        let saved = state.store.get("algorithm", candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["status"], "pending_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn independently_measured_native_plateau_can_try_another_operator() {
        let input=json!({"rsiNativeInduction":true,"selectionPolicy":"automatic"});
        let neutral=json!({"origin":"native_inductive_source_mutation",
            "status":"inconclusive"});
        assert!(native_plateau_prefers_next_operator(&input,&neutral));
        let model=json!({"origin":"model_driven_detached_worktree",
            "status":"inconclusive"});
        assert!(!native_plateau_prefers_next_operator(&input,&model));
        let failed=json!({"origin":"native_inductive_source_mutation",
            "status":"proposed"});
        assert!(!native_plateau_prefers_next_operator(&input,&failed));
        let mut explicit=input.clone();
        explicit["selectionPolicy"]=json!("manual");
        assert!(!native_plateau_prefers_next_operator(&explicit,&neutral));
    }

    #[test]
    fn pending_trial_candidate_survives_for_a_later_scheduled_generation() {
        let (state, root) = state();
        let active = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search","name":"objective-first-pending",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        queue_search_trial(&state, &candidate, &active, &[], "context", "campaign", 1);
        assert_eq!(
            pending_trial_candidate(&state, "context").unwrap()["id"],
            candidate["id"],
            "a pending candidate is reused by a later scheduled generation"
        );
        assert!(
            pending_trial_candidate(&state, "a different objective").is_none(),
            "a pending candidate is scoped to its objective"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
