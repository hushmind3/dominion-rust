//! Source-model and adopted-design contract tests.
    use super::*;
    #[test]
    fn invented_roles_require_current_executable_source_linkage(){
        let root=std::env::temp_dir().join(format!(
            "sub1-invented-executable-role-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/rsi_new_method.rs"),
            "pub fn improve() -> bool { true }\n").unwrap();
        std::fs::write(root.join("src/engine/mod.rs"),
            "pub mod rsi_new_method;\n").unwrap();
        std::fs::write(root.join("src/engine/tools.rs"),
            "pub fn propose() -> bool { crate::engine::rsi_new_method::improve() }\n").unwrap();
        let g=json!({
            "jobId":"generation","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "generationJobId":"generation","generatorCommit":"a".repeat(40),
                "component":"system","components":["new_method","improvement_methods"],
                "nextDomain":"improver",
                "targetFiles":["src/engine/rsi_new_method.rs","src/engine/mod.rs",
                    "src/engine/tools.rs"],
                "componentFiles":{"new_method":["src/engine/rsi_new_method.rs"],
                    "improvement_methods":["src/engine/mod.rs"],
                    "tools":["src/engine/tools.rs"]},
                "newComponents":[{"role":"new_method","domain":"improver",
                    "purpose":"Discover a new source-based recursive improvement algorithm",
                    "sourcePaths":["src/engine/rsi_new_method.rs"],
                    "integrationPaths":["src/engine/mod.rs","src/engine/tools.rs"]}]},
            "acceptedSourceHashes":{"src/engine/rsi_new_method.rs":"c".repeat(64),
                "src/engine/mod.rs":"d".repeat(64),
                "src/engine/tools.rs":"e".repeat(64)}
        });
        let first=blueprint(&root,"ancestor",&[g.clone()]);
        assert!(first["architecture"].as_array().unwrap().iter().any(|r|
            r["role"]=="new_method"
                && r["origin"]=="accepted_compiled_descendant_wired_intelligence_v1"));
        std::fs::write(root.join("src/engine/mod.rs"),
            "pub mod something_else;\n").unwrap();
        let second=blueprint(&root,"child",&[g]);
        assert!(!second["architecture"].as_array().unwrap().iter().any(|r|
            r["role"]=="new_method"));
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn python_model_architecture_mutations_change_whole_system_identity(){
        let root=std::env::temp_dir().join(format!("rsi-python-model-{}",
            uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        let source=root.join("src/engine/model_architecture.py");
        fs::write(&source,"def model(): return 'v1'\n").unwrap();
        let first=blueprint(&root,"same-revision",&[]);
        let paths=first["sourceInventory"].as_array().unwrap();
        assert!(paths.iter().any(|row|
            row["path"]=="src/engine/model_architecture.py"));
        assert_eq!(source_role("src/engine/model_architecture.py"),"models");
        fs::write(&source,"def model(): return 'v2'\n").unwrap();
        let second=blueprint(&root,"same-revision",&[]);
        assert_ne!(first["sourceTreeSha256"],second["sourceTreeSha256"]);
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
        assert_eq!(first["architecture"].as_array().unwrap().len(),9);
        assert_eq!(first["generatorCommit"],"commit-one");
        assert_eq!(first["historyCount"],1);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace_range"));
        let baseline=first["architecture"][0]["representativeSources"][0]["sha256"].clone();
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
        let mut bad=request.clone();
        bad["targetFiles"]=json!(["src/engine/../escape.rs"]);
        assert!(propose(&state,"session",&bad).is_err());
        let mut disabled=state.store.get("job","descendant").unwrap();
        disabled["input"]["rsiSystemEvolution"]=json!(false);
        state.store.put("job",&disabled).unwrap();
        assert!(propose(&state,"session",&request).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn only_adopted_model_design_can_control_a_new_compiled_descendant() {
        let base=json!({"generatorCommit":"parent","jobId":"worker",
            "acceptedCommit":"cccccccccccccccccccccccccccccccccccccccc",
            "candidateOrigin":"model_driven_detached_worktree",
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
    fn compiled_meta_stages_are_revisable_improvement_methods_not_generic_harness() {
        for path in [
            "src/engine/rsi/meta_controller/provenance.rs",
            "src/engine/rsi/meta_controller/reciprocal.rs",
            "src/engine/rsi/meta_controller/selection.rs",
            "src/engine/rsi/meta_controller/transfer.rs",
            "src/engine/rsi/meta_controller/feedback.rs",
        ] {
            assert_eq!(source_role(path),"improvement_methods");
            assert!(executable_meta_source(path));
        }
        assert!(!executable_meta_source("src/engine/rsi/meta_controller/tests.rs"));
        assert_eq!(source_role("src/engine/rsi/meta_controller/tests.rs"),"improvement_methods");
        for stage in ["inventory","blueprint","discovery","frontier","agenda","design"] {
            let path=format!("src/engine/rsi/system/{stage}.rs");
            assert_eq!(source_role(&path),"improvement_methods");
            assert!(executable_meta_source(&path));
        }
        assert!(!executable_meta_source("src/engine/rsi/system/tests.rs"));
    }
