use super::*;
    #[test] fn mixed_model_or_helper_calls_cannot_earn_single_actor_credit(){
        let selected=identity("provider-a","model-a");
        let events=vec![
            json!({"event":"model.usage","detail":{"sessionId":"session-1",
                "purpose":"chat","providerId":"provider-a",
                "model":"model-a","totalTokens":40}}),
            json!({"event":"model.usage","detail":{"sessionId":"session-1",
                "purpose":"chat","providerId":"provider-b",
                "model":"model-b","totalTokens":70}}),
            json!({"event":"model.usage","detail":{"sessionId":"session-1",
                "purpose":"helper","providerId":"provider-c",
                "model":"model-c","totalTokens":20}}),
            json!({"event":"model.usage","detail":{"sessionId":"other",
                "purpose":"chat","providerId":"provider-d",
                "model":"model-d","totalTokens":10}})
        ];
        let (selected_calls,mixed,actors)=session_chat_calls(&events,"session-1",&selected);
        assert_eq!(selected_calls,1);
        assert_eq!(mixed,1);
        assert_eq!(actors.len(),2);
        let mut incomplete=events.clone();
        incomplete.push(json!({"event":"model.usage","detail":{
            "sessionId":"session-1","purpose":"chat","totalTokens":13}}));
        let (accepted,ambiguous,actors)=session_chat_calls(
            &incomplete,"session-1",&selected);
        assert_eq!(accepted,1);
        assert_eq!(ambiguous,2);
        assert!(actors.contains("unknown_model_actor"));
        let (only,foreign,_)=session_chat_calls(&events[..1],"session-1",&selected);
        assert_eq!((only,foreign),(1,0));
        let (none,_,_)=session_chat_calls(&events,"",&selected);
        assert_eq!(none,0);
    }
    #[test] fn fake_single_model_receipt_from_mixed_session_is_rejected(){
        let mut g=json!({"modelEcologyExecuted":{
            "source":"real_descendant_model_usage_v1",
            "generationJobId":"generation","generatorCommit":"a".repeat(40),
            "providerId":"p","model":"m","selectedBy":"policy",
            "routeMatchedWorkerInput":true,
            "auditComplete":true,"auditWindowStartRowid":0,"auditWindowEndRowid":1,
            "sessionOwnedByGeneration":true,
            "mixedChatModelCalls":1,
            "actualModelCalls":1,"actuallyUsedInGeneration":true},
            "jobId":"generation","generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "modelEcologyPolicy":{"revision":"policy",
                "selected":{"providerId":"p","model":"m"}},
            "pairedMeasured":true,
            "evaluation":{"metrics":{"measurements":[{"id":"holdout"}]}}
        });
        assert!(outcome(&g).is_none());
        g["modelEcologyExecuted"]["mixedChatModelCalls"]=json!(0);
        assert!(outcome(&g).is_some());
        g["modelEcologyExecuted"]["sessionOwnedByGeneration"]=json!(false);
        assert!(outcome(&g).is_none());
    }
    #[test] fn cross_campaign_verified_pair_is_recovered_and_tampering_excluded(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "sub4-source-matched-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let base="a".repeat(40);
        let cases=json!((0..8).map(|i|json!({
            "id":format!("case-{i}"),"domain":"memory","holdout":i<2,
            "filter":format!("rsi_frozen_oracle::case_{i}"),
            "metrics":[{"key":"quality","direction":"higher",
                "minRelativeGain":0.1}]
        })).collect::<Vec<_>>());
        let patch="diff --git a/src/engine/memory.rs b/src/engine/memory.rs\n--- a/src/engine/memory.rs\n+++ b/src/engine/memory.rs\n@@ -1 +1 @@\n-old\n+model-edit\n";
        let mut candidate=crate::engine::rsi::propose(&state,&json!({
            "target":"algorithm","baseCommit":base,"patch":patch,
            "hypothesis":"A measured source change in memory implementation.",
            "cases":cases,"oracleVersion":"rsi-worktree-v2",
            "generatorModel":"actor-model"})).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        let oracle=format!("{:x}",Sha256::digest(
            include_bytes!("../../src/engine/rsi_oracle_fixture.rs")));
        let report=json!({
            "baseCommit":base,"patchSha256":candidate["patchSha256"],
            "oracleVersion":"rsi-worktree-v2",
            "reportSha256":"d".repeat(64),
            "oracleSha256":oracle,"runner":"test",
            "baselineBuildOk":true,"candidateBuildOk":true,
            "cases":(0..8).map(|i|json!({
                "id":format!("case-{i}"),"baselinePass":true,
                "candidatePass":true,
                "metrics":{"quality":{"baseline":0.5,"candidate":0.51}}
            })).collect::<Vec<_>>()
        });
        candidate=crate::engine::rsi::evaluate(&state,&id,&report).unwrap();
        candidate["origin"]=json!("model_driven_detached_worktree");
        candidate["pairedComparisonComplete"]=json!(true);
        candidate["evaluation"]["source"]=json!(
            "server_invoked_local_paired_git_worktrees");
        candidate["evaluation"]["evidenceSha256"]=json!("d".repeat(64));
        state.store.put("rsi_candidate",&candidate).unwrap();
        state.store.put("rsi_pair",&json!({
            "id":format!("rsi-pair-{id}"),"candidateId":id,
            "runner":"local_gitrepo_not_os_sandbox",
            "report":report,"evidenceSha256":"d".repeat(64),
            "metrics":candidate["evaluation"]["metrics"],
            "verdict":candidate["evaluation"]["verdict"]
        })).unwrap();
        assert!(crate::engine::rsi::replayed_pair_metrics(
            &state,&candidate).is_some());
        let route=identity("provider-a","actor-model");
        let policy=json!({"revision":"policy-executed","active":true,
            "selected":route});
        state.store.put("session",&json!({
            "id":"real-child-session","rsiRun":true,
            "rsiGenerationJobId":"real-child"
        })).unwrap();
        let audit_start=state.store.audit_rowid_cursor().unwrap();
        state.store.audit("model.usage",json!({
            "sessionId":"real-child-session","purpose":"chat",
            "providerId":"provider-a","model":"actor-model",
            "totalTokens":17
        }));
        let audit_end=state.store.audit_rowid_cursor().unwrap();
        let compiled=json!({"source":"running_compiled_descendant_binary"});
        let input=json!({
            "rsiGeneratorCommit":base,"rsiModelEcologyPolicy":policy,
            "rsiFrozenCases":cases,"providerId":"provider-a",
            "model":"actor-model"
        });
        let mut job=json!({"id":"real-child","kind":"rsi_generation",
            "sessionId":"real-child-session",
            "rsiAuditStartRowid":audit_start,
            "rsiAuditEndRowid":audit_end,
            "input":input,"rsiCompiledPlanExecuted":compiled});
        let executed=executed(&state,&job);
        assert_eq!(executed["actuallyUsedInGeneration"],true);
        job["rsiModelEcologyExecuted"]=executed.clone();
        state.store.put("job",&job).unwrap();
        let profile=crate::engine::rsi_lineage_transfer::profile(&input).unwrap();
        let objective=crate::engine::memory::stable_id(&["memory"]);
        let record=json!({
            "id":"cross-campaign-1","campaignId":"older-campaign",
            "jobId":"real-child","generatorCommit":base,
            "comparisonProfileSha256":profile,"objectiveId":objective,
            "domain":"memory","candidateId":id,
            "candidateOrigin":"model_driven_detached_worktree",
            "modelEcologyPolicy":policy,"modelEcologyExecuted":executed,
            "compiledPlanExecuted":compiled,
            "accepted":false,"pairedMeasured":true,
            "evaluation":candidate["evaluation"]
        });
        state.store.put("rsi_generation",&record).unwrap();
        let saved=historical_same_source_trials(
            &state,&[],&base,&profile,&objective,"memory");
        assert_eq!(saved.len(),1);
        let matched=matched_cohort(&saved,&route,&base,&profile,&objective,"memory");
        assert_eq!(matched["comparedGenerations"],1);
        // Later traffic from a reused session must never be retroactively
        // credited to the sealed generation.
        state.store.audit("model.usage",json!({
            "sessionId":"real-child-session","purpose":"chat",
            "providerId":"later-model","model":"unrelated",
            "totalTokens":99
        }));
        assert_eq!(historical_same_source_trials(
            &state,&[],&base,&profile,&objective,"memory").len(),1);
        let mut moved=job.clone();
        moved["rsiAuditEndRowid"]=json!(state.store.audit_rowid_cursor().unwrap());
        state.store.put("job",&moved).unwrap();
        assert!(historical_same_source_trials(
            &state,&[],&base,&profile,&objective,"memory").is_empty());
        state.store.put("job",&job).unwrap();
        // The current campaign already loaded this job: never double count.
        assert!(historical_same_source_trials(
            &state,&[record.clone()],&base,&profile,&objective,"memory").is_empty());
        // A claimed model outcome must still match its original job record.
        let mut forged=job.clone();
        forged["input"]["model"]=json!("other-model");
        state.store.put("job",&forged).unwrap();
        assert!(historical_same_source_trials(
            &state,&[],&base,&profile,&objective,"memory").is_empty());
        state.store.put("job",&job).unwrap();
        let mut changed=candidate.clone();
        changed["evaluation"]["evidenceSha256"]=json!("c".repeat(64));
        state.store.put("rsi_candidate",&changed).unwrap();
        assert!(historical_same_source_trials(
            &state,&[],&base,&profile,&objective,"memory").is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }

    #[test] fn cross_campaign_model_comparison_rejects_missing_original_job(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "sub4-cross-campaign-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let revision="a".repeat(40);
        let profile="p".repeat(64);
        let objective=crate::engine::memory::stable_id(&["tools"]);
        let fake=json!({
            "id":"claimed-success","jobId":"nonexistent-job",
            "generatorCommit":revision,"comparisonProfileSha256":profile,
            "objectiveId":objective,"domain":"tools",
            "candidateId":"nonexistent-candidate",
            "modelEcologyPolicy":{"revision":"p","selected":{
                "providerId":"api","model":"m"}},
            "modelEcologyExecuted":{
                "source":"real_descendant_model_usage_v1",
                "generationJobId":"nonexistent-job",
                "generatorCommit":revision,
                "selectedBy":"p","providerId":"api","model":"m",
                "sessionOwnedByGeneration":true,
                "routeMatchedWorkerInput":true,
            "auditComplete":true,"auditWindowStartRowid":0,"auditWindowEndRowid":1,
                "mixedChatModelCalls":0,
                "actuallyUsedInGeneration":true,"actualModelCalls":1},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "candidateOrigin":"model_driven_detached_worktree",
            "accepted":true,"pairedMeasured":true,
            "pairedRevalidated":true,"acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"measurements":[{"id":"holdout"}]}}
        });
        state.store.put("rsi_generation",&fake).unwrap();
        let historical=historical_same_source_trials(
            &state,&[],&revision,&profile,&objective,"tools");
        assert!(historical.is_empty());
        assert!(historical_same_source_trials(
            &state,&[],&"c".repeat(40),&profile,&objective,"tools").is_empty());
        assert!(historical_same_source_trials(
            &state,&[],&revision,&"x".repeat(64),&objective,"tools").is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test] fn owned_real_session_and_exclusive_model_usage_are_required(){
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "sub4-actor-owned-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let session=state.store.put("session",&json!({
            "id":"owned-session","rsiRun":true,
            "rsiGenerationJobId":"generation-1"})).unwrap();
        assert_eq!(session["id"],"owned-session");
        let mut job=json!({"id":"generation-1","sessionId":"owned-session",
            "rsiAuditStartRowid":0,"rsiAuditEndRowid":0,
            "input":{"rsiGeneratorCommit":"a".repeat(40),
                "providerId":"provider-a","model":"model-a",
                "rsiModelEcologyPolicy":{"active":true,
                    "revision":"compiled-selection",
                    "selected":{"providerId":"provider-a","model":"model-a"}}}});
        state.store.audit("model.usage",json!({
            "sessionId":"owned-session","purpose":"chat",
            "providerId":"provider-a","model":"model-a","totalTokens":81}));
        job["rsiAuditEndRowid"]=json!(state.store.audit_rowid_cursor().unwrap());
        let measured=executed(&state,&job);
        assert_eq!(measured["actualModelCalls"],1);
        assert_eq!(measured["mixedChatModelCalls"],0);
        assert_eq!(measured["actuallyUsedInGeneration"],true);
        state.store.audit("model.usage",json!({
            "sessionId":"owned-session","purpose":"helper",
            "providerId":"helper-provider","model":"helper","totalTokens":12}));
        job["rsiAuditEndRowid"]=json!(state.store.audit_rowid_cursor().unwrap());
        assert_eq!(executed(&state,&job)["actuallyUsedInGeneration"],true);
        state.store.audit("model.usage",json!({
            "sessionId":"owned-session","purpose":"chat",
            "providerId":"provider-b","model":"model-b","totalTokens":12}));
        // 715 later unrelated events exceed recent_audit(800)'s true 500
        // result cap. Both earlier chat actors must still be visible.
        for i in 0..715{
            state.store.audit("tool.call",json!({"number":i}));
        }
        job["rsiAuditEndRowid"]=json!(state.store.audit_rowid_cursor().unwrap());
        let mixed=executed(&state,&job);
        assert_eq!(mixed["actualModelCalls"],1);
        assert_eq!(mixed["mixedChatModelCalls"],1);
        assert_eq!(mixed["actuallyUsedInGeneration"],false);
        let mut missing_end=job.clone();
        missing_end["rsiAuditEndRowid"]=Value::Null;
        assert_eq!(executed(&state,&missing_end)["auditComplete"],false);
        assert_eq!(executed(&state,&missing_end)["actuallyUsedInGeneration"],false);
        job["id"]=json!("generation-other");
        assert_eq!(executed(&state,&job)["sessionOwnedByGeneration"],false);
        job["id"]=json!("generation-1");
        job["input"]["model"]=json!("model-other");
        assert_eq!(executed(&state,&job)["routeMatchedWorkerInput"],false);
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test] fn execution_requires_real_model_traffic(){
        let g=json!({"modelEcologyExecuted":{"source":"real_descendant_model_usage_v1",
            "providerId":"api-1","model":"model-a",
            "actuallyUsedInGeneration":true,"actualModelCalls":0},
            "generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"evaluation":{"metrics":{"measurements":[]}}});
        assert!(outcome(&g).is_none());
    }
    #[test] fn independent_source_receipt_not_token_loss_is_reward(){
        let mut g=json!({"modelEcologyExecuted":{"source":"real_descendant_model_usage_v1",
            "providerId":"api-1","model":"model-a",
            "generationJobId":"generation-1",
            "generatorCommit":"a".repeat(40),
            "selectedBy":"chosen-policy",
            "routeMatchedWorkerInput":true,
            "auditComplete":true,"auditWindowStartRowid":0,"auditWindowEndRowid":1,
            "sessionOwnedByGeneration":true,"mixedChatModelCalls":0,
            "actuallyUsedInGeneration":true,"actualModelCalls":1},
            "jobId":"generation-1",
            "modelEcologyPolicy":{"revision":"chosen-policy",
                "selected":{"providerId":"api-1","model":"model-a"}},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"accepted":true,"pairedRevalidated":true,
            "acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"eligible":true,
                "measurements":[{"id":"holdout"}],
                "improvedHoldouts":2}}});
        assert!(outcome(&g).unwrap()>1.0);
        g["pairedMeasured"]=json!(false);
        assert!(outcome(&g).is_none());
    }
    #[test] fn selection_does_not_credit_unmeasured_models(){
        let route=identity("api-1","model-a");
        let history=vec![json!({"modelEcologyExecuted":{"source":
            "real_descendant_model_usage_v1","providerId":"api-1",
            "model":"model-a","actuallyUsedInGeneration":false,
            "actualModelCalls":0}})];
        let stats=statistics(&history,&route,"memory");
        assert_eq!(stats["independentlyMeasuredTrials"],0);
    }
    #[test] fn records_models_as_strata_not_parameter_updates(){
        assert!(!same_route(&identity("p","m1"),&identity("p","m2")));
        assert!(!same_route(&identity("p1","m"),&identity("p2","m")));
    }
    #[test] fn a_model_route_is_not_parameter_update_evidence(){
        let route=identity("api-provider","gpt-external");
        assert_eq!(route["providerId"],"api-provider");
        assert_eq!(route["model"],"gpt-external");
        assert!(route.get("weightsUpdated").is_none());
    }
    #[test] fn same_commit_and_frozen_profile_required_for_strong_actor_comparison(){
        let route=identity("remote","model-1");
        let objective=crate::engine::memory::stable_id(&["tools"]);
        let mut row=json!({
            "generatorCommit":"a".repeat(40),
            "comparisonProfileSha256":"p".repeat(64),
            "objectiveId":objective.clone(),
            "domain":"tools",
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "candidateId":"independent-source-1",
            "jobId":"trial-1",
            "modelEcologyPolicy":{"revision":"policy-1",
                "selected":{"providerId":"remote","model":"model-1"}},
            "modelEcologyExecuted":{"source":"real_descendant_model_usage_v1",
                "routeMatchedWorkerInput":true,
            "auditComplete":true,"auditWindowStartRowid":0,"auditWindowEndRowid":1,
            "sessionOwnedByGeneration":true,"mixedChatModelCalls":0,
                "actuallyUsedInGeneration":true,"actualModelCalls":1,
                "generationJobId":"trial-1",
                "generatorCommit":"a".repeat(40),"selectedBy":"policy-1",
                "providerId":"remote","model":"model-1"},
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"accepted":true,"pairedRevalidated":true,
            "acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"eligible":true,
                "measurements":[{"id":"real-holdout"}],"improvedHoldouts":2}}
        });
        let measure=matched_cohort(&[row.clone()],&route,&"a".repeat(40),
            &"p".repeat(64),&objective,"tools");
        assert_eq!(measure["comparedGenerations"],1);
        row["generatorCommit"]=json!("c".repeat(40));
        let invalid=matched_cohort(&[row],&route,&"a".repeat(40),
            &"p".repeat(64),&objective,"tools");
        assert_eq!(invalid["comparedGenerations"],0);
    }

    #[test] fn eligible_external_api_routes_rotate_into_real_descendant_work(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub4-actor-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config.clone(),store,Vault::open(&config.key_path()));
        state.store.put("provider",&json!({
            "id":"external-one","enabled":true,
            "baseUrl":"https://example.invalid/v1","kind":"openai",
            "model":"actor-a","capabilities":{"text":true,"tools":true}
        })).unwrap();
        state.store.put("provider",&json!({
            "id":"external-two","enabled":true,
            "baseUrl":"https://example.invalid/v1","kind":"openai",
            "model":"actor-b","capabilities":{"text":true,"tools":true}
        })).unwrap();
        let input=json!({"rsiSystemEvolution":true,"rsiModelEvolution":true,
            "selectionPolicy":"automatic",
            "providerId":"external-one","model":"actor-a"});
        let first=plan(&state,&[],&input,"tools","a");
        assert_eq!(first["active"],true);
        assert_eq!(first["selected"]["providerId"],"external-one");
        assert!(validate(&state,&first,&input,"tools","a").is_ok());
        let performed=json!({"modelEcologyExecuted":{
            "source":"real_descendant_model_usage_v1",
            "providerId":"external-one","model":"actor-a",
            "generationJobId":"adopted-agent-1",
            "generatorCommit":"a".repeat(40),
            "selectedBy":"policy-1",
            "routeMatchedWorkerInput":true,
            "auditComplete":true,"auditWindowStartRowid":0,"auditWindowEndRowid":1,
            "sessionOwnedByGeneration":true,"mixedChatModelCalls":0,
            "actuallyUsedInGeneration":true,"actualModelCalls":2},
            "jobId":"adopted-agent-1",
            "modelEcologyPolicy":{"revision":"policy-1",
                "selected":{"providerId":"external-one","model":"actor-a"}},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "generatorCommit":"a".repeat(40),
            "candidateOrigin":"model_driven_detached_worktree",
            "pairedMeasured":true,"accepted":true,"pairedRevalidated":true,
            "acceptedCommit":"b".repeat(40),"domain":"tools",
            "evaluation":{"metrics":{"eligible":true,
                "measurements":[{"id":"held-out"}],"improvedHoldouts":1}}
        });
        let next=plan(&state,&[performed],&input,"tools","b");
        assert_eq!(next["selected"]["providerId"],"external-two");
        assert!(validate(&state,&next,&input,"tools","b").is_ok());
        let forged=identity("external-unknown","nonexistent");
        let mut corrupted=next.clone();
        corrupted["selected"]=forged;
        assert!(validate(&state,&corrupted,&input,"tools","b").is_err());
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test] fn manual_model_route_is_never_displaced_by_ecology(){
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub4-fixed-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),
            web_dir:dir.join("web"),workspace:dir.clone()};
        let store=Store::open(&config.db_path()).unwrap();
        let state=AppState::new(config.clone(),store,Vault::open(&config.key_path()));
        let input=json!({"providerId":"owner-provider","model":"owner-model",
            "selectionPolicy":"manual","rsiModelEvolution":true});
        let chosen=plan(&state,&[],&input,"tools","a");
        assert_eq!(chosen["selected"]["providerId"],"owner-provider");
        assert_eq!(chosen["active"],false);
        assert!(validate(&state,&chosen,&input,"tools","a").is_ok());
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
