use super::*;
    #[test]
    fn forged_prediction_history_without_original_job_and_pair_is_censored() {
        let root=std::env::temp_dir().join(format!("champion-meta-{}",
            uuid::Uuid::new_v4()));
        let config=crate::config::Config {
            port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()
        };
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&config.key_path());
        let state=crate::state::AppState::new(config,store,vault);
        let source=json!({"jobId":"missing-job","candidateId":"invented",
            "pairedMeasured":true,"pairedRevalidated":true,"accepted":true,
            "metaPolicy":{"source":"compiled_editable_meta_controller_v1",
                "revision":"f".repeat(64),
                "target":{"role":"memory","path":"src/engine/memory.rs",
                    "sourceSha256":"a".repeat(64)},
                "forecast":{"acceptanceProbability":0.9}},
            "metaLearning":{"source":"source_bound_prequential_feedback_v1",
                "brierError":0.0,"actualTargetAdoption":true,
                "nextPolicyCanTrainFromObservation":true}});
        let checked=revalidated_history(&state,&[source]);
        assert_eq!(checked[0]["pairedMeasured"],false);
        assert_eq!(checked[0]["pairedRevalidated"],false);
        assert_eq!(checked[0]["metaLearning"]["independentlyMeasured"],false);
        assert_eq!(checked[0]["metaLearning"]["nextPolicyCanTrainFromObservation"],false);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn systemic_handoff_requires_rebuilt_model_authored_design_and_real_source_receipts() {
        let make=|i:usize,component:&str,domain:&str| {
            let parent=format!("{:040x}",i+1);
            let next=format!("{:040x}",i+2);
            let path=format!("src/engine/{}.rs",component);
            json!({
                "accepted":true,"systemEvolution":true,"pairedRevalidated":true,
                "candidateOrigin":"model_driven_detached_worktree",
                "generatorCommit":parent,"acceptedCommit":next,
                "generatorBinarySha256":format!("{:064x}",i+1),
                "jobId":format!("job-{i}"),"domain":domain,
                "evaluation":{"metrics":{"eligible":true,"regressions":0,
                    "improvedHoldouts":1,"metaHoldoutGains":if domain=="improver"{1}else{0}}},
                "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                    "generatorCommit":parent,"architecture":[]},
                "descendantPlan":{"generatorCommit":parent,
                    "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                        "generatorCommit":parent,"architecture":[]}},
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":parent,"generationJobId":format!("job-{i}"),
                    "component":component,"targetFiles":[path]},
                "acceptedSourceHashes":{(path):"a".repeat(64)}
            })
        };
        let proof=vec![make(0,"algorithms","algorithm"),
            make(1,"improvement_methods","improver"),
            make(2,"memory","memory")];
        assert!(verified_handoff(&proof));
        let mut forged=proof.clone();
        forged[1]["systemDesign"]["generatorCommit"]=json!("stale");
        assert!(!verified_handoff(&forged));
        let mut copied=proof.clone();
        copied[2]["generatorBinarySha256"]=copied[1]["generatorBinarySha256"].clone();
        assert!(!verified_handoff(&copied));
        let mut no_edits=proof.clone();
        no_edits[1]["acceptedSourceHashes"]=json!({});
        assert!(!verified_handoff(&no_edits));
        let mut no_descendant=proof.clone();
        no_descendant[2]["descendantPlan"]=Value::Null;
        assert!(!verified_handoff(&no_descendant));
        let mut native=proof;
        native[1]["candidateOrigin"]=json!("native_inductive_source_mutation");
        assert!(!verified_handoff(&native));
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
        assert_eq!(first["architecture"].as_array().unwrap().len(),10);
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
    fn model_and_python_modules_are_in_the_rebuilt_source_graph() {
        let root=std::env::temp_dir().join(format!("rsi-model-graph-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::create_dir_all(root.join("src-tauri/src")).unwrap();
        fs::write(root.join("src/engine/model_planner.py"),"def plan(): return 1").unwrap();
        fs::write(root.join("src/engine/novel_meta_operator.rs"),"pub fn meta() {}").unwrap();
        fs::write(root.join("src-tauri/src/main.rs"),"fn main() {}").unwrap();
        let first=blueprint(&root,"before",&[]);
        let model=first["architecture"].as_array().unwrap().iter()
            .find(|row|row["role"]=="models").unwrap();
        assert!(model["representativeSources"].as_array().unwrap().iter()
            .any(|row|row["path"]=="src/engine/model_planner.py"));
        assert!(first["architecture"].as_array().unwrap().iter()
            .any(|row|row["role"]=="improvement_methods"
                && row["representativeSources"].as_array().unwrap().iter()
                    .any(|file|file["path"]=="src/engine/novel_meta_operator.rs")));
        fs::write(root.join("src/engine/model_planner.py"),"def plan(): return 2").unwrap();
        let second=blueprint(&root,"after",&[]);
        assert_ne!(first["sourceTreeSha256"],second["sourceTreeSha256"]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn model_authored_method_maps_to_actual_runtime_stages() {
        let allowed=json!(["code_search","workspace_read_range",
            "code_search","workspace_replace_range","workspace_read_range"]);
        let program=compile_action_program(&allowed).unwrap();
        assert_eq!(program["stages"],json!([
            "discover","ground","contrast","edit","reground","verify"]));
        assert!(compile_action_program(&json!([
            "workspace_replace","workspace_read_range"])).is_none());
        assert!(compile_action_program(&json!([
            "shell_exec","workspace_read_range","workspace_write"])).is_none());
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
        // New executable modules must be designable, not only edits to a
        // predeclared static set of optimizer operators.
        let mut novel=request.clone();
        novel["targetFiles"]=json!(["src/engine/novel_meta_planner.rs"]);
        assert!(propose(&state,"session",&novel).is_ok());
        // A symlinked parent cannot be used to write outside the experiment.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc",workspace.join("src/engine/escape")).unwrap();
            novel["targetFiles"]=json!(["src/engine/escape/outside.rs"]);
            assert!(propose(&state,"session",&novel).is_err());
        }
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
