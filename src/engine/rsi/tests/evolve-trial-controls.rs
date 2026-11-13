    #[test]
    fn search_trial_requires_matched_holdout_results_not_adoption_flags() {
        let mut active=active_generation("a",1,"context",true,"rev",1000,100);
        let mut candidate=trial_generation("b",2,"context",true,"rev",800,90,"candidate");
        assert_eq!(trial_verdict(&candidate,&active).0,"inconclusive");
        with_frozen_trial_receipts(&mut active,&mut candidate);
        assert_eq!(trial_verdict(&candidate,&active).0,"supported");
        candidate["generatorModel"]=json!("different-model");
        assert_eq!(trial_verdict(&candidate,&active).0,"inconclusive");
        candidate["generatorModel"]=json!("test-model");
        candidate["evaluation"]["metrics"]["measurements"][0]["id"]=json!("forged-case");
        assert_eq!(trial_verdict(&candidate,&active).0,"inconclusive");
        candidate["evaluation"]["metrics"]["measurements"][0]["id"]=json!("frozen-0");
        candidate["evaluation"]["metrics"]["measurements"][0]["candidatePass"]=json!(false);
        active["evaluation"]["metrics"]["measurements"][1]["candidatePass"]=json!(true);
        candidate["evaluation"]["metrics"]["measurements"][1]["candidatePass"]=json!(false);
        assert_eq!(trial_verdict(&candidate,&active).0,"rejected",
            "a frozen regression must override savings and accepted flags");
    }

    #[test]
    fn barren_active_controls_trigger_one_real_policy_trial_without_false_promotion() {
        let (state,root)=state();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"escape-stalled-search",
            "source":"rsi-generated","implementation":"expression",
            "parameters":{"expression":"novelty * 5 + exploration"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        let queued=queue_search_trial(&state,&candidate,
            &crate::engine::algorithms::active(&state,"selfimprove.search").unwrap(),
            &[],"","campaign-explore",1);
        assert_eq!(queued["status"],"pending_trial");
        let first=json!({"jobId":"control-one","domain":"memory",
            "generatorCommit":"commit-1","domainSelection":"algorithm_active",
            "candidateId":null,"accepted":false,"evaluation":null,
            "searchStrategy":{"pendingCandidateId":id,"pendingCandidateRevision":candidate["revision"]}});
        let second=json!({"jobId":"control-two","domain":"memory",
            "generatorCommit":"commit-1","domainSelection":"algorithm_active",
            "candidateId":null,"accepted":false,"evaluation":null,
            "searchStrategy":{"pendingCandidateId":id,"pendingCandidateRevision":candidate["revision"]}});
        // Force an explicit domain constraint; the policy has no evaluated
        // active control after two attempts and must get to execute itself.
        let objective="기억";
        let (_domain,mode,strategy)=select_generation_strategy(
            &state,&[first.clone(),second.clone()],objective,None,
            Some(id.clone()),None,"commit-1");
        assert_eq!(mode,"algorithm_trial");
        assert_eq!(strategy["trialFallback"],"exploratory_after_two_no_patch_controls");
        assert_eq!(strategy["policyEvidence"],"unpaired_exploration_not_promotion");
        let trial=json!({"jobId":"experimental","attempt":3,"domain":"memory",
            "domainSelection":"algorithm_trial",
            "accepted":false,"generatorCommit":"commit-1",
            "searchStrategy":{"id":id,"mode":"trial"}});
        let result=record_trial_outcome(&state,&id,&trial,&[first,second]);
        assert_eq!(result["reason"],"comparable_baseline_missing",
            "running the policy alone is not evidence it is better");
        assert_ne!(crate::engine::algorithms::active(&state,"selfimprove.search")
            .unwrap()["id"],id,
            "never automatically promote an exploratory search trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn pending_executable_policy_gets_its_turn_before_another_research_continuation() {
        let (state,root)=state();
        let proposal=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"pending-before-research",
            "source":"rsi-generated","implementation":"expression",
            "parameters":{"expression":"objectiveMatch + exploration"}
        })).unwrap();
        let id=proposal["id"].as_str().unwrap().to_string();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        assert_eq!(queue_search_trial(&state,&proposal,&active,&[],
            "memory","pending-owner",1)["status"],"pending_trial");
        let history=vec![json!({"domain":"memory","accepted":false,
            "candidateId":null,"newFindings":2})];
        assert_eq!(research_continuation(&history).as_deref(),Some("memory"));
        let (domain,selection,strategy)=select_generation_strategy(
            &state,&history,"memory",research_continuation(&history),
            Some(id.clone()),None,"commit-1");
        assert_eq!(domain,"memory");
        assert_eq!(selection,"algorithm_active",
            "control for an executable policy must not wait behind another research generation");
        assert_eq!(strategy["pendingCandidateId"],id);
        assert_ne!(strategy["id"],proposal["id"],
            "the first turn still executes the active control, not a false trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn candidate_scoped_active_control_pins_domain_and_unlocks_the_next_trial() {
        let (state,root)=state();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"adaptive-control-pin",
            "source":"rsi-generated","implementation":"expression",
            "parameters":{"expression":"objectiveMatch * 5 + exploration * 3"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        assert_eq!(queue_search_trial(&state,&candidate,&active,&[],
            "multi domain objective","pin-campaign",1)["status"],"pending_trial");
        let (domain,mode,control_strategy)=select_generation_strategy(
            &state,&[],"multi domain objective",None,Some(id.clone()),None,"commit-1");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(control_strategy["pendingCandidateId"],id);
        assert_eq!(control_strategy["pendingCandidateRevision"],candidate["revision"]);
        let mut control=active_generation("pin-control",1,&domain,false,"commit-1",4000,100);
        control["searchStrategy"]=control_strategy;
        let (next_domain,next_mode,_)=select_generation_strategy(
            &state,&[control.clone()],"multi domain objective",None,
            Some(id.clone()),None,"commit-1");
        assert_eq!(next_domain,domain,"history updates must not move the pending trial target");
        assert_eq!(next_mode,"algorithm_trial",
            "the control's frozen evaluation should permit the candidate to run");
        assert!(comparable_trial_baseline(&[control.clone()],
            &json!({"domain":domain,"generatorCommit":"commit-1"}),&candidate).is_some());
        let mut foreign=control.clone();
        foreign["searchStrategy"]["pendingCandidateId"]=json!("another-candidate");
        assert!(comparable_trial_baseline(&[foreign.clone()],
            &json!({"domain":domain,"generatorCommit":"commit-1"}),&candidate).is_none(),
            "an unrelated policy's control is not causal evidence for this candidate");
        let (_,mode,_)=select_generation_strategy(
            &state,&[foreign],"multi domain objective",None,
            Some(id.clone()),None,"commit-1");
        assert_eq!(mode,"algorithm_active",
            "a foreign control cannot unlock the candidate trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_policy_produces_same_domain_control_before_trial() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"alternate-domain",
            "source":"rsi-generated","implementation":"expression",
            "parameters":{"expression":"novelty + exploration"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        let intended=select_domain(&candidate,&[],"autonomous domain discovery");
        let (domain,mode,strategy)=select_generation_strategy(&state,&[],
            "autonomous domain discovery",None,Some(id.clone()),None,"commit-1");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(domain,intended);
        assert_eq!(strategy["id"],active["id"]);
        assert_eq!(strategy["decisionSource"],"candidate_target_active_control");
        assert_eq!(strategy["pendingCandidateId"],id);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn trial_verdict_requires_success_before_supporting_cost_savings() {
        let mut baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let mut trial = trial_generation("trial", 2, "context", false, "commit-1", 5_000, 500, "candidate");
        with_frozen_trial_receipts(&mut baseline,&mut trial);
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

        let mut baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let mut first = trial_generation("trial-1", 2, "context", true, "commit-1", 9_000, 900, id);
        with_frozen_trial_receipts(&mut baseline,&mut first);
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
        assert_eq!(strategy["trialFallback"], "collecting_same_domain_control");
        let saved = state.store.get("algorithm", id).unwrap();
        assert_eq!(saved["rsiTrial"]["trials"].as_array().unwrap().len(), 1);
        assert!(comparable_trial_baseline(&[baseline.clone()], &first, &saved).is_none());

        let mut baseline2 = active_generation("base-2", 3, "context", false, "commit-1", 10_000, 1_000);
        baseline2["searchStrategy"]["pendingCandidateId"]=json!(id);
        baseline2["searchStrategy"]["pendingCandidateRevision"]=candidate["revision"].clone();
        let (_, mode, _) = select_generation_strategy(
            &state, &[baseline.clone(), first.clone(), baseline2.clone()], "context", None, Some(id.into()), None, "commit-1");
        assert_eq!(mode, "algorithm_trial", "a fresh control permits the pending trial");
        let mut second = trial_generation("trial-2", 4, "context", true, "commit-1", 9_000, 900, id);
        with_frozen_trial_receipts(&mut baseline2,&mut second);
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
    fn pending_search_algorithm_never_crosses_sibling_campaigns_with_same_objective() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"same-formula-shared-id",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + novelty"}
        })).unwrap();
        let first=queue_search_trial(&state,&candidate,&active,&[],"context","owner-a",1);
        assert_eq!(first["status"],"pending_trial");
        let second=queue_search_trial(&state,&candidate,&active,&[],"context","owner-b",1);
        assert_eq!(second["status"],"owned_by_other_campaign");
        let saved=state.store.get("algorithm",candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["campaignId"],"owner-a");
        assert!(pending_trial_candidate(&state,"context",&["owner-b".into()]).is_none(),
            "matching objective hashes cannot cross unrelated experiment branches");
        assert!(pending_trial_candidate(&state,"context",&["owner-a".into()]).is_some());
        let fresh=queue_search_trial(&state,&saved,&active,&[],"context","owner-a",2);
        assert_eq!(fresh["status"],"pending_trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn trial_inheritance_requires_verified_exact_campaign_source_lineage() {
        let (state,root)=state();
        let prior=json!({"id":"parent-campaign","sourceDir":"/repo","sourceBranch":"dominion-sub2",
            "baseCommit":"a".repeat(40),"sourceFingerprint":"b".repeat(64),
            "headCommit":"c".repeat(40)});
        state.store.put("rsi_campaign",&prior).unwrap();
        let mut record=prior.clone();
        record["id"]=json!("descendant-campaign");
        record["lineage"]=json!({"reused":true,"selectedCommit":"c".repeat(40)});
        let input=json!({"rsiContinueFrom":"parent-campaign"});
        assert_eq!(allowed_trial_campaigns(&state,&input,&record,"descendant-campaign"),
            vec!["descendant-campaign","parent-campaign"]);
        record["sourceBranch"]=json!("dominion-sub3");
        assert_eq!(allowed_trial_campaigns(&state,&input,&record,"descendant-campaign"),
            vec!["descendant-campaign"]);
        record["sourceBranch"]=json!("dominion-sub2");
        record["sourceFingerprint"]=json!("f".repeat(64));
        assert_eq!(allowed_trial_campaigns(&state,&input,&record,"descendant-campaign"),
            vec!["descendant-campaign"]);
        let _=std::fs::remove_dir_all(root);
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
            pending_trial_candidate(&state, "context", &["campaign".into()]).unwrap()["id"],
            candidate["id"],
            "a pending candidate is reused by a later scheduled generation"
        );
        assert!(
            pending_trial_candidate(&state, "a different objective", &["campaign".into()]).is_none(),
            "a pending candidate is scoped to its objective"
        );
        let _ = std::fs::remove_dir_all(root);
    }
