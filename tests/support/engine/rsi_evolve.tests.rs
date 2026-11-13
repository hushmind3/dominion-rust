#[cfg(test)]
mod branch_ownership_tests {
    use super::owned_source_origin;
    #[test]
    fn a_main_checkout_cannot_masquerade_as_the_campaigns_internal_lab() {
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        use serde_json::json;
        let root=std::env::temp_dir().join(format!("rsi-sub3-lab-boundary-{}",uuid::Uuid::new_v4()));
        let source=root.join("checkout");
        let lab=root.join("rsi-labs").join("lab-test");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&lab).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let make_campaign=|lab_dir:&std::path::Path|json!({
            "id":"test-campaign","labDir":lab_dir,"sourceDir":source,
            "sourceBranch":"dominion-sub3","headCommit":"a".repeat(40),
            "status":"running","nativeApproved":true
        });
        let input=json!({"rsiCampaignId":"test-campaign","rsiSourceBranch":"dominion-sub3"});
        state.store.put("rsi_campaign",&make_campaign(&source)).unwrap();
        assert!(!super::campaign_source(&state,&input,&source,&"a".repeat(40)));
        state.store.put("rsi_campaign",&make_campaign(&lab)).unwrap();
        assert!(super::campaign_source(&state,&input,&lab,&"a".repeat(40)));
        assert!(!super::campaign_source(&state,
            &json!({"rsiCampaignId":"test-campaign","rsiSourceBranch":"main"}),
            &lab,&"a".repeat(40)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_source_commit_never_substitutes_for_owned_branch_provenance() {
        assert!(owned_source_origin(Some("dominion-sub3"),"dominion-sub3"));
        for foreign in ["main","master","dominion-sub","dominion-sub1",
            "dominion-sub2","dominion-sub4","dominion-champion","HEAD"] {
            assert!(!owned_source_origin(Some(foreign),foreign));
            assert!(!owned_source_origin(Some("dominion-sub3"),foreign));
        }
        assert!(!owned_source_origin(None,"dominion-sub3"));
        assert!(!owned_source_origin(Some(""),"dominion-sub3"));
        assert!(!owned_source_origin(Some("main"),"dominion-sub3"));
    }
}

#[cfg(test)]
mod paired_descendant_target_tests {
    use super::*;
    #[test]
    fn immutable_paired_target_outvotes_unmeasured_descendant_exploration() {
        let evidence=json!({"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"improver",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"improver"}});
        assert!(paired_descendant_target(&evidence));
        let mut mismatched=evidence.clone();
        mismatched["rsiSelectedDomain"]=json!("memory");
        assert!(!paired_descendant_target(&mismatched));
        let mut speculative=evidence;
        speculative["rsiDecisionInput"]["basis"]=json!("parent_next_decision");
        assert!(!paired_descendant_target(&speculative));
        let only_system=json!({"rsiSystemEvolution":true,"rsiSelectedDomain":"improver"});
        assert!(!paired_descendant_target(&only_system),
            "system mode cannot masquerade as independently measured target");
    }
}

#[cfg(test)]
mod system_evolution_handoff_tests {
    use super::*;
    #[test]
    fn measured_descendant_targets_survive_but_stale_parent_hints_do_not() {
        let first=json!({"rsiSystemEvolution":true,"rsiAutonomous":true,"rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&first,1),Some("improver".into()));
        assert_eq!(system_evolution_target_hint(&first,2),None);
        let explicit=json!({"rsiSystemEvolution":true,"rsiAutonomous":false,
            "rsiObjective":"Improve memory","rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&explicit,1),None,
            "owner-defined objectives must outrank the default improver seed");
        let focused=json!({"rsiSystemEvolution":true,"rsiAutonomous":false,
            "rsiAutoFocus":false,"rsiFocusFiles":["src/engine/memory.rs"],
            "rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&focused,1),Some("memory".into()),
            "a successor planner must honor assigned source tasks as well");
        let measured=json!({"rsiSystemEvolution":true,"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"context",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"context"}});
        assert_eq!(system_evolution_target_hint(&measured,2),Some("context".into()));
    }
}

#[cfg(test)]
mod descendant_controller_contract_tests {
    use super::*;
    #[test]
    fn successor_plan_is_authoritative_and_rejects_parent_override() {
        let commit="a".repeat(40);
        let lineage=json!({"source":"descendant_curated_lineage_v1",
            "historyCount":1,"sha256":"f".repeat(64)});
        let history=vec![json!({"attempt":1})];
        let frontier=vec![json!({"role":"learning","domain":"learning",
            "path":"src/engine/learning.rs","sourceBytes":100,
            "sourceSha256":"d".repeat(64)})];
        let influence=crate::engine::rsi_interaction::influence_graph(&history,&frontier);
        let meta=crate::engine::rsi_meta_controller::synthesize(
            &history,&frontier,&["learning"],None,&json!({}),&json!({}),&influence);
        let co_design=crate::engine::rsi_interaction::coupled_portfolio(
            &meta["target"],&frontier,&influence,&["learning"]);
        let plan=json!({
            "source":"compiled_descendant_system_controller_v2",
            "generatorCommit":commit,"historyCount":1,"domain":"learning",
            "selection":"algorithm_active",
            "strategy":{"id":"descendant-search","revision":"r2"},
            "focusFiles":["src/engine/learning.rs"],
            "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                "generatorCommit":commit,"historyCount":1,"lineageSha256":lineage["sha256"],
                "sourceTreeSha256":"d".repeat(64)},
            "lineageCapsule":lineage,
            "peerLineages":{"source":"cross_campaign_comparative_archive_v1",
                "profileSha256":"f".repeat(64),"references":[],"automaticPatchReuse":false},
            "comparativeMode":"local_source_redesign",
            "metaPolicy":meta,"successorStage":null,
            "architectureTarget":meta["target"],"architectureFrontier":frontier,
            "interactionGraph":influence,"coDesignPortfolio":co_design,
            "mutualProgram":null,
            "improvementDirective":{"mode":"change_approach"},
            "editPolicy":{"revision":"descendant-method-r2"}
        });
        let input=json!({
            "rsiSystemEvolution":true,"rsiGeneratorCommit":commit,
            "rsiSelectedDomain":"learning","rsiTargetDomain":"learning",
            "rsiTargetPlan":{"domain":"learning","selection":"algorithm_active",
                "strategy":plan["strategy"],"generatorCommit":commit},
            "rsiFocusFiles":plan["focusFiles"],
            "rsiFocusSelection":{"source":"rebuilt_descendant_controller"},
            "rsiEditPolicy":plan["editPolicy"],
            "rsiSystemBlueprint":plan["systemBlueprint"],
            "rsiImprovementDirective":plan["improvementDirective"],
            "rsiLineageCapsule":plan["lineageCapsule"],
            "rsiPeerLineages":plan["peerLineages"],
            "rsiComparativeMode":plan["comparativeMode"],
            "rsiMetaPolicy":plan["metaPolicy"],
            "rsiSuccessorStage":plan["successorStage"],
            "rsiInteractionGraph":plan["interactionGraph"],
            "rsiCoDesignPortfolio":plan["coDesignPortfolio"],
            "rsiFactorialExperiment":plan["factorialExperiment"],
            "rsiReflexiveProgram":plan["reflexiveProgram"],
            "rsiOpenImprovementProgram":plan["openImprovementProgram"],
            "rsiPrincipleLearning":plan["principleLearning"],
            "rsiApiRoutePolicy":plan["apiRoutePolicy"],
            "rsiMutualProgram":plan["mutualProgram"],
            "rsiControllerPlan":plan
        });
        // Reconstruct the same immutable boundary used by real descendant
        // workers; a handcrafted controller with no sealed contract must
        // remain rejected in production.
        let mut input=input;
        let profile=crate::engine::rsi_profile::from_input(&input).unwrap();
        input["rsiExperimentContract"]=crate::engine::rsi_evolve::experiment_contract::bind(
            &input["rsiControllerPlan"],profile["sha256"].as_str().unwrap()).unwrap();
        let validated=validated_descendant_controller(&input,&history,&commit)
            .unwrap().unwrap();
        assert_eq!(validated.0,"learning");
        assert_eq!(validated.2["id"],"descendant-search");
        let mut hijacked=input.clone();
        hijacked["rsiSelectedDomain"]=json!("improver");
        assert!(validated_descendant_controller(&hijacked,&history,&commit).is_err());
        let mut stale=input.clone();
        stale["rsiControllerPlan"]["generatorCommit"]=json!("old");
        assert!(validated_descendant_controller(&stale,&history,&commit).is_err());
        let manual=json!({"rsiSystemEvolution":true,"rsiAutoFocus":false,
            "rsiGeneratorSourceDir":"/pinned/source","rsiFocusFiles":["src/engine/memory.rs"],
            "rsiSystemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                "generatorCommit":commit},
            "rsiImprovementDirective":{"mode":"discover"}});
        assert!(validated_descendant_controller(&manual,&history,&commit)
            .unwrap().is_none(),"assigned downstream coding uses the normal compiled worker");
        let mut unsafe_manual=manual;
        unsafe_manual["rsiGeneratorSourceDir"]=Value::Null;
        assert!(validated_descendant_controller(&unsafe_manual,&history,&commit).is_err(),
            "unplanned self-evolution cannot bypass the compiled controller");
        let mut lost_lineage=input.clone();
        lost_lineage["rsiLineageCapsule"]["sha256"]=json!("a".repeat(64));
        assert!(validated_descendant_controller(&lost_lineage,&history,&commit).is_err(),
            "the resident coordinator cannot exchange the descendant's inherited experience");
        let mut swapped_bundle=input.clone();
        swapped_bundle["rsiCoDesignPortfolio"]["focus"][0]["sourceSha256"]=json!("wrong");
        assert!(validated_descendant_controller(&swapped_bundle,&history,&commit).is_err(),
            "the parent cannot swap a coupled redesign's original source");
        let mut wrong_method=input;
        wrong_method["rsiEditPolicy"]["revision"]=json!("parent");
        assert!(validated_descendant_controller(&wrong_method,&history,&commit).is_err());
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
    fn verified_adoption_opens_a_new_source_goal_only_when_opted_in() {
        let root=json!({"rsiExpandAfterGain":true,"rsiObjective":"fix one file"});
        let history=vec![json!({
            "accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"measurements":[
                {"id":"planning-quality","domain":"planning","metrics":[
                    {"key":"quality","candidate":1.0,"improved":true}]},
                {"id":"memory-quality","domain":"memory","metrics":[
                    {"key":"quality","candidate":0.7,"improved":false}]}
            ]}}
        })];
        let chosen=post_adoption_objective(&root,&history).unwrap();
        assert!(chosen["objective"].as_str().unwrap().contains("memory-quality"));
        assert!(!chosen["objective"].as_str().unwrap().contains("planning-quality"));
        assert_eq!(chosen["residuals"].as_array().unwrap().len(),1);
        assert!(chosen["prompt"].as_str().unwrap().contains(&"a".repeat(40)));
        let mut disabled=root.clone();
        disabled["rsiExpandAfterGain"]=json!(false);
        assert!(post_adoption_objective(&disabled,&history).is_none());
        let forged=vec![json!({"accepted":true,"acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"eligible":false}}})];
        assert!(post_adoption_objective(&root,&forged).is_none());
    }

    #[test]
    fn saturated_quality_switches_to_measured_context_cost_before_noisy_latency() {
        let root=json!({"rsiExpandAfterGain":true,"rsiFrozenCases":[
            {"id":"group-chat-input-cost","metrics":[{"key":"contextChars",
                "minRelativeGain":0.05,"minAbsoluteGain":50.0}]},
            {"id":"memory-quality","metrics":[{"key":"elapsedMs",
                "minRelativeGain":0.20,"minAbsoluteGain":1.0}]}
        ]});
        let history=vec![json!({"accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":1,"measurements":[
                {"id":"memory-quality","domain":"memory","metrics":[
                    {"key":"quality","candidate":1.0,"improved":false},
                    {"key":"elapsedMs","direction":"lower","candidate":230.0,"improved":false}]},
                {"id":"group-chat-input-cost","domain":"context","metrics":[
                    {"key":"quality","candidate":1.0,"improved":false},
                    {"key":"contextChars","direction":"lower","candidate":77000.0,"improved":false}]}
            ]}}})];
        let next=post_adoption_objective(&root,&history).unwrap();
        assert_eq!(next["costOpportunities"][0]["caseId"],"group-chat-input-cost");
        assert_eq!(next["costOpportunities"][0]["requiredReduction"],3850.0);
        assert!(next["objective"].as_str().unwrap().contains("contextChars"));
        assert!(next["residuals"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_nonmeta_first_adoption_learns_to_improve_the_improver_before_transfer() {
        let input=json!({"rsiExpandAfterGain":true,"rsiFrozenCases":[
            {"id":"search-quality","metrics":[{"key":"elapsedMs",
                "minRelativeGain":0.20,"minAbsoluteGain":1.0}]},
            {"id":"group-chat-input-cost","metrics":[{"key":"contextChars",
                "minRelativeGain":0.05,"minAbsoluteGain":50.0}]}
        ]});
        let history=vec![json!({"accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":0,
                "measurements":[
                    {"id":"search-quality","domain":"improver","metrics":[
                        {"key":"elapsedMs","direction":"lower",
                         "candidate":200.0,"improved":false}]},
                    {"id":"group-chat-input-cost","domain":"context","metrics":[
                        {"key":"contextChars","direction":"lower",
                         "candidate":77000.0,"improved":false}]}
                ]}}})];
        let target=post_adoption_objective(&input,&history).unwrap();
        assert_eq!(target["metaTransferTarget"],true);
        assert!(target["objective"].as_str().unwrap().contains("search-quality"));
        assert!(target["objective"].as_str().unwrap().contains("개선기"));
        let mut after=history.clone();
        after[0]["evaluation"]["metrics"]["metaHoldoutGains"]=json!(1);
        let transfer=post_adoption_objective(&input,&after).unwrap();
        assert_eq!(transfer["metaTransferTarget"],false);
        assert!(transfer["objective"].as_str().unwrap().contains("group-chat-input-cost"));
    }

    #[test]
    fn paired_behavioral_counterexamples_guide_descendants_without_forged_receipts(){
        let (state,root)=state();
        let a="a".repeat(64);
        let b="b".repeat(64);
        let mut candidate=json!({"id":"candidate-1","patchSha256":a,
            "cases":[{"id":"planning-development","holdout":false}],
            "pairedComparisonComplete":true,
            "evaluation":{"evidenceSha256":b}});
        state.store.put("rsi_candidate",&candidate).unwrap();
        let pair=json!({"id":"rsi-pair-candidate-1","candidateId":"candidate-1",
            "evidenceSha256":b,"report":{"patchSha256":a,
                "cases":[{"id":"planning-development",
                    "behavioralCounterexamples":{"candidate":[
                        {"input":"설정을 읽고 결과를 저장한 다음 검증해",
                         "expectedSegments":3,"observedSegments":2,
                         "steps":["설정을 읽고 결과를 저장한","검증해"]}
                    ]}}]}});
        state.store.put("rsi_pair",&pair).unwrap();
        let cases=paired_behavioral_feedback(&state,"candidate-1");
        assert_eq!(cases.len(),1);
        assert!(cases[0].contains("기대 단계=3"));
        assert!(cases[0].contains("후보 단계=2"));
        assert!(cases[0].contains("설정을 읽고"));
        candidate["cases"][0]["holdout"]=json!(true);
        state.store.put("rsi_candidate",&candidate).unwrap();
        assert!(paired_behavioral_feedback(&state,"candidate-1").is_empty(),
            "even authentic evaluator output must not expose a holdout answer");
        let mut stale=pair;
        stale["evidenceSha256"]=json!("0".repeat(64));
        state.store.put("rsi_pair",&stale).unwrap();
        assert!(paired_behavioral_feedback(&state,"candidate-1").is_empty(),
            "a different or stale paired report must never teach a descendant");
        let _=std::fs::remove_dir_all(root);
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
        assert!(!generation_fits_remaining_budget(&record,&json!({"prompt":"한".repeat(3000),
            "providerId":"budgeted-model-provider","model":"budgeted-model"})),"do not build or launch a paid-model worker when even its existing prompt cannot fit");
        assert!(generation_fits_remaining_budget(&json!({"limits":{}}),&json!({"prompt":"한".repeat(3000)})));
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
        assert!(!should_stop_no_progress(1, true, true, false, false));
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
        // An arbitrary continuation label must NOT import sibling evidence.
        let head="a".repeat(40);
        let fingerprint="f".repeat(64);
        state.store.put("rsi_campaign",&json!({
            "id":"digest-prior","headCommit":head,"sourceDir":"source-a",
            "sourceBranch":"dominion-sub3","baseCommit":"base",
            "sourceFingerprint":fingerprint,
            "history":[{"jobId":"digest-job","generatorCommit":head}]
        }));
        let requested=json!({"rsiContinueFrom":"digest-prior"});
        assert_eq!(research_digest(&state,&requested,&json!({"history":[]})),"",
            "goal matching alone must never import another campaign's reads");
        let verified=json!({
            "sourceDir":"source-a","sourceBranch":"dominion-sub3",
            "baseCommit":"base","sourceFingerprint":fingerprint,
            "headCommit":head,
            "lineage":{"reused":true,"selectedCommit":head},
            "history":[]
        });
        let continued=research_digest(&state,&requested,&verified);
        assert!(continued.contains("src/engine/memory.rs 10-20"));
        let mut updated=verified;
        updated["headCommit"]=json!("b".repeat(40));
        assert_eq!(research_digest(&state,&requested,&updated),"",
            "source offsets from earlier revisions must be re-read");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn descendant_replans_its_own_source_and_drops_stale_parent_excerpt() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/memory.rs"),
            "fn memory_static() { println!(\"unrelated\"); }\n").unwrap();
        std::fs::write(root.join("src/context.rs"),
            "fn context_worker() { println!(\"revised\"); }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut job=json!({"input":{
            "rsiSourceDir":root,"rsiKnowledgeSource":root,
            "rsiIndexId":index["id"],"rsiAutoFocus":true,
            "rsiTargetDomain":"memory",
            "rsiFocusFiles":["src/memory.rs"],
            "rsiTargetPlan":{"domain":"memory","selection":"algorithm_active",
                "strategy":{"implementation":"failure-clusters"},"generatorCommit":"before"},
            "rsiObjective":"","rsiPromptBase":"OWNER BASE",
            "rsiPromptEvidence":"\nVERIFIED PRIOR EVIDENCE",
            "rsiPromptResearch":"\nRESEARCH RECEIPT",
            "rsiRemainingCampaignTokens":5000,
            "model":"test-model","providerId":"test-provider",
            "rsiDecisionInput":{"nextDomain":"memory"}
        }});
        let revised=json!({"implementation":"failure-clusters"});
        assert!(reconcile_descendant_target(&state,&mut job,&[],"context",
            "algorithm_active",&revised,"after").unwrap());
        assert_eq!(job["input"]["rsiTargetDomain"],"context");
        assert_eq!(job["input"]["rsiFocusFiles"][0],"src/context.rs");
        assert_eq!(job["input"]["rsiFocusSelection"]["source"],"descendant_indexed_evidence");
        let new_prompt=job["input"]["prompt"].as_str().unwrap();
        assert!(new_prompt.starts_with("OWNER BASE"));
        assert!(new_prompt.contains("VERIFIED PRIOR EVIDENCE"));
        assert!(new_prompt.contains("context_worker"));
        assert!(!new_prompt.contains("memory_static"),
            "a wrong parent-selected source excerpt must not survive child replanning");
        assert_eq!(job["input"]["rsiDecisionInput"]["nextDomain"],"context");
        assert_eq!(job["rsiTargetReplan"]["reason"],"descendant_authoritative_selection");
        assert!(!reconcile_descendant_target(&state,&mut job,&[],"context",
            "algorithm_active",&revised,"after").unwrap(),
            "unchanged descendant plans must not reselect sources or append prompt text");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_success_becomes_revision_scoped_target_learning_not_a_patch_counter() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let beta=root.join("src/beta.rs");
        let original="fn search_dispatch() { println!(\"search\"); }\n";
        let improved="fn search_dispatch() { println!(\"search improved\"); }\n";
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),original).unwrap();
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
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let accepted_hash=crate::engine::code_index::indexed_source_hash(
            &state,index["id"].as_str().unwrap(),"src/beta.rs").unwrap();
        assert_ne!(selected_beta["sourceHash"],accepted_hash);
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
            "acceptedSourceHashes":{"src/beta.rs":accepted_hash},
            "createdAt":"2026-11-10T00:00:00Z"
        })).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/beta.rs");
        assert_eq!(learned[0]["evidence"]["priorProductive"],1.0);
        std::fs::write(&beta,original).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let reverted=choose_focus_files(&state,&input,&[],None);
        assert_eq!(reverted[0]["path"],"src/alpha.rs",
            "reverting to the pre-patch version must not earn a success reward");
        assert_eq!(reverted.iter().find(|row|row["path"]=="src/beta.rs").unwrap()
            ["evidence"]["priorProductive"],0.0);
        std::fs::write(&beta,"fn search_dispatch() { println!(\"new revision\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let stale=choose_focus_files(&state,&input,&[],None);
        assert_eq!(stale[0]["evidence"]["priorProductive"],0.0);
        // A falsely declared acceptance must not contribute feedback.
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"rejected",
            "paths":["src/beta.rs"],"evaluation":{"metrics":{"eligible":false}}
        })).unwrap();
        std::fs::write(&beta,improved).unwrap();
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
    fn fallback_source_selector_cannot_falsely_promote_a_generated_target_formula() {
        let (state,root)=state();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"invalid-runtime-target",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration",
                "targetExpression":"1 / pathEvidence"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        let generation=json!({"jobId":"actual-trial","attempt":2,
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
        assert_ne!(saved["status"],"active");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn indexed_focus_finds_opaque_sources_and_model_can_explore_new_targets() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/opaque.rs"),
            "fn dispatch() { let x = \"bm25\"; println!(\"{x}\"); }\n").unwrap();
        std::fs::write(root.join("src/engine/neutral.rs"),
            "fn unrelated() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiTargetDomain":"memory",
            "rsiObjective":"bm25","rsiKnowledgeSource":root});
        let autonomous=json!({"rsiAutonomous":true,"rsiTargetDomain":"tools",
            "rsiObjective":"knowledge experience generic default"});
        assert_eq!(target_focus_objective(&autonomous),"tools");
        assert_eq!(target_focus_objective(&input),"bm25");
        let ranked=choose_focus_files(&state,&input,&[],None);
        assert_eq!(ranked[0]["path"],"src/engine/opaque.rs");
        let explorer=json!({"revision":"new-source-space",
            "parameters":{"targetExpression":"novelty - sourceCost"}});
        let unknown=json!({"rsiIndexId":index["id"],"rsiTargetDomain":"",
            "rsiObjective":"uniqueunmatchedobjective","rsiKnowledgeSource":root});
        assert!(choose_focus_files(&state,&unknown,&[],None).is_empty());
        assert_eq!(choose_focus_files(&state,&unknown,&[],Some(&explorer)).len(),2,
            "a model-authored policy can search beyond fixed filenames and keywords");
        let _=std::fs::remove_dir_all(root);
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
    fn next_generation_inherits_unchanged_holdout_quality_and_failed_edit_evidence() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"plan better","rsiTargetDomain":"planning"});
        let record=json!({"history":[{
            "attempt":2,"domain":"planning","accepted":false,"candidateId":"candidate-failed",
            "evaluation":{"verdict":"inconclusive","metrics":{"measurements":[
                {"id":"planning-quality","domain":"planning","holdout":true,
                 "metrics":[{"key":"quality","baseline":0.75,"candidate":0.75,
                    "improved":false,"regressed":false}]}
            ]}},
            "editEvidence":{"attempts":3,"verifiedWrites":1,
                "recentFailures":["workspace_replace_range: unchanged"]},
            "error":"no quality gain"
        }]});
        let text=generation_context(&state,&input,&record);
        assert!(text.contains("planning-quality"),"{text}");
        assert!(text.contains("0.75"),"{text}");
        assert!(text.contains("편집 시도 3회"),"{text}");
        assert!(text.contains("workspace_replace_range: unchanged"),"{text}");
        assert!(!text.contains("개선 확인"),"{text}");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn edit_method_selection_learns_from_real_executions_not_model_prose() {
        let baseline=synthesize_edit_method(&[]);
        assert_eq!(baseline["method"],"balanced");
        let actual=json!({"jobId":"j1","tokens":50000,
            "editPolicy":{"method":"balanced"},"candidateId":null,
            "editEvidence":{"verifiedWrites":0},"accepted":false});
        let next=synthesize_edit_method(&[actual.clone()]);
        assert_ne!(next["method"],"balanced",
            "an unproductive measured attempt must explore a different executable method");
        let fake=json!({"jobId":"j2","tokens":0,
            "editPolicy":{"method":"focused-range"},"accepted":true,
            "evaluation":{"metrics":{"eligible":true}}});
        assert_eq!(synthesize_edit_method(&[actual,fake])["method"],next["method"],
            "unexecuted success claims cannot update the policy");
    }

    #[test]
    fn repeated_verified_success_can_outweigh_exploration_of_failed_methods() {
        let mut generations=Vec::new();
        for (method,checkpoint,tool,reward) in [
            ("balanced",3,"workspace_replace_range",true),
            ("generated-early",2,"workspace_replace_range",false),
            ("generated-exact",4,"workspace_replace",false)
        ] {
            for i in 0..8 {
                generations.push(json!({
                    "jobId":format!("{method}-{i}"),"tokens":10000,
                    "editPolicy":{"method":method,"checkpointAfter":checkpoint,
                        "preferredTool":tool,"repairLimit":2},
                    "accepted":reward,"evidenceVerified":reward,
                    "candidateId":if reward {json!(format!("candidate-{i}"))} else {Value::Null},
                    "evaluation":{"metrics":{"eligible":reward}}
                }));
            }
        }
        assert_eq!(synthesize_edit_method(&generations)["method"],"balanced",
            "verified improvement must preserve the winning composed controller");
    }

    #[test]
    fn edit_method_reward_requires_measured_quality_for_full_credit() {
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":true,
            "candidateOrigin":"native_inductive_source_mutation","tokens":9000,
            "evaluation":{"metrics":{"eligible":true}}})),0.0,
            "native mutations cannot credit an unused model editor, even after model calls");
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}}})),1.0);
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":false,
            "evaluation":{"metrics":{"eligible":true}}})),0.0,
            "unverified summaries cannot earn a successful improvement reward");
        assert_eq!(edit_method_reward(&json!({"candidateId":"c",
            "evaluation":{"verdict":"inconclusive","metrics":{"improvedHoldouts":0}}})),0.0);
        assert_eq!(edit_method_reward(&json!({"candidateId":"c",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "verdict":"inconclusive","metrics":{"improvedHoldouts":0}}})),0.10);
        assert_eq!(edit_method_reward(&json!({"accepted":false,"candidateId":null})),0.0);
        let invented=json!({"accepted":true,"evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"rev-1","rules":[{"id":"rule-1"}]}});
        assert_eq!(edit_method_reward(&invented),0.0,
            "a policy that did not execute cannot claim the improvement");
        let mut real=invented;
        real["ruleExecutions"]=json!([{"ruleId":"rule-1","policyRevision":"rev-1",
            "runId":"run-g1","toolCalls":1,"opSeqBefore":1,
            "allow":["workspace_replace_range"]}]);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule receipt without downstream tool execution is not causal evidence");
        real["toolTimeline"]=json!([
            {"runId":"run-g1","opSeq":1,"tool":"workspace_read_range","status":"completed"},
            {"runId":"run-g1","opSeq":2,"tool":"workspace_replace_range","status":"completed",
                "path":"src/engine/test.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}
        ]);
        real["acceptedSourceHashes"]=json!({"src/engine/test.rs":"a".repeat(64)});
        assert_eq!(edit_method_reward(&real),1.0);
        real["ruleExecutions"][0]["opSeqBefore"]=json!(3);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule activated after the patch must never inherit its reward");
        real["ruleExecutions"][0]["opSeqBefore"]=json!(1);
        real["ruleExecutions"][0]["allow"]=json!(["code_search"]);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule whose advertised action did not run is uncredited");
    }

    #[test]
    fn proof_goal_does_not_stop_on_unverified_streak_counters() {
        let (state,root)=state();
        assert!(!proof_goal_reached(&state,&json!({"rsiStopAfterProof":true}),
            &json!({"id":"forged","maxAcceptedStreak":3,"capabilityDemonstrated":true,"history":[]})));
        let _=std::fs::remove_dir_all(root);
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
        assert!(!should_stop_no_progress(1, true, true, false, false));
        assert!(should_stop_no_progress(1, true, false, false, false));
        assert!(should_stop_no_progress(1, false, true, false, false));
        assert!(!should_stop_no_progress(2, true, true, false, false));
        assert!(!should_stop_no_progress(3, true, true, false, false));
        assert!(should_stop_no_progress(3, true, false, false, false));
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
        // Persisted research from generation 1 becomes a line-scoped, model-sized
        // source edit in generation 2. The old source is not retransmitted.
        let change=json!({"path":"src/reuse.rs","startLine":1,"endLine":1,
            "newText":"pub fn remember() { /* IMPROVED_RESEARCH_SOURCE */ }",
            "expectedSha256":old_hash.clone()});
        let ctx=tools::ToolContext{state:&state,store:&state.store,workspace:&root,session_id:"research-2",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let written=tools::execute(&ctx,"workspace_replace_range",&change,"run-2").await.unwrap();
        assert_eq!(written["replaced"],true,"{written}");
        assert_eq!(std::fs::read_to_string(root.join("src/reuse.rs")).unwrap(),
            "pub fn remember() { /* IMPROVED_RESEARCH_SOURCE */ }\n");
        state.store.put("execution_operation",&json!({"id":"real-next-change","sessionId":"research-2","tool":"workspace_replace_range","arguments":change,"result":written,"status":"completed"}));
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
    fn failed_tools_and_unfinished_plans_survive_and_reuse_the_source_hash() {
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let source="fn reusable() {}\n";let hash=format!("{:x}",Sha256::digest(source.as_bytes()));
        std::fs::write(root.join("src/reuse.rs"),source).unwrap();
        state.store.put("session",&json!({"id":"before"}));
        state.store.put("session",&json!({"id":"after"}));
        state.store.write_message(&json!({"session_id":"before","role":"assistant","content":"",
            "reasoning":"UNFINISHED_IMPLEMENTATION_PLAN ".repeat(10)})).unwrap();
        state.store.put("execution_operation",&json!({"id":"read","sessionId":"before","opSeq":1,
            "tool":"workspace_read_range","status":"completed","arguments":{"path":"src/reuse.rs"},
            "result":{"content":source,"sourceSha256":hash,"startLine":1,"endLine":1}}));
        state.store.put("execution_operation",&json!({"id":"failed","sessionId":"before","opSeq":2,
            "tool":"workspace_replace_range","status":"failed","arguments":{"path":"src/reuse.rs"},
            "result":{"error":"STALE_SOURCE_EDIT_REJECTED"}}));
        let input=json!({"rsiSourceDir":root,"rsiObjective":"memory reuse","rsiFocusFiles":["src/reuse.rs"],"rsiSystemEvolution":true});
        let id=remember_research(&state,&input,&json!({"jobId":"failed-job","domain":"memory","accepted":false,"error":"tool limit"}),"before").unwrap();
        let saved=state.store.get("experience",&id).unwrap();
        assert_eq!(saved["evidence"]["accepted"],false);
        assert_eq!(saved["evidence"]["failure"],"tool limit");
        let prompt=reuse_research(&state,&input,&root,"after");
        assert!(prompt.contains("UNFINISHED_IMPLEMENTATION_PLAN"));
        assert!(prompt.contains("STALE_SOURCE_EDIT_REJECTED"));
        assert!(prompt.contains(&hash),"source cache must deliver its hash for a grounded edit");
        let _=std::fs::remove_dir_all(root);
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
        assert!(auto_write_allowed(
            &state,
            "session",
            "workspace_replace_range",
            &args
        ), "RSI line edit must not pause at the owner approval gate");
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
    #[test]
    fn downstream_episodes_share_the_campaign_budget_and_keep_their_own_cap() {
        let (state,root)=state();let context=parent(&state);
        context.checkpoint(json!({"input":{"limits":{"tokenBudget":2000}}})).unwrap();
        for (id,tokens) in [("a",1100),("b",300)] {
            state.store.put("session",&json!({"id":id,"rsiCampaignId":"campaign"}));
            state.store.put("job",&json!({"id":id,"kind":"rsi_generation","sessionId":id,
                "input":{"rsiCampaignId":"campaign","rsiMetaEpisode":true}}));
            state.store.audit("model.usage",json!({"sessionId":id,"totalTokens":tokens}));
        }
        assert_eq!(remaining_output_tokens(&state,"a",100).unwrap(),Some(500));
        state.store.put("session",&json!({"id":"b","rsiCampaignId":"campaign","rsiEpisodeTokenLimit":500}));
        assert!(remaining_output_tokens(&state,"b",100).is_err());
        let _=std::fs::remove_dir_all(root);
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
            fixtures.join("rsi-lab.mjs"),
            include_str!("../../../scripts/rsi-lab.mjs"),
        )
        .unwrap();
        std::fs::write(
            fixtures.join("rsi-measure.mjs"),
            include_str!("../../../scripts/rsi-measure.mjs"),
        )
        .unwrap();
        let report_file = fixtures.join("report.json");
        let mut cmd = Command::new(rsi_worktree::node_executable());
        cmd.current_dir(&source)
            .arg(fixtures.join("rsi-lab.mjs"))
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
        let accepted = adopt(&context, &lab, &candidate, &base).await.unwrap();
        assert_ne!(accepted, base);
        assert_eq!(
            git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap(),
            accepted
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
        git(&context, &source, &["checkout","-b","dominion-sub3"]).await.unwrap();
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
            "rsiSourceBranch":"dominion-sub3",
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
        git(&context,&source,&["checkout","-b","dominion-sub3"]).await.unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context.checkpoint(json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "rsiSourceBranch":"dominion-sub3",
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
        assert_eq!(history[1]["researchDelivered"], true, "next generation did not receive stored research");
        assert_eq!(history[1]["accepted"], true, "{result}");
        assert_eq!(history[1]["decisionInput"]["nextDomain"], "memory",
            "the user goal did not drive the next capability selection");
        let accepted_commit = history[1]["acceptedCommit"].as_str().unwrap();
        assert_eq!(history[2]["generatorCommit"], accepted_commit);
        assert_ne!(history[2]["generatorBinarySha256"], history[1]["generatorBinarySha256"]);
        assert_eq!(history[2]["workerMarker"], "improved_execution");
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
        git(&context,&source,&["checkout","main"]).await.unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_err(),
            "main must never be an application target");
        assert_eq!(git(&context,&source,&["rev-parse","HEAD"]).await.unwrap(),base);
        git(&context,&source,&["checkout","dominion-sub3"]).await.unwrap();
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
    fn autonomous_objective_is_open_ended_while_explicit_goals_are_respected() {
        let generated=json!({
            "rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다",
            "rsiFocusFiles":[]
        });
        assert_eq!(selection_objective(&generated),"");
        assert_eq!(candidate_domains(&selection_objective(&generated)),DOMAINS);
        let explicit=json!({
            "rsiAutonomous":false,
            "rsiObjective":"자기개선 알고리즘 검색을 개선",
            "rsiFocusFiles":[]
        });
        let eligible=candidate_domains(&selection_objective(&explicit));
        assert!(eligible.contains(&"improver"));
        assert!(!eligible.contains(&"ui"));
        assert!(!eligible.contains(&"memory"));
        let latest=json!({"rsiAutonomous":true,"rsiFocusFiles":["src/engine/memory.rs"]});
        assert!(selection_objective(&latest).contains("memory"),
            "explicit focus files retain their intended capability even for open objectives");
        let automatically_focused=json!({
            "rsiAutonomous":true,"rsiAutoFocus":true,
            "rsiObjective":"지식과 경험을 활용해 개선",
            "rsiFocusFiles":["src/engine/memory.rs"]
        });
        assert_eq!(selection_objective(&automatically_focused),"",
            "automatic file discovery must never turn into an owner constraint");
    }

    #[test]
    fn invalid_generated_search_expression_cannot_redirect_an_explicit_goal() {
        let candidate=json!({"implementation":"expression","parameters":{
            "expression":"unknownRuntimeFeature + 1"
        }});
        assert_eq!(select_domain(&candidate,&[],"브라우저 도구 연결"),"tools");
        assert_eq!(select_domain(&candidate,&[],"자기개선 알고리즘"),"improver");
        let history=vec![json!({"domain":"memory","accepted":true,"tokens":1})];
        assert_eq!(select_domain(&candidate,&history,"화면 버튼 개선"),"ui");
    }

    #[test]
    fn worker_feedback_is_valid_json_and_compacts_before_token_overflow() {
        let prior=vec![json!({
            "domain":"memory","accepted":false,"tokens":20_000,
            "error":"too much telemetry ".repeat(2000),
            "evaluation":{"verdict":"inconclusive","metrics":{
                "measurements":[{"id":"quality","metrics":[{"improved":true,"regressed":false}]}]
            }}
        })];
        let (minimal,_) = fit_worker_prompt("owner objective","memory","memory",&[],None).unwrap();
        let max_tokens=crate::engine::chat::estimate_text_tokens(&minimal)+256+64;
        let (prompt,plan)=fit_worker_prompt("owner objective","memory","memory",&prior,
            Some(max_tokens)).unwrap();
        assert!(prompt.starts_with("owner objective"));
        assert_ne!(plan["mode"],"full");
        assert!(crate::engine::chat::estimate_text_tokens(&prompt)+256<=max_tokens);
        assert!(fit_worker_prompt("owner objective","memory","memory",&prior,Some(1))
            .unwrap_err().starts_with("rsi_token_budget:"));
        let (unbounded,_) = fit_worker_prompt("owner objective","memory","memory",&prior,None).unwrap();
        let json_text=unbounded.split("이 실험 계보의 과거 결과: ").nth(1).unwrap();
        assert!(serde_json::from_str::<Value>(json_text).unwrap().is_array(),
            "feedback must be complete, parsable JSON rather than truncated bytes");
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
                        let (_,source)=content.trim_start().split_once(' ').unwrap();
                        let knowledge:Value=serde_json::from_str(source.trim()).unwrap();
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
    fn autonomous_trial_queue_identity_matches_descendant_lookup_objective() {
        let input=json!({"rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 자기개선"});
        assert_eq!(selection_objective(&input),"");
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"aligned-goal",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"exploration + objectiveMatch"}})).unwrap();
        let objective=selection_objective(&input);
        let report=queue_search_trial(&state,&candidate,&active,&[],&objective,"campaign",1);
        assert_eq!(report["status"],"pending_trial");
        let pending=pending_trial_candidate(&state,&objective,&["campaign".into()]);
        assert_eq!(pending.unwrap()["id"],candidate["id"],
            "a policy queued by an autonomous campaign must be selectable by its child");
        let raw_prompt=input["rsiObjective"].as_str().unwrap();
        assert!(pending_trial_candidate(&state,raw_prompt,&["campaign".into()]).is_none(),
            "generic system wording must not replace the canonical selection objective");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn observed_quality_gain_autonomously_queues_a_real_policy_trial() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let baseline=active["id"].clone();
        let base="a".repeat(40);
        let accepted_commit="b".repeat(40);
        let plan=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "domain":"tools","holdout":n<3,"metrics":[]}))
            .collect::<Vec<_>>();
        let measurements=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "baselinePass":n!=0,"candidatePass":true}))
            .collect::<Vec<_>>();
        let metrics=crate::engine::rsi::evaluate_cases(&plan,&measurements,"harness").unwrap();
        state.store.put("rsi_candidate",&json!({
            "id":"measured","target":"harness","status":"verified","cases":plan,
            "baseCommit":base,"patchSha256":"p".repeat(64),
            "oracleVersion":"frozen-v1","experimentAdoption":{"commit":accepted_commit},
            "evaluation":{"metrics":metrics,"verdict":metrics["verdict"],
                "source":"server_invoked_local_paired_git_worktrees","evidenceSha256":"e".repeat(64)}
        })).unwrap();
        state.store.put("rsi_pair",&json!({
            "id":"rsi-pair-measured","candidateId":"measured",
            "runner":"local_gitrepo_not_os_sandbox","evidenceSha256":"e".repeat(64),
            "report":{"baseCommit":base,"patchSha256":"p".repeat(64),
                "oracleVersion":"frozen-v1","reportSha256":"e".repeat(64),
                "baselineBuildOk":true,"candidateBuildOk":true,"cases":measurements},
            "metrics":metrics,"verdict":metrics["verdict"]
        })).unwrap();
        let history=vec![
            json!({"id":"1","domain":"memory","accepted":false,"newFindings":0,"tokens":5000}),
            json!({"id":"2","domain":"tools","accepted":true,"tokens":7000,
                "candidateId":"measured","generatorCommit":base,
                "acceptedCommit":accepted_commit}),
            json!({"id":"3","domain":"memory","accepted":false,"newFindings":0,"tokens":14000}),
            json!({"id":"4","domain":"tools","accepted":false,"newFindings":1,"tokens":18000})
        ];
        let queued=queue_adaptive_search_trial(&state,&history,&active,
            "","campaign",4).expect("verified outcomes must generate a trialable policy");
        assert_eq!(queued["status"],"pending_trial");
        let id=queued["candidateId"].as_str().unwrap();
        let new_policy=state.store.get("algorithm",id).unwrap();
        assert_eq!(new_policy["source"],"rsi-generated");
        assert_eq!(new_policy["rsiTrial"]["campaignId"],"campaign");
        assert_eq!(new_policy["rsiTrial"]["status"],"pending_trial");
        assert!(new_policy["parameters"]["expression"].as_str().unwrap().contains("holdoutGains"));
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            baseline,"automatic proposal must not activate without later real trials");
        let old=state.store.get("rsi_pair","rsi-pair-measured").unwrap();
        let mut tampered=old.clone();
        tampered["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&tampered).unwrap();
        let revalidated=history_with_verified_payoffs(&state,&history);
        assert_eq!(revalidated[1]["evidenceVerified"],false);
        assert!(adaptive_search_expression(&revalidated).is_none(),
            "mutable outcome claims cannot bootstrap an evolved policy");
        let no_proof=queue_adaptive_search_trial(&state,&history,&active,
            "","fresh-campaign",4);
        assert!(no_proof.is_none(),"forged raw report must not generate an improved policy");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn self_improver_evolves_an_executable_search_formula_from_measured_history() {
        let rows=vec![
            json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":5000}),
            json!({"domain":"tools","accepted":true,"evidenceVerified":true,
                "verifiedHoldoutGains":2,"tokens":6000}),
            json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":9000}),
            json!({"domain":"tools","accepted":false,"newFindings":1,"tokens":11000})
        ];
        assert!(adaptive_search_expression(&rows[..3]).is_none());
        let expression=adaptive_search_expression(&rows).unwrap();
        let candidate=json!({"implementation":"expression","parameters":{"expression":expression}});
        assert!(scored_domains(&candidate,&rows,"").is_some(),
            "self-generated policy must run in the ordinary search algorithm kernel");
        let mut later=rows.clone();
        later.push(json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":40000}));
        let revised=adaptive_search_expression(&later).unwrap();
        assert_ne!(expression,revised,"the formula itself must evolve with observed outcomes");
        let no_proof=rows.iter().map(|row|{
            let mut row=row.clone();
            row["evidenceVerified"]=json!(false);
            row
        }).collect::<Vec<_>>();
        assert!(adaptive_search_expression(&no_proof).is_none(),
            "claimed acceptance without independently verified payoff is insufficient");
    }

    #[test]
    fn repeating_a_failed_patch_is_skipped_but_a_new_base_or_patch_can_retry() {
        let (state,root)=state();
        let base="a".repeat(40);
        let digest="d".repeat(64);
        state.store.put("rsi_candidate",&json!({"id":"old","baseCommit":base,
            "patchSha256":digest})).unwrap();
        state.store.put("rsi_candidate",&json!({"id":"new","baseCommit":base,
            "patchSha256":digest})).unwrap();
        let mut history=vec![json!({"candidateId":"old","accepted":false,
            "evaluation":{"verdict":"regressed"}})];
        assert_eq!(identical_failed_patch(&state,&history,"new"),Some("old".into()));
        history[0]["accepted"]=json!(true);
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        history[0]["accepted"]=json!(false);
        history[0]["evaluation"]["verdict"]=json!("unmeasured");
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        history[0]["evaluation"]["verdict"]=json!("inconclusive");
        let mut changed=state.store.get("rsi_candidate","new").unwrap();
        changed["baseCommit"]=json!("b".repeat(40));
        state.store.put("rsi_candidate",&changed).unwrap();
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        changed["baseCommit"]=json!(base);
        changed["patchSha256"]=json!("e".repeat(64));
        state.store.put("rsi_candidate",&changed).unwrap();
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_authored_search_formulas_receive_every_advertised_runtime_feature() {
        let history=vec![
            json!({"domain":"tools","accepted":false,"newFindings":3,"tokens":1000}),
            json!({"domain":"tools","accepted":false,"newFindings":0,"tokens":2000})
        ];
        let features=search_features(&history,"tools","tools");
        for key in crate::engine::algorithms::SELFIMPROVE_FEATURES {
            assert!(features.contains_key(*key),"advertised feature missing: {key}");
        }
        assert_eq!(features["usefulGenerations"],1.0);
        assert_eq!(features["barrenRate"],0.5);
        let candidate=json!({"implementation":"expression","parameters":{
            "expression":"usefulGenerations + verifiedOutcomes + holdoutGains + gainPer10kTokens - barrenRate"
        }});
        assert!(score_domain(&candidate,&features).is_some(),
            "a registered model-authored formula must not silently fall back at runtime");
    }

    #[test]
    fn original_paired_holdout_gains_change_the_next_improvement_target() {
        let (state,root)=state();
        let policy=json!({"implementation":"failure-clusters"});
        let base="a".repeat(40);
        let accepted_commit="b".repeat(40);
        let plan=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "domain":"tools","holdout":n<3,"metrics":[]}))
            .collect::<Vec<_>>();
        let raw=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "baselinePass":n!=0,"candidatePass":true}))
            .collect::<Vec<_>>();
        let metrics=crate::engine::rsi::evaluate_cases(&plan,&raw,"harness").unwrap();
        assert_eq!(metrics["eligible"],true);
        let report=json!({"baseCommit":base,"patchSha256":"p".repeat(64),
            "oracleVersion":"frozen-v1","reportSha256":"e".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,"cases":raw});
        let candidate=json!({"id":"tools-verified","status":"verified",
            "baseCommit":base,"patchSha256":"p".repeat(64),"oracleVersion":"frozen-v1",
            "target":"harness","cases":plan,
            "experimentAdoption":{"commit":accepted_commit},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"e".repeat(64),"metrics":metrics,"verdict":metrics["verdict"]}});
        state.store.put("rsi_candidate",&candidate).unwrap();
        state.store.put("rsi_pair",&json!({"id":"rsi-pair-tools-verified",
            "candidateId":"tools-verified","runner":"local_gitrepo_not_os_sandbox",
            "evidenceSha256":"e".repeat(64),"report":report,
            "metrics":metrics,"verdict":metrics["verdict"]})).unwrap();
        // The older accepted-count heuristic sees both domains as equal
        // and chooses the first domain, not the one with the actual gain.
        let history=vec![
            json!({"domain":"memory","candidateId":"memory-unproven",
                "accepted":true,"tokens":10000,"generatorCommit":base,
                "acceptedCommit":accepted_commit}),
            json!({"domain":"tools","candidateId":"tools-verified",
                "accepted":true,"tokens":10000,"generatorCommit":base,
                "acceptedCommit":accepted_commit})
        ];
        assert_eq!(select_domain(&policy,&history,"memory tools"),"memory");
        let verified=history_with_verified_payoffs(&state,&history);
        assert_eq!(verified[0]["evidenceVerified"],false);
        assert_eq!(verified[1]["verifiedHoldoutGains"],1);
        assert_eq!(select_domain(&policy,&verified,"memory tools"),"tools",
            "replayed verified outcome must guide the next actual domain selection");
        assert_eq!(select_domain(&policy,&verified,"memory"),"memory",
            "the user's explicit objective stays authoritative");
        // A forged generation summary without a matching raw paired receipt
        // must not earn meta-learning credit.
        let mut forged=history.clone();
        forged[0]["evidenceVerified"]=json!(true);
        forged[0]["verifiedHoldoutGains"]=json!(64);
        let rechecked=history_with_verified_payoffs(&state,&forged);
        assert_eq!(rechecked[0]["verifiedHoldoutGains"],0);
        let mut tampered=state.store.get("rsi_pair","rsi-pair-tools-verified").unwrap();
        tampered["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&tampered).unwrap();
        assert_eq!(select_domain(&policy,&history_with_verified_payoffs(&state,&history),
            "memory tools"),"memory","altered raw evidence cannot steer the search policy");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_generation_changes_the_next_model_experiment_instructions() {
        let (state,root)=state();
        state.store.put("rsi_candidate",&json!({"id":"failed-patch",
            "hypothesis":"과거 가설을 반복해도 성능이 향상될 것이다"}));
        let record=json!({"history":[{"attempt":1,"domain":"memory",
            "accepted":false,"candidateId":"failed-patch",
            "evaluation":{"verdict":"regressed"},"error":"baseline 대비 품질 하락"}]});
        let prompt=generation_context(&state,&json!({
            "rsiTargetDomain":"memory","rsiObjective":"기억 성능 개선"}),&record);
        assert!(prompt.contains("실패를 반영한 다음 개선 실험"),"{prompt}");
        assert!(prompt.contains("과거 가설을 반복해도 성능이 향상될 것이다"),"{prompt}");
        assert!(prompt.contains("수정 범위·가설·기대하는 작업 성능 지표를 바꾼 실험"),"{prompt}");
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
    fn search_trial_control_uses_candidate_domain_then_unlocks_real_trial() {
        let (state,root)=state();
        let history=vec![generation("tools",false)];
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"sub3-prefer-attempted",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"attempts * 100 + objectiveMatch"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        assert_eq!(select_domain(&candidate,&history,"unrelated goal"),"tools");
        assert_ne!(select_domain(&active,&history,"unrelated goal"),"tools");
        let (domain,mode,strategy)=select_generation_strategy(
            &state,&history,"unrelated goal",None,Some(id.into()),None,"commit-one");
        assert_eq!(domain,"tools");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(strategy["controlForCandidateId"],id);
        let mut updated=history;
        updated.push(active_generation("control",2,"tools",false,"commit-one",1000,100));
        let (domain,mode,_)=select_generation_strategy(
            &state,&updated,"unrelated goal",None,Some(id.into()),None,"commit-one");
        assert_eq!(domain,"tools");
        assert_eq!(mode,"algorithm_trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn unresolved_trial_cannot_be_claimed_by_sibling_campaign() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let first=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"first-owned",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + novelty"}
        })).unwrap();
        let second=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"second-owned",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration"}
        })).unwrap();
        assert_eq!(queue_search_trial(&state,&first,&active,&[],"context","campaign-a",1)["status"],"pending_trial");
        assert_eq!(queue_search_trial(&state,&second,&active,&[],"context","campaign-b",1)["status"],"pending_trial");
        assert_eq!(pending_trial_candidate(&state,"context",&["campaign-a".into()]).unwrap()["id"],first["id"]);
        assert_eq!(pending_trial_candidate(&state,"context",&["campaign-b".into()]).unwrap()["id"],second["id"]);
        assert!(pending_trial_candidate(&state,"context",&["unknown".into()]).is_none());
        // Requeue the old in-memory object: ownership must be checked against
        // the latest stored copy, not overwritten by a stale proposal.
        let duplicate=queue_search_trial(&state,&first,&active,&[],"context","campaign-b",2);
        assert_eq!(duplicate["status"],"owned_by_other_campaign");
        assert_eq!(duplicate["ownerCampaignId"],"campaign-a");
        assert_eq!(state.store.get("algorithm",first["id"].as_str().unwrap()).unwrap()
            ["rsiTrial"]["campaignId"],"campaign-a");
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
    #[test]
    fn autonomous_barren_generations_execute_their_evolved_policy_without_false_promotion() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let active_id=active["id"].clone();
        let history=(0..3).map(|n|json!({
            "id":format!("generation-{n}"),
            "jobId":format!("job-{n}"),"attempt":n+1,
            "domain":"tools","accepted":false,"candidateId":Value::Null,
            "newFindings":0,"tokens":5000+n*1000,
            "domainSelection":"algorithm_active","generatorCommit":"commit-a"
        })).collect::<Vec<_>>();
        assert!(recovery_search_expression(&history[..2]).is_none());
        let report=queue_adaptive_search_trial(&state,&history,&active,"","campaign",3)
            .expect("three real, barren generations must propose an executable alternative");
        assert_eq!(report["hypothesisSource"],"observed_unproductive_generations");
        let id=report["candidateId"].as_str().unwrap();
        let candidate=state.store.get("algorithm",id).unwrap();
        assert!(scored_domains(&candidate,&history,"").is_some());
        assert_eq!(candidate["rsiTrial"]["status"],"pending_trial");
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            active_id,"a recovery hypothesis is not improvement evidence");
        // Even when research yielded an apparent continuation, the scheduled
        // candidate must get control measurements, then an actual trial.
        let (domain,mode,strategy)=select_generation_strategy(
            &state,&history,"",Some("memory".into()),Some(id.into()),None,"commit-b");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(strategy["controlForCandidateId"],id);
        let mut later=history.clone();
        later.push(json!({"jobId":"control-1","domain":domain,
            "domainSelection":"algorithm_active","generatorCommit":"commit-b",
            "searchStrategy":strategy,
            "candidateId":Value::Null,"accepted":false,"tokens":3000}));
        later.push(json!({"jobId":"control-2","domain":domain,
            "domainSelection":"algorithm_active","generatorCommit":"commit-b",
            "candidateId":Value::Null,"accepted":false,"tokens":4000}));
        let (trial_domain,trial_mode,trial_strategy)=select_generation_strategy(
            &state,&later,"",Some("memory".into()),Some(id.into()),None,"commit-b");
        assert_eq!(trial_domain,domain);
        assert_eq!(trial_mode,"algorithm_trial");
        assert_eq!(trial_strategy["id"],id);
        assert_eq!(trial_strategy["trialFallback"],"exploratory_after_barren_controls");
        let trial=trial_generation("trial-1",6,&domain,false,"commit-b",1000,100,id);
        let outcome=record_trial_outcome(&state,id,&trial,&later);
        assert_eq!(outcome["status"],"inconclusive",
            "a real exploratory run with no paired outcome is not adoption");
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            active_id);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn neutral_descendant_reloads_current_source_instead_of_stale_base_excerpt() {
        let variants=generation_prompt_variants(
            "Improve behavior", "Paired quality unchanged",
            "STALE_READ_RANGE=10-20", "STALE_BASE_SHA=abc", Some("neutral-id"));
        assert_eq!(variants.len(),2);
        for (mode, prompt, defer_focus, defer_research) in variants {
            assert!(mode.starts_with("neutral_"));
            assert!(defer_focus && defer_research);
            assert!(prompt.contains("neutral-id"));
            assert!(prompt.contains("workspace_read_range"));
            assert!(!prompt.contains("STALE_READ_RANGE"));
            assert!(!prompt.contains("STALE_BASE_SHA"));
            assert!(prompt.contains("실질적으로 다른 동작"),
                "child prompt must demand behavioral correction");
        }
    }

    #[test]
    fn ordinary_generation_still_has_full_and_deferred_source_context() {
        let variants=generation_prompt_variants(
            "Improve behavior", "measured", "research", "CURRENT_SOURCE", None);
        assert_eq!(variants.len(),3);
        assert_eq!(variants[0].0,"full");
        assert!(variants[0].1.contains("CURRENT_SOURCE"));
        assert!(!variants[0].2 && !variants[0].3);
        assert!(variants[2].2 && variants[2].3);
    }

    #[test]
    fn autonomous_recovery_budget_is_bounded_but_does_not_stop_before_trial() {
        assert!(should_stop_no_progress(1,false,false,false,false));
        assert!(!should_stop_no_progress(1,false,false,false,true));
        assert!(!should_stop_no_progress(2,false,false,false,true));
        assert!(should_stop_no_progress(3,false,false,false,true));
        assert!(!should_stop_no_progress(4,false,false,true,true));
        assert!(!should_stop_no_progress(5,false,false,true,true));
        assert!(should_stop_no_progress(6,false,false,true,true));
        assert!(!should_stop_no_progress(2,true,true,false,true));
    }

}
