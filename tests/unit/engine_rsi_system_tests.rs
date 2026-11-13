use super::*;
    #[test]
    fn factorial_history_reconstructs_an_exact_job_bound_receipt(){
        let commit="a".repeat(40);
        let cases=json!([{"id":"frozen-case","holdout":true}]);
        let profile=crate::engine::rsi_lineage_transfer::profile(
            &json!({"rsiFrozenCases":cases})).unwrap();
        let plan=json!({"source":"compiled_open_topology_counterfactual_lab_v1",
            "generatorCommit":commit,"comparisonProfileSha256":profile,
            "selected":{"phase":"isolate_a",
                "a":{"path":"src/engine/memory.rs"},
                "b":{"path":"src/engine/tools.rs"}}});
        let job=json!({"id":"job-one","kind":"rsi_generation",
            "input":{"rsiGeneratorCommit":commit,"rsiFrozenCases":cases,
                "rsiFactorialExperiment":plan}});
        let generation=json!({"jobId":"job-one","generatorCommit":commit,
            "factorialExperiment":plan,"factorialMeasured":false,
            "factorialCompliance":{"eligibleForFactorial":false}});
        let candidate=json!({"baseCommit":commit,
            "paths":["src/engine/memory.rs"],
            "evaluation":{"metrics":{"measurements":[{"id":"frozen-case",
                "holdout":true,"baselinePass":false,"candidatePass":true}]}}});
        let (compliance,measured)=verified_factorial_receipt(
            &generation,&candidate,&job);
        assert!(measured);
        assert_eq!(compliance["phase"],"isolate_a");
        assert_eq!(compliance["eligibleForFactorial"],true);
    }
    #[test]
    fn forged_factorial_flags_cannot_override_actual_edited_paths(){
        let commit="b".repeat(40);
        let cases=json!([{"id":"case-one"}]);
        let profile=crate::engine::rsi_lineage_transfer::profile(
            &json!({"rsiFrozenCases":cases})).unwrap();
        let plan=json!({"source":"compiled_open_topology_counterfactual_lab_v1",
            "generatorCommit":commit,"comparisonProfileSha256":profile,
            "selected":{"phase":"isolate_a",
                "a":{"path":"src/engine/memory.rs"},
                "b":{"path":"src/engine/tools.rs"}}});
        let job=json!({"id":"child","kind":"rsi_generation",
            "input":{"rsiGeneratorCommit":commit,"rsiFrozenCases":cases,
                "rsiFactorialExperiment":plan}});
        let mut generation=json!({"jobId":"child","generatorCommit":commit,
            "factorialExperiment":plan,"factorialMeasured":true,
            "factorialCompliance":{"eligibleForFactorial":true}});
        let candidate=json!({"baseCommit":commit,
            "paths":["src/engine/tools.rs"],
            "evaluation":{"metrics":{"measurements":[{"id":"case-one",
                "holdout":true,"baselinePass":true,"candidatePass":true}]}}});
        let (receipt,eligible)=verified_factorial_receipt(
            &generation,&candidate,&job);
        assert!(!eligible);
        assert_eq!(receipt["eligibleForFactorial"],false);
        // Even a well-formed invented phase is not the original job input.
        generation["factorialExperiment"]["selected"]["phase"]=json!("isolate_b");
        let (receipt,eligible)=verified_factorial_receipt(
            &generation,&candidate,&job);
        assert!(!eligible);
        assert!(receipt.is_null());
    }
    #[test]
    fn unrelated_job_or_missing_paired_measurements_cannot_train_factorial(){
        let commit="c".repeat(40);
        let cases=json!([{"id":"case-one"}]);
        let profile=crate::engine::rsi_lineage_transfer::profile(
            &json!({"rsiFrozenCases":cases})).unwrap();
        let plan=json!({"source":"compiled_open_topology_counterfactual_lab_v1",
            "generatorCommit":commit,"comparisonProfileSha256":profile,
            "selected":{"phase":"isolate_a",
                "a":{"path":"src/engine/memory.rs"}}});
        let job=json!({"id":"child","kind":"rsi_generation",
            "input":{"rsiGeneratorCommit":commit,"rsiFrozenCases":cases,
                "rsiFactorialExperiment":plan}});
        let generation=json!({"jobId":"child","generatorCommit":commit,
            "factorialExperiment":plan});
        let mut candidate=json!({"baseCommit":commit,
            "paths":["src/engine/memory.rs"],
            "evaluation":{"metrics":{"measurements":[{"id":"case-one",
                "holdout":true,"baselinePass":false,"candidatePass":true}]}}});
        let (receipt,eligible)=verified_factorial_receipt(
            &generation,&candidate,&job);
        assert!(eligible);
        assert_eq!(receipt["eligibleForFactorial"],true);
        candidate["evaluation"]["metrics"]["measurements"]=json!([]);
        assert!(!verified_factorial_receipt(&generation,&candidate,&job).1);
        candidate["evaluation"]["metrics"]["measurements"]=Value::Null;
        assert!(!verified_factorial_receipt(&generation,&candidate,&job).1);
        let mut foreign=job;
        foreign["id"]=json!("another-worker");
        assert!(!verified_factorial_receipt(&generation,&candidate,&foreign).1);
    }
    #[test]
    fn prequential_history_replays_rejected_receipts_instead_of_trusting_flags(){
        let root=std::env::temp_dir().join(format!("rsi-preq-replay-{}",uuid::Uuid::new_v4()));
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.join("workspace")};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let predicted=json!({
            "source":"compiled_editable_meta_controller_v1",
            "revision":"a".repeat(64),
            "target":{"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs",
                "sourceSha256":"b".repeat(64)},
            "forecast":{"acceptanceProbability":0.8}});
        let row=json!({"jobId":"fake","generatorCommit":"c".repeat(40),
            "candidateId":"nonexistent","candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"pairedRevalidated":true,"accepted":false,
            "metaPolicy":predicted,"descendantPlan":{
                "metaPolicy":predicted,"architectureTarget":predicted["target"],
                "generatorCommit":"c".repeat(40)},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "verdict":"unchanged","evidenceSha256":"d".repeat(64),
                "metrics":{"eligible":false}},
            "metaLearning":{"source":"source_bound_prequential_feedback_v1",
                "nextPolicyCanTrainFromObservation":true,"brierError":0.0,
                "actualTargetAdoption":true}});
        let checked=revalidated_history(&state,&[row]);
        assert_eq!(checked[0]["pairedMeasured"],false);
        assert_eq!(checked[0]["pairedRevalidated"],false);
        assert_eq!(checked[0]["metaLearning"]["independentlyMeasured"],false);
        assert!(checked[0]["metaLearning"]["brierError"].is_null());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn system_self_model_tracks_real_source_revision_and_covers_all_capabilities() {
        let root=std::env::temp_dir().join(format!("rsi-system-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn policy() {}").unwrap();
        fs::write(root.join("src/engine/rsi_evolve.rs"),"pub fn evolve() {}").unwrap();
        let history=vec![json!({"generatorCommit":"first","accepted":true,
            "evaluation":{"verdict":"improved","metrics":{"improvedHoldouts":1}}})];
        let first=blueprint(&root,"commit-one",&history);
        assert_eq!(first["architecture"].as_array().unwrap().len(),8);
        assert_eq!(first["generatorCommit"],"commit-one");
        assert_eq!(first["historyCount"],1);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace_range"));
        let baseline=first["architecture"][0]["representativeSources"][0]["sha256"].clone();
        let full_tree_before=first["sourceTree"]["sourceTreeSha256"].clone();
        fs::write(root.join("src/engine/unlisted.rs"),"pub fn extra_capability() {}").unwrap();
        let new_scope=blueprint(&root,"commit-one",&history);
        assert_ne!(full_tree_before,new_scope["sourceTree"]["sourceTreeSha256"]);
        assert_eq!(new_scope["sourceTree"]["fileCount"],3);
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn changed_policy() {}").unwrap();
        let second=blueprint(&root,"commit-two",&history);
        assert_ne!(baseline,second["architecture"][0]["representativeSources"][0]["sha256"]);
        assert_ne!(first["generatorCommit"],second["generatorCommit"]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn model_can_propose_design_only_in_its_own_active_system_generation() {
        let root=std::env::temp_dir().join(format!("rsi-system-proposal-{}",uuid::Uuid::new_v4()));
        let workspace=root.join("work");
        fs::create_dir_all(workspace.join("src/engine")).unwrap();
        fs::write(workspace.join("src/engine/algorithms.rs"),"pub fn base() {}").unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:workspace.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve",
            "status":"running","input":{"rsiNativeApproved":true}})).unwrap();
        state.store.put("job",&json!({"id":"descendant","kind":"rsi_generation",
            "status":"running","input":{"rsiCampaignId":"campaign",
                "rsiSystemEvolution":true,"rsiGeneratorCommit":"revision-a"}})).unwrap();
        state.store.put("session",&json!({"id":"session","rsiRun":true,
            "rsiCampaignId":"campaign","rsiGenerationJobId":"descendant"})).unwrap();
        let request=json!({"component":"algorithms","nextDomain":"learning",
            "hypothesis":"The compiled planner has no causal source revision feedback.",
            "design":"Introduce an inherited source-specific planning policy with evidence checks.",
            "targetFiles":["src/engine/algorithms.rs"]});
        let proposed=propose(&state,"session",&request).unwrap();
        assert_eq!(proposed["proposal"]["status"],"proposed");
        assert_eq!(proposed["proposal"]["qualityProven"],false);
        assert_eq!(proposed["proposal"]["generatorCommit"],"revision-a");
        assert_eq!(state.store.get("session","session").unwrap()["rsiSystemDesign"]["nextDomain"],"learning");
        let mut new_module=request.clone();
        new_module["targetFiles"]=json!(["src/engine/new_evolution_policy.rs"]);
        assert!(propose(&state,"session",&new_module).is_ok(),
            "A successor must be able to design and create a new source module");
        let mut escaped=request.clone();
        escaped["targetFiles"]=json!(["src/engine/unknown_dir/new_policy.rs"]);
        assert!(propose(&state,"session",&escaped).is_err());
        let mut bad=request.clone();
        bad["targetFiles"]=json!(["src/engine/../escape.rs"]);
        assert!(propose(&state,"session",&bad).is_err());
        fs::write(workspace.join("src/engine/learning.rs"),"pub fn learn() {}").unwrap();
        let mut coupled=request.clone();
        coupled["component"]=json!("system");
        coupled["components"]=json!(["algorithms","learning"]);
        coupled["targetFiles"]=json!(["src/engine/algorithms.rs","src/engine/learning.rs"]);
        coupled["componentFiles"]=json!({
            "algorithms":["src/engine/algorithms.rs"],
            "learning":["src/engine/learning.rs"]
        });
        let whole=propose(&state,"session",&coupled).unwrap();
        assert_eq!(whole["proposal"]["components"][1],"learning");
        assert_eq!(whole["proposal"]["targetFiles"].as_array().unwrap().len(),2);
        let mut duplicate=coupled.clone();
        duplicate["components"]=json!(["algorithms","algorithms"]);
        assert!(propose(&state,"session",&duplicate).is_err());
        let mut one_file=coupled;
        one_file["targetFiles"]=json!(["src/engine/learning.rs"]);
        assert!(propose(&state,"session",&one_file).is_err());
        let mut disabled=state.store.get("job","descendant").unwrap();
        disabled["input"]["rsiSystemEvolution"]=json!(false);
        state.store.put("job",&disabled).unwrap();
        assert!(propose(&state,"session",&request).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_adopted_model_design_can_control_a_new_compiled_descendant() {
        let base=json!({"generatorCommit":"parent","jobId":"worker",
            "acceptedCommit":"child","candidateOrigin":"model_driven_detached_worktree",
            "acceptedSourceHashes":{"src/engine/learning.rs":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"parent","generationJobId":"worker",
                "targetFiles":["src/engine/learning.rs"],
                "nextDomain":"learning"}});
        assert_eq!(inherited_domain(&[base.clone()]),Some("learning".into()));
        for (key,value) in [("accepted",json!(false)),
                            ("pairedRevalidated",json!(false)),
                            ("candidateOrigin",json!("native_inductive_source_mutation"))] {
            let mut invalid=base.clone();
            invalid[key]=value;
            assert_eq!(inherited_domain(&[invalid]),None);
        }
        let mut irrelevant=base.clone();
        irrelevant["systemDesign"]["targetFiles"]=json!(["src/engine/browser.rs"]);
        assert_eq!(inherited_domain(&[irrelevant]),None);
        let mut wrong=base;
        wrong["systemDesign"]["generatorCommit"]=json!("stale");
        assert_eq!(inherited_domain(&[wrong]),None);
    }

    #[test]
    fn rejected_or_research_only_generations_do_not_erase_verified_design() {
        let accepted=json!({"accepted":true,"pairedRevalidated":true,
            "acceptedCommit":"successful-child","generatorCommit":"initial-parent",
            "jobId":"worker-one","candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "acceptedSourceHashes":{"src/engine/memory.rs":"f".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"initial-parent","generationJobId":"worker-one",
                "hypothesis":"Improving retrieval changes the downstream planner.",
                "design":"Implement more reliable source-grounded retrieval for subsequent generations.",
                "targetFiles":["src/engine/memory.rs"],"nextDomain":"learning"}});
        let failure=json!({"accepted":false,"pairedRevalidated":false,
            "generatorCommit":"successful-child","systemDesign":{"nextDomain":"ui"}});
        let history=vec![accepted.clone(),failure];
        assert_eq!(inherited_domain(&history),Some("learning".into()));
        let root=std::env::temp_dir();
        let model=blueprint(&root,"newer-child",&history);
        assert_eq!(model["inheritedDesign"]["nextDomain"],"learning");
        assert_eq!(model["inheritedDesignEvidence"]["acceptedCommit"],"successful-child");
        assert!(instruction(&model,"redesign the whole agent").contains("source-grounded retrieval"));
        let mut partial=accepted.clone();
        partial["systemDesign"]["component"]=json!("system");
        partial["systemDesign"]["components"]=json!(["memory","learning"]);
        partial["systemDesign"]["targetFiles"]=json!(["src/engine/memory.rs","src/engine/learning.rs"]);
        assert!(inherited_domain(&[partial.clone()]).is_none(),
            "A model cannot claim coupled system change after editing one file");
        partial["acceptedSourceHashes"]["src/engine/learning.rs"]=json!("a".repeat(64));
        assert_eq!(inherited_domain(&[partial]),Some("learning".into()));
        let mut forged=accepted;
        forged["acceptedSourceHashes"]["src/engine/memory.rs"]=json!("not-a-real-hash");
        assert!(inherited_domain(&[forged]).is_none());
    }

    #[test]
    fn new_descendant_discovers_unmodified_core_modules_from_real_index() {
        let root=std::env::temp_dir().join(format!("rsi-frontier-{}",uuid::Uuid::new_v4()));
        let workspace=root.join("work");
        fs::create_dir_all(workspace.join("src/engine")).unwrap();
        fs::write(workspace.join("src/engine/memory.rs"),
            "pub fn memory_recall() -> bool { true }").unwrap();
        fs::write(workspace.join("src/engine/learning.rs"),
            "pub fn learning_update() -> bool { true }").unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:workspace.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&workspace).unwrap();
        let model=blueprint(&workspace,"descendant",&[]);
        let frontier=architecture_frontier(&state,&model,&[],
            index["id"].as_str().unwrap(),&workspace);
        assert!(frontier.iter().any(|row|row["role"]=="memory"));
        assert!(frontier.iter().any(|row|row["role"]=="learning"));
        assert!(!frontier.iter().any(|row|row["role"]=="frontend"));
        for row in &frontier {
            assert_eq!(row["verifiedRoleImproved"],false);
            assert_eq!(row["qualityProven"],false);
            assert_eq!(crate::engine::code_index::indexed_source_hash(
                &state,index["id"].as_str().unwrap(),row["path"].as_str().unwrap())
                .as_deref(),row["sourceSha256"].as_str());
        }
        fs::write(workspace.join("src/engine/memory.rs"),
            "pub fn memory_recall() -> bool { false }").unwrap();
        let stale=architecture_frontier(&state,&model,&[],
            index["id"].as_str().unwrap(),&workspace);
        assert!(!stale.iter().any(|row|row["role"]=="memory"),
            "a stale indexed SHA must never drive a source edit");
        fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn architectural_map_discovers_new_meta_and_harness_modules_without_static_registration(){
        let root=std::env::temp_dir().join(format!("rsi-dynamic-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/rsi_new_mutator.rs"),
            "pub fn modify_learning_policy() {}").unwrap();
        fs::write(root.join("src/engine/future_new_agent.rs"),
            "pub fn future() {}").unwrap();
        let model=blueprint(&root,"new-child",&[]);
        let architecture=model["architecture"].as_array().unwrap();
        let meta=architecture.iter().find(|item|item["role"]=="improvement_methods").unwrap();
        assert!(meta["representativeSources"].as_array().unwrap().iter().any(|item|
            item["path"]=="src/engine/rsi_new_mutator.rs"));
        let harness=architecture.iter().find(|item|item["role"]=="harness").unwrap();
        assert!(harness["representativeSources"].as_array().unwrap().iter().any(|item|
            item["path"]=="src/engine/future_new_agent.rs"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_experiment_cannot_enter_descendant_model() {
        let root=std::env::temp_dir().join(format!("rsi-system-escape-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts",root.join("src/engine/algorithms.rs")).unwrap();
        let map=blueprint(&root,"pinned",&[]);
        assert_eq!(map["architecture"][0]["representativeSources"].as_array().unwrap().len(),0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn architecture_discovery_ignores_test_only_and_frozen_oracle_sources(){
        let root=std::env::temp_dir().join(format!(
            "dominion-frontier-clean-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::create_dir_all(root.join("src/bin")).unwrap();
        std::fs::write(root.join("src/engine/memory.rs"),"pub fn recall() {}").unwrap();
        std::fs::write(root.join("src/engine/rsi_oracle_fixture.rs"),"pub fn frozen() {}").unwrap();
        std::fs::write(root.join("src/engine/rsi_method_tests.rs"),"pub fn test_only() {}").unwrap();
        std::fs::write(root.join("src/bin/dominion-benchmark.rs"),"pub fn benchmark() {}").unwrap();
        let found=discovered_architecture_sources(&root);
        assert!(found.iter().any(|row|row.0=="src/engine/memory.rs"));
        assert!(!found.iter().any(|row|
            row.0.ends_with("_tests.rs")
            ||row.0.ends_with("rsi_oracle_fixture.rs")
            ||row.0=="src/bin/dominion-benchmark.rs"));
        let _=std::fs::remove_dir_all(root);
    }
