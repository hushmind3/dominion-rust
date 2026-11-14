//! Tests isolated from the production jobs.rs source.
#[test]
    fn an_explicit_owner_patch_gets_a_grounded_edit_stage() {
        let explicit=json!({"rsiAutonomous":false,
            "rsiFocusFiles":["src/engine/decompose.rs"],
            "rsiImprovementDirective":{"mode":"implement_research",
                "reason":"explicit_owner_patch_request_with_source"}});
        assert!(rsi_focused_edit_goal(&explicit));
        let mut inherited=explicit.clone();
        inherited["rsiImprovementDirective"]["mode"]=json!("change_approach");
        assert!(rsi_focused_edit_goal(&inherited));
        let mut broad=explicit.clone();
        broad["rsiAutonomous"]=json!(true);
        broad["rsiImprovementDirective"]["reason"]=json!("no_recorded_generation");
        assert!(rsi_focused_edit_goal(&broad),
            "Autonomous descendants executing a source patch must reach their edit-only continuation");
        broad["rsiImprovementDirective"]["reason"]=json!("explicit_owner_patch_request_with_source");
        assert!(rsi_focused_edit_goal(&broad));
        let mut research=explicit;
        research["rsiImprovementDirective"]["mode"]=json!("discover");
        assert!(!rsi_focused_edit_goal(&research));
    }

    #[test]
    fn patch_generation_retries_once_only_after_real_no_edit_outcome() {
        let base=json!({"rsiImprovementDirective":{"mode":"implement_research"}});
        let missing=json!({"rsi":{"diffBytes":0,"candidateId":null,
            "sourceClean":true,
            "captureError":"모델이 실제 소스 파일을 변경하지 않았습니다.",
            "modelError":""}});
        assert!(rsi_no_edit_retry_needed(&base,&Ok(missing.clone())));
        let discovery=json!({"rsiImprovementDirective":{"mode":"discover"}});
        assert!(!rsi_no_edit_retry_needed(&discovery,&Ok(missing.clone())));
        let mut errored=missing.clone();
        errored["rsi"]["modelError"]=json!("provider_unavailable");
        assert!(!rsi_no_edit_retry_needed(&base,&Ok(errored)));
        let mut edited=missing.clone();
        edited["rsi"]["diffBytes"]=json!(200);
        assert!(!rsi_no_edit_retry_needed(&base,&Ok(edited)));
        let inherited=json!({"rsi":{"diffBytes":500,"candidateId":null,
            "sourceClean":true,
            "captureError":"복원한 개선 후보를 이번 세대에서 실제로 수정하지 않았습니다.",
            "modelError":""}});
        let mut repair=base.clone();
        repair["rsiRepairCandidateId"]=json!("failed-candidate");
        assert!(rsi_no_edit_retry_needed(&repair,&Ok(inherited)),
            "Restored but untouched source must get its one corrective edit opportunity");
        let mut unclean=missing.clone();
        unclean["rsi"]["sourceClean"]=json!(false);
        assert!(!rsi_no_edit_retry_needed(&base,&Ok(unclean)));
        let disabled=json!({"rsiImprovementDirective":{"mode":"implement_research"},
            "toolsExecutionEnabled":false});
        assert!(!rsi_no_edit_retry_needed(&disabled,&Ok(missing)));
        let fail:Result<Value,String>=Err("model failed".into());
        assert!(!rsi_no_edit_retry_needed(&base,&fail));
    }

    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-jobs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn enqueue_creates_a_queued_job() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w1", "input": "hi" }),
        )
        .expect("job");
        assert_eq!(job["status"], "queued");
        assert_eq!(job["kind"], "workflow");
        assert_eq!(job["stepTotal"], 0);

        let queued = next_queued(&state).expect("worker sees it");
        assert_eq!(queued["id"], job["id"]);

        let mut running = queued.clone();
        running["status"] = json!("running");
        state.store.put("job", &running);
        assert!(next_queued(&state).is_none());
    }

    #[test]
    fn learning_jobs_are_priority_last_and_serialized() {
        let state = temp_state();
        let learn1 = enqueue(&state, "learn", json!({"sessionId":"s"})).unwrap();
        let learn2 = enqueue(&state, "learn", json!({"sessionId":"t"})).unwrap();
        let prompt = enqueue(&state, "prompt", json!({"prompt":"interactive"})).unwrap();

        let first = next_queued(&state).expect("prompt is picked first");
        assert_eq!(first["id"], prompt["id"]);
        state.store.patch_if(
            "job",
            prompt["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        let second = next_queued(&state).expect("one learning job runs");
        assert!(second["id"] == learn1["id"] || second["id"] == learn2["id"]);
        state.store.patch_if(
            "job",
            second["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"running"}),
        );

        assert!(
            next_queued(&state).is_none(),
            "a second learning job must wait while one is running"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }

    #[test]
    fn recover_requeues_running_jobs() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w", "input": "" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "queued");
        assert_eq!(stored["recovered"], true);

        running["attempts"] = json!(3);
        running["status"] = json!("running");
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "needs_review");
    }

    #[test]
    fn recover_retires_orphan_generations_and_stuck_campaigns() {
        let state = temp_state();
        let parent = enqueue(&state, "rsi_evolve", json!({})).unwrap();
        let mut done = parent.clone();
        done["status"] = json!("completed");
        state.store.put("job", &done);
        let child = enqueue(
            &state,
            "rsi_generation",
            json!({"rsiCampaignId": parent["id"]}),
        )
        .unwrap();
        let mut running = child.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        state.store.put("job", &running);
        state.store.put(
            "rsi_campaign",
            &json!({"id": parent["id"], "status": "running"}),
        );
        recover(&state);
        let stored = state.store.get("job", child["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "needs_review", "orphan generation must not block starts");
        let campaign = state
            .store
            .get("rsi_campaign", parent["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(campaign["status"], "completed");
    }

    #[test]
    fn recover_does_not_replay_an_execution_with_unknown_external_effects() {
        let state = temp_state();
        let job = enqueue(
            &state,
            "workflow",
            json!({ "workflowId": "w", "input": "", "sessionId": "session-1" }),
        )
        .unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        // No inFlight flag: the evidence must come from the operation record.
        state.store.put("job", &running);
        state.store.put(
            "execution",
            &json!({"id":"run-1","sessionId":"session-1","status":"interrupted"}),
        );
        state.store.put(
            "execution_operation",
            &json!({
                "id":"run-1:0:0:call","runId":"run-1","sessionId":"session-1",
                "tool":"shell_exec","status":"interrupted","outcomeUnknown":true
            }),
        );
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            stored["status"], "needs_review",
            "an unknown external result must not be replayed: {stored}"
        );
        assert!(stored["error"].as_str().is_some_and(|text| text.contains("확정")));
    }

    #[test]
    fn job_errors_are_recorded_as_failed() {
        let state = temp_state();
        let job = enqueue(&state, "room", json!({ "roomId": "missing", "goal": "x" })).unwrap();
        let id = job["id"].as_str().unwrap().to_string();
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_job(state.clone(), job));
        let stored = state.store.get("job", &id).unwrap();
        assert_eq!(stored["status"], "failed");
        assert!(stored["error"].as_str().unwrap().contains("대화방"));
    }

    #[test]
    fn uncertain_goal_recovery_updates_job_and_goal_without_replaying() {
        let state = temp_state();
        state
            .store
            .put("goal", &json!({"id":"g","status":"running","goal":"write"}));
        state.store.put(
            "job",
            &json!({"id":"j","kind":"goal","status":"running","attempts":1,
            "input":{"goalId":"g"},"goalState":{"inFlight":true}}),
        );
        recover(&state);
        assert_eq!(
            state.store.get("job", "j").unwrap()["status"],
            "needs_review"
        );
        assert_eq!(
            state.store.get("goal", "g").unwrap()["status"],
            "needs_review"
        );
        assert!(next_queued(&state).is_none());
    }

    #[tokio::test]
    async fn goal_review_state_is_preserved_on_the_parent_job() {
        let state = temp_state();
        state
            .store
            .put("goal", &json!({"id":"g","status":"running","goal":"실행"}));
        let job = state
            .store
            .put(
                "job",
                &json!({"id":"j","kind":"goal","status":"running","attempts":1,
            "input":{"goalId":"g"},"goalState":{"inFlight":true}}),
            )
            .unwrap();
        run_job(state.clone(), job).await;
        assert_eq!(
            state.store.get("job", "j").unwrap()["status"],
            "needs_review"
        );
        assert_eq!(
            state.store.get("goal", "g").unwrap()["status"],
            "needs_review"
        );
    }

    #[test]
    fn failed_learning_jobs_are_requeued_with_backoff() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-retry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({"id":"learn-failed","kind":"learn","status":"failed","attempts":1,"input":{},"error":"model unavailable"}),
        );
        assert_eq!(retry_failed_learning(&state), 1);
        let job = state.store.get("job", "learn-failed").unwrap();
        assert_eq!(job["status"], "queued");
        assert!(job["availableAt"].is_string());
        assert_eq!(job["lastError"], "model unavailable");
        let _ = std::fs::remove_dir_all(dir);
    }

#[test]
fn recover_resumes_only_a_complete_read_only_goal_review() {
    let state = temp_state();
    state.store.put("goal",&json!({"id":"g","status":"running","goal":"write report"}));
    let snapshot = crate::engine::goal_review::snapshot("s",0,
        &[json!({"tool":"workspace_write","result":{"success":true}})],"done",&[],None,&[]);
    state.store.put("job",&json!({"id":"j","kind":"goal","status":"running",
        "attempts":1,"input":{"goalId":"g"},"goalState":snapshot}));
    recover(&state);
    let job=state.store.get("job","j").unwrap();
    assert_eq!(job["status"],"queued");
    assert_eq!(job["goalState"]["reviewPending"],true);
    assert_eq!(state.store.get("goal","g").unwrap()["status"],"running");
}

#[test]
fn incomplete_or_legacy_review_checkpoint_requires_human_reconciliation() {
    let state = temp_state();
    state.store.put("goal",&json!({"id":"g","status":"running","goal":"write report"}));
    state.store.put("job",&json!({"id":"j","kind":"goal","status":"running","attempts":1,
        "input":{"goalId":"g"},"goalState":{"inFlight":false,"reviewPending":true,
            "nextIteration":1,"evidence":[]}}));
    recover(&state);
    assert_eq!(state.store.get("job","j").unwrap()["status"],"needs_review");
    assert_eq!(state.store.get("goal","g").unwrap()["status"],"needs_review");
}

#[test]
fn completed_goal_is_reconciled_without_restarting_a_side_effecting_turn() {
    let state=temp_state();
    state.store.put("goal",&json!({"id":"g","status":"completed","goal":"write",
        "sessionId":"s","iterations":2,"evidence":[{"tool":"workspace_write"}]}));
    state.store.put("job",&json!({"id":"j","kind":"goal","status":"running","attempts":3,
        "input":{"goalId":"g"},"goalState":{"inFlight":true}}));
    recover(&state);
    let recovered=state.store.get("job","j").unwrap();
    assert_eq!(recovered["status"],"completed");
    assert_eq!(recovered["result"]["status"],"completed");
    assert_eq!(recovered["result"]["iterations"],2);
    assert!(next_queued(&state).is_none());
}

#[test]
fn goal_review_recovery_never_replays_unknown_external_effect_from_goal_session() {
    let state = temp_state();
    state.store.put("goal",&json!({"id":"g","status":"running","goal":"write report"}));
    let snapshot=crate::engine::goal_review::snapshot("session-goal",0,&[],"done",&[],None,&[]);
    state.store.put("job",&json!({"id":"j","kind":"goal","status":"running",
        "attempts":1,"input":{"goalId":"g"},"goalState":snapshot}));
    state.store.put("execution",&json!({"id":"e","sessionId":"session-goal","status":"interrupted"}));
    state.store.put("execution_operation",&json!({"id":"e:o","runId":"e","status":"interrupted",
        "outcomeUnknown":true,"tool":"shell_exec"}));
    recover(&state);
    assert_eq!(state.store.get("job","j").unwrap()["status"],"needs_review");
    assert_eq!(state.store.get("goal","g").unwrap()["status"],"needs_review");
}

#[test]
fn recurring_multi_agent_workflow_runs_once_per_slot_and_freezes_model(){
    let state=temp_state();
    state.store.put("provider",&json!({"id":"p","enabled":true,"model":"local-v1"}));
    state.store.put("agent",&json!({"id":"a","name":"writer",
        "providerId":"p","model":"local-v1"}));
    state.store.put("workflow",&json!({"id":"w","name":"draft/review",
        "steps":[{"id":"step-one","name":"write","agentId":"a","prompt":"{{input}}"}]}));
    let due=(chrono::Utc::now()-chrono::Duration::seconds(8)).to_rfc3339();
    state.store.put("automation",&json!({"id":"auto1","enabled":true,
        "kind":"workflow","input":{"workflowId":"w","input":"report"},
        "intervalMinutes":10,"nextAt":due}));
    schedule_automations(&state);
    let jobs=state.store.all("job");
    assert_eq!(jobs.len(),1);
    assert_eq!(jobs[0]["kind"],"workflow");
    assert_eq!(jobs[0]["input"]["definition"]["steps"][0]["model"],"local-v1");
    assert_eq!(jobs[0]["input"]["automationId"],"auto1");
    assert_eq!(jobs[0]["input"]["scheduledAt"],due);
    let first=jobs[0]["id"].as_str().unwrap().to_string();
    schedule_automations(&state);
    assert_eq!(state.store.count("job"),1,"normal scheduler ticks never duplicate work");
    // Emulate process crash immediately after paid job insert, but before
    // writing nextAt. A recurring slot must reuse its existing job.
    let current=state.store.get("automation","auto1").unwrap();
    state.store.patch_if("automation","auto1","nextAt",&current["nextAt"],
        &json!({"nextAt":due}));
    schedule_automations(&state);
    assert_eq!(state.store.count("job"),1);
    assert_eq!(state.store.get("automation","auto1").unwrap()["lastJobId"],first);
    // A newly due slot with disabled provider cannot silently queue API work.
    state.store.patch_if("provider","p","id",&json!("p"),
        &json!({"enabled":false}));
    let current=state.store.get("automation","auto1").unwrap();
    state.store.patch_if("automation","auto1","nextAt",&current["nextAt"],
        &json!({"nextAt":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()}));
    schedule_automations(&state);
    assert_eq!(state.store.count("job"),1);
    assert_eq!(state.store.get("automation","auto1").unwrap()["enabled"],false);
    assert_eq!(state.store.get("automation","auto1").unwrap()["status"],"needs_review");
    let _=std::fs::remove_dir_all(state.config.data_dir.clone());
}
