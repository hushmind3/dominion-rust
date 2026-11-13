#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_blueprint_exposes_each_runtime_boundary_to_descendant_planning() {
        let root=std::env::temp_dir().join(format!("rsi-harness-modules-{}",uuid::Uuid::new_v4()));
        let module_dir=root.join("src/engine/chat/runtime");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(root.join("src/engine/chat/runtime.rs"),"mod intent;").unwrap();
        let modules=[
            "intent.rs","turn.rs","adaptation.rs","tool_catalog.rs",
            "execution_program.rs","dispatch.rs","planning.rs","finalization.rs"
        ];
        for module in modules {
            fs::write(module_dir.join(module),format!("// {module}: mutable harness source")).unwrap();
        }
        let model=blueprint(&root,"modular-descendant",&[]);
        let harness=model["architecture"].as_array().unwrap().iter()
            .find(|entry|entry["role"]=="harness").unwrap();
        let representatives=harness["representativeSources"].as_array().unwrap();
        assert_eq!(harness["discoveredSourceCount"],9);
        for module in modules {
            let expected=format!("src/engine/chat/runtime/{module}");
            assert!(representatives.iter().any(|item|item["path"]==expected),
                "mutable module {expected} must be visible to the compiled descendant");
        }
        let prior=model["sourceTreeSha256"].clone();
        fs::write(module_dir.join("execution_program.rs"),"// revised executor").unwrap();
        let next=blueprint(&root,"new-descendant",&[]);
        assert_ne!(prior,next["sourceTreeSha256"],
            "changes to a moved execution module must affect lineage identity");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn compiled_model_frontier_includes_modular_runtime_and_transfer_sources() {
        let root=std::env::temp_dir().join(format!("rsi-model-boundaries-{}",uuid::Uuid::new_v4()));
        let dir=root.join("src/engine/models");
        fs::create_dir_all(&dir).unwrap();
        fs::write(root.join("src/engine/models.rs"),"mod inventory;").unwrap();
        let modules=["inventory.rs","runtime_install.rs","registration.rs",
            "session.rs","catalog.rs","downloads.rs"];
        for name in modules {
            fs::write(dir.join(name),format!("// {name}: model runtime module")).unwrap();
            assert_eq!(source_role(&format!("src/engine/models/{name}")),"models");
        }
        let plan=blueprint(&root,"model-refactor",&[]);
        let models=plan["architecture"].as_array().unwrap().iter()
            .find(|row|row["role"]=="models").unwrap();
        assert_eq!(models["discoveredSourceCount"],7);
        for name in modules {
            let expected=format!("src/engine/models/{name}");
            assert!(models["representativeSources"].as_array().unwrap().iter()
                .any(|row|row["path"]==expected),"{expected} is absent from model frontier");
        }
        let previous=plan["sourceTreeSha256"].clone();
        fs::write(dir.join("downloads.rs"),"// changed transfer method").unwrap();
        let next=blueprint(&root,"next",&[]);
        assert_ne!(previous,next["sourceTreeSha256"]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn provider_api_modules_retain_model_ownership_after_refactoring() {
        let root=std::env::temp_dir().join(format!("rsi-provider-modules-{}",uuid::Uuid::new_v4()));
        let dir=root.join("src/api/providers");
        fs::create_dir_all(&dir).unwrap();
        fs::write(root.join("src/api/providers.rs"),"mod auth_sources;").unwrap();
        let modules=["auth_sources.rs","discovery.rs","cli_credentials.rs",
            "codex_login.rs","provider_store.rs","connection_test.rs","model_list.rs"];
        for module in modules {
            fs::write(dir.join(module),format!("// {module}")).unwrap();
            assert_eq!(source_role(&format!("src/api/providers/{module}")),"models");
        }
        let model=blueprint(&root,"provider-registry",&[]);
        let frontier=model["architecture"].as_array().unwrap().iter()
            .find(|row|row["role"]=="models").unwrap();
        assert_eq!(frontier["discoveredSourceCount"],8);
        for module in modules {
            let path=format!("src/api/providers/{module}");
            assert!(frontier["representativeSources"].as_array().unwrap().iter()
                .any(|row|row["path"]==path),"provider source {path} was omitted");
        }
        let original=model["sourceTreeSha256"].clone();
        fs::write(dir.join("model_list.rs"),"// changed provider discovery").unwrap();
        assert_ne!(original,blueprint(&root,"new-provider",&[])["sourceTreeSha256"]);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn adopted_flags_do_not_mint_causal_closed_loop() {
        let dir=std::env::temp_dir().join(format!("rsi-closure-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.join("data"),
            web_dir:dir.join("web"),workspace:dir.join("workspace")};
        let state=crate::state::AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        let fabricated=(0..4).map(|n|json!({"accepted":true,
            "systemEvolution":true,"generatorCommit":format!("fake-{n}"),
            "acceptedCommit":format!("fake-{}",n+1),
            "behaviorCounterfactual":{"status":"behavior_changed"},
            "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":3}}}))
            .collect::<Vec<_>>();
        let report=crate::engine::rsi_causal_closure::audit(&state,&fabricated);
        assert_eq!(report["demonstrated"],false);
        assert_eq!(report["closedLoopCount"],0);
        fs::remove_dir_all(dir).unwrap();
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
        assert_eq!(first["sourceInventory"].as_array().unwrap().len(),2);
        assert_eq!(first["sourceTreeSha256"].as_str().unwrap().len(),64);
        assert_eq!(first["recentMeasuredGenerations"][0]["accepted"],true);
        assert!(instruction(&first,"improve the whole system").contains("workspace_replace_range"));
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
    fn large_agent_source_map_has_full_digest_but_bounded_transfer() {
        let root=std::env::temp_dir().join(format!("rsi-system-many-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        for n in 0..90 {
            fs::write(root.join(format!("src/engine/method_{n:03}.rs")),
                format!("pub fn method_{n:03}() {{}}")).unwrap();
        }
        let full=blueprint(&root,"old",&[]);
        assert_eq!(full["sourceFileCount"],90);
        assert_eq!(full["sourceInventory"].as_array().unwrap().len(),64);
        assert_eq!(full["sourceInventoryTruncated"],true);
        fs::write(root.join("src/engine/method_089.rs"),"pub fn changed() {}").unwrap();
        let changed=blueprint(&root,"new",&[]);
        assert_eq!(full["sourceInventory"],changed["sourceInventory"],
            "the changed file falls outside the bounded model preview");
        assert_ne!(full["sourceTreeSha256"],changed["sourceTreeSha256"],
            "the complete source fingerprint must include every file");
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
        new_module["targetFiles"]=json!(["src/engine/new_agent_method.rs"]);
        assert!(propose(&state,"session",&new_module).is_ok(),
            "a successor may design a new source module");
        let mut fresh_component=new_module.clone();
        fresh_component["component"]=json!("new_reasoning_controller");
        assert!(propose(&state,"session",&fresh_component).is_ok(),
            "the intelligence may invent architectural roles beyond original fixed components");
        fresh_component["component"]=json!("../outside");
        assert!(propose(&state,"session",&fresh_component).is_err());

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
    fn compiled_successor_distinguishes_whole_system_design_from_downstream_tasks() {
        let blueprint=json!({"generatorCommit":"source-revision",
            "architecture":[{"role":"harness","representativeSources":[]}],
            "recentMeasuredGenerations":[]});
        let task="Improve a specific Rust parser and preserve existing tests";
        let instructions=instruction(&blueprint,task);
        assert!(instructions.contains(task));
        assert!(instructions.contains("solve THAT task"));
        assert!(instructions.contains("rsi_system_plan"));
        assert!(instructions.contains("workspace_replace_range"));
        assert!(!instructions.contains("workspace_replace_function"),
            "do not advertise a tool missing from the active harness");
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
            "acceptedCommit":"child","candidateId":"verified-candidate",
            "candidateOrigin":"model_driven_detached_worktree",
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
        let mut after_failure=vec![base.clone(),json!({
            "id":"failed-next","accepted":false,"domain":"learning",
            "error":"first attempt did not compile","newFindings":1
        })];
        assert_eq!(inherited_domain(&after_failure),Some("learning".into()),
            "one failure must not erase an adopted design");
        assert_eq!(lineage_capsule(&after_failure)["acceptedDesigns"].as_array().unwrap().len(),1);
        assert_eq!(lineage_capsule(&after_failure)["failureCounterexamples"].as_array().unwrap().len(),1);
        after_failure.push(json!({"id":"failed-again","accepted":false,
            "domain":"learning","error":"different approach failed"}));
        assert_eq!(inherited_domain(&after_failure),None,
            "after repeated failure a descendant may explore another domain");
        assert_eq!(lineage_capsule(&after_failure)["acceptedDesigns"].as_array().unwrap().len(),1,
            "target exploration must not discard adopted design history");
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
}

#[cfg(test)]
mod integration_contract_tests {
    use super::*;
    use crate::{config::Config,db::Store,state::AppState,vault::Vault};
    fn state()->(AppState,std::path::PathBuf) {
        let root=std::env::temp_dir().join(format!("rsi-integrated-contract-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("src/engine")).unwrap();
        let config=Config{port:0,data_dir:root.join("data"),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        (state,root)
    }
    #[test]
    fn adopted_successor_program_survives_failures_and_rebinds_new_source() {
        let original=json!({
            "id":"generation-1","jobId":"worker-1",
            "generatorCommit":"a".repeat(40),"acceptedCommit":"b".repeat(40),
            "candidateId":"rsi-model-patch","accepted":true,
            "evaluation":{"metrics":{"eligible":true}},
            "acceptedSourceHashes":{"src/engine/rsi_meta_controller.rs":"c".repeat(64)},
            "systemDesign":{
                "id":"model-program","source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),"generationJobId":"worker-1",
                "targetFiles":["src/engine/rsi_meta_controller.rs"],
                "successorStages":[{
                    "role":"improvement_methods","domain":"improver",
                    "path":"src/engine/rsi_meta_controller.rs",
                    "objective":"Improve the meta-controller's choice of new source policies",
                    "learningPolicy":"retry_failed","toolPolicy":"patch_only",
                    "executionPolicy":"grounded_edit"}]
            }
        });
        let current_source=json!({
            "role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs",
            "sourceSha256":"d".repeat(64)
        });
        let history=vec![original,json!({"id":"generation-2",
            "accepted":false,"focusFiles":["src/engine/rsi_meta_controller.rs"],
            "error":"compiler rejected first patch"})];
        let selected=inherited_stage(&history,&[current_source]).unwrap();
        assert_eq!(selected["source"],"inherited_model_authored_improvement_program_v1");
        assert_eq!(selected["sourceSha256"],"d".repeat(64),
            "programs must bind to the new version, not the old source");
        assert_eq!(selected["failuresSinceAdoption"],1);
        assert_eq!(selected["toolPolicy"],"patch_only");
        assert_eq!(selected["qualityProven"],false);
        assert!(inherited_stage(&history,&[]).is_none(),
            "stale or absent source modules cannot run an inherited program");
    }
    #[test]
    fn source_portfolio_exposes_new_meta_mechanisms_to_descendants() {
        let (state,root)=state();
        fs::write(root.join("src/engine/rsi_evolve.rs"),"pub fn parent() {}\n").unwrap();
        fs::write(root.join("src/engine/rsi_meta_controller.rs"),
            "pub fn meta_improve() {}\n").unwrap();
        fs::write(root.join("src/engine/rsi_system.rs"),"pub fn plan() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let blueprint=blueprint(&root,"source-commit",&[]);
        let frontier=architecture_frontier(&state,&blueprint,&[],
            index["id"].as_str().unwrap(),&root);
        let improvers=frontier.iter().filter(|row|
            row["role"]=="improvement_methods").collect::<Vec<_>>();
        assert!(improvers.len()>=3,
            "meta-self-modification needs multiple candidate source files");
        assert!(improvers.iter().any(|row|
            row["path"]=="src/engine/rsi_meta_controller.rs"));
        let target=improvers[0].clone();
        let plan=json!({"systemBlueprint":blueprint,
            "architectureFrontier":frontier,"architectureTarget":target});
        assert!(validate_plan_evidence(&state,&root,"source-commit",
            index["id"].as_str().unwrap(),&plan).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn supervisor_accepts_new_frontier_order_and_role_but_rejects_stale_source() {
        let (state,root)=state();
        fs::write(root.join("src/engine/memory.rs"),"pub fn recall() {}\n").unwrap();
        fs::write(root.join("src/engine/tools.rs"),"pub fn act() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut model=blueprint(&root,"revision",&[]);
        let frontier=architecture_frontier(&state,&model,&[],index["id"].as_str().unwrap(),&root);
        assert_eq!(frontier.len(),2);
        let mut changed_order=frontier.clone();changed_order.reverse();
        let plan=json!({"systemBlueprint":model,"architectureFrontier":changed_order,
            "architectureTarget":changed_order.last()});
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&plan).is_ok(),
            "the old parent must not require its own ranking or first target");
        let mut new_role=plan.clone();
        model["architecture"].as_array_mut().unwrap().push(json!({"role":"novel_executor","domain":"tools",
            "representativeSources":[{"path":"src/engine/tools.rs","sha256":frontier.iter().find(|r|r["role"]=="tools").unwrap()["sourceSha256"]}]}));
        new_role["systemBlueprint"]=model;
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&new_role).is_ok());
        fs::write(root.join("src/engine/tools.rs"),"pub fn act_differently() {}\n").unwrap();
        assert!(validate_plan_evidence(&state,&root,"revision",index["id"].as_str().unwrap(),&new_role).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_agent_failures_become_descendant_scheduling_evidence() {
        let (state,root)=state();fs::create_dir_all(root.join("src/engine/chat")).unwrap();
        fs::write(root.join("src/engine/chat/runtime.rs"),"pub fn run() {}\n").unwrap();
        fs::write(root.join("src/engine/decompose.rs"),"pub fn short() {}\n").unwrap();
        state.store.put("execution",&json!({"id":"ordinary-task","taskOutcome":"failed","failureClass":"tool","error":"file operation failed"}));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let rows=architecture_frontier(&state,&blueprint(&root,"r",&[]),&[],index["id"].as_str().unwrap(),&root);
        assert_eq!(rows[0]["path"],"src/engine/chat/runtime.rs");
        assert!(rows[0]["usageSignalCount"].as_u64().unwrap()>0);
        assert_eq!(usage_evidence(&state)["signals"][0]["evidence"][0]["executionId"],"ordinary-task");
        assert_eq!(rows[0]["qualityProven"],false);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unverified_success_claim_does_not_close_an_architecture_gap() {
        let (state,root)=state();fs::write(root.join("src/engine/memory.rs"),"pub fn recall() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();let model=blueprint(&root,"r",&[]);
        let fake=json!({"accepted":true,"pairedRevalidated":true,"candidateId":"nonexistent",
            "focusFiles":["src/engine/memory.rs"],"systemDesign":{"component":"memory"}});
        let rows=architecture_frontier(&state,&model,&[fake],index["id"].as_str().unwrap(),&root);
        assert_eq!(rows[0]["verifiedRoleChanged"],false);assert_eq!(rows[0]["sourceAttempts"],1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multi_component_design_requires_complete_source_mapping_and_complete_patch() {
        let (state,root)=state();
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"running"}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"campaign","rsiSystemEvolution":true,"rsiGeneratorCommit":"a".repeat(40)}}));
        state.store.put("session",&json!({"id":"session","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"child"}));
        let mut body=json!({"component":"system","components":["memory","novel_planner"],"nextDomain":"memory",
            "hypothesis":"A source-backed coupled redesign should retain original facts and reduce retries",
            "design":"Introduce a new planner module and bind its memory access to an independently checked context protocol",
            "targetFiles":["src/engine/memory.rs","src/engine/new_planner.rs"]});
        assert!(propose(&state,"session",&body).is_err());
        body["componentFiles"]=json!({"memory":["src/engine/memory.rs"],"novel_planner":["src/engine/new_planner.rs"]});
        let proposal=propose(&state,"session",&body).unwrap()["proposal"].clone();
        assert_eq!(proposal["components"].as_array().unwrap().len(),2);
        assert!(!design_implemented(&proposal,&[json!("src/engine/memory.rs")]));
        assert!(design_implemented(&proposal,body["targetFiles"].as_array().unwrap()));
        // The adopted-design contract requires a nonempty original candidate ID.
        // A fake "accepted" flag without source-candidate provenance is not
        // a transferable improvement, even if its role mapping looks complete.
        let mut history=json!({"accepted":true,"evaluation":{"metrics":{"eligible":true}},"generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"jobId":"child","systemDesign":proposal,
            "acceptedSourceHashes":{"src/engine/memory.rs":"c".repeat(64)}});
        assert!(inherited_domain(&[history.clone()]).is_none());
        history["acceptedSourceHashes"]["src/engine/new_planner.rs"]=json!("d".repeat(64));
        assert!(inherited_domain(&[history.clone()]).is_none(),
            "a complete map alone must not inherit an unattributed design");
        history["candidateId"]=json!("measured-contract-candidate");
        assert_eq!(inherited_domain(&[history.clone()]),Some("memory".into()));
        history["acceptedSourceHashes"]["src/engine/new_planner.rs"]=Value::Null;
        assert!(inherited_domain(&[history]).is_none(),
            "the improvement must retain all mapped source digests");
        fs::remove_dir_all(root).unwrap();
    }
    // These are simulated recorded-experiment receipts, not claims of new live
    // RSI gains. Exercise the real independent result replayer and reject
    // forged flags, incomplete changes and unwitnessed method transfer.
    fn verified_generation(state:&AppState,n:usize,roles:[&str;2])->Value {
        let generator=format!("{}",n+1).repeat(40);let accepted=format!("{}",n+2).repeat(40);
        let paths=[format!("src/engine/{}_part.rs",roles[0]),format!("src/engine/{}_part.rs",roles[1])];
        let patch=paths.iter().map(|path|format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-fn before() {{}}\n+fn after() {{}}\n")).collect::<String>();
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i<4{"improver"}else{"memory"},
            "holdout":i%2==0,"filter":format!("fixture::case_{i}"),"metrics":[{"key":"quality","direction":"higher"}]})).collect::<Vec<_>>();
        let c=crate::engine::rsi::propose(state,&json!({"target":"harness","baseCommit":generator,"patch":patch,
            "hypothesis":"An independently measured fixture generation with causal source and design binding",
            "cases":cases,"oracleVersion":"contract-fixture","generatorModel":"fixture"})).unwrap();
        let report=json!({"baseCommit":generator,"patchSha256":c["patchSha256"],"oracleVersion":"contract-fixture",
            "runner":"fixture","reportSha256":"f".repeat(64),"baselineBuildOk":true,"candidateBuildOk":true,
            "cases":cases.iter().map(|case|json!({"id":case["id"],"baselinePass":true,"candidatePass":true,
                "metrics":{"quality":{"baseline":0.5,"candidate":0.8}}})).collect::<Vec<_>>()});
        let id=c["id"].as_str().unwrap();let mut c=crate::engine::rsi::evaluate(state,id,&report).unwrap();
        c["origin"]=json!("model_driven_detached_worktree");c["experimentAdoption"]=json!({"commit":accepted});
        c["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
        c["evaluation"]["evidenceSha256"]=json!("f".repeat(64));state.store.put("rsi_candidate",&c);
        state.store.put("rsi_pair",&json!({"id":format!("rsi-pair-{id}"),"candidateId":id,"runner":"local_gitrepo_not_os_sandbox",
            "report":report,"evidenceSha256":"f".repeat(64),"metrics":c["evaluation"]["metrics"],"verdict":c["evaluation"]["verdict"]}));
        let mut hashes=serde_json::Map::new();for path in &paths{hashes.insert(path.clone(),json!("e".repeat(64)));}
        let mut mapping=serde_json::Map::new();for (role,path) in roles.iter().zip(paths.iter()){mapping.insert(role.to_string(),json!([path]));}
        let blueprint=json!({"generatorCommit":generator,"sourceTreeSha256":"a".repeat(64)});
        json!({"id":format!("generation-{n}"),"jobId":format!("job-{n}"),"accepted":true,"systemEvolution":true,
            "generatorCommit":generator,"acceptedCommit":accepted,"generatorBinarySha256":format!("{}",n+1).repeat(64),
            "candidateId":id,"systemBlueprintVerified":true,"systemBlueprint":blueprint,"tokenUsage":{"calls":1},
            "controllerPlan":{"source":"compiled_descendant_system_controller_v2","generatorCommit":generator,"systemBlueprint":blueprint},
            "acceptedSourceHashes":hashes,"systemDesign":{"source":"model_authored_system_design","component":"system","components":roles,
                "componentFiles":mapping,"generatorCommit":generator,"generationJobId":format!("job-{n}"),"targetFiles":paths}})
    }
    #[test]
    fn complete_handoff_requires_real_pair_replay_complete_design_and_inherited_method_execution() {
        let (state,root)=state();
        let mut history=vec![verified_generation(&state,0,["algorithms","harness"]),
            verified_generation(&state,1,["memory","learning"]),verified_generation(&state,2,["tools","improvement_methods"])];
        let first=handoff_evidence(&state,&history);
        assert_eq!(first["verifiedLineageLength"],3);assert_eq!(first["demonstrated"],false);
        assert_eq!(first["missingComponents"],json!([]));assert_eq!(first["metaImprovementObserved"],true);
        history[0]["methodProposals"]=json!([{"id":"new-method","revision":"a".repeat(64),"sourceGenerationJobId":"job-0",
            "verification":{"type":"procedure","nodes":[{"tool":"code_search"}]}}]);
        history[1]["methodExecutions"]=json!([{"methodId":"new-method","revision":"a".repeat(64),"executionId":"exec-1","operationIds":["proof-write"],"result":{"verified":true,"followed":true}}]);
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false,"an assertion of execution is not causal proof");
        state.store.put("experience",&json!({"id":"method-use","executableMethods":[{"id":"new-method","revision":"a".repeat(64),
            "applications":[{"jobId":"job-1","methodWroteAcceptedCandidate":true,"verificationVerdict":"passed"}]}]}));
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false,"a stored application flag is not a tool receipt");
        state.store.put("job",&json!({"id":"job-1","sessionId":"session-1"}));
        let mut operation=json!({"id":"proof-write","runId":"exec-1","sessionId":"session-1",
            "origin":"procedure","procedureStrategyId":"new-method","procedureRevision":"a".repeat(64),
            "status":"completed","tool":"workspace_replace_range","arguments":{"path":"src/engine/memory_part.rs"},
            "result":{"written":true},"toolReceipt":{"verified":true,"observedVersion":format!("sha256:{}","e".repeat(64))}});
        state.store.put("execution_operation",&operation);
        let verified_handoff=handoff_evidence(&state,&history);
        assert_eq!(verified_handoff["inventedMethodTransferred"],true);
        assert_eq!(verified_handoff["structuralHandoffVerified"],true);
        assert_eq!(verified_handoff["metaQualityEvidenceVerified"],false);
        assert_eq!(verified_handoff["demonstrated"],false,
            "a tool receipt proves lineage transfer but not improved offspring");
        operation["toolReceipt"]["observedVersion"]=json!(format!("sha256:{}","d".repeat(64)));
        state.store.put("execution_operation",&operation);
        assert_eq!(handoff_evidence(&state,&history)["structuralHandoffVerified"],false,
            "superseded method output cannot claim the adopted code");
        operation["toolReceipt"]["observedVersion"]=json!(format!("sha256:{}","e".repeat(64)));
        state.store.put("execution_operation",&operation);
        history[1]["acceptedSourceHashes"]["src/engine/learning_part.rs"]=Value::Null;
        assert_eq!(handoff_evidence(&state,&history)["demonstrated"],false);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod modular_rsi_source_role_tests {
    use super::*;

    #[test]
    fn nested_rsi_sources_remain_selectable_by_the_descendant_meta_learner() {
        let modules = [
            "src/engine/rsi/planner/policy.rs",
            "src/engine/rsi/planner/context.rs",
            "src/engine/rsi/planner/descendant.rs",
            "src/engine/rsi/execution/campaign.rs",
            "src/engine/rsi/execution/generation.rs",
            "src/engine/rsi/execution/worker.rs",
            "src/engine/rsi/evaluation/trials.rs",
            "src/engine/rsi/lineage/source.rs",
            "src/engine/rsi/lineage/campaign.rs",
            "src/engine/rsi/learning/methods.rs",
            "src/engine/rsi/learning/research.rs",
            "src/engine/rsi/learning/generation_feedback.rs",
            "src/engine/rsi_system/lineage.rs",
            "src/engine/rsi_system/guidance.rs"
        ];
        let listed = COMPONENTS.iter()
            .find(|(role,_)|*role=="improvement_methods")
            .expect("an independently editable improvement-method role").1;
        for path in [
            "src/engine/rsi/proof.rs",
            "src/engine/rsi_system/design.rs",
            "src/engine/rsi_system/proposal.rs",
            "src/engine/rsi_system/handoff.rs",
            "src/engine/rsi_system/peer_verifier.rs",
            "src/engine/rsi/execution/compiled_planning.rs",
            "src/engine/rsi/execution/generation_evidence.rs",
            "src/engine/rsi/execution/generation_finalization.rs"
        ] {
            assert!(!crate::engine::rsi::safe_path(path),
                "a descendant candidate cannot modify its own final judge: {path}");
        }
        for path in modules {
            assert_eq!(source_role(path),"improvement_methods",
                "modular RSI descendants must not be mistaken for task algorithms");
            assert!(listed.contains(&path),
                "the descendant blueprint must keep new module sources discoverable: {path}");
        }
    }
}
