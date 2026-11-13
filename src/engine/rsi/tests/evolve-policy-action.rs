    #[test]
    fn observed_meta_method_failure_reaches_compiled_focus_without_owner_override() {
        let repair=json!({
            "metaRepairChosen":true,"mode":"repair_own_prediction_mechanism",
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
        wrong["mode"]=json!("repair_own_transfer_method");
        assert!(!evidence_based_meta_repair_focus(&wrong,&Value::Null,false),
            "the transfer-method focus must be the compiled peer-transfer algorithm");
        wrong["target"]["path"]=json!(
            crate::engine::rsi_meta_controller::TRANSFER_METHOD_SOURCE);
        assert!(evidence_based_meta_repair_focus(&wrong,&Value::Null,false));
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
            "rsiCampaignId":"compiled-source-scan-fixture",
            "rsiFrozenCases":(0..8).map(|i|
                json!({"id":format!("frozen-{i}"),"domain":"improver",
                    "holdout":i<3,"filter":format!("test::{i}")}))
                .collect::<Vec<_>>(),
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
        assert_eq!(first["systemBlueprint"]["architecture"].as_array().unwrap().len(),8,
            "the source scanner has 8 seed roles: algorithms, harness, memory, learning, tools, improvement_methods, models and frontend");
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

