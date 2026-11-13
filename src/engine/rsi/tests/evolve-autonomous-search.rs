    #[test]
    fn learned_action_training_never_pairs_unrelated_campaigns() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"isolate autonomous improvement",
            "rsiSourceDir":root.join("src").to_string_lossy()});
        let history=(0..4).map(|i|json!({
            "id":format!("g{i}"),"jobId":format!("j{i}"),
            "campaignId":format!("c{i}"),"attempt":1,
            "createdAt":format!("2026-11-11T01:00:{i:02}Z"),
            "accepted":false,"tokens":1000,
            "actionObservation":{"status":"edit_failed","editToolCalls":1,
                "appliedEditCalls":0},
            "actionDirective":{"mode":"repair_failed_edit"}
        })).collect::<Vec<_>>();
        let selected=evolved_execution_action(&state,&input,&history);
        assert_eq!(selected["mode"],"repair_failed_edit");
        assert_eq!(selected["policy"]["status"],"insufficient_evidence");
        assert_eq!(selected["policy"]["trainingTransitions"],0);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_generation_yields_specific_next_edit_not_generic_research() {
        let (state,root)=state();
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-child-1","jobId":"child-1",
            "captureError":"모델이 실제 소스 파일을 변경하지 않았습니다.","checks":[]
        }));
        let generation=json!({"jobId":"child-1","accepted":false,
            "focusFiles":["src/engine/memory.rs"]});
        let directive=grounded_next_experiment(&state,&[generation.clone()]).unwrap();
        assert!(directive.contains("src/engine/memory.rs"));
        assert!(directive.contains("workspace_replace"),
            "next generation must receive the actual source-edit tool contract: {directive}");
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-child-1","jobId":"child-1",
            "checks":[{"label":"cargo check --all-targets","passed":false,
                "stderr":"error[E0308]: mismatched types in memory.rs"}]
        }));
        let repair=grounded_next_experiment(&state,&[generation.clone()]).unwrap();
        assert!(repair.contains("error[E0308]"),"{repair}");
        assert!(!repair.contains("수정 도구"),"compiler diagnostics take priority over no-patch advice");
        let holdout=json!({"accepted":false,"evaluation":{"verdict":"regressed","metrics":{
            "measurements":[{"id":"long-recall-quality","baselinePass":true,"candidatePass":false}]
        }}});
        let remedy=grounded_next_experiment(&state,&[holdout]).unwrap();
        assert!(remedy.contains("long-recall-quality"));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_search_synthesizes_executable_new_policy_from_wasted_generations() {
        let (state,root)=state();
        crate::engine::algorithms::seed(&state);
        let history=(0..3).map(|attempt|json!({
            "id":format!("failed-{attempt}"),"attempt":attempt+1,
            "accepted":false,"newFindings":0,"candidateId":null,
            "domain":"memory","tokens":12000,"focusFiles":["src/engine/memory.rs"],
            "generatorCommit":"base-1"
        })).collect::<Vec<_>>();
        assert!(autonomous_search_hypothesis(&history[0..1]).is_none());
        let hypothesis=autonomous_search_hypothesis(&history).unwrap();
        assert_eq!(hypothesis["hypothesisEvidence"]["class"],"escape_barren_research");
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        assert_ne!(hypothesis["parameters"]["targetExpression"],active["parameters"]["targetExpression"]);
        let candidate=crate::engine::algorithms::register(&state,&hypothesis).unwrap();
        assert_eq!(candidate["implementation"],"expression");
        assert!(valid_domain_scores(&candidate,&history,"").is_some());
        let report=propose_autonomous_search_trial(
            &state,&history,"","campaign-synth",3,true);
        assert_eq!(report["status"],"pending_trial","{report}");
        assert_eq!(report["origin"],"engine_synthesized_failure_hypothesis");
        assert_eq!(report["hypothesisEvidence"]["repeatedFiles"],1);
        let saved=state.store.get("algorithm",report["candidateId"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["status"],"pending_trial");
        assert_eq!(saved["hypothesisEvidence"]["generationIds"],json!(["failed-2","failed-1","failed-0"]),
            "every new policy must preserve the failure history that caused it");
        assert_ne!(saved["status"],"active","a synthesized hypothesis must not auto-promote");
        let repeated=propose_autonomous_search_trial(
            &state,&history,"","campaign-synth",4,true);
        assert_eq!(repeated["status"],"existing_trial");
        // A later campaign may independently examine the same formula. It
        // cannot steal the first candidate's comparison or trial ownership.
        let next=propose_autonomous_search_trial(
            &state,&history,"","campaign-next",3,true);
        assert_eq!(next["status"],"pending_trial","{next}");
        assert_ne!(next["candidateId"],report["candidateId"]);
        let next_candidate=state.store.get("algorithm",next["candidateId"].as_str().unwrap()).unwrap();
        assert_eq!(next_candidate["rsiTrial"]["campaignId"],"campaign-next");
        let (domain,mode,_strategy)=select_generation_strategy(
            &state,&history,"",None,Some(saved["id"].as_str().unwrap().into()),None,"base-1");
        assert_eq!(mode,"algorithm_active","the first comparison collects a baseline control");
        assert!(DOMAINS.contains(&domain.as_str()));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_objective_trial_can_be_retrieved_by_descendant_after_restart() {
        let (state,root)=state();
        let input=json!({"rsiAutonomous":true,
            "rsiObjective":"전수조사 기반 자율 개선: 기억·도구·자기개선",
            "rsiAutoFocus":true});
        let objective=selection_objective(&input);
        assert!(objective.is_empty(),
            "the generated instruction is not an owner-imposed domain constraint");
        let history=(0..2).map(|i|json!({"id":format!("failed-{i}"),
            "accepted":false,"tokens":5000,"newFindings":0,"candidateId":null,
            "domain":"memory","focusFiles":["src/engine/memory.rs"]})).collect::<Vec<_>>();
        let queued=propose_autonomous_search_trial(
            &state,&history,&objective,"campaign-autonomous",2,true);
        assert_eq!(queued["status"],"pending_trial","{queued}");
        let selected=pending_trial_candidate(
            &state,&selection_objective(&input),&["campaign-autonomous".into()]);
        assert_eq!(selected.as_ref().map(|a|a["id"].clone()),Some(queued["candidateId"].clone()),
            "the child must discover the synthesized trial with the same scope and owner");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn new_autonomous_campaign_reuses_real_prior_failures_without_claiming_success() {
        let (state,root)=state();
        let input=json!({"rsiAutonomous":true,"rsiSourceDir":root,
            "rsiObjective":"autonomous improvement","rsiFocusFiles":[]});
        let scope=research_scope(&input).partition_key;
        let objective=crate::engine::memory::stable_id(&["autonomous improvement"]);
        for i in 0..3 {
            state.store.put("rsi_generation",&json!({
                "id":format!("prior-{i}"),"campaignId":"previous",
                "knowledgePartition":scope,"objectiveId":objective,
                "createdAt":format!("2026-11-11T00:0{i}:00Z"),
                "accepted":false,"candidateId":null,"newFindings":i+1,
                "tokens":4000,"focusFiles":["src/engine/memory.rs"]
            }));
        }
        state.store.put("rsi_generation",&json!({
            "id":"foreign","campaignId":"unrelated",
            "knowledgePartition":"another-repository","objectiveId":objective,
            "createdAt":"2026-11-11T00:04:00Z",
            "accepted":false,"candidateId":null,"tokens":9000
        }));
        let failures=previous_autonomous_failures(&state,&input,"current");
        assert_eq!(failures.len(),3);
        assert_eq!(failures[0]["id"],"prior-0");
        let synthesized=propose_autonomous_search_trial(
            &state,&failures,&selection_objective(&input),"current",0,true);
        assert_eq!(synthesized["status"],"pending_trial","{synthesized}");
        assert_eq!(synthesized["hypothesisEvidence"]["class"],"escape_research_loop");
        assert_eq!(state.store.get("algorithm",synthesized["candidateId"].as_str().unwrap())
            .unwrap()["status"],"available","prior failures can only seed a fresh experiment");
        let mut explicit=input.clone();
        explicit["rsiAutonomous"]=json!(false);
        assert!(previous_autonomous_failures(&state,&explicit,"current").is_empty());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_research_without_code_switches_to_executable_policy_trial() {
        let history=(0..3).map(|i|json!({
            "id":format!("reading-{i}"),"domain":"memory","attempt":i+1,
            "accepted":false,"candidateId":null,"newFindings":i+1,
            "tokens":5000,"focusFiles":["src/engine/memory.rs"]
        })).collect::<Vec<_>>();
        assert_eq!(research_continuation(&history[..2]).as_deref(),Some("memory"));
        assert!(research_continuation(&history).is_none(),
            "three novel research passes without a patch must not monopolize the next generation");
        let hypothesis=autonomous_search_hypothesis(&history).unwrap();
        assert_eq!(hypothesis["hypothesisEvidence"]["class"],"escape_research_loop");
        let (state,root)=state();
        let report=propose_autonomous_search_trial(
            &state,&history,"","research-loop",3,true);
        assert_eq!(report["status"],"pending_trial","{report}");
        let queued=pending_trial_candidate(&state,"",&["research-loop".to_string()]);
        assert!(queued.is_some(),"next worker can execute the synthesized experiment");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn synthesis_does_not_override_explicit_owner_source_targets() {
        let (state,root)=state();
        let history=(0..2).map(|i|json!({"id":format!("failure-{i}"),
            "accepted":false,"tokens":7000,"newFindings":0,"candidateId":null,
            "focusFiles":["src/engine/tools.rs"]})).collect::<Vec<_>>();
        let blocked=propose_autonomous_search_trial(
            &state,&history,"브라우저 도구","owner-fixed",2,false);
        assert_eq!(blocked["status"],"user_pinned_search_target");
        let multi=propose_autonomous_search_trial(
            &state,&history,"","owner-multi",2,false);
        assert_eq!(multi["status"],"pending_trial","{multi}");
        let candidate=state.store.get("algorithm",multi["candidateId"].as_str().unwrap()).unwrap();
        assert!(candidate["parameters"]["targetExpression"].is_null(),
            "owner pinned source must remain under owner control");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_policy_trial_is_replaced_by_a_distinct_executable_search_strategy() {
        let (state,root)=state();
        let failed=(0..4).map(|i|json!({
            "id":format!("failed-{i}"),"accepted":false,
            "tokens":3000,"newFindings":0,"candidateId":null,
            "focusFiles":["src/engine/memory.rs"]
        })).collect::<Vec<_>>();
        let first=propose_autonomous_search_trial(
            &state,&failed[..2],"","retry-trial",2,true);
        assert_eq!(first["status"],"pending_trial","{first}");
        let first_id=first["candidateId"].as_str().unwrap().to_string();
        assert_eq!(pending_trial_candidate(&state,"",&["retry-trial".into()])
            .unwrap()["id"],first_id);

        let mut stale=state.store.get("algorithm",&first_id).unwrap();
        stale["rsiTrial"]["status"]=json!("trial_inconclusive");
        stale["rsiTrial"]["lastReason"]=json!("comparable_baseline_missing");
        state.store.put("algorithm",&stale);
        assert!(pending_trial_candidate(&state,"",&["retry-trial".into()]).is_none(),
            "a concluded inconclusive trial must not be selected on every restart");

        let second=propose_autonomous_search_trial(
            &state,&failed[..3],"","retry-trial",3,true);
        assert_eq!(second["status"],"pending_trial","{second}");
        let second_id=second["candidateId"].as_str().unwrap().to_string();
        assert_ne!(first_id,second_id);
        let replacement=state.store.get("algorithm",&second_id).unwrap();
        assert_eq!(replacement["hypothesisEvidence"]["previousFailedTrials"],1);
        assert_eq!(replacement["hypothesisEvidence"]["previousTrialReasons"][0],
            "comparable_baseline_missing");
        assert_ne!(replacement["parameters"]["targetExpression"],
            stale["parameters"]["targetExpression"],
            "next generation must execute a different hypothesis");
        assert_eq!(pending_trial_candidate(&state,"",&["retry-trial".into()])
            .unwrap()["id"],second_id);
        assert_eq!(propose_autonomous_search_trial(
            &state,&failed,"","retry-trial",4,true)["status"],"existing_trial");

        let mut second_record=replacement;
        second_record["rsiTrial"]["status"]=json!("trial_rejected");
        state.store.put("algorithm",&second_record);
        let third=propose_autonomous_search_trial(
            &state,&failed,"","retry-trial",4,true);
        assert_eq!(third["status"],"pending_trial","{third}");
        assert_ne!(third["candidateId"],second_id);
        let third_id=third["candidateId"].as_str().unwrap();
        let mut third_record=state.store.get("algorithm",third_id).unwrap();
        third_record["rsiTrial"]["status"]=json!("trial_inconclusive");
        state.store.put("algorithm",&third_record);
        assert_eq!(propose_autonomous_search_trial(
            &state,&failed,"","retry-trial",5,true)["status"],
            "campaign_trial_budget_exhausted",
            "recovery must not create an unbounded proposal loop");
        assert!(pending_trial_candidate(&state,"",&["retry-trial".into()]).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn unexecuted_source_formula_retries_with_executable_domain_only_policy() {
        let (state,root)=state();
        let history=(0..3).map(|i|json!({
            "id":format!("stalled-{i}"),"accepted":false,
            "tokens":3200,"newFindings":0,"candidateId":null,
            "focusFiles":["src/engine/memory.rs"]
        })).collect::<Vec<_>>();
        let first=propose_autonomous_search_trial(
            &state,&history[..2],"","source-unavailable",2,true);
        assert_eq!(first["status"],"pending_trial","{first}");
        let mut abandoned=state.store.get("algorithm",
            first["candidateId"].as_str().unwrap()).unwrap();
        abandoned["rsiTrial"]["status"]=json!("trial_inconclusive");
        abandoned["rsiTrial"]["lastReason"]=json!("target_formula_not_executed");
        state.store.put("algorithm",&abandoned);
        let retry=propose_autonomous_search_trial(
            &state,&history,"","source-unavailable",3,true);
        assert_eq!(retry["status"],"pending_trial","{retry}");
        let candidate=state.store.get("algorithm",
            retry["candidateId"].as_str().unwrap()).unwrap();
        assert!(candidate["parameters"]["targetExpression"].is_null(),
            "do not require a source scorer which the worker could not run");
        assert_eq!(candidate["hypothesisEvidence"]["recoveryMode"],
            "domain_only_after_unexecuted_target");
        assert_ne!(candidate["parameters"]["expression"],
            abandoned["parameters"]["expression"]);
        assert_eq!(pending_trial_candidate(&state,"",&["source-unavailable".into()])
            .unwrap()["id"],candidate["id"]);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn pinned_objective_retry_preserves_owner_target_and_adapts_domain_policy() {
        let (state,root)=state();
        let failures=(0..3).map(|i|json!({
            "id":format!("failure-{i}"),"accepted":false,
            "tokens":4000,"newFindings":0,"candidateId":null,
            "focusFiles":["src/engine/tools.rs"]
        })).collect::<Vec<_>>();
        let first=propose_autonomous_search_trial(
            &state,&failures[..2],"","pinned-retry",2,false);
        assert_eq!(first["status"],"pending_trial","{first}");
        let id=first["candidateId"].as_str().unwrap();
        let mut old=state.store.get("algorithm",id).unwrap();
        old["rsiTrial"]["status"]=json!("trial_rejected");
        state.store.put("algorithm",&old);
        let second=propose_autonomous_search_trial(
            &state,&failures,"","pinned-retry",3,false);
        assert_eq!(second["status"],"pending_trial","{second}");
        let replacement=state.store.get("algorithm",second["candidateId"].as_str().unwrap()).unwrap();
        assert!(replacement["parameters"]["targetExpression"].is_null(),
            "a retry cannot rewrite the user's source targets");
        assert_ne!(old["parameters"]["expression"],replacement["parameters"]["expression"]);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_search_requires_two_genuine_failed_attempts() {
        let done=json!({"id":"a","accepted":true,"tokens":5000,"newFindings":0});
        let failed=json!({"id":"b","accepted":false,"tokens":5000,"newFindings":0});
        let research=json!({"id":"c","accepted":false,"tokens":5000,"newFindings":2});
        let zero=json!({"id":"d","accepted":false,"tokens":0,"newFindings":0});
        assert!(autonomous_search_hypothesis(&[done.clone(),failed.clone()]).is_none());
        assert!(autonomous_search_hypothesis(&[research,failed.clone()]).is_none());
        assert!(autonomous_search_hypothesis(&[zero,failed.clone()]).is_none());
        assert!(autonomous_search_hypothesis(&[failed.clone(),failed]).is_some());
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
        assert!(!should_stop_no_progress(1,true,true,false));
        assert_eq!(new_research_findings(&state,&input,"next-worker",&hashes,&[]),0,"re-proposing the same program is not new research");
        let _=std::fs::remove_dir_all(root);
    }

