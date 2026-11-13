    #[test]
    fn inherited_repair_method_controls_the_descendant_target_and_edit_stage() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/memory.rs"),"fn memory() {}\n").unwrap();
        std::fs::write(root.join("src/engine/other.rs"),"fn unrelated() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut input=json!({"rsiAutoFocus":true,"rsiIndexId":index["id"],
            "rsiSelectedDomain":"memory","rsiFocusFiles":["src/engine/other.rs"],
            "rsiActionDirective":{"mode":"extend_verified_capability"},
            "rsiDecisionInput":{"methodRecoveryCandidateId":"derived-method",
                "reuseDecision":{"action":"execute_methods",
                    "methodIds":["derived-method"],
                    "repairMethodId":"derived-method",
                    "repairTarget":"src/engine/memory.rs"}}});
        assert_eq!(pending_method_recovery_domain(&input),Some("memory".into()));
        assert!(bind_action_to_source(&state,&mut input,&[]));
        assert_eq!(input["rsiFocusFiles"],json!(["src/engine/memory.rs"]));
        assert_eq!(input["rsiFocusSelection"]["executionSource"],"learned_executable_method");
        assert!(!policy_requests_source_edit(&input));
        input["rsiFocusContextChars"]=json!(120);
        input["rsiFocusGrounded"]=json!(true);
        assert!(policy_requests_source_edit(&input),
            "the actual descendant runtime must enforce its learned method");
        input["rsiDecisionInput"]["reuseDecision"]["methodIds"]=json!([]);
        assert!(!policy_requests_source_edit(&input),
            "a method absent from the parent's actual reuse decision cannot gain control");
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
    fn rsi_target_ranking_excludes_oracle_without_hiding_normal_index() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/rsi_oracle_fixture.rs"),
            "fn memory_holdout_secret() {}\n").unwrap();
        std::fs::write(root.join("src/engine/visible_memory.rs"),
            "fn memory_retrieve() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let all=state.store.all("code_index_file");
        assert!(all.iter().any(|f|f["path"]=="src/engine/rsi_oracle_fixture.rs"),
            "normal code tools still index the complete source");
        let ranked=choose_focus_files(&state,&json!({
            "rsiIndexId":index["id"],"rsiObjective":"memory holdout",
            "rsiTargetDomain":"memory"
        }),&[],None);
        assert!(ranked.iter().all(|row|
            !crate::engine::rsi::sealed_measurement_path(row["path"].as_str().unwrap_or(""))));
        assert_eq!(ranked[0]["path"],"src/engine/visible_memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn descendant_replans_source_excerpts_and_preserves_coordinator_receipt() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/irrelevant.rs"),"fn arbitrary() {}\n").unwrap();
        std::fs::write(root.join("src/retrieval.rs"),
            "fn memory_retrieve() { println!(\"memory retrieval\"); }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut job=json!({"input":{
            "rsiSourceDir":root,"rsiIndexId":index["id"],"rsiAutoFocus":true,
            "rsiObjective":"memory retrieval","rsiTargetDomain":"tools",
            "rsiFocusFiles":["src/irrelevant.rs"],
            "rsiPromptBase":"Improve the native harness",
            "rsiPromptEvidence":"inherited evidence",
            "rsiPromptResearch":"reusable research",
            "rsiTargetPlan":generation_target_plan("tools","algorithm_active",&json!({}),"revision"),
            "rsiRemainingCampaignTokens":null
        }});
        assert!(reconcile_descendant_target(&state,&mut job,&[],"memory",
            "algorithm_active",&json!({}),"revision").unwrap());
        assert_eq!(job["input"]["rsiTargetPlan"]["domain"],"memory");
        assert_eq!(job["rsiTargetReplan"]["coordinatorPlan"]["domain"],"tools");
        assert_eq!(job["input"]["rsiFocusFiles"][0],"src/retrieval.rs");
        let prompt=job["input"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("memory_retrieve"));
        assert!(!prompt.contains("fn arbitrary()"));
        assert!(prompt.contains("inherited evidence"));
        assert!(!reconcile_descendant_target(&state,&mut job,&[],
            "memory","algorithm_active",&json!({}),"revision").unwrap());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn descendant_replan_refuses_to_overrun_model_budget() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/target.rs"),"fn memory_retrieve() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut job=json!({"input":{
            "rsiSourceDir":root,"rsiIndexId":index["id"],"rsiAutoFocus":true,
            "rsiObjective":"memory","rsiFocusFiles":[],
            "model":"large","providerId":"cloud","rsiRemainingCampaignTokens":1,
            "rsiPromptBase":"must keep base",
            "rsiPromptEvidence":"must keep lineage",
            "rsiPromptResearch":"inherited research",
            "rsiTargetPlan":generation_target_plan("tools","algorithm_active",&json!({}),"rev")
        }});
        let error=reconcile_descendant_target(&state,&mut job,&[],"memory",
            "algorithm_active",&json!({}),"rev").unwrap_err();
        assert!(error.starts_with("rsi_token_budget:"));
        assert!(job["input"]["rsiTargetPlan"]["domain"]=="tools",
            "a failed replan never claims the descendant plan was executed");
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
        assert_eq!(strategy["trialFallback"], "collecting_same_domain_control");

        let mut baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        baseline["searchStrategy"]["pendingCandidateId"]=json!(id);
        baseline["searchStrategy"]["pendingCandidateRevision"]=candidate["revision"].clone();
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

    // Deterministic frozen receipts, independent of candidate adoption flags.
    fn with_frozen_trial_receipts(baseline:&mut Value,trial:&mut Value) {
        for generation in [&mut *baseline, &mut *trial] {
            generation["campaignId"]=json!("campaign");
            generation["objectiveId"]=json!("same-frozen-objective");
            generation["generatorModel"]=json!("test-model");
            generation["generatorProviderId"]=json!("test-provider");
        }
        let cases=|policy:&str| (0..8).map(|i|{
            let won=policy=="trial" && i==0;
            json!({"id":format!("frozen-{i}"),"domain":"context","holdout":i<4,
                "baselinePass":true,"candidatePass":if i==0 {won}else{true},
                "metrics":[]})
        }).collect::<Vec<_>>();
        baseline["evaluation"]["metrics"]["measurements"]=json!(cases("control"));
        baseline["searchStrategy"]["pendingCandidateId"]=trial["searchStrategy"]["id"].clone();
        trial["evaluation"]["metrics"]["measurements"]=json!(cases("trial"));
    }

