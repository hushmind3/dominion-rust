#[test]
fn reflection_provider_is_explicit_and_exclusive() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let first = app.post(
        "/api/providers",
        &json!({"name":"teacher-a","baseUrl":"http://127.0.0.1:9/v1","model":"model-a","reflection":true,"enabled":true}),
    );
    assert_eq!(first["reflection"], true, "{first}");
    assert_eq!(first["model"], "model-a", "{first}");
    let first_id = first["id"].as_str().unwrap().to_string();
    let second = app.post(
        "/api/providers",
        &json!({"name":"teacher-b","baseUrl":"http://127.0.0.1:9/v1","model":"model-b","reflection":true,"enabled":true}),
    );
    assert_eq!(second["reflection"], true, "{second}");
    let second_id = second["id"].as_str().unwrap().to_string();
    let providers = app.get("/api/providers");
    let items = providers.as_array().unwrap();
    let a = items.iter().find(|p| p["id"] == first_id.as_str()).unwrap();
    let b = items.iter().find(|p| p["id"] == second_id.as_str()).unwrap();
    assert_eq!(a["reflection"], false, "old reflection provider was not cleared: {providers}");
    assert_eq!(b["reflection"], true, "{providers}");
    assert_eq!(b["model"], "model-b", "{providers}");

    let response = app
        .client
        .post(format!("{}/api/providers", app.base))
        .json(&json!({"name":"broken","baseUrl":"http://127.0.0.1:9/v1","reflection":true}))
        .send()
        .unwrap();
    assert_eq!(
        response.status().as_u16(),
        400,
        "a reflection provider without a model must be rejected"
    );
}

#[test]
fn secret_key_rotation_preserves_provider_secrets_and_restores() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"secret 회전 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.post(
        "/api/providers",
        &json!({
            "name":"key rotation provider",
            "baseUrl":format!("http://127.0.0.1:{port}/v1"),
            "model":"mock",
            "apiKey":"sk-rotation-test",
            "enabled":true
        }),
    );
    let provider_id = provider["id"].as_str().unwrap().to_string();
    assert_eq!(provider["hasKey"], true, "{provider}");
    let stream = app.chat(&json!({"input":"키 회전 전 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    let backup = app.post("/api/system/backup", &json!({}));
    let backup_path = backup["path"].as_str().unwrap().to_string();

    let early = app
        .client
        .post(format!("{}/api/system/secret-key/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    assert_eq!(early.status(), reqwest::StatusCode::CONFLICT);

    let key_path = app._root.join("runtime/secret.key");
    let before = std::fs::read(&key_path).unwrap();
    app.post("/api/system/maintenance", &json!({"enabled":true}));
    let response = app
        .client
        .post(format!("{}/api/system/secret-key/rotate", app.base))
        .json(&json!({"keepOld":false}))
        .send()
        .unwrap();
    let status = response.status();
    let text = response.text().unwrap();
    assert!(status.is_success(), "secret rotation failed {status}: {text}");
    let rotated: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rotated["ok"], true, "{rotated}");
    assert!(
        rotated["secretsReencrypted"].as_u64().unwrap_or(0) >= 1,
        "{rotated}"
    );
    assert_eq!(rotated["previousKeys"], 0, "{rotated}");
    assert_eq!(rotated["oldKeyRetained"], false, "{rotated}");
    let after = std::fs::read(&key_path).unwrap();
    assert_ne!(before, after, "secret.key was not replaced");
    let status = app.get("/api/system/secret-key");
    assert_eq!(status["previousKeys"], 0, "{status}");
    assert!(status["oldKeyFiles"].as_array().unwrap().is_empty(), "{status}");

    // Provider secrets and encrypted backups still work after rotation.
    let providers = app.get("/api/providers");
    let provider = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == provider_id.as_str())
        .unwrap();
    assert_eq!(provider["hasKey"], true, "{provider}");
    let restored = app.post(
        "/api/system/restore",
        &json!({"path":backup_path,"confirm":true}),
    );
    assert_eq!(restored["ok"], true, "{restored}");
    assert_eq!(restored["executions"], 1, "{restored}");
    let stream = app.chat(&json!({"input":"키 회전 후 작업","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");

    // Explicit old-key retention keeps one fallback file.
    let kept = app.post(
        "/api/system/secret-key/rotate",
        &json!({"keepOld":true}),
    );
    assert_eq!(kept["previousKeys"], 1, "{kept}");
    assert_eq!(kept["oldKeyRetained"], true, "{kept}");
    let status = app.get("/api/system/secret-key");
    assert_eq!(status["previousKeys"], 1, "{status}");
    assert_eq!(status["oldKeyFiles"].as_array().unwrap().len(), 1, "{status}");
    app.post("/api/system/maintenance", &json!({"enabled":false}));
}

/// Minimal HTTP sink for remote backup PUTs. Success mode echoes the
/// x-dominion-sha256 header; failure mode returns 500.
fn remote_backup_sink(fail: bool) -> (u16, Arc<Mutex<Vec<(String, String)>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let observed = received.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let observed = observed.clone();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(at) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
                let path = headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                let hash = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("x-dominion-sha256:")
                            .map(|value| value.trim().to_string())
                    })
                    .unwrap_or_default();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                if fail {
                    let _ = stream.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                observed.lock().unwrap().push((path, hash.clone()));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nx-dominion-sha256: {hash}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    (port, received)
}

#[test]
fn scheduled_backup_uploads_to_a_remote_target() {
    let _serial = serial();
    let (provider_port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"원격 백업"},"finish_reason":"stop"}]})]
    });
    let (remote_port, received) = remote_backup_sink(false);
    let remote_url = format!("http://127.0.0.1:{remote_port}/backups");
    let extra = [
        ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
        ("DOMINION_BACKUP_KEEP", "2"),
        ("DOMINION_BACKUP_REMOTE_URL", remote_url.as_str()),
        ("DOMINION_BACKUP_REMOTE_RETRIES", "1"),
    ];
    let app = TestApp::new_with_env(provider_port, "native", &extra);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"원격 백업 작업","autoLearning":false}));
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut uploaded = false;
    while Instant::now() < deadline {
        if received.lock().unwrap().iter().any(|(path, hash)| {
            path.ends_with(".sqlite.enc") && !hash.is_empty()
        }) {
            uploaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(uploaded, "backup was not uploaded to the remote sink");
    let audit = app.get("/api/audit");
    assert!(
        audit.as_array().unwrap().iter().any(|event| {
            event["event"] == "backup.remote_uploaded"
        }),
        "remote upload audit missing: {audit}"
    );
}

#[test]
fn a_failed_remote_upload_keeps_the_local_snapshot() {
    let _serial = serial();
    let (provider_port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"원격 실패"},"finish_reason":"stop"}]})]
    });
    let (remote_port, _) = remote_backup_sink(true);
    let remote_url = format!("http://127.0.0.1:{remote_port}/backups");
    let extra = [
        ("DOMINION_BACKUP_INTERVAL_SECS", "1"),
        ("DOMINION_BACKUP_KEEP", "2"),
        ("DOMINION_BACKUP_REMOTE_URL", remote_url.as_str()),
        ("DOMINION_BACKUP_REMOTE_RETRIES", "0"),
        ("DOMINION_BACKUP_REMOTE_TIMEOUT_SECS", "2"),
    ];
    let app = TestApp::new_with_env(provider_port, "native", &extra);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"원격 실패 작업","autoLearning":false}));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut failed = false;
    while Instant::now() < deadline {
        let audit = app.get("/api/audit");
        if audit
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "backup.remote_failed")
        {
            failed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(failed, "remote failure audit missing");
    let backups = app._root.join("runtime/backups");
    let snapshots = std::fs::read_dir(&backups)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry.path().extension().and_then(|value| value.to_str()) == Some("enc")
                })
                .count()
        })
        .unwrap_or(0);
    assert!(snapshots > 0, "local snapshot was removed after remote failure");
}

#[test]
fn only_one_runner_can_settle_and_promote_an_experiment() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning","name":"cas-settle",
            "parameters":{"outline":true},"rationale":"settle CAS"
        }),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();
    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"cas-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);
    let body = json!({
        "claimToken": token,
        "experimentId":"cas-exp",
        "verdict":evidence["verdict"],
        "delta":evidence["delta"],"independentUnits":evidence["independentUnits"],
        "manifest":evidence["manifest"],"manifestHash":evidence["manifestHash"],
        "trials":evidence["trials"],
        "semanticDelta":0.5,"independentSemanticDelta":0.5,
        "semanticEvaluator":{"id":"plan-quality-main","version":1},
        "independentEvaluator":{"id":"plan-quality-independent","version":1},
        "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "plans":[
            {"pair":0,"arm":"baseline","plan":""},
            {"pair":0,"arm":"treatment","plan":good_plan}
        ]
    });
    let base = app.base.clone();
    let url = format!("{base}/api/experiment-requests/{request_id}/complete");
    let mut handles = Vec::new();
    for _ in 0..2 {
        let client = app.client.clone();
        let url = url.clone();
        let body = body.clone();
        handles.push(std::thread::spawn(move || {
            client.post(url).json(&body).send().unwrap().status().as_u16()
        }));
    }
    let mut statuses = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    statuses.sort();
    assert_eq!(statuses, vec![200, 409], "settlement was not exclusive");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["status"], "active", "{policy}");
    let requests = app.get("/api/experiment-requests");
    assert_eq!(requests.as_array().unwrap()[0]["status"], "completed");
}

#[test]
fn a_failed_call_is_not_repeated_and_the_model_adapts() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, _) = scripted_provider(move |_| {
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-2","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            3 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"adapt-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"adapted.txt\",\"content\":\"OK\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"다른 방법으로 완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _stream = app.chat(&json!({"input":"adapted.txt 파일을 만들어줘","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapted.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().cloned().unwrap_or_default();
    assert!(
        adaptations.iter().any(|item| item["observation"]["tool"] == "shell_exec"),
        "adaptation was not recorded: {execution}"
    );
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shell_calls = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(shell_calls, 1, "failed call was re-executed: {operations}");
    let writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_write")
        .count();
    assert_eq!(writes, 1, "adapted action did not run: {operations}");
    let write = operations
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["tool"] == "workspace_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(write["toolReceipt"]["verified"], true, "{write}");
    assert_eq!(
        write["toolReceipt"]["verifier"]["kind"],
        "sha256_readback",
        "{write}"
    );
    assert!(
        write["observedVersion"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")),
        "{write}"
    );
    assert_eq!(
        execution["effectSummary"]["verifiedEffects"],
        1,
        "{execution}"
    );
    let failed = operations
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["tool"] == "shell_exec")
        .unwrap();
    assert_eq!(failed["effectState"], "failed", "{failed}");
    assert_eq!(
        failed["toolReceipt"]["failureLayer"], "execution",
        "{failed}"
    );
}

#[test]
fn a_fully_specified_replacement_runs_without_a_model_call() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"MODEL SHOULD NOT RUN"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    std::fs::create_dir_all(app._root.join("bench")).unwrap();
    std::fs::write(app._root.join("bench/frozen.txt"), "start ALPHA end\n").unwrap();
    let _stream = app.chat(&json!({
        "input":"edit `bench/frozen.txt` and replace `ALPHA` with `BETA` and preserve everything else.",
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(app._root.join("bench/frozen.txt")).unwrap(),
        "start BETA end\n"
    );
    assert!(
        requests.lock().unwrap().is_empty(),
        "the model was called for a frozen transformation"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    assert_eq!(execution["taskOutcome"], "succeeded", "{execution}");
    assert_eq!(execution["modelCalls"], 0, "{execution}");
    assert_eq!(
        execution["deterministicShortcut"]["modelCalls"], 0,
        "{execution}"
    );
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let items = operations.as_array().unwrap();
    let write = items
        .iter()
        .find(|operation| operation["tool"] == "file_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(
        items
            .iter()
            .filter(|operation| operation["tool"] == "file_read")
            .count(),
        1
    );
    let conn = Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    let rules: i64 = conn
        .query_row(
            "SELECT count(*) FROM records WHERE kind='experience_rule'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rules, 0, "a deterministic shortcut became learning evidence");
}

#[test]
fn a_frozen_replacement_passes_the_opt_in_guard() {
    let _serial = serial();
    let root = temp_root();
    std::fs::create_dir_all(root.join("bench")).unwrap();
    std::fs::write(root.join("bench/guarded.txt"), "start ALPHA end\n").unwrap();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"MODEL SHOULD NOT RUN"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["file_write"]}));
    app.post(
        "/api/benchmark/policies/state",
        &json!({
            "slot":"mutation.preflight",
            "name":"deterministic-guard",
            "parameters":{"kernel":"same-file-revision-v1"}
        }),
    );
    let stream = app.chat(&json!({
        "input":"edit `bench/guarded.txt` and replace `ALPHA` with `BETA` and preserve everything else.",
        "autoLearning":false
    }));
    assert_eq!(
        std::fs::read_to_string(app._root.join("bench/guarded.txt")).unwrap(),
        "start BETA end\n",
        "{stream}"
    );
    assert!(requests.lock().unwrap().is_empty(), "the model was called");
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["taskOutcome"], "succeeded", "{execution}");
    assert_eq!(execution["modelCalls"], 0, "{execution}");
    assert_eq!(
        execution["deterministicShortcut"]["modelCalls"], 0,
        "{execution}"
    );
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let items = operations.as_array().unwrap();
    assert!(
        items.iter().any(|operation| operation["tool"] == "file_read"
            && operation["fileObservation"]["sha256"].is_string()),
        "the guard could not see the deterministic read: {operations}"
    );
    let write = items
        .iter()
        .find(|operation| operation["tool"] == "file_write")
        .unwrap();
    assert_eq!(write["effectState"], "verified", "{write}");
    assert_eq!(
        write["result"]["preflightBlocked"],
        Value::Null,
        "{write}"
    );
}

#[test]
fn a_typed_execution_program_takes_the_failure_branch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"프로그램 결과로 완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"e2e-program-1",
        "nodes":[
            {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
            {"type":"branch","id":"check",
             "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
             "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
             "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"e2e-program","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");

    let stream = app.chat(&json!({"input":"fallback.txt 파일을 만들어줘","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("fallback.txt")).unwrap_or_default(),
        "FALLBACK"
    );
    assert!(!app._root.join("primary.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "completed", "{execution}");
    assert_eq!(checkpoint["programRevision"], "e2e-program-1");
    let completed = checkpoint["completedOperationIds"].as_array().unwrap();
    assert!(completed.iter().any(|item| item == "try"));
    assert!(completed.iter().any(|item| item == "fallback"));
    assert!(
        checkpoint["pendingOperationIds"].as_array().unwrap().is_empty(),
        "{checkpoint}"
    );
}

/// Builds a verifiable evidence bundle: 10 pairs, baseline fails, treatment succeeds.
fn evidence_bundle(pairs: u64, baseline_ok: bool, treatment_ok: bool) -> Value {
    let mut trials = Vec::new();
    for pair in 0..pairs {
        for (arm, ok) in [("baseline", baseline_ok), ("treatment", treatment_ok)] {
            trials.push(json!({
                "pair":pair,"arm":arm,
                "executionId":format!("e2e-exec-{pair}-{arm}"),
                "status":"completed",
                "auditOk":ok,"reportOk":ok,
                "taskOutcome":if ok {"succeeded"} else {"failed"},
                "compliance":ok,"requiresCompliance":false,"errorClass":"",
                "planQuality":0.0,"planQualityIndependent":0.0,"elapsedMs":10
            }));
        }
    }
    let delta = if treatment_ok {1.0} else {0.0} - if baseline_ok {1.0} else {0.0};
    let n = pairs as f64;
    let halfwidth = (2.0 * (2.0f64 / 0.05).ln() / n).sqrt();
    let verdict = if pairs >= 10 && delta - halfwidth > 0.0 {
        "preliminary_support"
    } else if pairs >= 10 && delta + halfwidth < 0.0 {
        "regressed"
    } else {
        "inconclusive"
    };
    let manifest = json!({
        "pairs":pairs,"seed":94371,"oracleVersion":"s7-harness-audit-v1",
        "fixture":"planning_evaluation","mode":"smoke","model":{"model":"mock"}
    });
    let hash = format!("{:x}", Sha256::digest(manifest.to_string().as_bytes()));
    json!({
        "trials":trials,"manifest":manifest,"manifestHash":hash,
        "delta":delta,"independentUnits":pairs,"verdict":verdict
    })
}

#[test]
fn regressed_experiment_rolls_back_the_candidate() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({
            "slot":"planning","name":"regression-candidate",
            "parameters":{"outline":true},"rationale":"regression rollback test"
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
        &json!({"runnerId":"regression-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    let evidence = evidence_bundle(10, true, false);
    seed_evidence_executions(&app, &evidence);
    assert_eq!(evidence["verdict"], "regressed", "{evidence}");
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken": token,
            "experimentId":"regressed-1",
            "verdict":evidence["verdict"],
            "delta":evidence["delta"],
            "independentUnits":evidence["independentUnits"],
            "manifest":evidence["manifest"],
            "manifestHash":evidence["manifestHash"],
            "trials":evidence["trials"],
        }),
    );
    assert_eq!(completed["request"]["status"], "completed", "{completed}");
    assert_eq!(completed["request"]["verdict"], "regressed", "{completed}");
    assert_eq!(
        completed["request"]["promotionStatus"], "rolled_back",
        "{completed}"
    );
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|policy| policy["id"] == policy_id.as_str())
        .expect("policy");
    assert_eq!(policy["status"], "rolled_back", "{policy}");
}

#[test]
fn manual_learn_requests_semantic_extraction() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"{\"memories\":[],\"skill\":null}"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"semantic learn"}));
    let session_id = session["id"].as_str().unwrap();
    let path = app._root.join("runtime/dominion.sqlite");
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO messages(id,session_id,role,content,agent_id) VALUES(?1,?2,'user',?3,'a')",
        rusqlite::params!["learn-user", session_id, "앞으로 답변은 한국어로만 해"],
    ).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO messages(id,session_id,role,content,agent_id) VALUES(?1,?2,'assistant',?3,'a')",
        rusqlite::params!["learn-assistant", session_id, "알겠습니다"],
    ).unwrap();
    drop(conn);
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let response = app.client
        .post(format!("{}/api/sessions/{session_id}/learn", app.base))
        .json(&json!({"providerId":provider,"model":"mock"}))
        .send()
        .unwrap();
    let status = response.status();
    let queued: Value = serde_json::from_str(&response.text().unwrap()).unwrap_or(Value::Null);
    assert!(status.is_success(), "learn failed: {status} {queued}");
    let job = &queued["job"];
    assert_eq!(job["kind"], "learn", "{queued}");
    assert_eq!(job["input"]["semantic"], true, "{queued}");
    assert_eq!(job["input"]["automatic"], true, "{queued}");
}

#[test]
fn completion_rejects_fake_or_incomplete_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let make = |name: &str| -> (String, String) {
        let policy = app.post(
            "/api/benchmark/policies",
            &json!({"slot":"planning","name":name,"parameters":{"outline":true},"rationale":"evidence"}),
        );
        let policy_id = policy["id"].as_str().unwrap().to_string();
        let request = app.post(
            "/api/benchmark/experiment-requests",
            &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
        );
        let request_id = request["id"].as_str().unwrap().to_string();
        let claimed = app.post(
            "/api/experiment-requests/claim",
            &json!({"runnerId":name,"ttlSeconds":60}),
        );
        let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
        (policy_id, format!("{request_id}|{token}"))
    };
    let good_plan = "1. bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장한다\n2. bench/harness_audit.txt 파일을 만든다\n3. 두 파일을 검증한다";

    // 1) Claimed success numbers that do not match the raw failures.
    let (policy_id, target) = make("fake-numbers");
    let (request_id, token) = target.split_once('|').unwrap();
    let fake = evidence_bundle(10, false, false);
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken":token,"experimentId":"fake-exp",
            "verdict":"preliminary_support","delta":1.0,"independentUnits":10,
            "manifest":fake["manifest"],"manifestHash":fake["manifestHash"],"trials":fake["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":good_plan}]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("evidence rejected")),
        "{completed}"
    );
    let policies = app.get("/api/policies");
    assert_eq!(
        policies
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == policy_id.as_str())
            .unwrap()["status"],
        "candidate"
    );

    // 2) Missing arm for a planned pair.
    let (_policy_id, target) = make("incomplete-trials");
    let (request_id, token) = target.split_once('|').unwrap();
    let mut incomplete = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &incomplete);
    let trials = incomplete["trials"].as_array_mut().unwrap();
    let index = trials.iter().position(|t| t["pair"] == 9 && t["arm"] == "treatment").unwrap();
    trials.remove(index);
    let completed = app.post(
        &format!("/api/experiment-requests/{request_id}/complete"),
        &json!({
            "claimToken":token,"experimentId":"incomplete-exp",
            "verdict":incomplete["verdict"],"delta":incomplete["delta"],
            "independentUnits":incomplete["independentUnits"],"manifest":incomplete["manifest"],
            "manifestHash":incomplete["manifestHash"],"trials":incomplete["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
            "plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":good_plan}]
        }),
    );
    assert_eq!(completed["request"]["status"], "completed");
    assert!(
        completed["request"]["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("missing an arm") || text.contains("incomplete")),
        "{completed}"
    );
}

#[test]
fn a_crashed_settlement_recovers_without_reapplying_promotion_and_stale_claims_fail() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"unused"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    let policy = app.post(
        "/api/benchmark/policies",
        &json!({"slot":"planning","name":"crash-settle","parameters":{"outline":true},"rationale":"crash"}),
    );
    let policy_id = policy["id"].as_str().unwrap().to_string();
    let request = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy_id,"fixture":"planning_evaluation"}),
    );
    let request_id = request["id"].as_str().unwrap().to_string();
    let _claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"crash-runner","ttlSeconds":60}),
    );
    let root = app._root.clone();
    {
        let conn = Connection::open(root.join("runtime/dominion.sqlite")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let data: String = conn
            .query_row(
                "SELECT data FROM records WHERE kind='experiment_request' AND id=?1",
                [request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&data).unwrap();
        value["status"] = json!("settling");
        value["settleToken"] = json!("stale-settle");
        value["settleExpiresAtMs"] = json!(0);
        value["promotionIntent"] = json!({
            "kind":"policy","targetId":policy_id,"experimentId":"recovered-exp"
        });
        conn.execute(
            "UPDATE records SET data=?2 WHERE kind='experiment_request' AND id=?1",
            rusqlite::params![request_id, value.to_string()],
        )
        .unwrap();
        let policy_data: String = conn
            .query_row(
                "SELECT data FROM records WHERE kind='adaptation_policy' AND id=?1",
                [policy_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let mut policy_value: Value = serde_json::from_str(&policy_data).unwrap();
        policy_value["status"] = json!("active");
        policy_value["revision"] = json!("recovered-revision");
        policy_value["lastExperiment"] = json!({
            "experimentId":"recovered-exp","independentUnits":10,
            "semanticDelta":0.3,"independentSemanticDelta":0.3,
            "verdict":"preliminary_support"
        });
        conn.execute(
            "UPDATE records SET data=?2 WHERE kind='adaptation_policy' AND id=?1",
            rusqlite::params![policy_id, policy_value.to_string()],
        )
        .unwrap();
    }
    // Trigger settling recovery through the claim path. When no request is
    // queued the endpoint returns 204, so raw client is used here.
    let _ = app
        .client
        .post(format!("{}/api/experiment-requests/claim", app.base))
        .json(&json!({"runnerId":"recovery-check","ttlSeconds":60}))
        .send();

    let requests = app.get("/api/experiment-requests");
    let recovered = &requests.as_array().unwrap()[0];
    assert_eq!(recovered["status"], "completed", "{recovered}");
    assert!(
        recovered["promotionError"]
            .as_str()
            .is_some_and(|text| text.contains("recovered after restart")),
        "{recovered}"
    );
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["revision"], "recovered-revision", "promotion was reapplied");

    // Stale claim: expiry rejects completion.
    let policy2 = app.post(
        "/api/benchmark/policies",
        &json!({"slot":"planning","name":"stale-claim","parameters":{"outline":true},"rationale":"stale"}),
    );
    let policy2_id = policy2["id"].as_str().unwrap().to_string();
    let request2 = app.post(
        "/api/benchmark/experiment-requests",
        &json!({"policyId":policy2_id,"fixture":"planning_evaluation"}),
    );
    let request2_id = request2["id"].as_str().unwrap().to_string();
    let claimed = app.post(
        "/api/experiment-requests/claim",
        &json!({"runnerId":"stale-runner","ttlSeconds":60}),
    );
    let token = claimed["request"]["claimToken"].as_str().unwrap().to_string();
    {
        let conn = Connection::open(root.join("runtime/dominion.sqlite")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn.execute(
            "UPDATE records SET data=json_set(data,'$.claimExpiresAtMs',0) WHERE kind='experiment_request' AND id=?1",
            [request2_id.as_str()],
        )
        .unwrap();
    }
    let evidence = evidence_bundle(10, false, true);
    seed_evidence_executions(&app, &evidence);
    let response = app
        .client
        .post(format!("{}/api/experiment-requests/{request2_id}/complete", app.base))
        .json(&json!({
            "claimToken":token,"experimentId":"stale-exp",
            "verdict":evidence["verdict"],"delta":evidence["delta"],
            "independentUnits":evidence["independentUnits"],"manifest":evidence["manifest"],
            "manifestHash":evidence["manifestHash"],"trials":evidence["trials"],
            "semanticDelta":0.5,"independentSemanticDelta":0.5,
            "semanticEvaluator":{"id":"plan-quality-main","version":1},
            "independentEvaluator":{"id":"plan-quality-independent","version":1},
            "task":"task","plans":[{"pair":0,"arm":"baseline","plan":""},{"pair":0,"arm":"treatment","plan":"1. 검증"}]
        }))
        .send()
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);

    // Restart preserves the recovered settlement and does not re-promote.
    let root = app._root.clone();
    drop(app);
    let app = TestApp::from_root(port, "native", root, &[]);
    let requests = app.get("/api/experiment-requests");
    let recovered = requests
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == request_id.as_str())
        .expect("recovered request");
    assert_eq!(recovered["status"], "completed");
    let policies = app.get("/api/policies");
    let policy = policies
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == policy_id.as_str())
        .unwrap();
    assert_eq!(policy["revision"], "recovered-revision");
}

#[test]
fn a_model_generated_intervention_runs_and_refutes_its_hypothesis() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let adaptations = Arc::new(Mutex::new(0u32));
    let stream_counter = stream_calls.clone();
    let adaptation_counter = adaptations.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            let mut count = adaptation_counter.lock().unwrap();
            *count += 1;
            let (hypothesis, prediction, intervention) = if *count == 1 {
                ("h1", json!({"type":"equals","from":"result","path":"/written","value":false}), "i1")
            } else {
                ("h2", json!({"type":"equals","from":"result","path":"/written","value":true}), "i2")
            };
            let content = json!({
                "hypotheses":[{
                    "id":hypothesis,"claim":"the command environment is wrong",
                    "predictions":[{"predicate":prediction}]
                }],
                "interventions":[{
                    "id":intervention,"hypothesisIds":[hypothesis],
                    "tool":"workspace_write",
                    "inputs":{"path":format!("adapt{}.txt", *count),"content":"OK"},
                    "expectedChecks":["file_exists"],"estimatedCost":0.1
                }]
            }).to_string();
            return vec![json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]})];
        }
        let mut count = stream_counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"적응 결과로 완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let _ = app.chat(&json!({"input":"adapt1.txt와 adapt2.txt 파일을 만들어줘","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapt1.txt")).unwrap_or_default(),
        "OK"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("adapt2.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().cloned().unwrap_or_default();
    let model_adaptation = adaptations
        .iter()
        .find(|item| item["selected"].is_object())
        .expect("model adaptation record");
    assert!(
        model_adaptation["hypotheses"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(model_adaptation["selected"]["id"].is_string());
    let refuted = model_adaptation["hypothesisResults"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["status"] == "refuted");
    assert!(refuted, "wrong hypothesis was not refuted: {model_adaptation}");
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shells = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(
        shells, 2,
        "a retry after the environment changed should be allowed: {operations}"
    );
    let adapted_writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_write" && operation["origin"] == "adaptation")
        .count();
    assert!(adapted_writes >= 1, "{operations}");
}

#[test]
fn a_program_consumes_sequence_verify_loop_and_model_decision_nodes() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"프로그램 노드를 모두 소비했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"multi-node-program",
        "nodes":[
            {"type":"call_tool","id":"try","tool":"shell_exec","inputs":{"command":"exit 7"},"saveAs":"try"},
            {"type":"branch","id":"check",
             "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
             "then":[{"type":"call_tool","id":"primary","tool":"workspace_write","inputs":{"path":"primary.txt","content":"PRIMARY"}}],
             "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write","inputs":{"path":"fallback.txt","content":"FALLBACK"}}]},
            {"type":"sequence","id":"seq","nodes":[
                {"type":"call_tool","id":"seq1","tool":"workspace_write","inputs":{"path":"seq.txt","content":"SEQ"}}
            ]},
            {"type":"verify","id":"verify","checks":[
                {"predicate":{"type":"exists","from":"seq1","path":"/path"}}
            ]},
            {"type":"loop","id":"loop","maxIterations":2,
             "predicate":{"type":"not","predicate":{"type":"exists","from":"loop_done"}},
             "body":[{"type":"call_tool","id":"loop1","tool":"workspace_write","inputs":{"path":"loop.txt","content":"LOOP"},"saveAs":"loop_done"}]},
            {"type":"model_decision","id":"model"}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"multi-node","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"fallback.txt 파일을 만들어줘","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    for (path, expected) in [
        ("fallback.txt", "FALLBACK"),
        ("seq.txt", "SEQ"),
        ("loop.txt", "LOOP"),
    ] {
        assert_eq!(
            std::fs::read_to_string(app._root.join(path)).unwrap_or_default(),
            expected,
            "{path} was not written"
        );
    }
    assert!(!app._root.join("primary.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "paused", "{checkpoint}");
    assert_eq!(checkpoint["loopIterations"], 1, "{checkpoint}");
    assert_eq!(checkpoint["parallelMode"], Value::Null);
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let tools = operations
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|operation| operation["tool"].as_str())
        .collect::<Vec<_>>();
    assert!(tools.contains(&"shell_exec"));
    assert!(tools.contains(&"workspace_write"));
}

#[test]
fn consecutive_failed_adaptations_stop_the_no_progress_loop() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let adaptation_calls = Arc::new(Mutex::new(0u32));
    let adaptation_counter = adaptation_calls.clone();
    let counter = stream_calls.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            let mut adaptation = adaptation_counter.lock().unwrap();
            *adaptation += 1;
            let (hypothesis, intervention, command) = if *adaptation == 1 {
                ("h1", "i1", "exit 8")
            } else {
                ("h2", "i2", "exit 10")
            };
            let content = json!({
                "hypotheses":[{"id":hypothesis,"claim":"still wrong","predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]}],
                "interventions":[{"id":intervention,"hypothesisIds":[hypothesis],"tool":"shell_exec","inputs":{"command":command},"expectedChecks":["x"],"estimatedCost":0.1}]
            }).to_string();
            return vec![json!({"choices":[{"delta":{"content":content},"finish_reason":"stop"}]})];
        }
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"f1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"f2","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 9\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"진전이 없어 중단합니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let stream = app.chat(&json!({"input":"계속 시도해","autoLearning":false}));
    assert!(stream.contains("진전이 없어 중단합니다."), "{stream}");
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let adaptations = execution["adaptations"].as_array().unwrap();
    assert!(
        adaptations.iter().filter(|item| item["selected"].is_object()).count() >= 2,
        "{execution}"
    );
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let shells = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "shell_exec")
        .count();
    assert_eq!(shells, 4, "unexpected retry loop: {operations}");
}

#[test]
fn a_mid_adaptation_restart_restores_hypotheses_and_does_not_repeat_completed_steps() {
    let _serial = serial();
    let stream_calls = Arc::new(Mutex::new(0u32));
    let stream_counter = stream_calls.clone();
    let (port, _) = scripted_provider(move |request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        if request["stream"].as_bool() == Some(false) && system.contains("adaptation planner") {
            return vec![json!({"choices":[{"delta":{"content":json!({
                "hypotheses":[{
                    "id":"h1",
                    "claim":"the shell environment cannot run this command",
                    "predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]
                }],
                "interventions":[{
                    "id":"i1","hypothesisIds":["h1"],"tool":"workspace_write",
                    "inputs":{"path":"adapt.txt","content":"ADAPT"},
                    "expectedChecks":["file_exists"],"estimatedCost":0.1
                }]
            }).to_string()},"finish_reason":"stop"}]})];
        }
        let mut count = stream_counter.lock().unwrap();
        *count += 1;
        match *count {
            1 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fail-1","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            2 => {
                // Hold the model call open so the test can kill the process
                // after the adaptation tool has already run.
                std::thread::sleep(Duration::from_millis(3_000));
                vec![json!({"choices":[{"delta":{"content":"첫 실행 모델 응답"},"finish_reason":"stop"}]})]
            }
            _ => vec![json!({"choices":[{"delta":{"content":"재개 실행 완료."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"adapt-restart-program",
        "nodes":[
            {"type":"call_tool","id":"step1","tool":"workspace_write","inputs":{"path":"step1.txt","content":"ONE"},"saveAs":"step1"},
            {"type":"model_decision","id":"pause"},
            {"type":"call_tool","id":"step2","tool":"workspace_write","inputs":{"path":"step2.txt","content":"TWO"},"saveAs":"step2"},
            {"type":"model_decision","id":"pause2"}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"adapt-restart","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"step1.txt와 step2.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("adapt.txt").exists() {
        assert!(Instant::now() < deadline, "adaptation did not run");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Wait until the adaptation record (hypotheses + selected intervention) is
    // durable before killing the process.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let executions = app.get("/api/executions");
        let recorded = executions.as_array().unwrap().iter().any(|execution| {
            execution["adaptations"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| {
                    item["selected"]["id"] == "i1"
                        && item["hypotheses"].as_array().is_some_and(|h| !h.is_empty())
                }))
        });
        if recorded {
            break;
        }
        assert!(Instant::now() < deadline, "adaptation record was not persisted");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(150));
    drop(app);
    let _ = handle.join();

    let app = TestApp::from_root(port, "native", root.clone(), &[("DOMINION_BENCHMARK", "1")]);
    let deadline = Instant::now() + Duration::from_secs(25);
    let resumed = loop {
        let executions = app.get("/api/executions");
        if let Some(resumed) = executions.as_array().unwrap().iter().find(|execution| {
            execution["resumedFromExecutionId"].is_string()
                && execution["programCheckpoint"]["status"] == "completed"
        }) {
            break resumed.clone();
        }
        assert!(
            Instant::now() < deadline,
            "resumed execution did not restore: {executions}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // The saved hypothesis and chosen intervention are restored on the new
    // execution, not just left behind on the interrupted one.
    let adaptation = resumed["adaptations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["selected"]["id"] == "i1"))
        .expect("restored adaptation record");
    assert_eq!(adaptation["hypotheses"][0]["id"], "h1", "{adaptation}");
    assert_eq!(
        adaptation["selected"]["tool"], "workspace_write",
        "{adaptation}"
    );
    assert_eq!(
        resumed["adaptationCheckpoint"]["selectedIntervention"],
        "i1",
        "{resumed}"
    );
    assert!(
        resumed["adaptationCheckpoint"]["failureSignature"].is_string(),
        "{resumed}"
    );
    // The completed program step is not re-executed and the adaptation stays
    // a single side effect.
    let executions = app.get("/api/executions");
    let operations = executions
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|execution| {
            let id = execution["id"].as_str().unwrap_or("");
            app.get(&format!("/api/executions/{id}/operations"))
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let count = |path: &str| {
        operations
            .iter()
            .filter(|operation| {
                operation["tool"] == "workspace_write"
                    && operation["arguments"]["path"] == path
            })
            .count()
    };
    assert_eq!(count("step1.txt"), 1, "step1 was repeated: {operations:?}");
    assert_eq!(count("adapt.txt"), 1, "adaptation was repeated: {operations:?}");
    assert_eq!(std::fs::read_to_string(root.join("step1.txt")).unwrap_or_default(), "ONE");
    assert_eq!(std::fs::read_to_string(root.join("adapt.txt")).unwrap_or_default(), "ADAPT");
}

