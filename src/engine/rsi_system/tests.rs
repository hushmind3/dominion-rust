use super::*;
    #[test]
    fn source_receipt_revalidation_rejects_forged_positive_and_negative_training() {
        let root=std::env::temp_dir().join(format!("rsi-meta-audit-{}",
            uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let fake=json!({"candidateId":"nonexistent",
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40),
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"verdict":"improved",
                "evidenceSha256":"c".repeat(64),
                "metrics":{"eligible":true,"metaHoldoutGains":5}},
            "metaLearning":{"proposalBrierError":0.001}});
        let mut negative=fake.clone();
        negative["accepted"]=json!(false);
        negative["evaluation"]["verdict"]=json!("rejected");
        let checked=revalidated_method_history(&state,&[fake,negative]);
        for generation in &checked{
            assert_eq!(generation["pairedMeasured"],false);
            assert_eq!(generation["pairedRevalidated"],false);
            assert_eq!(generation["accepted"],false);
            assert_eq!(generation["evaluation"]["verdict"],Value::Null);
            assert_eq!(generation["evaluation"]["metrics"]["eligible"],Value::Null);
            assert_eq!(generation["metaLearning"],Value::Null);
        }
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn nested_rsi_modules_remain_editable_improvement_methods() {
        for module in [
            "src/engine/rsi/learning.rs",
            "src/engine/rsi/evaluation.rs",
            "src/engine/rsi/planner.rs",
            "src/engine/rsi/execution.rs",
            "src/engine/rsi_meta_controller/peer_transfer.rs",
            "src/engine/rsi_meta_controller/guidance.rs",
            "src/engine/rsi_meta_controller/observations.rs",
            "src/engine/rsi_system/evidence.rs",
            "src/engine/rsi_system/planning.rs",
        ] {
            assert_eq!(source_role(module),"improvement_methods");
            assert!(runtime_source_file(module));
        }
        assert_eq!(source_role("src/engine/learning.rs"),"learning");
        assert_eq!(source_role("src/engine/goals.rs"),"agent_harness");
        assert_eq!(source_role("src/engine/goals/project.rs"),"agent_harness");
        assert_eq!(source_role("src/engine/goals/receipts.rs"),"agent_harness");
        assert_eq!(architecture_role_domain("agent_harness"),Some("context"));
        assert!(!runtime_source_file("src/engine/goals/tests.rs"));
        assert_eq!(source_role("src/engine/models/installer.rs"),"models");
        assert_eq!(source_role("src/engine/models/loader.rs"),"models");
        assert_eq!(source_role("src/engine/chat/provider/openai.rs"),"models");
        assert_eq!(source_role("src/engine/video.rs"),"video");
        assert_eq!(source_role("src/engine/video/ltx.rs"),"video");
        assert_eq!(source_role("src/api/video.rs"),"video");
        assert_eq!(source_role("src/engine/rooms.rs"),"multiagent");
        assert_eq!(source_role("src/engine/rooms/model_tiering.rs"),"multiagent");
        assert_eq!(source_role("src/engine/chat/room_context/role_routing.rs"),"multiagent");
        assert_eq!(architecture_role_domain("video"),Some("tools"));
        assert_eq!(architecture_role_domain("multiagent"),Some("context"));
        assert!(!runtime_source_file("src/engine/video/tests.rs"));
        assert!(!runtime_source_file("src/engine/rooms/tests.rs"));
    }
    #[test]
    fn descendant_source_inventory_excludes_test_only_artifacts() {
        for artifact in [
            "src/engine/rsi_evolve_tests.rs",
            "src/engine/rsi_oracle_fixture.rs",
            "src/engine/tests/worker.rs",
            "src/engine/fixtures/mock.rs",
            "src/engine/rsi_meta_controller/tests.rs",
            "src/engine/rsi_system/tests.rs",
            "src/engine/rsi_worktree/tests.rs",
            "src/engine/chat/runtime/outcome_tests.rs",
            "src/engine/chat/runtime/harness_tool_selection_tests.rs",
        ] {
            assert!(!runtime_source_file(artifact), "{artifact} is not a release capability");
        }
        for runtime in [
            "src/engine/rsi_evolve.rs",
            "src/engine/rsi_meta_controller.rs",
            "src/engine/rsi_system.rs",
            "src/engine/learning.rs",
            "web/src/App.tsx",
        ] {
            assert!(runtime_source_file(runtime), "{runtime} is executable intelligence");
        }
    }
    #[test]
    fn system_instruction_renders_json_shapes_as_literals() {
        let blueprint=json!({
            "generatorCommit":"base-commit",
            "architecture":[],
            "recentMeasuredGenerations":[]
        });
        let prompt=instruction(&blueprint,"design improvements");
        for expected in [
            "[{role,newFiles,integrationFiles,interfaceContract,integrationSymbol,improvementMechanism}]",
            "[{fromRole,toRole,fromPath,toPath,mechanism,expectedEffect}]",
            "[{role,domain,path,objective,learningPolicy,toolPolicy,executionPolicy}]",
            "[{id,role,domain,path,objective,mechanism}]",
            "[{from,to,when}]",
        ] {
            assert!(prompt.contains(expected), "system prompt lost literal JSON template: {expected}");
        }
    }

    #[test]
    fn coupled_design_counts_only_all_real_changed_files_and_distinct_roles() {
        let design=json!({"component":"system","components":["memory","learning"],
            "targetFiles":["src/engine/memory.rs","src/engine/learning.rs"],
            "componentFiles":{
                "memory":["src/engine/memory.rs"],
                "learning":["src/engine/learning.rs"]
            }});
        assert!(!design_implemented(&design,&[json!("src/engine/memory.rs")]),
            "a partially written multi-role redesign must be rejected");
        assert!(design_implemented(&design,&[
            json!("src/engine/memory.rs"),json!("src/engine/learning.rs")]));
        let mut duplicate=design.clone();
        duplicate["componentFiles"]["learning"]=json!(["src/engine/memory.rs"]);
        assert!(!design_implemented(&duplicate,&[
            json!("src/engine/memory.rs"),json!("src/engine/learning.rs")]),
            "a shared source path must never count twice as two redesigned roles");
    }

    #[test]
    fn only_an_adopted_model_can_program_a_rebuilt_descendants_next_task() {
        let rows=vec![json!({"role":"learning","domain":"learning",
            "path":"src/engine/learning.rs",
            "sourceSha256":"a".repeat(64)})];
        let model=json!({"accepted":true,"generatorCommit":"parent",
            "acceptedCommit":"child","jobId":"episode",
            "evaluation":{"metrics":{"eligible":true}},
            "acceptedSourceHashes":{
                "src/engine/memory.rs":"b".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "id":"design1","component":"memory",
                "generatorCommit":"parent","generationJobId":"episode",
                "targetFiles":["src/engine/memory.rs"],
                "successorStages":[{"role":"learning","domain":"learning",
                    "path":"src/engine/learning.rs",
                    "objective":"Build a distinct learned policy informed by real runtime failures"}]}});
        let picked=inherited_stage(&[model.clone()],&rows).unwrap();
        assert_eq!(picked["path"],"src/engine/learning.rs");
        assert_eq!(picked["qualityProven"],false);
        let mut unadopted=model.clone();
        unadopted["accepted"]=json!(false);
        assert!(inherited_stage(&[unadopted],&rows).is_none());
        let mut mismatched=model;
        mismatched["acceptedSourceHashes"]=json!({});
        assert!(inherited_stage(&[mismatched],&rows).is_none());
        let stale=vec![json!({"role":"learning","domain":"learning",
            "path":"src/engine/unknown.rs"})];
        assert!(inherited_stage(&[json!({"accepted":false})],&stale).is_none());
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
        assert_eq!(first["architecture"].as_array().unwrap().len(),11);
        assert_eq!(first["generatorCommit"],"commit-one");
        assert_eq!(first["historyCount"],1);
        assert_eq!(first["sourceInventory"].as_array().unwrap().len(),2);
        assert_eq!(first["sourceTreeSha256"].as_str().unwrap().len(),64);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace"));
        let baseline=first["architecture"][0]["representativeSources"][0]["sha256"].clone();
        fs::write(root.join("src/engine/algorithms.rs"),"pub fn changed_policy() {}").unwrap();
        let second=blueprint(&root,"commit-two",&history);
        assert_ne!(baseline,second["architecture"][0]["representativeSources"][0]["sha256"]);
        assert_ne!(first["generatorCommit"],second["generatorCommit"]);
        assert_ne!(first["sourceTreeSha256"],second["sourceTreeSha256"],
            "the full system inventory must follow real source changes");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn architecture_frontier_rejects_unverified_adoption_and_uses_real_index() {
        let root=std::env::temp_dir().join(format!("rsi-frontier-{}",
            uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        fs::write(root.join("src/engine/algorithms.rs"),
            "pub fn selector() -> usize { 1 }").unwrap();
        fs::write(root.join("src/engine/memory.rs"),
            "pub fn memory() -> usize { 2 }").unwrap();
        fs::write(root.join("src/engine/tools.rs"),
            "pub fn tool() -> usize { 3 }").unwrap();
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let model=blueprint(&root,"a".repeat(40).as_str(),&[]);
        let baseline=architecture_frontier(&state,&model,&[],
            index["id"].as_str().unwrap(),&root);
        assert!(baseline.iter().any(|row|row["role"]=="memory"));
        assert!(baseline.iter().all(|row|row["qualityProven"]==false));
        let forged=json!({"accepted":true,"pairedRevalidated":true,
            "candidateId":"fabricated","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),
            "acceptedSourceHashes":{"src/engine/memory.rs":
                model["architecture"][2]["representativeSources"][0]["sha256"]}});
        let explored=architecture_frontier(&state,&model,&[forged],index["id"].as_str().unwrap(),&root);
        assert_eq!(explored,baseline,
            "a claimed adoption cannot remove an unmodified system component");
        fs::write(root.join("src/engine/memory.rs"),
            "pub fn memory() -> usize { 4 }").unwrap();
        let stale=architecture_frontier(&state,&model,&[],index["id"].as_str().unwrap(),&root);
        assert!(stale.iter().all(|row|row["role"]!="memory"),
            "a changed source without a fresh index must not inherit its old SHA");
        let _=fs::remove_dir_all(root);
    }
    #[test]
    fn self_modifying_meta_policy_survives_frontier_size_truncation() {
        let root=std::env::temp_dir().join(format!("rsi-meta-frontier-{}",
            uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        // 20 cheaper method sources would previously push the larger,
        // executable meta-policy itself beyond the per-role frontier cap.
        for i in 0..20 {
            fs::write(root.join(format!("src/engine/rsi_minor_{i}.rs")),
                format!("pub fn minor_{i}() {{}}")).unwrap();
        }
        for &source in method_lineage::EXECUTABLE_METHOD_SOURCES {
            let file=root.join(source);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file,"pub fn method() { let _source = 123456789; }".repeat(512))
                .unwrap();
        }
        let config=crate::config::Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let model=blueprint(&root,&"a".repeat(40),&[]);
        let frontier=architecture_frontier(&state,&model,&[],
            index["id"].as_str().unwrap(),&root);
        for &source in method_lineage::EXECUTABLE_METHOD_SOURCES {
            assert!(frontier.iter().any(|row|row["path"]==source
                && row["role"]=="improvement_methods"
                && row["qualityProven"]==false),
                "inheritable executable method must remain discoverable: {source}");
        }
        let _=fs::remove_dir_all(root);
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
        new_module["targetFiles"]=json!(["src/engine/new_agent_method.rs"]);
        assert!(propose(&state,"session",&new_module).is_ok(),
            "a successor may design a new source module");
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
    fn measured_outcomes_change_successors_without_rewarding_research_only_generations() {
        assert_eq!(improvement_directive(&[])["mode"],"discover");
        let research=json!({"id":"g1","accepted":false,"newFindings":3,
            "candidateId":null,"domain":"harness"});
        assert_eq!(improvement_directive(&[research.clone()])["mode"],"implement_research");
        let mut attempted=research.clone();
        attempted["improvementDirective"]=json!({"mode":"implement_research"});
        assert_eq!(improvement_directive(&[attempted])["mode"],"change_approach");
        let accepted=json!({"id":"g2","accepted":true,"candidateId":"real-candidate",
            "evaluation":{"metrics":{"eligible":true}}});
        assert_eq!(improvement_directive(&[research,accepted])["mode"],"extend_verified_capability");
        let unverified=json!({"id":"fake","accepted":true,
            "evaluation":{"metrics":{"eligible":false}},"candidateId":null});
        assert_ne!(improvement_directive(&[unverified])["mode"],"extend_verified_capability");
    }

    #[test]
    fn only_adopted_model_design_can_control_a_new_compiled_descendant() {
        let base=json!({"generatorCommit":"parent","jobId":"worker",
            "acceptedCommit":"child","candidateOrigin":"model_driven_detached_worktree",
            "acceptedSourceHashes":{"src/engine/learning.rs":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "accepted":true,
            "evaluation":{"metrics":{"eligible":true}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"parent","generationJobId":"worker",
                "targetFiles":["src/engine/learning.rs"],
                "nextDomain":"learning"}});
        assert_eq!(inherited_domain(&[base.clone()]),Some("learning".into()));
        for (key,value) in [("accepted",json!(false))] {
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
fn detached_evaluator_sources_remain_fingerprinted_but_not_editable_frontier() {
    let root=std::env::temp_dir().join(format!(
        "rsi-frozen-worktree-boundary-{}",uuid::Uuid::new_v4()));
    let frozen="src/engine/rsi_worktree/comparison.rs";
    let editable="src/engine/rsi_meta_controller/peer_transfer.rs";
    for source in [frozen,editable] {
        let full=root.join(source);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full,"pub fn example() {}\n").unwrap();
    }
    let map=blueprint(&root,"pinned",&[]);
    let inventory=map["sourceInventory"].as_array().unwrap();
    assert!(inventory.iter().any(|item|item["path"]==frozen));
    let published=map["architecture"].as_array().unwrap().iter()
        .flat_map(|role|role["representativeSources"].as_array()
            .into_iter().flatten())
        .filter_map(|source|source["path"].as_str())
        .collect::<Vec<_>>();
    assert!(!published.contains(&frozen));
    assert!(published.contains(&editable));
    fs::remove_dir_all(root).unwrap();
}
