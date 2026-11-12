// Inline module tests extracted from rsi_evolve.rs. Keep this file
// sibling to rsi_evolve.rs so include_str! relative paths remain stable.
    use super::*;

    #[test]
    fn observed_meta_method_failure_reaches_compiled_focus_without_owner_override() {
        let repair=json!({
            "metaRepairChosen":true,"mode":"repair_own_transfer_method",
            "target":{"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs",
                "sourceSha256":"a".repeat(64)}
        });
        assert!(evidence_based_meta_repair_focus(&repair,&Value::Null,false));
        assert!(!evidence_based_meta_repair_focus(&repair,&Value::Null,true),
            "explicit owner-selected source must not be overridden");
        assert!(!evidence_based_meta_repair_focus(&repair,
            &json!({"source":"model_authored_open_program_stage"}),false),
            "independently adopted predecessor programme keeps priority");
        let mut wrong=repair.clone();
        wrong["target"]["path"]=json!("src/engine/tools.rs");
        assert!(!evidence_based_meta_repair_focus(&wrong,&Value::Null,false));
        wrong=repair.clone();
        wrong["metaRepairChosen"]=json!(false);
        assert!(!evidence_based_meta_repair_focus(&wrong,&Value::Null,false));
        wrong=repair.clone();
        wrong["mode"]=json!("test_cross_role_portable_method");
        assert!(!evidence_based_meta_repair_focus(&wrong,&Value::Null,false));
        wrong["mode"]=json!("repair_own_prediction_mechanism");
        assert!(evidence_based_meta_repair_focus(&wrong,&Value::Null,false),
            "forecast-driven self-repair uses the same executable focus path");
    }

    #[test]
    fn bounded_local_excerpts_keep_source_lines_and_request_exact_missing_ranges() {
        let mut excerpt=String::from("src/engine/memory.rs\n");
        excerpt.push_str(&"let result = retrieve_memory();\n".repeat(300));
        let mut text=String::new();
        assert!(!append_bounded_excerpt(&mut text,&excerpt,1500));
        assert!(text.starts_with("src/engine/memory.rs\n"));
        assert!(text.ends_with("[발췌 제한: 빠진 범위는 workspace_read_range로 읽으세요.]\n"));
        assert!(text.chars().count()<=1500);
        assert!(!text.contains("retrieve_memo\n"));
        let mut short=String::new();
        assert!(append_bounded_excerpt(&mut short,"src/engine/tools.rs\nfn helper() {}\n",6000));
        assert!(short.contains("fn helper() {}"));
    }
    #[test]
    fn action_feedback_replaces_repeated_research_with_real_edit_instructions() {
        let research=json!({"attempt":1,"domain":"memory","newFindings":2,
            "accepted":false,"candidateId":null,
            "actionDirective":{"mode":"discover"},
            "toolsUsed":[{"tool":"code_search","calls":2}]});
        let directive=next_execution_action(&[research.clone()]);
        assert_eq!(directive["mode"],"implement_research");
        let no_edit=json!({"attempt":2,"domain":"memory","newFindings":1,
            "accepted":false,"candidateId":null,"actionDirective":directive,
            "actionObservation":{"status":"no_edit","editToolCalls":0}});
        assert_eq!(next_execution_action(&[research,no_edit])["mode"],"change_approach");
        let edited=json!({"attempt":3,"domain":"memory","newFindings":0,
            "accepted":false,"candidateId":null,
            "actionObservation":actual_edit_observation(
                &[json!({"tool":"workspace_replace","calls":1})],false)});
        assert_eq!(next_execution_action(&[edited])["mode"],"complete_candidate");
        let failed=json!({"attempt":4,"domain":"memory","candidateId":"candidate-1",
            "evaluation":{"verdict":"regressed"},
            "actionDirective":{"mode":"revise_candidate"}});
        assert_eq!(next_execution_action(&[failed])["mode"],"repair_failed_case");
        let changed=json!({"attempt":5,"domain":"memory","candidateId":"candidate-2",
            "evaluation":{"verdict":"regressed"},
            "actionDirective":{"mode":"repair_failed_case"}});
        assert_eq!(next_execution_action(&[changed])["mode"],"change_approach");
    }

    #[test]
    fn source_edit_receipts_cannot_be_faked_by_search_or_model_text() {
        assert_eq!(actual_edit_observation(
            &[json!({"tool":"code_search","calls":4})],false)["status"],"no_edit");
        let edited=actual_edit_observation(
            &[json!({"tool":"workspace_write","calls":2}),
              json!({"tool":"workspace_replace","calls":1})],false);
        assert_eq!(edited["status"],"edit_attempted");
        assert_eq!(edited["editToolCalls"],3);
        assert_eq!(edited["candidateCreated"],false);
    }
    #[test]
    fn compiled_descendant_source_excerpt_is_real_indexed_rust_not_research_text() {
        let (state,root)=state();
        let checkout=root.join("descendant-excerpt");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::write(checkout.join("src/engine/memory.rs"),
            "pub fn new_memory_policy() -> u32 {\n    19\n}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&checkout).unwrap();
        let source_sha=crate::engine::code_index::indexed_source_hash(
            &state,index["id"].as_str().unwrap(),"src/engine/memory.rs").unwrap();
        let input=json!({
            "rsiIndexId":index["id"],
            "rsiFocusFiles":["src/engine/memory.rs"],
            "rsiTargetDomain":"memory",
            "rsiObjective":"memory improvement",
            "rsiAutonomous":true,
            "rsiAutoFocus":true
        });
        let excerpt=focus_file_context(&state,&json!({"labDir":checkout}),&input);
        assert!(excerpt.contains("new_memory_policy"),
            "the real descendant must see implementation bytes, not research notes");
        assert!(excerpt.contains(&source_sha),
            "editing requires its actual current SHA, not a coordinator guess");
        assert!(excerpt.contains("src/engine/memory.rs"));
        assert!(!excerpt.contains("PRIOR RESEARCH"));
    }
    #[test]
    fn compiled_descendant_plans_from_its_actual_whole_source_graph() {
        let (state,root)=state();
        let source=root.join("architecture");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/rsi_evolve.rs"),
            "fn next_improvement() { /* inherited revision */ }\n").unwrap();
        std::fs::write(source.join("src/engine/room_state.rs"),
            "fn recall() { /* baseline implementation */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        let commit="a".repeat(40);
        let input=json!({
            "rsiSystemEvolution":true,
            "rsiSourceDir":source,
            "rsiGeneratorCommit":commit,
            "rsiIndexId":index["id"],
            "rsiObjective":"기억 및 자기개선 소스의 실행 방식을 바꿔라",
            "rsiSelectedDomain":"memory",
            "rsiAutonomous":true,
            "rsiAutoFocus":true,
            "rsiFocusFiles":[]
        });
        let first=compiled_system_generation_plan(
            &state,&input,&[],&commit).unwrap();
        assert_eq!(first["source"],"compiled_descendant_system_controller_v3");
        assert_eq!(first["generatorCommit"],commit);
        assert_eq!(first["historyCount"],0);
        assert_eq!(first["systemBlueprint"]["architecture"].as_array().unwrap().len(),7);
        assert_eq!(first["systemBlueprint"]["sourceTreeSha256"],
            first["sourceTreeSha256"]);
        assert_eq!(first["decisionOwner"],"rebuilt_descendant_binary");
        assert_eq!(first["improvementDirective"]["mode"],"discover");
        // The new binary must re-inspect the actual modified code graph.
        std::fs::write(source.join("src/engine/room_state.rs"),
            "fn recall() { /* NEW inherited implementation */ }\n").unwrap();
        let revised=compiled_system_generation_plan(
            &state,&input,&[],&commit).unwrap();
        assert_ne!(first["sourceTreeSha256"],revised["sourceTreeSha256"]);
        // Identity is a real code revision guard, not a prompt label.
        assert!(compiled_system_generation_plan(
            &state,&input,&[],"b").is_err());
        let mut disabled=input.clone();
        disabled["rsiSystemEvolution"]=json!(false);
        assert!(compiled_system_generation_plan(
            &state,&disabled,&[],&commit).is_err());
        let _=std::fs::remove_dir_all(root);
    }

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
    fn autonomous_learned_action_enforces_a_real_single_file_edit_target() {
        let (state,root)=state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/alpha.rs"),"fn alpha() {}\n").unwrap();
        std::fs::write(source.join("src/engine/beta.rs"),"fn beta() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        let mut input=json!({
            "rsiAutoFocus":true,"rsiIndexId":index["id"],
            "rsiFocusContextChars":240,
            "rsiFocusGrounded":true,
            "rsiFocusFiles":["src/engine/alpha.rs","src/engine/beta.rs"],
            "rsiFocusSelection":{"source":"indexed_evidence"},
            "rsiActionDirective":{"mode":"repair_failed_edit",
                "policy":{"revision":"learned-v2"}}
        });
        let previous=json!({"editActions":[
            {"path":"src/engine/beta.rs","written":false,"error":"stale SHA"}]});
        assert!(bind_action_to_source(&state,&mut input,&[previous]));
        assert_eq!(input["rsiFocusFiles"],json!(["src/engine/beta.rs"]));
        assert_eq!(input["rsiFocusSelection"]["usedFailedWriteProvenance"],true);
        assert_eq!(input["rsiFocusSelection"]["actionPolicyRevision"],"learned-v2");
        let focus=crate::engine::code_index::bounded_focus_context(
            &state,&source,index["id"].as_str().unwrap(),
            "src/engine/beta.rs","beta","tools").unwrap();
        assert!(focus_excerpt_has_source(&input,&focus),
            "the selected file's source hash must exist in the supplied excerpt");
        assert!(policy_requests_source_edit(&input),
            "the next native worker must enter its actual edit checkpoint");
        input["rsiFocusGrounded"]=json!(false);
        assert!(!policy_requests_source_edit(&input),
            "a nonempty prompt header is not verified source evidence");
        input["rsiFocusGrounded"]=json!(true);
        input["rsiFocusContextChars"]=json!(0);
        assert!(!policy_requests_source_edit(&input),
            "without verified source evidence an autonomous edit is not forced");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_discovery_keeps_broad_source_search() {
        let (state,root)=state();
        let mut input=json!({"rsiAutoFocus":true,
            "rsiIndexId":"idx","rsiFocusContextChars":100,
            "rsiFocusFiles":["src/engine/example.rs"],
            "rsiActionDirective":{"mode":"discover"}});
        assert!(!policy_requests_source_edit(&input));
        assert!(!bind_action_to_source(&state,&mut input,&[]));
        assert_eq!(input["rsiFocusFiles"],json!(["src/engine/example.rs"]));
        input["rsiActionDirective"]["mode"]=json!("repair_failed_edit");
        assert!(!bind_action_to_source(&state,&mut input,&[]),
            "stale or unindexed files cannot be promoted as edit targets");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_recovery_policy_mutates_after_repeated_real_failures() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"improve actual edit recovery",
            "rsiSourceDir":root.join("source").to_string_lossy()});
        let history=(1..=4).map(|n|json!({
            "id":format!("g{n}"),"jobId":format!("job{n}"),
            "campaignId":"c","attempt":n,
            "createdAt":format!("2026-11-11T01:00:{n:02}Z"),
            "accepted":false,"tokens":10000,
            "actionObservation":{"status":"edit_failed","editToolCalls":1,
                "appliedEditCalls":0,"sourceDiffBytes":0},
            "actionDirective":{"mode":"repair_failed_edit"}
        })).collect::<Vec<_>>();
        let decision=evolved_execution_action(&state,&input,&history);
        assert_eq!(decision["policy"]["kind"],"selfimprove.action");
        assert_eq!(decision["policy"]["trainingTransitions"],3);
        assert_eq!(decision["policy"]["previousDefault"],"repair_failed_edit");
        assert_ne!(decision["mode"],"repair_failed_edit",
            "repeating a failed source-edit repair should evolve the strategy: {decision}");
        assert_eq!(decision["policy"]["status"],"exploratory_policy_mutation");
        assert_eq!(decision["policy"]["implementation"],
            "synthesized_symbolic_action_program");
        assert!(decision["policy"]["scoreExpression"].as_str()
            .is_some_and(|expression|expression.contains("failureRate")));
        assert_eq!(decision["policy"]["programStructure"]["hasAdoptionReward"],false);
        assert!(decision["policy"]["measuredGain"].is_null());
        // An accepted improvement in the same research scope actually
        // changes the generated executable formula, not merely the label.
        let mut inherited=history.clone();
        inherited.insert(0,json!({"id":"previous-adopted",
            "jobId":"previous-adopted-job","campaignId":"older",
            "attempt":1,"createdAt":"2026-11-10T00:00:00Z",
            "accepted":true,"tokens":5000}));
        let revised=evolved_execution_action(&state,&input,&inherited);
        assert_eq!(revised["policy"]["programStructure"]["hasAdoptionReward"],true);
        assert!(revised["policy"]["scoreExpression"].as_str()
            .is_some_and(|expression|expression.contains("verifiedAdoption")));
        assert_ne!(decision["policy"]["revision"],revised["policy"]["revision"]);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn action_selector_can_evolve_its_own_exploration_coefficient() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"meta optimize recovery policy",
            "rsiSourceDir":root.join("repo").to_string_lossy()});
        // The same failure context is retained even when a later generation
        // makes a new finding; discoveries must not masquerade as adoption.
        let history=(1..=6).map(|n|{
            let mutated=(n==2 || n==3);
            json!({
                "id":format!("g{n}"),"jobId":format!("j{n}"),
                "campaignId":"c","attempt":n,
                "createdAt":format!("2026-11-11T01:00:{n:02}Z"),
                "accepted":false,"tokens":1000,
                "newFindings":if mutated {1}else{0},
                "actionObservation":{"status":"edit_failed",
                    "sourceDiffBytes":0,"editToolCalls":1,"appliedEditCalls":0},
                "actionDirective":{
                    "mode":if mutated {"change_approach"}else{"repair_failed_edit"},
                    "policy":{"status":if mutated {"exploratory_policy_mutation"}
                        else {"evidence_based_retention"}}
                }
            })
        }).collect::<Vec<_>>();
        let next=evolved_execution_action(&state,&input,&history);
        assert_eq!(next["policy"]["trainingTransitions"],5);
        assert_eq!(next["policy"]["metaTuning"]["mutatedObservations"],2);
        assert_eq!(next["policy"]["metaTuning"]["retainedObservations"],3);
        assert!(next["policy"]["metaTuning"]["explorationCoefficient"]
            .as_f64().unwrap()>0.55,
            "successful research progress in a mutated method changes
             the learner's own exploration parameter: {next}");
        assert!(next["policy"]["metaTuning"]["causalAttribution"]=="unmeasured");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_policy_does_not_credit_a_strategy_that_never_reached_the_runtime() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"control the actual recovery harness",
            "rsiSourceDir":root.join("source").to_string_lossy()});
        let mut history=(1..=3).map(|n|json!({
            "id":format!("generation-{n}"),
            "jobId":format!("job-{n}"),
            "campaignId":"campaign","attempt":n,
            "createdAt":format!("2026-11-11T03:00:{n:02}Z"),
            "accepted":false,"tokens":2000,
            "actionObservation":{"status":"edit_failed","editToolCalls":1,
                "appliedEditCalls":0},
            "actionDirective":{"mode":"repair_failed_edit",
                "policy":{"kind":"selfimprove.action","status":"exploratory_policy_mutation"}},
            "actionExecution":{"focusApplied":false}
        })).collect::<Vec<_>>();
        let skipped=evolved_execution_action(&state,&input,&history);
        assert_eq!(skipped["policy"]["trainingTransitions"],0);
        assert_eq!(skipped["policy"]["status"],"insufficient_evidence");
        for row in &mut history {row["actionExecution"]["focusApplied"]=json!(true);}
        let applied=evolved_execution_action(&state,&input,&history);
        assert_eq!(applied["policy"]["trainingTransitions"],2);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_action_does_not_count_patches_as_adopted_capabilities() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"evolve the harness",
            "rsiSourceDir":root.join("src").to_string_lossy()});
        let sample=json!({"accepted":false,"candidateId":Value::Null,
            "actionObservation":{"status":"source_changed_unverified",
                "sourceDiffBytes":250},"tokens":1200});
        assert!(observed_action_value(&sample)<1.0);
        let last=json!({"campaignId":"campaign","attempt":1,"id":"g1",
            "accepted":true,"actionObservation":{"status":"candidate_created"}});
        let decision=evolved_execution_action(&state,&input,&[last]);
        assert_eq!(decision["mode"],"extend_verified_capability");
        let _=std::fs::remove_dir_all(root);
    }

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
        // Reuse an earlier campaign's read ledger only for a verified exact
        // descendant. The ID alone is not a source-lineage receipt.
        let prior=json!({"id":"digest-prior","sourceDir":"/repo","sourceBranch":"dominion-sub2",
            "baseCommit":"a".repeat(40),"sourceFingerprint":"b".repeat(64),
            "headCommit":"c".repeat(40),"history":[{
                "jobId":"digest-job","generatorCommit":"c".repeat(40)}]});
        state.store.put("rsi_campaign",&prior);
        let current=json!({"history":[],"sourceDir":"/repo","sourceBranch":"dominion-sub2",
            "baseCommit":"a".repeat(40),"sourceFingerprint":"b".repeat(64),
            "headCommit":"c".repeat(40),
            "lineage":{"reused":true,"selectedCommit":"c".repeat(40)}});
        let previous=json!({"rsiContinueFrom":"digest-prior"});
        let continued=research_digest(&state,&previous,&current);
        assert!(continued.contains("src/engine/memory.rs 10-20"));
        let mut revised=current.clone();
        revised["headCommit"]=json!("d".repeat(40));
        assert!(!research_digest(&state,&previous,&revised).contains("src/engine/memory.rs 10-20"),
            "read offsets from an old source revision must not suppress new inspections");
        let mut foreign=current.clone();
        foreign["sourceBranch"]=json!("dominion-sub3");
        assert!(!research_digest(&state,&previous,&foreign).contains("src/engine/memory.rs 10-20"),
            "a sibling branch must not inject source offsets into a new campaign");
        let _ = std::fs::remove_dir_all(root);
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
        let original="fn search_dispatch() { println!(\"search\"); }\n";
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),original).unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        assert_eq!(choose_focus_files(&state,&input,&[],None)[0]["path"],"src/alpha.rs");
        let selected_beta=crate::engine::code_index::rank_focus_files(
            &state,index["id"].as_str().unwrap(),"search dispatch","tools",
            &BTreeMap::new(),None,2).into_iter()
            .find(|v|v["path"]=="src/beta.rs").unwrap();
        // Model really edits beta; its post-adoption hash is different from
        // the revision that the target selector originally examined.
        let improved="fn search_dispatch() { println!(\"search improved\"); }\n";
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

        // Reverting to *exactly* the pre-patch content must NOT resurrect
        // reward based on the old focusSelection.sourceHash.
        std::fs::write(&beta,original).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let reverted=choose_focus_files(&state,&input,&[],None);
        assert_eq!(reverted[0]["path"],"src/alpha.rs");
        assert_eq!(reverted.iter().find(|v|v["path"]=="src/beta.rs").unwrap()
            ["evidence"]["priorProductive"],0.0);

        std::fs::write(&beta,"fn search_dispatch() { println!(\"new revision\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let stale=choose_focus_files(&state,&input,&[],None);
        assert_eq!(stale[0]["evidence"]["priorProductive"],0.0);
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"rejected",
            "paths":["src/beta.rs"],"evaluation":{"metrics":{"eligible":false}}
        })).unwrap();
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let unverified=choose_focus_files(&state,&input,&[],None);
        assert_eq!(unverified[0]["evidence"]["priorProductive"],0.0);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_local_target_revision_does_not_poison_updated_source() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),
                "fn search_dispatch() { println!(\"search\"); }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiObjective":"search dispatch",
            "rsiTargetDomain":"tools"});
        let before=choose_focus_files(&state,&input,&[],None);
        let failure=json!({"id":"failed-1","candidateId":null,
            "accepted":false,"newFindings":0,
            "focusFiles":["src/alpha.rs"],"focusSelection":{"candidates":before}});
        assert_eq!(choose_focus_files(&state,&input,&[failure.clone()],None)[0]["path"],
            "src/beta.rs");
        std::fs::write(root.join("src/alpha.rs"),
            "fn search_dispatch() { println!(\"revised search\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let recovered=choose_focus_files(&state,&input,&[failure],None);
        let alpha=recovered.iter().find(|item|item["path"]=="src/alpha.rs").unwrap();
        assert_eq!(alpha["evidence"]["priorUnproductive"],0.0,
            "failure of an old version cannot down-rank the rebuilt code");
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
    fn autonomous_campaign_ignores_generic_prompt_domain_words() {
        let input=json!({"rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다",
            "rsiAutoFocus":true,"rsiFocusFiles":["src/engine/memory.rs"]});
        let scope=selection_objective(&input);
        assert_eq!(scope,"");
        assert_eq!(eligible_domains(&scope),DOMAINS.to_vec(),
            "generic bootstrap words must not constrain autonomous domain discovery");
        let explicit=json!({"rsiAutonomous":false,"rsiObjective":"학습 기록 검색"});
        assert_eq!(eligible_domains(&selection_objective(&explicit)),vec!["learning"]);
        let focus=json!({"rsiAutonomous":true,"rsiObjective":"기본 목표",
            "rsiFocusFiles":["web/src/pages/Memory.tsx"]});
        assert_eq!(eligible_domains(&selection_objective(&focus)),vec!["ui"],
            "owner-selected source files still constrain the candidate domain");
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
        assert!(!should_stop_no_progress(1,true,true,false));
        assert!(!should_stop_no_progress(1,true,false,false),
            "first unproductive generation is diagnosed, not terminal");
        assert!(!should_stop_no_progress(1,false,true,false));
        assert!(!should_stop_no_progress(2,true,true,false));
        assert!(should_stop_no_progress(2,true,false,false));
        assert!(!should_stop_no_progress(2,true,false,true),
            "pending search trial must be allowed to run");
        assert!(!should_stop_no_progress(4,false,false,true),
            "a replacement trial queued by the fourth generation needs an actual fifth run");
        assert!(!should_stop_no_progress(5,false,false,true),
            "a pending replacement is allowed only inside the fixed six-empty-turn bound");
        assert!(should_stop_no_progress(5,false,false,false),
            "no unfinished trial means another barren generation is wasteful");
        assert!(!should_stop_no_progress(3,true,true,false));
        assert!(should_stop_no_progress(6,true,true,true),
            "novel reading cannot postpone implementation forever");
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
        state.store.put("execution_operation",&json!({"id":"real-next-change","sessionId":"research-2","tool":"workspace_replace","arguments":change,"result":written,"status":"completed"}));
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
            include_str!("../../scripts/rsi-lab.sh"),
        )
        .unwrap();
        std::fs::write(
            fixtures.join("rsi-measure.mjs"),
            include_str!("../../scripts/rsi-measure.mjs"),
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
            include_str!("../../tests/fixtures/rsi-generation-worker.rs"),
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
        std::fs::write(source.join("src/main.rs"), include_str!("../../tests/fixtures/rsi-generation-worker.rs")).unwrap();
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
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap_err().contains("dominion-sub"),"main must never be an application target");
        git(&context,&source,&["checkout","-b","dominion-sub2"]).await.unwrap();
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
        assert_eq!(applied["branch"],"dominion-sub2");
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

    #[tokio::test]
    async fn reconstructed_existing_file_repair_runs_through_actual_native_tools() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        async fn provider(Json(_body):Json<Value>)->axum::response::Response {
            let events=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"완료"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],events).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(provider)))
                .await.unwrap()
        });
        let (state,root)=state();
        let _parent=parent(&state);
        let target="src/engine/memory.rs";
        std::fs::create_dir_all(state.config.workspace.join("src/engine")).unwrap();
        std::fs::write(state.config.workspace.join(target),"fn old() {}\n").unwrap();
        state.store.put("provider",&json!({"id":"repair-provider","kind":"openai",
            "baseUrl":format!("http://{address}"),"model":"test",
            "toolMode":"native","enabled":true}));
        let original=json!({"id":"create","revision":"old","name":"repair file",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{
                    "path":target,"content":"fn improved() {}\n"}}],
                "after":[{"kind":"file_equals","path":target,
                    "text":"fn improved() {}\n"}]}});
        let failed=json!({"id":"failed-create","origin":"procedure","status":"failed",
            "tool":"workspace_write","procedureStrategyId":"create",
            "procedureRevision":"old","procedureNodeId":"write",
            "arguments":{"path":target,"content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}});
        let repaired=synthesize_existing_file_write_repair(&original,&failed).unwrap();
        let input=json!({"rsiObjective":"memory improvement","rsiDomain":"memory",
            "rsiKnowledgeSource":state.config.workspace,"rsiSourceDir":state.config.workspace});
        state.store.put("experience",&json!({"id":"repaired-experience",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[repaired.clone()]})).unwrap();
        state.store.put("job",&json!({"id":"repair-child","kind":"rsi_generation",
            "status":"running","rsiWorkspace":state.config.workspace,
            "input":{"rsiCampaignId":"campaign"}})).unwrap();
        let mut session=json!({"id":"repair-session","providerId":"repair-provider",
            "rsiRun":true,"rsiObjective":"memory improvement",
            "rsiCampaignId":"campaign","rsiGenerationJobId":"repair-child"});
        configure_method_session(&state,&input,&mut session);
        assert_eq!(method_strategies(&state,&session).len(),1);
        state.store.put("session",&session).unwrap();
        let request=crate::engine::chat::TurnRequest::from_value(&json!({
            "sessionId":"repair-session","input":"memory improvement",
            "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false
        })).unwrap();
        tokio::time::timeout(Duration::from_secs(10),
            crate::engine::run_quiet(&state,&request)).await.unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(
            state.config.workspace.join(target)).unwrap(),"fn improved() {}\n");
        let operations=state.store.all("execution_operation").into_iter()
            .filter(|op|op["sessionId"]=="repair-session"
                && op["origin"]=="procedure")
            .collect::<Vec<_>>();
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_read_range" && op["status"]=="completed"));
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_replace" && op["status"]=="completed"
                && op["result"]["written"]==true));
        collect_method_executions(&state,"repair-session",&state.config.workspace);
        let feedback=record_method_feedback(&state,
            &json!({"jobId":"repair-child","accepted":false,
                "newFindings":1,"tokens":12,"elapsedMs":10}),"repair-session");
        assert_eq!(feedback.len(),1);
        assert_eq!(feedback[0]["result"]["followed"],true);
        assert_eq!(state.store.get("experience","repaired-experience").unwrap()
            ["executableMethods"][0]["successfulApplications"],json!(0),
            "successful tool execution without independently verified source adoption is not quality gain");
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn success_compacted_descendant_edits_source_via_real_indexed_tool_runtime() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        async fn provider(Json(_body):Json<Value>)->axum::response::Response {
            let response=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"완료"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],response).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(provider)))
                .await.unwrap()
        });
        let (state,root)=state();
        let _parent=parent(&state);
        let path="src/engine/memory.rs";
        std::fs::create_dir_all(state.config.workspace.join("src/engine")).unwrap();
        std::fs::write(state.config.workspace.join(path),"fn old() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(
            &state,&state.config.workspace).unwrap();
        state.store.put("provider",&json!({"id":"index-provider","kind":"openai",
            "baseUrl":format!("http://{address}"),"model":"test",
            "toolMode":"native","enabled":true})).unwrap();
        let input=json!({"rsiObjective":"memory improvement","rsiDomain":"memory",
            "rsiKnowledgeSource":state.config.workspace,
            "rsiSourceDir":state.config.workspace});
        let parent=json!({"id":"accepted-parent","revision":"p1",
            "name":"source patch","domain":"memory","status":"executable",
            "assistanceMode":"harness_assisted","successfulApplications":1,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"read","tool":"workspace_read_range","arguments":{
                    "path":path,"startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                        "path":path,"startLine":1,"endLine":1,
                        "expectedSha256":"$read.sourceSha256",
                        "newText":"fn improved() {}\n"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_equals","path":path,
                    "text":"fn improved() {}\n"}]}});
        let child=json!({"id":"compacted-child","revision":"c1",
            "name":"compact patch","domain":"memory","status":"candidate",
            "assistanceMode":"harness_assisted","successfulApplications":0,
            "synthesis":{"kind":"successful_read_ablation",
                "parentMethodId":"accepted-parent","parentRevision":"p1"},
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":path,"startLine":1,"endLine":1,
                    "expectedSha256":null,"newText":"fn improved() {}\n"}}],
                "after":[{"kind":"file_equals","path":path,
                    "text":"fn improved() {}\n"}]}});
        state.store.put("experience",&json!({"id":"compact-experience",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[parent,child]})).unwrap();
        state.store.put("job",&json!({"id":"compact-generation",
            "kind":"rsi_generation","status":"running",
            "rsiWorkspace":state.config.workspace,
            "input":{"rsiCampaignId":"campaign",
                "rsiSourceDir":state.config.workspace,"rsiIndexId":index["id"]}})).unwrap();
        let mut session=json!({"id":"compact-session","providerId":"index-provider",
            "rsiRun":true,"rsiObjective":"memory improvement",
            "rsiCampaignId":"campaign",
            "rsiGenerationJobId":"compact-generation",
            "rsiCodeIndexId":index["id"]});
        configure_method_session(&state,&input,&mut session);
        assert_eq!(session["rsiDecisionInput"]["reuseDecision"]["repairMethodId"],
            "compacted-child");
        assert_eq!(method_strategies(&state,&session).len(),1);
        state.store.put("session",&session).unwrap();
        let request=crate::engine::chat::TurnRequest::from_value(&json!({
            "sessionId":"compact-session","input":"memory improvement",
            "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false
        })).unwrap();
        tokio::time::timeout(Duration::from_secs(10),
            crate::engine::run_quiet(&state,&request)).await.unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(
            state.config.workspace.join(path)).unwrap(),"fn improved() {}\n");
        let operations=state.store.all("execution_operation").into_iter()
            .filter(|op|op["sessionId"]=="compact-session"
                && op["origin"]=="procedure").collect::<Vec<_>>();
        assert_eq!(operations.iter().filter(|op|op["tool"]=="workspace_read_range").count(),0,
            "learned compact program must truly omit the read");
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_replace" && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["result"]["sourceHashRecoveredFromIndex"]==true));
        collect_method_executions(&state,"compact-session",&state.config.workspace);
        let receipt=state.store.get("session","compact-session").unwrap()
            ["rsiMethodExecutions"].as_array().cloned().unwrap_or_default();
        assert_eq!(receipt.len(),1);
        assert_eq!(receipt[0]["result"]["followed"],true);
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
            assert_eq!(method["successfulApplications"],0,
                "a passed procedure unrelated to a verified changed path receives no gain");
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_source_rebase_program_executes_a_fresh_hash_binding() {
        let original=json!({"id":"old-method","revision":"old-revision",
            "name":"source edit","domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":3,"endLine":3,
                    "newText":"let fixed = true;","expectedSha256":"old-sha"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let failed=json!({"id":"failed-op","origin":"procedure","status":"failed",
            "procedureStrategyId":"old-method","procedureRevision":"old-revision",
            "procedureNodeId":"edit","tool":"workspace_replace",
            "arguments":{"path":"src/engine/memory.rs","startLine":3,"endLine":3,
                "newText":"let fixed = true;","expectedSha256":"old-sha"},
            "result":{"error":"stale version","staleVersion":true}});
        let learned=synthesize_stale_edit_repair(&original,&failed).unwrap();
        assert_eq!(learned["status"],"candidate");
        assert_eq!(learned["synthesis"]["kind"],"stale_source_rebase");
        assert_ne!(learned["revision"],original["revision"]);
        let mut results=std::collections::HashMap::new();
        let (node,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory improvement",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["startLine"],3);
        let read_result=json!({"sourceSha256":"new-observed-hash",
            "path":"src/engine/memory.rs","content":"3 let old = true;"});
        let read_op=json!({"id":"observed-read","origin":"procedure",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":node,"tool":"workspace_read_range",
            "status":"completed","result":read_result});
        results.insert(node,read_result);
        let (next,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[read_op],"memory improvement",&results).unwrap();
        assert_eq!(next,"edit");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["expectedSha256"],"new-observed-hash");
        assert_eq!(args["newText"],"let fixed = true;");
        let mut fake=failed.clone();
        fake["result"]["staleVersion"]=json!(false);
        assert!(synthesize_stale_edit_repair(&original,&fake).is_none());
        assert!(synthesize_stale_edit_repair(&learned,&failed).is_none());
    }

    #[test]
    fn stale_oldtext_edit_rebases_hash_and_preserves_exact_patch() {
        let method=json!({"id":"old","revision":"old-sha","name":"text patch",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","oldText":"let before = 1;",
                    "newText":"let after = 2;","expectedSha256":"stale"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let op=json!({"id":"failed-oldtext","origin":"procedure",
            "status":"failed","tool":"workspace_replace",
            "procedureStrategyId":"old","procedureRevision":"old-sha",
            "procedureNodeId":"edit","arguments":{"path":"src/engine/memory.rs",
                "oldText":"let before = 1;"},"result":{"staleVersion":true}});
        let learned=synthesize_stale_edit_repair(&method,&op).unwrap();
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["startLine"],1);
        results.insert(read.clone(),json!({"sourceSha256":"fresh-hash"}));
        let done=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":read,"result":{"sourceSha256":"fresh-hash"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[done],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["oldText"],"let before = 1;");
        assert_eq!(args["newText"],"let after = 2;");
        assert_eq!(args["expectedSha256"],"fresh-hash");
        assert!(synthesize_stale_edit_repair(&learned,&op).is_none());
    }

    #[test]
    fn existing_file_write_failure_compiles_a_versioned_executable_dag() {
        let method=json!({"id":"write","revision":"write-v1","name":"patch source",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"change","tool":"workspace_write","arguments":{
                    "path":"src/engine/memory.rs","content":"fn improved() {}\n"}}],
                "after":[{"kind":"tool_called","tool":"workspace_write",
                    "path":"src/engine/memory.rs"},
                    {"kind":"file_equals","path":"src/engine/memory.rs",
                    "text":"fn improved() {}\n"}]}});
        let failed=json!({"id":"failed-write","origin":"procedure",
            "status":"failed","tool":"workspace_write",
            "procedureStrategyId":"write","procedureRevision":"write-v1",
            "procedureNodeId":"change",
            "arguments":{"path":"src/engine/memory.rs","content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}});
        let learned=synthesize_existing_file_write_repair(&method,&failed).unwrap();
        assert_eq!(learned["synthesis"]["kind"],"existing_file_to_versioned_edit");
        assert_ne!(learned["revision"],method["revision"]);
        assert_eq!(learned["verification"]["after"][0]["tool"],"workspace_replace");
        assert_eq!(learned["verification"]["after"][1]["text"],"fn improved() {}\n");
        assert!(is_synthesized_source_repair(&learned));
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["endLine"],120);
        results.insert(read.clone(),json!({"sourceSha256":"observed-hash",
            "totalLines":3}));
        let read_op=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":read,"result":{"sourceSha256":"observed-hash",
            "totalLines":3}});
        let (node,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[read_op],"memory",&results).unwrap();
        assert_eq!(node,"change");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["startLine"],1);
        assert_eq!(args["endLine"],3);
        assert_eq!(args["expectedSha256"],"observed-hash");
        assert_eq!(args["newText"],"fn improved() {}\n");
        let mut ungrounded=failed.clone();
        ungrounded["result"]["error"]=json!("permission denied");
        assert!(synthesize_existing_file_write_repair(&method,&ungrounded).is_none());
        ungrounded=failed.clone();
        ungrounded["arguments"]["path"]=json!("src/engine/other.rs");
        assert!(synthesize_existing_file_write_repair(&method,&ungrounded).is_none());
        assert!(synthesize_existing_file_write_repair(&learned,&failed).is_none());
    }

    #[test]
    fn indexed_hash_retry_is_a_different_executable_mutation() {
        let method=json!({"id":"original","revision":"original-rev",
            "name":"small edit","domain":"memory",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":3,"endLine":3,
                    "expectedSha256":"stale-hash","newText":"fn fixed() {}"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let op=json!({"id":"failed","origin":"procedure","status":"failed",
            "tool":"workspace_replace","procedureStrategyId":"original",
            "procedureRevision":"original-rev","procedureNodeId":"edit",
            "arguments":{"path":"src/engine/memory.rs","startLine":3,"endLine":3},
            "result":{"error":"stale","staleVersion":true}});
        let observed=synthesize_stale_edit_repair(&method,&op).unwrap();
        let indexed=synthesize_indexed_edit_repair(&method,&op).unwrap();
        assert_ne!(observed["id"],indexed["id"]);
        assert_eq!(indexed["synthesis"]["kind"],"indexed_source_rebase");
        assert_eq!(indexed["verification"]["nodes"].as_array().unwrap().len(),1);
        assert!(indexed["verification"]["nodes"][0]["arguments"]["expectedSha256"].is_null());
        let (id,tool,args)=crate::engine::procedure::ready_intent(
            &indexed,&[],"memory",&std::collections::HashMap::new()).unwrap();
        assert_eq!(id,"edit");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["startLine"],3);
        assert!(args["expectedSha256"].is_null(),
            "the tool must use the generation-bound repository index");
        let mut old_text=method.clone();
        old_text["verification"]["nodes"][0]["arguments"]["oldText"]=json!("before");
        let mut old_op=op.clone();
        old_op["arguments"]["oldText"]=json!("before");
        assert!(synthesize_indexed_edit_repair(&old_text,&old_op).is_none(),
            "unversioned oldText replacement is not an allowed alternative");
    }

    #[test]
    fn real_method_feedback_changes_which_repair_variant_a_descendant_receives() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let scope=research_scope(&input).partition_key;
        let original=json!({"id":"old","revision":"r0","name":"source patch",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":"stale","newText":"fn changed() {}"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let failed=json!({"id":"failed","origin":"procedure","status":"failed",
            "tool":"workspace_replace","procedureStrategyId":"old",
            "procedureRevision":"r0","procedureNodeId":"edit",
            "arguments":{"path":"src/engine/memory.rs","startLine":1,"endLine":1},
            "result":{"error":"stale","staleVersion":true}});
        let a=synthesize_stale_edit_repair(&original,&failed).unwrap();
        let b=synthesize_indexed_edit_repair(&original,&failed).unwrap();
        state.store.put("experience",&json!({"id":"two-variants",
            "status":"active","partitionKey":scope,
            "executableMethods":[a.clone(),b.clone()]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["methodIds"].as_array().unwrap().len(),1,
            "never execute two competing versions against the same source");
        assert_eq!(decision["repairMethodId"],a["id"],
            "read-and-rebase is the first untried conservative operator");
        let mut experience=state.store.get("experience","two-variants").unwrap();
        experience["executableMethods"][0]["applications"]=json!([{
            "verificationVerdict":"passed","postconditionsPassed":true,
            "accepted":false,"tokens":100}]);
        experience["executableMethods"][0]["status"]=json!("executable");
        state.store.put("experience",&experience).unwrap();
        let next=method_reuse_decision(&state,&input,"memory");
        assert_eq!(next["repairMethodId"],b["id"],
            "one verified method call without adoption is not proof of improvement");
        experience["executableMethods"][1]["successfulApplications"]=json!(1);
        experience["executableMethods"][1]["applications"]=json!([{
            "verificationVerdict":"passed","postconditionsPassed":true,
            "accepted":true,"tokens":60}]);
        experience["executableMethods"][1]["status"]=json!("executable");
        state.store.put("experience",&experience).unwrap();
        let adopted=method_reuse_decision(&state,&input,"memory");
        assert_eq!(adopted["repairMethodId"],b["id"],
            "observed adoption should determine the future execution method");
        assert_eq!(adopted["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_source_read_retargets_the_actual_indexed_file_and_postconditions() {
        let (state,root)=state();
        let repo=root.join("source");
        let source_dir=repo.to_string_lossy().to_string();
        let old="src/legacy/memory.rs";
        let new="src/engine/memory.rs";
        state.store.put("job",&json!({"id":"read-failure","kind":"rsi_generation",
            "status":"running","input":{
                "rsiSourceDir":source_dir,"rsiIndexId":"index-one"}})).unwrap();
        state.store.put("code_index",&json!({"id":"index-one",
            "sourceDir":source_dir,"files":1})).unwrap();
        state.store.put("code_index_file",&json!({"id":"new-file",
            "indexId":"index-one","path":new,"symbols":[]})).unwrap();
        let method=json!({"id":"old-source","revision":"v1","name":"memory refactor",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range","arguments":{
                        "path":old,"startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                        "path":old,"startLine":1,"endLine":1,
                        "expectedSha256":"old-hash","newText":"fn improved() {}"}}
                ],
                "after":[{"kind":"file_contains","path":old,"text":"fn improved()"}]}});
        let failed=json!({"id":"missing-file","origin":"procedure",
            "status":"failed","tool":"workspace_read_range",
            "procedureStrategyId":"old-source","procedureRevision":"v1",
            "procedureNodeId":"read","arguments":{"path":old},
            "result":{"error":"No such file or directory (os error 2)"}});
        let generation=json!({"jobId":"read-failure","domain":"memory",
            "accepted":false,"newFindings":0});
        let repaired=synthesize_indexed_path_repair(&state,&method,&failed,&generation).unwrap();
        assert_eq!(repaired["synthesis"]["kind"],"indexed_path_retarget");
        assert_eq!(repaired["synthesis"]["oldPath"],old);
        assert_eq!(repaired["synthesis"]["targetPath"],new);
        assert_eq!(repaired["verification"]["nodes"][0]["arguments"]["path"],new);
        assert_eq!(repaired["verification"]["nodes"][1]["arguments"]["path"],new);
        assert_eq!(repaired["verification"]["after"][0]["path"],new);
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &repaired,&[],"memory",&results).unwrap();
        assert_eq!(read,"read");
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["path"],new);
        results.insert("read".to_string(),json!({"sourceSha256":"new-sha"}));
        let observed=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":repaired["id"],"procedureRevision":repaired["revision"],
            "procedureNodeId":"read","result":{"sourceSha256":"new-sha"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &repaired,&[observed],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["path"],new);
        assert_eq!(args["expectedSha256"],"new-sha");
        // The indexed identity is part of the compiler's evidence. If there
        // are multiple possible targets it must not rewrite either of them.
        state.store.put("code_index_file",&json!({"id":"ambiguous-file",
            "indexId":"index-one","path":"src/other/memory.rs",
            "symbols":[]})).unwrap();
        assert!(synthesize_indexed_path_repair(
            &state,&method,&failed,&generation).is_none());
        let mut wrong=failed.clone();
        wrong["result"]["error"]=json!("permission denied");
        assert!(synthesize_indexed_path_repair(
            &state,&method,&wrong,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_existing_write_is_learned_for_next_generation_not_credited() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let partition=research_scope(&input).partition_key;
        let original=json!({"id":"create-only","revision":"write-v1","name":"create source",
            "domain":"memory","status":"executable","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{
                    "path":"src/engine/memory.rs","content":"fn improved() {}\n"}}],
                "after":[{"kind":"tool_called","tool":"workspace_write",
                    "path":"src/engine/memory.rs"},
                    {"kind":"file_equals","path":"src/engine/memory.rs",
                    "text":"fn improved() {}\n"}]}});
        state.store.put("experience",&json!({"id":"old-method",
            "status":"active","partitionKey":partition,
            "executableMethods":[original]})).unwrap();
        state.store.put("execution_operation",&json!({"id":"blocked-write",
            "sessionId":"write-session","origin":"procedure","status":"failed",
            "procedureStrategyId":"create-only","procedureRevision":"write-v1",
            "procedureNodeId":"write","tool":"workspace_write",
            "arguments":{"path":"src/engine/memory.rs","content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}})).unwrap();
        state.store.put("session",&json!({"id":"write-session",
            "rsiMethodExecutions":[{"experienceId":"old-method",
                "methodId":"create-only","revision":"write-v1",
                "executionId":"run-one","operationIds":["blocked-write"],
                "result":{"verified":true,"status":"violated",
                "violations":["failed"]}}]})).unwrap();
        let generation=json!({"jobId":"gen-one","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":50,"elapsedMs":1});
        record_method_feedback(&state,&generation,"write-session");
        assert_eq!(state.store.get("experience","old-method").unwrap()
            ["executableMethods"][0]["status"],"suspended");
        let proposal=state.store.get("session","write-session").unwrap()
            ["rsiMethodProposals"][0].clone();
        assert_eq!(proposal["status"],"candidate");
        assert_eq!(proposal["successfulApplications"],0);
        let saved=remember_research(&state,&input,&generation,"write-session").unwrap();
        assert_eq!(state.store.get("experience",&saved).unwrap()
            ["executableMethods"][0]["id"],proposal["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["action"],"execute_methods");
        assert_eq!(decision["repairMethodId"],proposal["id"]);
        assert_eq!(decision["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn adopted_method_can_synthesize_a_smaller_executable_dag_in_multiple_domains() {
        for (domain,path) in [
            ("memory","src/engine/memory.rs"),
            ("tools","src/engine/tools.rs"),
        ] {
            let (state,root)=state();
            let source=root.join("source");
            std::fs::create_dir_all(source.join("src/engine")).unwrap();
            std::fs::write(source.join(path),"fn original() {}\n").unwrap();
            let index=crate::engine::code_index::refresh(&state,&source).unwrap();
            state.store.put("job",&json!({"id":"accepted-generation",
                "kind":"rsi_generation","status":"running",
                "input":{"rsiIndexId":index["id"],"rsiSourceDir":source}})).unwrap();
            let method=json!({"id":"accepted-procedure","revision":"old-method-revision",
                "name":"source edit","domain":domain,
                "assistanceMode":"harness_assisted",
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["improve"]},
                    "nodes":[
                        {"id":"read","tool":"workspace_read_range","arguments":{
                            "path":path,"startLine":1,"endLine":1}},
                        {"id":"edit","tool":"workspace_replace","arguments":{
                            "path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn improved() {}"}}
                    ],
                    "edges":[{"from":"read","to":"edit"}],
                    "after":[{"kind":"file_contains","path":path,
                        "text":"fn improved()"}]}});
            let sha="a".repeat(64);
            let read=json!({"id":"op-read","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"read",
                "tool":"workspace_read_range","result":{"sourceSha256":sha}});
            let edit=json!({"id":"op-edit","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"edit",
                "tool":"workspace_replace","arguments":{
                    "path":path,"expectedSha256":sha},
                "result":{"written":true,"replaced":true,
                    "postSha256":"c".repeat(64)}});
            let receipt=json!({"executionId":"execution-1",
                "methodId":"accepted-procedure","revision":"old-method-revision",
                "operationIds":["op-read","op-edit"],
                "result":{"verified":true,"followed":true,"status":"followed"}});
            state.store.put("rsi_candidate",&json!({"id":"accepted-source",
                "status":"verified","paths":[path],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut adopted_hashes=serde_json::Map::new();
            adopted_hashes.insert(path.to_string(),json!("c".repeat(64)));
            let generation=json!({"jobId":"accepted-generation","accepted":true,
                "candidateId":"accepted-source",
                "acceptedSourceHashes":Value::Object(adopted_hashes),
                "domain":domain,"tokens":500,"elapsedMs":5000});
            let candidate=synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&generation).unwrap();
            assert_eq!(candidate["synthesis"]["kind"],"successful_read_ablation");
            assert_eq!(candidate["status"],"candidate");
            assert_eq!(candidate["successfulApplications"],0);
            assert_eq!(candidate["synthesis"]["qualityGain"],Value::Null);
            state.store.put("rsi_candidate",&json!({"id":"unrelated-accepted",
                "status":"verified","paths":["src/engine/unrelated.rs"],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut unrelated=generation.clone();
            unrelated["candidateId"]=json!("unrelated-accepted");
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&unrelated).is_none(),
                "a successful but unrelated method must not inherit a different source change");
            let mut changed_again=generation.clone();
            changed_again["acceptedSourceHashes"][path]=json!("e".repeat(64));
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&changed_again).is_none(),
                "a later write to the same path must not credit this method");
            assert_eq!(candidate["verification"]["nodes"].as_array().unwrap().len(),1);
            assert_eq!(candidate["verification"]["edges"],json!([]));
            let (node,tool,args)=crate::engine::procedure::ready_intent(
                &candidate,&[],"improve",&std::collections::HashMap::new()).unwrap();
            assert_eq!(node,"edit");
            assert_eq!(tool,"workspace_replace");
            assert_eq!(args["path"],path);
            assert!(args["expectedSha256"].is_null(),
                "the next descendant must bind SHA through its own source index");
            let denied=json!({"jobId":"accepted-generation","accepted":false});
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&denied).is_none());
            let not_written=json!({"id":"op-edit","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"edit",
                "tool":"workspace_replace","arguments":{
                    "path":path,"expectedSha256":"a".repeat(64)},
                "result":{"written":false}});
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),not_written],&generation).is_none());
            let mut dependent=method.clone();
            dependent["verification"]["after"]=json!([{"kind":"tool_called",
                "tool":"workspace_read_range","path":path}]);
            assert!(synthesize_successful_dag_ablation(
                &state,&dependent,&receipt,&[read.clone(),edit.clone()],&generation).is_none());
            let mut different=method.clone();
            different["verification"]["nodes"][1]["arguments"]["expectedSha256"]=
                json!("manually-pinned-hash");
            assert!(synthesize_successful_dag_ablation(
                &state,&different,&receipt,&[read,edit],&generation).is_none());
            let _=std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn accepted_tool_trace_persists_structural_mutation_for_descendant_execution() {
        let (state,root)=state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/memory.rs"),"fn original() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        state.store.put("job",&json!({"id":"accepted-generation",
            "kind":"rsi_generation","status":"running",
            "input":{"rsiIndexId":index["id"],"rsiSourceDir":source}})).unwrap();
        let input=json!({"rsiSourceDir":source,"rsiObjective":"memory improve"});
        let parent=json!({"id":"accepted-parent","revision":"p1",
            "status":"candidate","assistanceMode":"harness_assisted",
            "name":"repair memory","domain":"memory",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1,
                            "newText":"fn improved() {}",
                            "expectedSha256":"$read.sourceSha256"}}
                ],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_contains","path":"src/engine/memory.rs",
                    "text":"fn improved()"}]}});
        state.store.put("experience",&json!({"id":"parent-experience",
            "partitionKey":research_scope(&input).partition_key,
            "status":"active","executableMethods":[parent]})).unwrap();
        for op in [
            json!({"id":"completed-read","sessionId":"accepted-session",
                "status":"completed","origin":"procedure",
                "tool":"workspace_read_range","procedureStrategyId":"accepted-parent",
                "procedureRevision":"p1","procedureNodeId":"read",
                "result":{"sourceSha256":"b".repeat(64)}}),
            json!({"id":"completed-edit","sessionId":"accepted-session",
                "status":"completed","origin":"procedure","tool":"workspace_replace",
                "procedureStrategyId":"accepted-parent",
                "procedureRevision":"p1","procedureNodeId":"edit",
                "arguments":{"path":"src/engine/memory.rs",
                    "expectedSha256":"b".repeat(64)},
                "result":{"written":true,"replaced":true,
                    "postSha256":"d".repeat(64)}})
        ] {
            state.store.put("execution_operation",&op).unwrap();
        }
        state.store.put("session",&json!({"id":"accepted-session",
            "rsiMethodExecutions":[{"experienceId":"parent-experience",
                "methodId":"accepted-parent","revision":"p1",
                "executionId":"accepted-execution",
                "operationIds":["completed-read","completed-edit"],
                "result":{"verified":true,"followed":true,"status":"followed"}}]})).unwrap();
        state.store.put("rsi_candidate",&json!({"id":"accepted-source",
            "status":"verified","paths":["src/engine/memory.rs"],
            "evaluation":{"metrics":{"eligible":true}}})).unwrap();
        let generation=json!({"jobId":"accepted-generation",
            "domain":"memory","accepted":true,"candidateId":"accepted-source",
            "acceptedSourceHashes":{"src/engine/memory.rs":"d".repeat(64)},
            "newFindings":1,"tokens":650,"elapsedMs":900});
        let first=record_method_feedback(&state,&generation,"accepted-session");
        assert_eq!(first.len(),1);
        let revised=state.store.get("session","accepted-session").unwrap();
        let candidate=revised["rsiMethodProposals"][0].clone();
        assert_eq!(candidate["synthesis"]["kind"],"successful_read_ablation");
        assert_eq!(candidate["status"],"candidate");
        assert_eq!(candidate["successfulApplications"],0);
        let baseline=state.store.get("experience","parent-experience").unwrap();
        assert_eq!(baseline["executableMethods"][0]["successfulApplications"],1);
        assert_eq!(baseline["executableMethods"][0]["status"],"executable");
        // Restarted coordinators must not count a finished execution twice.
        record_method_feedback(&state,&generation,"accepted-session");
        let saved=state.store.get("session","accepted-session").unwrap();
        assert_eq!(saved["rsiMethodProposals"].as_array().unwrap().len(),1);
        assert_eq!(state.store.get("experience","parent-experience").unwrap()
            ["executableMethods"][0]["applications"].as_array().unwrap().len(),1);
        let experience=remember_research(&state,&input,&generation,"accepted-session").unwrap();
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["id"],candidate["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["repairMethodId"],candidate["id"]);
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        assert_eq!(decision["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn proven_ablation_operator_transfers_to_new_capability_without_gain_credit() {
        let (state,root)=state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/tools.rs"),"fn tool() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        let input=json!({"rsiSourceDir":source,"rsiObjective":"tools improve"});
        state.store.put("job",&json!({"id":"tools-generation",
            "kind":"rsi_generation","status":"running","input":{
                "rsiSourceDir":source,"rsiIndexId":index["id"]}})).unwrap();
        let donor=json!({"id":"adopted-memory-ablation","revision":"m1",
            "status":"executable","domain":"memory","successfulApplications":1,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "synthesis":{"kind":"successful_read_ablation"}});
        state.store.put("experience",&json!({"id":"memory-donor",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[donor]})).unwrap();
        let target=json!({"id":"target-tools","revision":"t1","name":"improve tools",
            "status":"executable","domain":"tools",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["tools"]},
                "nodes":[{"id":"read","tool":"workspace_read_range",
                    "arguments":{"path":"src/engine/tools.rs","startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace",
                    "arguments":{"path":"src/engine/tools.rs","startLine":1,
                        "endLine":1,"newText":"fn improved() {}",
                        "expectedSha256":"$read.sourceSha256"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_contains","path":"src/engine/tools.rs",
                    "text":"fn improved()"}]}});
        let receipt=json!({"executionId":"tools-execution",
            "operationIds":["tools-read","tools-edit"],
            "result":{"verified":true,"status":"followed","followed":true}});
        let ops=vec![
            json!({"id":"tools-read","status":"completed","origin":"procedure",
                "tool":"workspace_read_range","procedureNodeId":"read",
                "procedureStrategyId":"target-tools","procedureRevision":"t1",
                "result":{"sourceSha256":"a".repeat(64)}}),
            json!({"id":"tools-edit","status":"completed","origin":"procedure",
                "tool":"workspace_replace","procedureNodeId":"edit",
                "procedureStrategyId":"target-tools","procedureRevision":"t1",
                "arguments":{"path":"src/engine/tools.rs",
                    "expectedSha256":"a".repeat(64)},
                "result":{"written":true}})];
        let generation=json!({"jobId":"tools-generation","accepted":false});
        let transferred=synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).unwrap();
        assert_eq!(transferred["synthesis"]["kind"],"transferred_read_ablation");
        assert_eq!(transferred["synthesis"]["transferDonor"]["domain"],"memory");
        assert_eq!(transferred["synthesis"]["transferDonor"]["methodId"],
            "adopted-memory-ablation");
        assert_eq!(transferred["successfulApplications"],0);
        assert_eq!(transferred["synthesis"]["qualityGain"],Value::Null);
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &transferred,&[],"tools improve",&std::collections::HashMap::new()).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert!(args["expectedSha256"].is_null());
        // A previously accepted method in another domain is an operator
        // hypothesis, not proof the target method's quality has improved.
        state.store.put("experience",&json!({"id":"tools-methods",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[target.clone(),transferred]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"tools");
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        assert_eq!(decision["repairTarget"],"src/engine/tools.rs");
        // A descendant that later gets adopted can teach the same operator
        // to a third domain. Every transfer is a fresh, uncredited candidate.
        let mut unrelated=state.store.get("experience","memory-donor").unwrap();
        unrelated["executableMethods"][0]["synthesis"]["kind"]=
            json!("transferred_read_ablation");
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(1);
        state.store.put("experience",&unrelated).unwrap();
        let second_hop=synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).unwrap();
        assert_eq!(second_hop["synthesis"]["transferDepth"],2);
        assert_eq!(second_hop["synthesis"]["qualityGain"],Value::Null);
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(3);
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "unbounded reuse of the same donor is not recursive proof");
        unrelated["executableMethods"][0]["synthesis"]["transferDepth"]=json!(1);
        unrelated["partitionKey"]=json!("rsi:foreign-repository");
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "cross-repository donor inheritance is forbidden");
        unrelated["partitionKey"]=research_scope(&input).partition_key.into();
        unrelated["executableMethods"][0]["successfulApplications"]=json!(0);
        state.store.put("experience",&unrelated).unwrap();
        assert!(synthesize_successful_dag_ablation(
            &state,&target,&receipt,&ops,&generation).is_none(),
            "a donor with no accepted descendant cannot train the meta operator");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_program_rewrites_its_own_research_dag_and_preserves_live_dataflow() {
        for (domain,investigation,path) in [
            ("memory","code_search","src/engine/memory.rs"),
            ("tools","http_fetch","src/engine/tools.rs"),
        ] {
            let (state,root)=state();
            let method=json!({"id":"accepted-method","revision":"parent-v1",
                "domain":domain,"name":"research then edit",
                "assistanceMode":"harness_assisted",
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["improve"]},
                    "nodes":[
                        {"id":"research","tool":investigation,"arguments":
                            if investigation=="code_search" {
                                json!({"query":"memory"})
                            }else{json!({"url":"https://example.org"})}},
                        {"id":"read","tool":"workspace_read_range","arguments":{
                            "path":path,"startLine":1,"endLine":1}},
                        {"id":"edit","tool":"workspace_replace","arguments":{
                            "path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn improved() {}\n"}}],
                    "edges":[{"from":"research","to":"read"},{"from":"read","to":"edit"}],
                    "after":[{"kind":"file_contains","path":path,
                        "text":"fn improved()"}]}});
            let op_ids=json!(["op-search","op-read","op-edit"]);
            let method_op=|id:&str,node:&str,tool:&str,arguments:Value,result:Value|
                json!({"id":id,"status":"completed","origin":"procedure",
                    "procedureStrategyId":"accepted-method",
                    "procedureRevision":"parent-v1","procedureNodeId":node,
                    "tool":tool,"arguments":arguments,"result":result});
            let search=method_op("op-search","research",investigation,
                json!({}),json!({"matches":[{"path":path}]}));
            let read=method_op("op-read","read","workspace_read_range",
                json!({"path":path}),json!({"sourceSha256":"a".repeat(64)}));
            let edit=method_op("op-edit","edit","workspace_replace",
                json!({"path":path,"expectedSha256":"a".repeat(64)}),
                json!({"written":true,"postSha256":"b".repeat(64)}));
            let ops=vec![search.clone(),read.clone(),edit.clone()];
            let receipt=json!({"methodId":"accepted-method","revision":"parent-v1",
                "executionId":"accepted-execution","operationIds":op_ids,
                "result":{"verified":true,"followed":true,"status":"followed"}});
            state.store.put("rsi_candidate",&json!({"id":"accepted-candidate",
                "status":"verified","paths":[path],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut accepted=serde_json::Map::new();
            accepted.insert(path.to_string(),json!("b".repeat(64)));
            let generation=json!({"jobId":"gen1","accepted":true,
                "candidateId":"accepted-candidate",
                "acceptedSourceHashes":Value::Object(accepted)});
            let proposed=synthesize_observed_dag_pruning(
                &state,&method,&receipt,&ops,&generation).unwrap();
            assert_eq!(proposed["synthesis"]["kind"],"successful_observation_pruning");
            assert_eq!(proposed["status"],"candidate");
            assert_eq!(proposed["successfulApplications"],0);
            assert_eq!(proposed["synthesis"]["qualityGain"],Value::Null);
            assert_eq!(proposed["synthesis"]["prunedNodeId"],"research");
            assert_eq!(proposed["verification"]["nodes"].as_array().unwrap().len(),2);
            assert_eq!(proposed["verification"]["edges"],
                json!([{"from":"read","to":"edit"}]));
            let bindings=std::collections::HashMap::new();
            let (first,tool,_)=crate::engine::procedure::ready_intent(
                &proposed,&[],"improve",&bindings).unwrap();
            assert_eq!(first,"read");
            assert_eq!(tool,"workspace_read_range");
            let mut bindings=bindings;
            bindings.insert("read".to_string(),json!({"sourceSha256":"fresh-hash"}));
            let done=json!({"id":"new-read","origin":"procedure","status":"completed",
                "procedureStrategyId":proposed["id"],
                "procedureRevision":proposed["revision"],"procedureNodeId":"read",
                "result":{"sourceSha256":"fresh-hash"}});
            let (_,tool,args)=crate::engine::procedure::ready_intent(
                &proposed,&[done],"improve",&bindings).unwrap();
            assert_eq!(tool,"workspace_replace");
            assert_eq!(args["expectedSha256"],"fresh-hash");
            let rejected=json!({"jobId":"gen1","accepted":false});
            assert!(synthesize_observed_dag_pruning(
                &state,&method,&receipt,&ops,&rejected).is_none());
            let mut necessary=method.clone();
            necessary["verification"]["nodes"][1]["arguments"]["dependsOn"]=
                json!("$research.matches");
            assert!(synthesize_observed_dag_pruning(
                &state,&necessary,&receipt,&ops,&generation).is_none(),
                "a consumed exploration result cannot be removed");
            necessary=method.clone();
            necessary["verification"]["after"].as_array_mut().unwrap()
                .push(json!({"kind":"tool_called","tool":investigation}));
            assert!(synthesize_observed_dag_pruning(
                &state,&necessary,&receipt,&ops,&generation).is_none(),
                "a required tool observation cannot be pruned");
            assert!(synthesize_observed_dag_pruning(
                &state,&method,&receipt,&[read,edit],&generation).is_none(),
                "never invent a completed observation that did not occur");
            let _=std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn adopted_meta_operator_transfers_across_domains_without_earning_target_credit() {
        let (state,root)=state();
        let source=root.join("source");
        let input=json!({"rsiSourceDir":source,"rsiObjective":"tools improve"});
        state.store.put("job",&json!({"id":"tools-generation",
            "kind":"rsi_generation","status":"running",
            "input":input})).unwrap();
        let donor=json!({"id":"accepted-pruner","revision":"memory-v2",
            "domain":"memory","status":"executable",
            "successfulApplications":2,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "synthesis":{"kind":"successful_observation_pruning",
                "transferDepth":0}});
        state.store.put("experience",&json!({"id":"donor-exp",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[donor]})).unwrap();
        let path="src/engine/tools.rs";
        let target=json!({"id":"tools-method","revision":"tools-v1",
            "domain":"tools","name":"tool search then source edit",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["tools"]},
                "nodes":[
                    {"id":"search","tool":"code_search",
                        "arguments":{"query":"tools source"}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":path,"startLine":1,"endLine":1,
                            "newText":"fn source() {}","expectedSha256":"a".repeat(64)}}],
                "edges":[{"from":"search","to":"edit"}],
                "after":[{"kind":"file_contains","path":path,"text":"fn source()"}]}});
        let receipts=json!({"methodId":"tools-method","revision":"tools-v1",
            "executionId":"tools-execution","operationIds":["op-search","op-write"],
            "result":{"verified":true,"followed":true,"status":"followed"}});
        let op=|id:&str,node:&str,tool:&str,result:Value|json!({
            "id":id,"origin":"procedure","status":"completed",
            "procedureStrategyId":"tools-method","procedureRevision":"tools-v1",
            "procedureNodeId":node,"tool":tool,
            "arguments":{"path":path},"result":result});
        let calls=vec![
            op("op-search","search","code_search",json!({"matches":["tools"]})),
            op("op-write","edit","workspace_replace",json!({"written":true}))
        ];
        let generation=json!({"jobId":"tools-generation","accepted":false});
        let evolved=synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).unwrap();
        assert_eq!(evolved["synthesis"]["kind"],"transferred_observation_pruning");
        assert_eq!(evolved["synthesis"]["transferDonor"]["domain"],"memory");
        assert_eq!(evolved["synthesis"]["transferDepth"],1);
        assert_eq!(evolved["status"],"candidate");
        assert_eq!(evolved["successfulApplications"],0);
        assert_eq!(evolved["synthesis"]["qualityGain"],Value::Null);
        assert_eq!(evolved["verification"]["nodes"].as_array().unwrap().len(),1);
        state.store.put("experience",&json!({"id":"tools-exp",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[target.clone(),evolved.clone()]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"tools");
        assert_eq!(decision["repairMethodId"],evolved["id"]);
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        // The new domain's success may later teach a third domain only if
        // it independently gets an accepted candidate and a method receipt.
        let mut donor=state.store.get("experience","donor-exp").unwrap();
        donor["partitionKey"]=json!("rsi:other-repository");
        state.store.put("experience",&donor).unwrap();
        assert!(synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).is_none());
        donor["partitionKey"]=json!(research_scope(&input).partition_key);
        donor["executableMethods"][0]["successfulApplications"]=json!(0);
        state.store.put("experience",&donor).unwrap();
        assert!(synthesize_observed_dag_pruning(
            &state,&target,&receipts,&calls,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn method_scheduler_learns_evidence_weighted_utility_not_lifetime_popularity() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("src").to_string_lossy(),
            "rsiObjective":"memory improve"});
        let method=|id:&str,kind:&str,wins:usize,trials:usize|{
            let mut applications=Vec::new();
            for i in 0..trials {
                let accepted=i<wins;
                applications.push(json!({"accepted":accepted,
                    "postconditionsPassed":accepted,
                    "methodWroteAcceptedCandidate":accepted}));
            }
            json!({"id":id,"revision":id,"name":id,
                "domain":"memory","status":"executable",
                "assistanceMode":"harness_assisted",
                "successfulApplications":wins,
                "applications":applications,
                "synthesis":{"kind":kind},
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["memory"]},
                    "nodes":[{"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1,
                            "newText":"fn improved() {}","expectedSha256":null}}],
                    "after":[{"kind":"file_contains",
                        "path":"src/engine/memory.rs","text":"improved"}]}})
        };
        let historical=method("historical","stale_source_rebase",6,20);
        let efficient=method("efficient","indexed_source_rebase",2,2);
        let (observations,coefficient)=method_selection_feedback(
            &[historical.clone(),efficient.clone()]);
        assert_eq!(observations,22);
        assert!(coefficient>0.20 && coefficient<0.45);
        assert!(method_observed_selection_score(
            &efficient,observations,coefficient)
                >method_observed_selection_score(
                    &historical,observations,coefficient));
        state.store.put("experience",&json!({"id":"observed-methods",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[historical,efficient]})).unwrap();
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],"efficient",
            "fewer but more reliable adopted changes must outrank raw usage counts");
        assert_eq!(selected["methodIds"].as_array().unwrap().len(),1,
            "alternative source-edit programs may not mutate one path together");
        assert_eq!(selected["repairSelectionEvidence"]["selectionPolicy"],
            "observed_method_outcomes_posterior_v1");
        assert_eq!(selected["repairSelectionEvidence"]["selectionPolicyTrials"],22);
        let (n_old,e_old)=method_selection_feedback(
            &[method("a","stale_source_rebase",8,8)]);
        let (n_bad,e_bad)=method_selection_feedback(
            &[method("a","stale_source_rebase",8,20)]);
        assert_eq!(n_old,8);
        assert_eq!(n_bad,20);
        assert!(e_bad>e_old,
            "after more observed failures the next scheduler explores more");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn pruning_preserves_predecessor_successor_dependencies() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let method=json!({"id":"program","revision":"p1","name":"three steps",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["improve"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range",
                        "arguments":{"path":path,"startLine":1,"endLine":1}},
                    {"id":"search","tool":"code_search",
                        "arguments":{"query":"index"}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn optimized() {}\n"}}],
                "edges":[{"from":"read","to":"search"},{"from":"search","to":"edit"}],
                "after":[{"kind":"file_contains","path":path,
                    "text":"fn optimized()"}]}});
        state.store.put("rsi_candidate",&json!({"id":"accept",
            "status":"verified","paths":[path],
            "evaluation":{"metrics":{"eligible":true}}})).unwrap();
        let op=|id:&str,node:&str,tool:&str,args:Value,result:Value|json!({
            "id":id,"status":"completed","origin":"procedure",
            "procedureStrategyId":"program","procedureRevision":"p1",
            "procedureNodeId":node,"tool":tool,"arguments":args,"result":result});
        let ops=vec![
            op("op1","read","workspace_read_range",
                json!({}),json!({"sourceSha256":"a".repeat(64)})),
            op("op2","search","code_search",
                json!({}),json!({"matches":[{"path":path}]})),
            op("op3","edit","workspace_replace",
                json!({"path":path}),json!({"written":true,
                    "postSha256":"b".repeat(64)}))
        ];
        let receipt=json!({"methodId":"program","revision":"p1",
            "executionId":"run","operationIds":["op1","op2","op3"],
            "result":{"verified":true,"followed":true}});
        let generation=json!({"jobId":"generation","accepted":true,
            "candidateId":"accept",
            "acceptedSourceHashes":{"src/engine/memory.rs":"b".repeat(64)}});
        let proposed=synthesize_observed_dag_pruning(
            &state,&method,&receipt,&ops,&generation).unwrap();
        assert_eq!(proposed["verification"]["edges"],
            json!([{"from":"read","to":"edit"}]),
            "the edit still waits for the source read after pruning search");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn accepted_parent_tries_compacted_child_once_then_uses_observed_outcome() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improve"});
        let scope=research_scope(&input).partition_key;
        let parent=json!({"id":"parent","revision":"p1","status":"executable",
            "assistanceMode":"harness_assisted","name":"parent",
            "domain":"memory","successfulApplications":2,
            "applications":[{"accepted":true},{"accepted":true}],
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"read","tool":"workspace_read_range",
                    "arguments":{"path":"src/engine/memory.rs","startLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":"$read.sourceSha256",
                    "newText":"fn original() {}"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_exists","path":"src/engine/memory.rs"}]}});
        let child=json!({"id":"child","revision":"c1","status":"candidate",
            "assistanceMode":"harness_assisted","name":"compact child",
            "domain":"memory","successfulApplications":0,
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":null,"newText":"fn better() {}"}}],
                "after":[{"kind":"file_contains","path":"src/engine/memory.rs",
                    "text":"fn better()"}]},
            "synthesis":{"kind":"successful_read_ablation",
                "parentMethodId":"parent","parentRevision":"p1"}});
        state.store.put("experience",&json!({"id":"success-lineage",
            "partitionKey":scope,"status":"active",
            "executableMethods":[parent,child]})).unwrap();
        let first=method_reuse_decision(&state,&input,"memory");
        assert_eq!(first["repairMethodId"],"child",
            "a previously successful parent must actually test its untried child");
        assert_eq!(first["repairSelectionEvidence"]["selection"],"success_ablation_trial");
        let mut history=state.store.get("experience","success-lineage").unwrap();
        history["executableMethods"][1]["applications"]=json!([{
            "accepted":false,"verificationVerdict":"passed"}]);
        history["executableMethods"][1]["status"]=json!("executable");
        state.store.put("experience",&history).unwrap();
        let second=method_reuse_decision(&state,&input,"memory");
        assert!(second["methodIds"].as_array().unwrap().iter().any(|id|id=="parent"));
        assert_eq!(second["repairMethodId"],Value::Null,
            "unadopted child no longer has priority over proven parent");
        // An adoption must have an actual accepted, postcondition-passing
        // application receipt. A counter alone is not proof of a child gain.
        history["executableMethods"][1]["applications"]=json!([{
            "accepted":true,"postconditionsPassed":true,"verificationVerdict":"passed"
        }]);
        history["executableMethods"][1]["successfulApplications"]=json!(1);
        state.store.put("experience",&history).unwrap();
        let third=method_reuse_decision(&state,&input,"memory");
        assert_eq!(third["repairMethodId"],"child",
            "once the improved descendant is itself adopted, prefer it");
        history["executableMethods"][0]["successfulApplications"]=json!(100);
        state.store.put("experience",&history).unwrap();
        let promoted=method_reuse_decision(&state,&input,"memory");
        assert_eq!(promoted["repairMethodId"],"child",
            "cumulative historical uses must not permanently block adopted descendants");
        assert_eq!(promoted["repairSelectionEvidence"]["selection"],
            "adopted_structural_descendant");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn zero_patch_model_failure_learns_a_new_descendant_tool_program() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let objective="fix memory";
        let session=json!({"id":"failed-model-session","rsiRun":true,
            "rsiGenerationJobId":"failed-generation",
            "rsiObjective":objective,"rsiFocusFiles":[path]});
        let gen=json!({"jobId":"failed-generation","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":250,"elapsedMs":50});
        let op=json!({"id":"failed-model-edit","origin":"model",
            "sessionId":"failed-model-session","status":"completed",
            "tool":"workspace_replace","arguments":{"path":path,
                "startLine":3,"endLine":3,
                "expectedSha256":"stale","newText":"fn improved() {}"},
            "result":{"error":"읽은 뒤 소스가 변경되었습니다",
                "staleVersion":true,"written":false}});
        let repair=synthesize_failed_model_edit(&session,&op,&gen).unwrap();
        assert_eq!(repair["synthesis"]["kind"],"failed_model_tool_rebase");
        assert_eq!(repair["status"],"candidate");
        assert_eq!(repair["successfulApplications"],0);
        assert_ne!(repair["id"],Value::Null);
        let mut results=std::collections::HashMap::new();
        let (read_id,tool,args)=crate::engine::procedure::ready_intent(
            &repair,&[],objective,&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["path"],path);
        assert_eq!(args["startLine"],3);
        results.insert(read_id.clone(),json!({"sourceSha256":"fresh-current-sha"}));
        let witnessed=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":repair["id"],"procedureRevision":repair["revision"],
            "procedureNodeId":read_id,"result":{"sourceSha256":"fresh-current-sha"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &repair,&[witnessed],objective,&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["expectedSha256"],"fresh-current-sha");
        assert_eq!(args["newText"],"fn improved() {}");
        // No original method was ever proposed by the model. This cold-start
        // repair is created from the raw failed tool receipt alone.
        state.store.put("session",&session).unwrap();
        state.store.put("execution_operation",&op).unwrap();
        let feedback=record_method_feedback(&state,&gen,"failed-model-session");
        assert!(feedback.is_empty(),
            "there was no method execution, only an observed model failure");
        let saved=state.store.get("session","failed-model-session").unwrap();
        assert_eq!(saved["rsiMethodProposals"][0]["id"],repair["id"]);
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":objective});
        let experience=remember_research(
            &state,&input,&gen,"failed-model-session").unwrap();
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["status"],"candidate");
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],repair["id"]);
        assert_eq!(selected["repairTarget"],path);
        assert_eq!(selected["repairSelectionEvidence"]["verifiedAdoptions"],0);
        // A different session, missing stale evidence, or unbound path
        // cannot manufacture a reusable procedure from model prose.
        let mut bad=op.clone();
        bad["result"]["staleVersion"]=json!(false);
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        bad=op.clone();
        bad["arguments"]["path"]=json!("src/engine/unobserved.rs");
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        let mut blocked=op.clone();
        blocked["result"]["staleVersion"]=json!(false);
        blocked["result"]["preflightBlocked"]=json!(true);
        assert!(synthesize_failed_model_edit(&session,&blocked,&gen).is_some(),
            "observed preflight blocks should produce a re-read-before-edit method");
        bad=op.clone();
        bad["origin"]=json!("procedure");
        assert!(synthesize_failed_model_edit(&session,&bad,&gen).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn genuine_reversed_model_span_evolves_into_versioned_descendant_program() {
        let (state,root)=state();
        let path="src/engine/memory.rs";
        let session=json!({"id":"reversed-s","rsiRun":true,
            "rsiGenerationJobId":"g1","rsiObjective":"fix memory",
            "rsiFocusFiles":[path]});
        let generation=json!({"jobId":"g1","accepted":false,
            "newFindings":0,"tokens":2400});
        let failure=json!({"id":"edit-reversed","origin":"model",
            "sessionId":"reversed-s","status":"failed",
            "tool":"workspace_replace","arguments":{"path":path,
                "startLine":"14","endLine":"12",
                "newText":"let improved = true;"},
            "result":{"error":"endLine은 startLine 이상이어야 합니다.",
                "written":false}});
        let learned=synthesize_failed_reversed_span(
            &session,&failure,&generation).unwrap();
        assert_eq!(learned["synthesis"]["kind"],"failed_model_reversed_span");
        assert_eq!(learned["status"],"candidate");
        assert_eq!(learned["successfulApplications"],0);
        assert_eq!(learned["synthesis"]["qualityGain"],Value::Null);
        let mut results=std::collections::HashMap::new();
        let (read_id,read_tool,read_args)=crate::engine::procedure::ready_intent(
            &learned,&[],"fix memory",&results).unwrap();
        assert_eq!(read_tool,"workspace_read_range");
        assert_eq!(read_args["startLine"],12);
        assert_eq!(read_args["endLine"],14);
        results.insert(read_id.clone(),json!({"sourceSha256":"witnessed-version"}));
        let receipt=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],
            "procedureRevision":learned["revision"],"procedureNodeId":read_id,
            "result":{"sourceSha256":"witnessed-version"}});
        let (_,edit_tool,edit_args)=crate::engine::procedure::ready_intent(
            &learned,&[receipt],"fix memory",&results).unwrap();
        assert_eq!(edit_tool,"workspace_replace");
        assert_eq!(edit_args["expectedSha256"],"witnessed-version");
        assert_eq!(edit_args["startLine"],12);
        assert_eq!(edit_args["endLine"],14);
        assert!(is_synthesized_source_repair(&learned));
        state.store.put("session",&session).unwrap();
        state.store.put("execution_operation",&failure).unwrap();
        record_method_feedback(&state,&generation,"reversed-s");
        let saved=state.store.get("session","reversed-s").unwrap();
        assert_eq!(saved["rsiMethodProposals"][0]["id"],learned["id"]);
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"fix memory"});
        let experience=remember_research(&state,&input,&generation,"reversed-s").unwrap();
        let selected=method_reuse_decision(&state,&input,"memory");
        assert_eq!(selected["repairMethodId"],learned["id"]);
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["status"],"candidate");
        let mut unrelated=failure.clone();
        unrelated["sessionId"]=json!("other");
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["arguments"]["endLine"]=json!(0);
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["result"]["error"]=json!("no change");
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        unrelated=failure.clone();
        unrelated["arguments"]["startLine"]=json!(140);
        assert!(synthesize_failed_reversed_span(&session,&unrelated,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn witnessed_model_timeout_can_run_one_inherited_fix_not_repeat_full_research() {
        let failed=json!({"actionObservation":{"failedEditCalls":2},
            "editActions":[{"status":"failed","written":false}],
            "candidateId":null,"accepted":false});
        assert!(retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,false,0,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,1,300,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,119,1,4));
        assert!(!retry_learned_model_failure(
            "실행 시간 제한(10분)을 초과했습니다.",&failed,true,0,300,4,4));
        assert!(!retry_learned_model_failure(
            "provider authentication failed",&failed,true,0,300,1,4));
        let never_tried=json!({"actionObservation":{"failedEditCalls":0},"editActions":[]});
        assert!(!retry_learned_model_failure(
            "model timeout",&never_tried,true,0,300,1,4));
    }

    #[test]
    fn exhausted_model_recovery_changes_the_next_generation_strategy() {
        let episode=json!({"id":"generation-one","attempt":1,
            "accepted":false,"candidateId":null,"newFindings":4,
            "domain":"memory","actionDirective":{"mode":"discover"},
            "actionObservation":{"status":"research_only","sourceDiffBytes":0,
                "editToolCalls":0,"appliedEditCalls":0},
            "selfRepair":{"attempts":1,
                "modes":["source_edit_bootstrap"],
                "outcome":"no_patch_after_recovery",
                "qualityGain":null}});
        assert_eq!(action_learning_context(&episode),"exhausted_recovery");
        let adapted=next_execution_action(&[episode.clone()]);
        assert_eq!(adapted["mode"],"change_approach",
            "repeated search after a failed source-edit retry must not be the default");
        assert!(action_choices("exhausted_recovery").contains(&"change_approach"));
        let mut untried=episode.clone();
        untried["selfRepair"]["outcome"]=json!("not_attempted");
        assert_eq!(action_learning_context(&untried),"research_only");
        assert_eq!(next_execution_action(&[untried])["mode"],"implement_research");
        let mut successful=episode;
        successful["selfRepair"]["outcome"]=json!("candidate_after_recovery");
        successful["candidateId"]=json!("candidate-one");
        assert_eq!(action_learning_context(&successful),"unadopted_candidate");
        assert_eq!(next_execution_action(&[successful])["mode"],"revise_candidate");
    }

    #[test]
    fn failed_method_evolves_into_a_separate_inherited_candidate() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let partition=research_scope(&input).partition_key;
        let original=json!({"id":"old-method","revision":"old-revision",
            "name":"old source edit","domain":"memory","status":"executable",
            "assistanceMode":"harness_assisted","verification":{
                "type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "newText":"fn fixed() {}","expectedSha256":"outdated"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        state.store.put("experience",&json!({"id":"prior-method",
            "status":"active","partitionKey":partition,
            "executableMethods":[original.clone()]})).unwrap();
        state.store.put("execution_operation",&json!({"id":"failed-edit",
            "sessionId":"s","origin":"procedure","status":"failed",
            "procedureStrategyId":"old-method","procedureRevision":"old-revision",
            "procedureNodeId":"edit","tool":"workspace_replace",
            "arguments":{"path":"src/engine/memory.rs","startLine":1,"endLine":1,
                "expectedSha256":"outdated","newText":"fn fixed() {}"},
            "result":{"error":"stale SHA","staleVersion":true}})).unwrap();
        state.store.put("session",&json!({"id":"s","rsiMethodExecutions":[{
            "experienceId":"prior-method","methodId":"old-method",
            "revision":"old-revision","executionId":"e","operationIds":["failed-edit"],
            "result":{"verified":true,"status":"violated","violations":["failed"]}
        }]})).unwrap();
        let generation=json!({"jobId":"generation-1","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":100,"elapsedMs":50});
        record_method_feedback(&state,&generation,"s");
        assert_eq!(state.store.get("experience","prior-method").unwrap()
            ["executableMethods"][0]["status"],"suspended");
        let proposed=state.store.get("session","s").unwrap()
            ["rsiMethodProposals"][0].clone();
        assert_eq!(proposed["status"],"candidate");
        assert_eq!(proposed["sourceGenerationJobId"],"generation-1");
        assert_eq!(proposed["successfulApplications"],0);
        let saved=remember_research(&state,&input,&generation,"s").unwrap();
        assert_eq!(state.store.get("experience",&saved).unwrap()
            ["executableMethods"][0]["id"],proposed["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["action"],"execute_methods");
        assert_eq!(decision["methodIds"][0],proposed["id"]);
        assert_ne!(decision["methodIds"][0],original["id"]);
        let _=std::fs::remove_dir_all(root);
    }

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
