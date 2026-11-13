#[test]
fn resetting_a_room_keeps_members_and_archives_the_old_transcript() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"기억했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/agents", &json!({"id":"a","name":"Alice","systemPrompt":"answer"}));
    app.post("/api/agents", &json!({"id":"b","name":"Bob","systemPrompt":"answer"}));
    let room = app.post("/api/rooms", &json!({
        "id":"reset-room","name":"reset room","agentIds":["a","b"],
        "rounds":1,"maxTurns":1,"toolsEnabled":false,"autoLearning":false
    }));
    let sent = app.post("/api/rooms/reset-room/messages", &json!({"input":"안녕"}));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = app.get(&format!("/api/jobs/{}", sent["jobId"].as_str().unwrap()));
        if job["status"] == "completed" { break; }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(30));
    }
    let old_session = sent["sessionId"].as_str().unwrap().to_string();
    let before = app.get("/api/rooms/reset-room/messages");
    assert!(!before.as_array().unwrap().is_empty(), "{before}");

    let reset = app.post("/api/rooms/reset-room/reset", &json!({}));
    assert_eq!(reset["previousSessionId"], old_session, "{reset}");
    assert_ne!(reset["sessionId"], old_session, "{reset}");

    let after = app.get("/api/rooms/reset-room/messages");
    assert!(after.as_array().unwrap().is_empty(), "reset kept visible context: {after}");
    let room_after = app.get("/api/rooms/reset-room");
    assert_eq!(room_after["agentIds"], room["agentIds"], "members changed on reset");
    assert_eq!(room_after["sessionId"], reset["sessionId"]);
    let archived = app.get(&format!("/api/sessions/{old_session}/messages"));
    assert_eq!(
        archived.as_array().unwrap().len(),
        before.as_array().unwrap().len(),
        "the previous transcript must remain archived"
    );
}

#[test]
fn derived_turns_cannot_forge_or_reuse_a_stale_root_task() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"확인했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"task scope"}));
    let session_id = session["id"].as_str().unwrap();

    let forged = app.chat(&json!({
        "sessionId": session_id,
        "input": "다른 작업에 붙어보자",
        "taskId": "forged-root",
        "autoLearning": false
    }));
    assert!(forged.contains("error"), "{forged}");

    let first = app.chat(&json!({
        "sessionId": session_id,
        "input": "첫 번째 작업",
        "autoLearning": false
    }));
    assert!(first.contains("확인했습니다."), "{first}");
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == session_id)
        .expect("execution");
    let task_id = execution["taskId"].as_str().unwrap();

    let stale = app.chat(&json!({
        "sessionId": session_id,
        "input": "낡은 계약으로 실행",
        "taskId": task_id,
        "taskRevision": "stale-revision",
        "autoLearning": false
    }));
    assert!(stale.contains("revision"), "{stale}");
}

#[test]
fn reused_model_call_ids_do_not_overwrite_prior_tool_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let count = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "tool")
            .count();
        if count >= 2 {
            vec![
                json!({"choices":[{"delta":{"content":"두 번 확인했습니다."},"finish_reason":"stop"}]}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"reused","type":"function","function":{"name":"workspace_list","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        }
    });
    let app = TestApp::new(port, "native");
    let stream = app.chat(&json!({"input":"두 번 확인","autoLearning":false}));
    assert!(stream.contains("두 번 확인했습니다."), "{stream}");
    let execution = &app.get("/api/executions")[0];
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let operations = operations.as_array().unwrap();
    assert_eq!(operations.len(), 2);
    assert_ne!(operations[0]["id"], operations[1]["id"]);
    assert!(operations
        .iter()
        .all(|operation| operation["status"] == "completed"));
}

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn approval_is_operation_bound_and_survives_restart() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let finished = request["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .is_some_and(|message| message["role"] == "tool");
        if finished {
            vec![
                json!({"choices":[{"delta":{"content":"저장했습니다."},"finish_reason":"stop"}]}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"durable-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"durable.txt\",\"content\":\"DURABLE-OK\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        }
    });
    let app = TestApp::new(port, "native");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    let pending = app.wait_pending_approval(20);
    assert_eq!(pending["status"], "pending");
    assert!(pending["runId"].as_str().is_some_and(|value| !value.is_empty()));
    assert!(pending["operationId"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(
        pending["argsDigest"].as_str().map(str::len),
        Some(32),
        "approval must bind the exact tool arguments"
    );
    assert!(
        !root.join("durable.txt").exists(),
        "tool ran before approval"
    );
    let session_id = pending["sessionId"].as_str().unwrap().to_string();
    let task_id = pending["taskId"].as_str().unwrap().to_string();
    let task_revision = pending["taskRevision"].as_str().unwrap().to_string();
    assert!(!task_id.is_empty());
    assert!(
        pending["operationKey"]
            .as_str()
            .is_some_and(|value| value.starts_with("tool:")),
        "approval must bind the logical operation"
    );
    drop(app);
    let _ = handle.join();

    // Restart on the same data root: the pending approval is still there and
    // can be decided without a live waiter.
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let pending = app.wait_pending_approval(10);
    let id = pending["id"].as_str().unwrap().to_string();
    let resolved = app.post("/api/approvals", &json!({"id":id,"allow":true}));
    assert_eq!(resolved["resolved"], true, "{resolved}");
    assert_eq!(resolved["resumed"], false, "{resolved}");
    assert!(!root.join("durable.txt").exists());

    // Counterexample: the same session/tool/arguments in a *different* task
    // must not inherit this approval.
    let other = app.chat_async(&json!({
        "sessionId":session_id,
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    let other_pending = app.wait_pending_approval(10);
    assert_ne!(
        other_pending["id"].as_str().unwrap_or(""),
        id,
        "a different task reused an unrelated approval"
    );
    app.post(
        "/api/approvals",
        &json!({"id":other_pending["id"],"allow":false}),
    );
    // The scripted model retries after a denial; deny those retries too so the
    // unfinished task cannot silently reuse the original approval.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !other.is_finished() {
        if let Some(pending) = app
            .get("/api/approvals")
            .as_array()
            .and_then(|items| items.first())
        {
            app.post(
                "/api/approvals",
                &json!({"id":pending["id"],"allow":false}),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = other.join();
    assert!(!root.join("durable.txt").exists());
    assert!(
        app.get("/api/approvals").as_array().unwrap().is_empty(),
        "the unrelated task left an unresolved approval"
    );

    // A retry that explicitly resumes the original task/revision/operation
    // consumes the orphaned approval exactly once.
    let stream = app.chat(&json!({
        "sessionId":session_id,
        "taskId":task_id,
        "taskRevision":task_revision,
        "input":"durable.txt 파일을 만들어줘",
        "toolsEnabled":true,
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(root.join("durable.txt")).unwrap_or_default(),
        "DURABLE-OK",
        "{stream}"
    );
    assert!(app.get("/api/approvals").as_array().unwrap().is_empty());
}

#[test]
fn closing_the_sse_client_does_not_cancel_background_work() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(600));
        vec![
            json!({"choices":[{"delta":{"content":"백그라운드에서 완료했습니다."},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let response = app
        .client
        .post(format!("{}/api/chat", app.base))
        .json(&json!({"input":"백그라운드 작업","autoLearning":false}))
        .send()
        .unwrap();
    // Simulate the desktop/web UI closing the SSE connection immediately.
    drop(response);

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut finished = None;
    while Instant::now() < deadline {
        if let Some(execution) = app
            .get("/api/executions")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            match execution["status"].as_str() {
                Some("completed") => {
                    finished = Some(execution);
                    break;
                }
                Some("cancelled") => {
                    panic!("client disconnect cancelled background work: {execution}")
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let execution = finished.expect("background turn did not finish");
    assert_eq!(
        execution["taskOutcome"], "answered_unverified",
        "{execution}"
    );
    let session_id = execution["sessionId"].as_str().unwrap();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(
        messages.as_array().unwrap().iter().any(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("백그라운드에서 완료"))
        }),
        "assistant reply was not persisted: {messages}"
    );
}

#[test]
fn a_clean_interrupted_run_is_woken_and_resumed_after_restart() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(1_200));
        vec![
            json!({"choices":[{"delta":{"content":"재개 완료."},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"재개 테스트 작업",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.get("/api/executions/active").as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "turn did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(app);
    let _ = handle.join();

    // Restart on the same data root. The dead run has no side effects and no
    // pending approval, so startup wake must enqueue exactly one resume job.
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut wake = Value::Null;
    while Instant::now() < deadline {
        if let Some(record) = app
            .get("/api/run-wakes")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            wake = record;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(wake["decision"], "queued", "{wake}");
    let job_id = wake["jobId"].as_str().expect("wake job").to_string();

    let deadline = Instant::now() + Duration::from_secs(25);
    let mut completed = false;
    while Instant::now() < deadline {
        if let Some(job) = app
            .get("/api/jobs")
            .as_array()
            .and_then(|items| items.iter().find(|job| job["id"] == job_id.as_str()))
        {
            if matches!(job["status"].as_str(), Some("completed" | "failed")) {
                completed = job["status"] == "completed";
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(completed, "resume job did not complete");
    let executions = app.get("/api/executions");
    assert!(
        executions.as_array().unwrap().len() >= 2,
        "resume did not create a new attempt: {executions}"
    );
    let session_id = wake["sessionId"].as_str().unwrap();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    let user_messages = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .count();
    assert_eq!(
        user_messages, 1,
        "resume must not duplicate the owner message: {messages}"
    );
    assert!(!requests.lock().unwrap().is_empty());
}

#[test]
fn load_matrix_completes_parallel_sessions_without_cross_talk() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        std::thread::sleep(Duration::from_millis(80));
        let input = request["messages"]
            .as_array()
            .and_then(|messages| {
                messages
                    .iter()
                    .rev()
                    .find(|message| message["role"] == "user")
            })
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        vec![json!({"choices":[{"delta":{"content":format!("응답:{input}")},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let mut handles = Vec::new();
    for index in 0..6 {
        handles.push(app.chat_async(&json!({
            "input":format!("동시 작업 {index}"),
            "autoLearning":false
        })));
    }
    for handle in handles {
        let stream = handle.join().unwrap();
        assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    }
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 6);
    assert!(executions
        .as_array()
        .unwrap()
        .iter()
        .all(|execution| execution["status"] == "completed"));
    assert!(app.get("/api/executions/active").as_array().unwrap().is_empty());
}

#[test]
fn crash_drill_leaves_no_running_execution_after_restart() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(900));
        vec![json!({"choices":[{"delta":{"content":"크래시 드릴 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let root = app._root.clone();
    let mut handles = Vec::new();
    for index in 0..4 {
        handles.push(app.chat_async(&json!({
            "input":format!("크래시 드릴 {index}"),
            "autoLearning":false
        })));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.get("/api/executions/active").as_array().unwrap().is_empty() {
        assert!(Instant::now() < deadline, "runs did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let each turn persist its owner message before the crash so the wake
    // policy has a real prompt to resume.
    std::thread::sleep(Duration::from_millis(150));
    drop(app);
    for handle in handles {
        let _ = handle.join();
    }

    let app = TestApp::from_root(port, "native", root, &[]);
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let executions = app.get("/api/executions");
        let running = executions
            .as_array()
            .unwrap()
            .iter()
            .filter(|execution| execution["status"] == "running")
            .count();
        let jobs = app.get("/api/jobs");
        let busy = jobs
            .as_array()
            .unwrap()
            .iter()
            .filter(|job| matches!(job["status"].as_str(), Some("queued" | "running")))
            .count();
        if running == 0 && busy == 0 {
            let executions = executions.as_array().unwrap();
            assert!(!executions.is_empty(), "no execution was recorded");
            assert!(
                executions.iter().all(|execution| matches!(
                    execution["status"].as_str(),
                    Some("interrupted" | "completed" | "failed" | "cancelled")
                )),
                "dangling status: {executions:?}"
            );
            assert!(
                !app.get("/api/run-wakes").as_array().unwrap().is_empty(),
                "crash recovery did not record wake decisions"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recovery did not settle: running={running} busy={busy}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn live_restore_requires_maintenance_and_preserves_backup_executions() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"복구 드릴 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let stream = app.chat(&json!({"input":"복구 드릴 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 1);
    let session_id = executions.as_array().unwrap()[0]["sessionId"]
        .as_str()
        .unwrap()
        .to_string();

    let backup = app.post("/api/system/backup", &json!({}));
    assert_eq!(backup["ok"], true, "{backup}");
    assert_eq!(backup["encrypted"], true, "{backup}");
    let backup_path = backup["path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&backup_path).is_file());

    // A later turn must disappear after restoring the older snapshot.
    let _ = app.chat(&json!({"input":"백업 이후 작업","autoLearning":false}));
    assert_eq!(app.get("/api/executions").as_array().unwrap().len(), 2);

    // Restore is rejected outside maintenance mode.
    let early = app
        .client
        .post(format!("{}/api/system/restore", app.base))
        .json(&json!({"path":backup_path,"confirm":true}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    let enabled = app.post("/api/system/maintenance", &json!({"enabled":true}));
    assert_eq!(enabled["maintenance"], true);
    let blocked = app
        .client
        .post(format!("{}/api/sessions", app.base))
        .json(&json!({"title":"should be blocked"}))
        .send()
        .unwrap();
    assert_eq!(blocked.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert!(app.get("/api/executions").as_array().is_some());

    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let executions = app.get("/api/executions");
    assert_eq!(
        executions.as_array().unwrap().len(),
        1,
        "live restore did not roll back to the snapshot: {executions}"
    );
    assert_eq!(executions.as_array().unwrap()[0]["status"], "completed");
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(
        messages.as_array().unwrap().iter().any(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("복구 드릴 응답"))
        }),
        "restored transcript missing: {messages}"
    );

    let disabled = app.post("/api/system/maintenance", &json!({"enabled":false}));
    assert_eq!(disabled["maintenance"], false);
    let session = app.post("/api/sessions", &json!({"title":"after restore"}));
    assert!(session["id"].as_str().is_some_and(|value| !value.is_empty()));
}

#[test]
fn the_isolated_runner_claims_and_completes_a_queued_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let provider_id = app.get("/api/providers")[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent = app.post(
        "/api/agents",
        &json!({"name":"scheduler agent","providerId":provider_id,"model":"mock"}),
    );
    let agent_id = agent["id"].as_str().unwrap();
    let verification = json!({
        "type":"procedure",
        "appliesTo":{"paths":["bench/harness_report.txt"]},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":"bench/harness_audit.txt","content":"audit-harness"}},
            {"id":"report","tool":"workspace_write","arguments":{"path":"bench/harness_report.txt","content":"REPORT-OK"}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":"bench/harness_audit.txt","text":"audit-harness"}]
    });
    let strategy = app.post(
        "/api/benchmark/strategies",
        &json!({
            "agentId":agent_id,
            "assistanceMode":"harness_assisted",
            "text":"harness 절차로 audit 후 report를 실행한다",
            "rationale":"scheduler integration test",
            "verification":verification,
        }),
    );
    let strategy_id = strategy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy_id, "fixture":"audit_report"}),
    );
    assert_eq!(request["status"], "queued", "{request}");

    let bench_root = std::env::temp_dir().join(format!(
        "dominion-scheduler-{}",
        uuid::Uuid::new_v4()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_dominion-benchmark"))
        .env("DOMINION_BIN", env!("CARGO_BIN_EXE_dominion-rust"))
        .env("BENCH_CLAIM_URL", &app.base)
        .env(
            "BENCH_PROVIDER_BASE",
            format!("http://127.0.0.1:{port}/v1"),
        )
        .env("BENCH_MODEL", "mock")
        .env("BENCH_MODE", "smoke")
        .env("BENCH_PAIRS", "10")
        .env("BENCH_FIXTURE", "audit_report")
        .env("BENCH_ROOT", &bench_root)
        .output()
        .expect("run benchmark scheduler");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "runner failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("request completed: completed"),
        "runner did not complete the request\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let requests = app.get("/api/experiment-requests");
    let request = &requests.as_array().unwrap()[0];
    assert_eq!(request["status"], "completed", "{request}");
    assert!(
        request["experimentId"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{request}"
    );
    assert_eq!(request["independentUnits"], 10, "{request}");
    let improvements = app.get("/api/improvements");
    let improvement = improvements
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == strategy_id.as_str())
        .expect("strategy");
    assert_eq!(
        improvement["evidenceGrade"], "E2",
        "experiment did not promote: {improvement}"
    );
    assert_eq!(improvement["validatedBy"], "isolated_experiment");
    assert_eq!(improvement["rollout"], "canary");
    let _ = std::fs::remove_dir_all(bench_root);
}

#[test]
fn experiment_claim_is_token_scoped_and_can_be_released() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let agent = app.post(
        "/api/agents",
        &json!({"name":"claim agent","model":"mock"}),
    );
    let verification = json!({
        "type":"procedure",
        "appliesTo":{"paths":["bench/harness_report.txt"]},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":"bench/harness_audit.txt","content":"audit-harness"}},
            {"id":"report","tool":"workspace_write","arguments":{"path":"bench/harness_report.txt","content":"REPORT-OK"}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":"bench/harness_audit.txt","text":"audit-harness"}]
    });
    let strategy = app.post(
        "/api/benchmark/strategies",
        &json!({
            "agentId":agent["id"],
            "assistanceMode":"harness_assisted",
            "text":"claim lifecycle",
            "rationale":"claim lifecycle test",
            "verification":verification,
        }),
    );
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy["id"], "fixture":"audit_report"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();

    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"test-runner","ttlSeconds":60}),
    );
    assert_eq!(claimed["request"]["id"], request_id.as_str());
    assert_eq!(claimed["request"]["status"], "claimed");
    let token = claimed["request"]["claimToken"].as_str().unwrap();
    assert!(!token.is_empty());

    // Wrong token cannot complete or release.
    let wrong = app
        .client
        .post(format!(
            "{}/api/experiment-requests/{request_id}/complete",
            app.base
        ))
        .json(&json!({"claimToken":"wrong","verdict":"inconclusive"}))
        .send()
        .unwrap();
    assert_eq!(wrong.status(), reqwest::StatusCode::CONFLICT);
    let wrong_release = app
        .client
        .post(format!(
            "{}/api/experiment-requests/{request_id}/release",
            app.base
        ))
        .json(&json!({"claimToken":"wrong"}))
        .send()
        .unwrap();
    assert_eq!(wrong_release.status(), reqwest::StatusCode::CONFLICT);

    let released = app.post(
        &format!("/api/experiment-requests/{request_id}/release"),
        &json!({"claimToken":token,"reason":"runner stopped"}),
    );
    assert_eq!(released["request"]["status"], "queued");
    let reclaimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"second-runner","ttlSeconds":60}),
    );
    assert_eq!(reclaimed["request"]["status"], "claimed");
    assert_ne!(
        reclaimed["request"]["claimToken"].as_str().unwrap_or(""),
        token
    );
    // A crashed scheduler runner can release every claim it owns in one call.
    let released = app.post(
        "/api/experiment-requests/release-runner",
        &json!({"runnerId":"second-runner"}),
    );
    assert_eq!(released["released"], 1, "{released}");
    let requests = app.get("/api/experiment-requests");
    assert_eq!(requests.as_array().unwrap()[0]["status"], "queued");
}

#[test]
fn scheduled_backup_creates_a_snapshot_without_a_request() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"백업 테스트"},"finish_reason":"stop"}]})]
    });
    let offsite = std::env::temp_dir().join(format!(
        "dominion-offsite-{}",
        uuid::Uuid::new_v4()
    ));
    let app = TestApp::new_with_env(
        port,
        "native",
        &[
            ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
            ("DOMINION_BACKUP_KEEP", "2"),
            ("DOMINION_BACKUP_OFFSITE_DIR", offsite.to_str().unwrap()),
        ],
    );
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"백업 대상 작업","autoLearning":false}));
    let directory = app._root.join("runtime/backups");
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut snapshots = 0;
    while Instant::now() < deadline {
        snapshots = std::fs::read_dir(&directory)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry
                            .path()
                            .extension()
                            .and_then(|value| value.to_str())
                            == Some("enc")
                    })
                    .count()
            })
            .unwrap_or(0);
        if snapshots > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(snapshots > 0, "scheduler did not create an encrypted snapshot");
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut offsite_files = 0;
    while Instant::now() < deadline {
        offsite_files = std::fs::read_dir(&offsite)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        entry.path().extension().and_then(|value| value.to_str())
                            == Some("enc")
                    })
                    .count()
            })
            .unwrap_or(0);
        if offsite_files > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(offsite_files > 0, "offsite copy was not created");
    let _ = std::fs::remove_dir_all(offsite);
}

#[test]
fn policy_experiment_activates_a_candidate_through_the_runner() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("You are the planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일 내용을 검증한다"},"finish_reason":"stop"}]})];
        }
        let has_plan = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Internal execution plan")
        });
        if !has_plan {
            // Without a plan the model claims completion without doing work.
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let done = messages
            .last()
            .is_some_and(|message| message["role"] == "tool")
            || messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("모든 검증 조건이 충족")
            });
        if done {
            vec![json!({"choices":[{"delta":{"content":"파일을 생성했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"policy-w1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_audit.txt\",\"content\":\"audit-harness\"}"}},
                {"index":1,"id":"policy-w2","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_report.txt\",\"content\":\"REPORT-OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})]
        }
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning",
            "name":"plan-before-acting-test",
            "parameters":{"outline":true},
            "rationale":"policy evaluator test",
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    assert_eq!(policy["status"], "candidate");
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id, "fixture":"planning_evaluation"}),
    );
    assert_eq!(request["status"], "queued");
    assert_eq!(request["kind"], "policy");

    let bench_root = std::env::temp_dir().join(format!(
        "dominion-policy-eval-{}",
        uuid::Uuid::new_v4()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_dominion-benchmark"))
        .env("DOMINION_BIN", env!("CARGO_BIN_EXE_dominion-rust"))
        .env("BENCH_CLAIM_URL", &app.base)
        .env("BENCH_PROVIDER_BASE", format!("http://127.0.0.1:{port}/v1"))
        .env("BENCH_MODEL", "mock")
        .env("BENCH_MODE", "smoke")
        .env("BENCH_PAIRS", "10")
        .env("BENCH_ROOT", &bench_root)
        .output()
        .expect("run policy experiment");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "policy runner failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("request completed: completed"),
        "policy request not completed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let requests = app.get("/api/experiment-requests");
    let request = &requests.as_array().unwrap()[0];
    assert_eq!(request["status"], "completed", "{request}");
    assert_eq!(request["kind"], "policy");
    assert_eq!(request["independentUnits"], 10, "{request}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .expect("policy");
    assert_eq!(
        policy["status"], "active",
        "outer gate did not activate the candidate: {policy}\nrequest: {request}"
    );
    assert_eq!(policy["evidenceGrade"], "E2");
    assert_eq!(policy["parameters"]["outline"], true);
    assert_eq!(policy["lastExperiment"]["independentUnits"], 10);
    assert_eq!(policy["semanticVerified"], true);
    assert!(
        policy["lastExperiment"]["semanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "semantic gate metric missing: {policy}"
    );
    assert!(
        policy["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "independent semantic gate metric missing: {policy}"
    );
    let _ = std::fs::remove_dir_all(bench_root);
}

#[test]
fn the_production_scheduler_runs_a_queued_experiment_without_manual_invocation() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"스케줄러 응답"},"finish_reason":"stop"}]})]
    });
    let provider_base = format!("http://127.0.0.1:{port}/v1");
    let extra = [
        ("DOMINION_BENCHMARK", "1"),
        ("DOMINION_EXPERIMENT_SCHEDULER", "1"),
        (
            "DOMINION_EXPERIMENT_RUNNER",
            env!("CARGO_BIN_EXE_dominion-benchmark"),
        ),
        ("DOMINION_EXPERIMENT_INTERVAL_SECS", "1"),
        ("DOMINION_EXPERIMENT_PAIRS", "1"),
        ("DOMINION_EXPERIMENT_MODEL", "mock"),
        (
            "DOMINION_EXPERIMENT_PROVIDER_BASE",
            provider_base.as_str(),
        ),
    ];
    let app = TestApp::new_with_env(port, "native", &extra);
    let agent = app.post(
        "/api/agents",
        &json!({"name":"scheduler daemon agent","model":"mock"}),
    );
    let verification = json!({
        "type":"procedure",
        "appliesTo":{"paths":["bench/harness_report.txt"]},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":"bench/harness_audit.txt","content":"audit-harness"}},
            {"id":"report","tool":"workspace_write","arguments":{"path":"bench/harness_report.txt","content":"REPORT-OK"}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":"bench/harness_audit.txt","text":"audit-harness"}]
    });
    let strategy = app.post(
        "/api/benchmark/strategies",
        &json!({
            "agentId":agent["id"],
            "assistanceMode":"harness_assisted",
            "text":"scheduler daemon procedure",
            "rationale":"production scheduler test",
            "verification":verification,
        }),
    );
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"strategyId":strategy["id"], "fixture":"audit_report"}),
    );
    assert_eq!(request["status"], "queued");

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut completed = Value::Null;
    while Instant::now() < deadline {
        if let Some(record) = app
            .get("/api/experiment-requests")
            .as_array()
            .and_then(|items| items.first())
            .cloned()
        {
            if record["status"] == "completed" || record["status"] == "failed" {
                completed = record;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(
        completed["status"], "completed",
        "scheduler did not complete the request: {completed}"
    );
    let run = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(run) = app
                .get("/api/scheduler-runs")
                .as_array()
                .and_then(|items| items.first())
                .cloned()
            {
                break run;
            }
            assert!(
                Instant::now() < deadline,
                "scheduler run record was not written after the request completed"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    assert_eq!(run["status"], "success", "{run}");
    assert!(
        run["stdoutTail"]
            .as_str()
            .is_some_and(|text| text.contains("request completed")),
        "runner output missing from scheduler record: {run}"
    );
}

#[test]
fn policy_activation_is_rejected_when_the_independent_evaluator_disagrees() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let make_request = |name: &str| -> (String, String, String) {
        let policy = app.post(
            "/api/benchmark/policies",
            &json!({
                "slot":"planning","name":name,
                "parameters":{"outline":true},"rationale":"meta gate test"
            }),
        );
        let policy_id = policy["id"].as_str().unwrap().to_string();
        let request = app.post(
            "/api/benchmark/experiment-requests",
            &json!({"policyId":policy_id, "fixture":"planning_evaluation"}),
        );
        let request_id = request["id"].as_str().unwrap().to_string();
        let claimed = app.post(
            "/api/experiment-requests/claim",
            &json!({"runnerId":format!("meta-{name}"),"ttlSeconds":60}),
        );
        (policy_id, request_id, claimed["request"]["claimToken"].as_str().unwrap().to_string())
    };
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";
    let vague_plan = "1. 작업을 진행한다\n2. 완료한다";
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);

    // 1) Version mismatch rejects promotion even with a perfect claimed delta.
    let (policy_id, request_id, token) = make_request("version-mismatch");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-1",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":999},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":""},
                {"pair":0,"arm":"treatment","plan":good_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "candidate", "{policy}");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("version")),
        "{completed}"
    );

    // 2) Correct versions, but the independent score does not improve.
    let (policy_id, request_id, token) = make_request("independent-disagrees");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-2",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":vague_plan},
                {"pair":0,"arm":"treatment","plan":vague_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "candidate", "{policy}");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("semantic gate rejected")),
        "{completed}"
    );

    // 3) Both evaluators agree, so the candidate activates.
    let (policy_id, request_id, token) = make_request("both-agree");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"meta-3",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
            "semanticDelta":0.9,"independentSemanticDelta":0.9,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[
                {"pair":0,"arm":"baseline","plan":""},
                {"pair":0,"arm":"treatment","plan":good_plan}
            ]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "active", "{policy}");
    assert!(
        policy["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05,
        "{policy}"
    );
}

#[test]
fn policy_search_selects_the_best_revision_through_the_scheduler() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("You are the planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다"},"finish_reason":"stop"}]})];
        }
        let has_plan = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("Internal execution plan")
        });
        if !has_plan {
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let done = messages
            .last()
            .is_some_and(|message| message["role"] == "tool")
            || messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("모든 검증 조건이 충족")
            });
        if done {
            vec![json!({"choices":[{"delta":{"content":"파일을 생성했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"search-w1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_audit.txt\",\"content\":\"audit-harness\"}"}},
                {"index":1,"id":"search-w2","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"bench/harness_report.txt\",\"content\":\"REPORT-OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})]
        }
    });
    let provider_base = format!("http://127.0.0.1:{port}/v1");
    let extra = [
        ("DOMINION_BENCHMARK", "1"),
        ("DOMINION_EXPERIMENT_SCHEDULER", "1"),
        ("DOMINION_EXPERIMENT_RUNNER", env!("CARGO_BIN_EXE_dominion-benchmark")),
        ("DOMINION_EXPERIMENT_INTERVAL_SECS", "1"),
        ("DOMINION_EXPERIMENT_PAIRS", "10"),
        ("DOMINION_EXPERIMENT_MODEL", "mock"),
        ("DOMINION_EXPERIMENT_PROVIDER_BASE", provider_base.as_str()),
    ];
    let app = TestApp::new_with_env(port, "native", &extra);
    let search = app.post(
        "/api/benchmark/policy-search",
        &json!({
            "slot":"planning",
            "candidates":[
                {"name":"outline-all","parameters":{"outline":true}},
                {"name":"outline-only-long","parameters":{"outline":true,"minChars":1000}}
            ]
        }),
    );
    let search_id = search["searchId"].as_str().unwrap().to_string();
    let candidates = search["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["candidate"]["name"].as_str().unwrap().to_string(),
                item["candidate"]["id"].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let outline_all = candidates
        .iter()
        .find(|(name, _)| name == "outline-all")
        .unwrap()
        .1
        .clone();

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut selection = Value::Null;
    loop {
        let requests = app.get("/api/experiment-requests");
        let completed = requests
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| request["status"] == "completed")
            .count();
        if let Some(record) = app
            .get("/api/policy-searches")
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == search_id.as_str()))
            .cloned()
        {
            selection = record;
            if completed >= 2 && !selection["selectedPolicyId"].is_null() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "search did not settle: completed={completed} selection={selection}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(
        selection["selectedPolicyId"], outline_all.as_str(),
        "wrong revision selected: {selection}"
    );
    let policies = app.get("/api/policies");
    let policies = policies.as_array().unwrap();
    let selected = policies
        .iter()
        .find(|policy| policy["id"] == outline_all.as_str())
        .unwrap();
    assert_eq!(selected["status"], "active", "{selected}");
    assert!(
        selected["lastExperiment"]["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0)
            >= 0.05
    );
    let long_only = policies
        .iter()
        .find(|policy| policy["name"] == "outline-only-long")
        .unwrap();
    assert_ne!(
        long_only["status"], "active",
        "candidate without a plan must not be selected: {long_only}"
    );
    assert_eq!(selection["candidates"].as_array().unwrap().len(), 2);
}

#[test]
fn policy_patches_activate_rollback_and_reject_stale_bases() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let activate = |name: &str, parameters: Value| -> Value {
        app.post(
            "/api/benchmark/policies/state",
            &json!({"slot":"planning","name":name,"parameters":parameters}),
        )["active"]
            .clone()
    };
    let base = activate("patch-base", json!({"outline":true}));
    let base_id = base["id"].as_str().unwrap().to_string();
    let patch = app.post(
        "/api/benchmark/policy-patches",
        &json!({
            "slot":"planning",
            "targetPolicyId":base_id,
            "body":{"parameters":{"minChars":120}},
            "rationale":"threshold patch"
        }),
    );
    let patch_id = patch["id"].as_str().unwrap().to_string();
    assert_eq!(patch["status"], "candidate");

    let weak = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.4,"independentUnits":4
        }),
    );
    assert_eq!(weak["status"], "candidate", "{weak}");

    let good = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.3,"independentUnits":8
        }),
    );
    assert_eq!(good["status"], "active", "{good}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == base_id.as_str())
        .unwrap();
    assert_eq!(policy["parameters"]["minChars"], 120, "{policy}");
    assert_eq!(policy["appliedPatchId"], patch_id.as_str());

    let rollback = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch_id,"verdict":"regressed","delta":-0.3,
            "semanticDelta":0.0,"independentSemanticDelta":0.0,"independentUnits":8
        }),
    );
    assert_eq!(rollback["status"], "rolled_back", "{rollback}");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == base_id.as_str())
        .unwrap();
    assert!(policy["parameters"]["minChars"].is_null(), "{policy}");
    assert_eq!(policy["parameters"]["outline"], true);

    // A patch is invalidated when the target policy moves to a new revision.
    let base2 = activate("patch-base-2", json!({"outline":true}));
    let patch2 = app.post(
        "/api/benchmark/policy-patches",
        &json!({
            "slot":"planning","targetPolicyId":base2["id"],
            "body":{"parameters":{"minChars":60}},"rationale":"stale base"
        }),
    );
    let patch2_id = patch2["id"].as_str().unwrap().to_string();
    let _ = activate("patch-base-3", json!({"outline":true,"minChars":10}));
    let rejected = app.post(
        "/api/benchmark/policy-patches/experiment",
        &json!({
            "patchId":patch2_id,"verdict":"preliminary_support","delta":0.4,
            "semanticDelta":0.4,"independentSemanticDelta":0.3,"independentUnits":8
        }),
    );
    assert_eq!(rejected["status"], "rejected", "{rejected}");
    assert!(rejected["reason"]
        .as_str()
        .is_some_and(|text| text.contains("base revision")));
}

#[test]
fn backup_key_rotation_reencrypts_and_restores() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"키 회전 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let stream = app.chat(&json!({"input":"키 회전 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let backup = app.post("/api/system/backup", &json!({}));
    let backup_path = backup["path"].as_str().unwrap().to_string();

    // Rotation is maintenance-only.
    let early = app
        .client
        .post(format!("{}/api/system/backup-keys/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    app.post("/api/system/maintenance", &json!({"enabled":true}));
    let before = app.get("/api/system/backup-keys");
    assert_eq!(before["activeVersion"], 1, "{before}");
    let rotated = app.post(
        "/api/system/backup-keys/rotate",
        &json!({"keepOld":false}),
    );
    assert_eq!(rotated["ok"], true, "{rotated}");
    assert_eq!(rotated["activeVersion"], 2, "{rotated}");
    assert_eq!(rotated["previousVersion"], 1, "{rotated}");
    assert_eq!(rotated["reencrypted"], 1, "{rotated}");
    assert_eq!(rotated["retainedVersions"], json!([2]), "{rotated}");
    assert!(!app._root.join("runtime/backup-keys/key-1.key").exists());

    // The re-encrypted snapshot restores under the new key.
    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let executions = app.get("/api/executions");
    assert_eq!(executions.as_array().unwrap().len(), 1);
    assert_eq!(executions.as_array().unwrap()[0]["status"], "completed");

    let kept = app.post(
        "/api/system/backup-keys/rotate",
        &json!({"keepOld":true}),
    );
    assert_eq!(kept["activeVersion"], 3, "{kept}");
    assert_eq!(kept["retainedVersions"], json!([2,3]), "{kept}");
    let status = app.get("/api/system/backup-keys");
    assert_eq!(status["activeVersion"], 3, "{status}");
    assert_eq!(status["versions"], json!([2,3]), "{status}");
    app.post("/api/system/maintenance", &json!({"enabled":false}));
}

#[test]
fn the_recovery_slot_changes_retry_behavior_only_after_a_valid_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"recovery",
            "name":"fewer-acceptance-retries",
            "parameters":{"maxAcceptanceRetries":0,"maxProcedureRepairs":0},
            "rationale":"recovery slot test"
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    assert_eq!(policy["status"], "candidate");

    // Underpowered experiment cannot activate the slot.
    let underpowered = app.post(
        "/api/benchmark/policies/experiment",
        &json!({
            "policyId":policy_id,"experimentId":"recovery-small",
            "verdict":"preliminary_support","delta":0.2,
            "semanticDelta":0.2,"independentSemanticDelta":0.2,
            "independentUnits":4,"oracleVersion":"v1"
        }),
    );
    assert_eq!(underpowered["status"], "candidate", "{underpowered}");
    let stream = app.chat(&json!({
        "input":"bench/harness_report.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    assert!(
        stream.contains("acceptance_retry"),
        "candidate policy must not change behavior: {stream}"
    );

    let activated = app.post(
        "/api/benchmark/policies/experiment",
        &json!({
            "policyId":policy_id,"experimentId":"recovery-powered",
            "verdict":"preliminary_support","delta":0.2,
            "semanticDelta":0.2,"independentSemanticDelta":0.2,
            "independentUnits":8,"oracleVersion":"v1"
        }),
    );
    assert_eq!(activated["status"], "active", "{activated}");
    let stream = app.chat(&json!({
        "input":"bench/harness_report.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    assert!(
        !stream.contains("acceptance_retry"),
        "active recovery policy did not change the retry branch: {stream}"
    );
    let executions = app.get("/api/executions");
    assert!(
        executions
            .as_array()
            .unwrap()
            .iter()
            .any(|execution| execution["status"] == "failed"),
        "{executions}"
    );
    let policies = app.get("/api/policies");
    let active = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(active["status"], "active");
    assert_eq!(active["slot"], "recovery");
}

