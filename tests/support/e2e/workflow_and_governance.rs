#[test]
fn a_paused_program_resumes_after_restart_without_repeating_completed_steps() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_millis(1_800));
        vec![json!({"choices":[{"delta":{"content":"재개 완료."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"restart-program",
        "nodes":[
            {"type":"call_tool","id":"step1","tool":"workspace_write","inputs":{"path":"step1.txt","content":"ONE"},"saveAs":"step1"},
            {"type":"model_decision","id":"pause"},
            {"type":"branch","id":"branch-after-restart",
             "predicate":{"type":"exists","from":"step1","path":"/path"},
             "then":[{"type":"call_tool","id":"step2","tool":"workspace_write","inputs":{"path":"step2.txt","content":"TWO"},"saveAs":"step2"}],
             "else":[{"type":"call_tool","id":"wrong","tool":"workspace_write","inputs":{"path":"wrong.txt","content":"STALE"}}]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"restart-program","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let root = app._root.clone();
    let handle = app.chat_async(&json!({
        "input":"step1.txt와 step2.txt 파일을 만들어줘",
        "autoLearning":false
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("step1.txt").exists() {
        assert!(Instant::now() < deadline, "program did not write step1");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    drop(app);
    let _ = handle.join();

    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let deadline = Instant::now() + Duration::from_secs(25);
    while !root.join("step2.txt").exists() {
        assert!(
            Instant::now() < deadline,
            "resumed program did not write step2: {}",
            app.get("/api/executions")
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let executions = app.get("/api/executions");
        if executions.as_array().unwrap().iter().any(|execution| {
            execution["programCheckpoint"]["status"] == "completed"
                && execution["resumedFromExecutionId"].is_string()
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "resumed execution did not complete: {executions}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("step1.txt")).unwrap_or_default(),
        "ONE"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("step2.txt")).unwrap_or_default(),
        "TWO"
    );
    assert!(!root.join("wrong.txt").exists(), "restart took a branch without durable observations");
    let executions = app.get("/api/executions");
    assert!(
        executions.as_array().unwrap().iter().any(|execution| {
            execution["resumedFromExecutionId"].is_string()
                && execution["programCheckpoint"]["resolvedInputs"]["step1"]["path"].is_string()
        }),
        "completed program observation disappeared across process restart: {executions}"
    );
    let step1_writes = executions
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
        .filter(|operation| {
            operation["tool"] == "workspace_write"
                && operation["arguments"]["path"] == "step1.txt"
        })
        .count();
    assert_eq!(
        step1_writes, 1,
        "completed program step was re-executed after restart"
    );
}

#[test]
fn parallel_and_nested_program_nodes_execute_concurrently() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"병렬 프로그램 완료."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new_with_env(port, "native", &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let program = json!({
        "revision":"parallel-nested-program",
        "nodes":[
            {"type":"call_tool","id":"seed","tool":"workspace_write",
             "inputs":{"path":"seed.txt","content":"SEED"},"saveAs":"seed"},
            {"type":"parallel","id":"par","nodes":[
                {"type":"sequence","id":"s1","nodes":[
                    {"type":"call_tool","id":"p1","tool":"shell_exec",
                     "inputs":{"command":"sleep 2.0"},"saveAs":"p1"},
                    {"type":"branch","id":"b1",
                     "predicate":{"type":"exists","from":"seed","path":"/path"},
                     "then":[{"type":"call_tool","id":"p1b","tool":"workspace_write",
                              "inputs":{"path":"p1b.txt","content":"P1B"}}],
                     "else":[{"type":"call_tool","id":"p1e","tool":"workspace_write",
                              "inputs":{"path":"p1e.txt","content":"P1E"}}]}
                ]},
                {"type":"sequence","id":"s2","nodes":[
                    {"type":"call_tool","id":"p2","tool":"shell_exec",
                     "inputs":{"command":"sleep 2.0"},"saveAs":"p2"},
                    {"type":"loop","id":"l2","maxIterations":1,
                     "predicate":{"type":"not","predicate":{"type":"exists","from":"p2b"}},
                     "body":[{"type":"call_tool","id":"p2b","tool":"workspace_write",
                              "inputs":{"path":"p2b.txt","content":"P2B"},"saveAs":"p2b"}]}
                ]}
            ]},
            {"type":"verify","id":"verify","checks":[
                {"predicate":{"type":"exists","from":"seed","path":"/path"}},
                {"predicate":{"type":"exists","from":"p1b","path":"/path"}},
                {"predicate":{"type":"exists","from":"p2b","path":"/path"}}
            ]}
        ]
    });
    let activated = app.post(
        "/api/benchmark/policies/state",
        &json!({"slot":"execution","name":"parallel-nested","parameters":{"program":program}}),
    );
    assert_eq!(activated["active"]["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"병렬 프로그램을 실행해","autoLearning":false}));
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    assert!(app._root.join("seed.txt").exists());
    assert_eq!(
        std::fs::read_to_string(app._root.join("p1b.txt")).unwrap_or_default(),
        "P1B"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("p2b.txt")).unwrap_or_default(),
        "P2B"
    );
    assert!(!app._root.join("p1e.txt").exists());
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let shells = operations.as_array().unwrap().iter()
        .filter(|operation| operation["tool"] == "shell_exec").collect::<Vec<_>>();
    assert_eq!(shells.len(), 2, "{operations}");
    let timestamp = |operation: &Value, key: &str| {
        chrono::DateTime::parse_from_rfc3339(operation[key].as_str().unwrap()).unwrap()
    };
    assert!(
        timestamp(shells[0], "startedAt") < timestamp(shells[1], "finishedAt")
            && timestamp(shells[1], "startedAt") < timestamp(shells[0], "finishedAt"),
        "parallel shell operations must overlap: {operations}"
    );
    let checkpoint = &execution["programCheckpoint"];
    assert_eq!(checkpoint["status"], "completed", "{checkpoint}");
    assert_eq!(checkpoint["parallelMode"], "concurrent", "{checkpoint}");
    assert_eq!(
        checkpoint["resolvedInputs"]["lastVerify"]["ok"], true,
        "verify node did not run: {checkpoint}"
    );
    assert!(checkpoint["pendingOperationIds"].as_array().unwrap().is_empty());
}

fn seed_evidence_executions(app: &TestApp, evidence: &Value) {
    let path = app._root.join("runtime/dominion.sqlite");
    let Ok(conn) = Connection::open(path) else {
        return;
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    for trial in evidence["trials"].as_array().into_iter().flatten() {
        if trial["taskOutcome"].as_str() != Some("succeeded") {
            continue;
        }
        let Some(id) = trial["executionId"].as_str() else {
            continue;
        };
        let record = json!({
            "id": id,
            "status": "completed",
            "taskOutcome": "succeeded",
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO records(kind,id,data) VALUES('execution',?1,?2)",
            rusqlite::params![id, record.to_string()],
        );
    }
}

#[test]
fn block_write_without_read_algorithm_blocks_and_then_allows_the_write() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let last_tool = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_tool.contains("toolSelectionBlocked") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"list-1","type":"function","function":{"name":"workspace_list","arguments":"{\"path\":\"\"}"}}
            ]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("entries") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"guarded.txt\",\"content\":\"OK\"}"}}
            ]},"finish_reason":"tool_calls"}]})];
        }
        if messages.iter().any(|message| message["role"] == "tool") {
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"early-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"guarded.txt\",\"content\":\"EARLY\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({
            "kind":"tool.selection",
            "name":"block-write-without-read",
            "implementation":"block-write-without-read",
            "parameters":{}
        }),
    );
    let activated = app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    assert_eq!(activated["status"], "active", "{activated}");
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 만들어줘","autoLearning":false}));
    assert!(
        stream.contains("toolSelectionBlocked") || stream.contains("차단"),
        "{stream}"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("guarded.txt")).unwrap_or_default(),
        "OK"
    );
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let writes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| {
            operation["tool"] == "workspace_write"
                && operation["status"] == "completed"
        })
        .count();
    assert_eq!(writes, 1, "{operations}");
}

#[test]
fn file_preflight_blocks_blind_and_stale_writes_then_accepts_a_fresh_read() {
    let _serial = serial();
    let root = temp_root();
    let target = root.join("guarded.txt");
    std::fs::write(&target, "original").unwrap();
    let step = std::sync::atomic::AtomicUsize::new(0);
    let (port, _) = scripted_provider(move |_| {
        let index = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if index == 2 {
            std::fs::write(&target, "changed outside the run").unwrap();
        }
        let (name, args) = match index {
            0 | 2 | 4 => ("workspace_write", json!({"path":"guarded.txt","content":"final"})),
            1 | 3 => ("workspace_read", json!({"path":"guarded.txt"})),
            _ => return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":format!("preflight-{index}"),"type":"function",
            "function":{"name":name,"arguments":args.to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"preflight-regression",
        "parameters":{"kernel":"same-file-revision-v1"}
    }));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "final", "{stream}");
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["taskOutcome"], "answered_unverified", "{execution}");
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let writes = operations.as_array().unwrap().iter().filter(|operation| operation["tool"] == "workspace_write").collect::<Vec<_>>();
    assert_eq!(writes.len(), 3, "{operations}");
    assert_eq!(writes.iter().filter(|operation| operation["result"]["preflightBlocked"] == true).count(), 2);
    assert_eq!(writes.iter().filter(|operation| operation["status"] == "completed"
        && operation["result"]["observedSha256"] == format!("{:x}", Sha256::digest(b"changed outside the run"))).count(), 1);
}

#[test]
fn preflight_v2_reuses_a_rejected_write_observation_without_a_helper_call() {
    let _serial = serial();
    let root = temp_root();
    std::fs::write(root.join("guarded.txt"), "mode=old\nowner=keep\n").unwrap();
    let (port, requests) = scripted_provider(|request| {
        let last_tool = request["messages"].as_array().unwrap().iter().rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str());
        let (id, path, content) = match last_tool {
            None => ("escape", "/guarded.txt", "blind"),
            Some(result) if result.contains("invalidWorkspacePath") => ("blind", "guarded.txt", "blind"),
            Some(result) if result.contains("currentContent") && result.contains("owner=keep") => {
                ("retry", "guarded.txt", "mode=new\nowner=keep\n")
            }
            _ => return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":id,"type":"function","function":{"name":"workspace_write",
                "arguments":json!({"path":path,"content":content}).to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"preflight-inline-read",
        "parameters":{"kernel":"same-file-revision-v2"}
    }));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "mode=new\nowner=keep\n", "{stream}");
    let execution = &app.get("/api/executions")[0];
    assert_eq!(execution["taskOutcome"], "answered_unverified", "{execution}");
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let operations = operations.as_array().unwrap();
    assert_eq!(operations.len(), 3, "{operations:?}");
    assert!(operations.iter().any(|op| op["result"]["preflightBlocked"] == true
        && op["fileObservation"]["sha256"].is_string()));
    assert!(requests.lock().unwrap().iter().all(|request| request["stream"] == true));
}

#[test]
fn preflight_inline_observation_allows_retry_without_a_separate_read() {
    let _serial = serial();
    let root = temp_root();
    let target = root.join("inline.txt");
    std::fs::write(&target, "original").unwrap();
    let step = std::sync::atomic::AtomicUsize::new(0);
    let (port, _) = scripted_provider(move |_| {
        let index = step.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if index < 2 {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{
                "index":0,"id":format!("inline-{index}"),"type":"function",
                "function":{"name":"workspace_write","arguments":"{\"path\":\"inline.txt\",\"content\":\"final\"}"}
            }]},"finish_reason":"tool_calls"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::from_root(port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"inline-preflight",
        "parameters":{"kernel":"same-file-revision-v2"}
    }));
    let stream = app.chat(&json!({"input":"inline.txt 파일을 수정해.","autoLearning":false}));
    assert_eq!(std::fs::read_to_string(app._root.join("inline.txt")).unwrap(), "final", "{stream}");
    let execution = &app.get("/api/executions")[0];
    let operations = app.get(&format!("/api/executions/{}/operations", execution["id"].as_str().unwrap()));
    let writes = operations.as_array().unwrap().iter()
        .filter(|operation| operation["tool"] == "workspace_write")
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2, "{operations}");
    let blocked = writes.iter()
        .find(|operation| operation["result"]["preflightBlocked"] == true)
        .expect("blocked write");
    assert_eq!(blocked["result"]["currentContent"], "original", "{operations}");
    let completed = writes.iter()
        .find(|operation| operation["status"] == "completed" && operation["result"]["written"] == true)
        .expect("completed write");
    assert_eq!(
        completed["result"]["observedSha256"],
        format!("{:x}", Sha256::digest(b"original")),
        "{operations}"
    );
}

#[test]
fn preflight_guard_stays_selected_after_provider_failover() {
    let _serial = serial();
    let root = temp_root();
    std::fs::write(root.join("guarded.txt"), "old").unwrap();
    let (working_port, _) = scripted_provider(|request| {
        let result = request["messages"].as_array().unwrap().iter().rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str());
        let (id, content) = match result {
            None => ("blind", "discard"),
            Some(result) if result.contains("currentContent") => ("retry", "final"),
            _ => return vec![json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})],
        };
        vec![json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":id,"type":"function","function":{"name":"workspace_write","arguments":
                json!({"path":"guarded.txt","content":content}).to_string()}
        }]},"finish_reason":"tool_calls"}]})]
    });
    let dead_port = free_port();
    let app = TestApp::from_root(dead_port, "native", root, &[("DOMINION_BENCHMARK", "1")]);
    let primary = app.get("/api/providers")[0]["id"].as_str().unwrap().to_owned();
    let primary_base = format!("http://127.0.0.1:{dead_port}/v1");
    let fallback_base = format!("http://127.0.0.1:{working_port}/v1");
    for (id, base, model) in [(&primary[..], &primary_base, "primary"), ("fallback", &fallback_base, "fallback")] {
        app.post("/api/providers", &json!({"id":id,"name":model,"baseUrl":base,
            "model":model,"local":true,"enabled":true,"toolMode":"native"}));
    }
    let guard = app.post("/api/benchmark/policies/state", &json!({
        "slot":"mutation.preflight","name":"shared-file-guard","parameters":{"kernel":"same-file-revision-v2"},
    }));
    let revision = &guard["active"]["revision"];
    assert!(revision.is_string(), "{guard}");
    app.post("/api/policy", &json!({"autoApprove":["workspace_write"]}));
    let stream = app.chat(&json!({"input":"guarded.txt 파일을 수정해.","providerId":primary,"autoLearning":false}));
    assert!(stream.contains("provider_failover"), "{stream}");
    assert_eq!(std::fs::read_to_string(app._root.join("guarded.txt")).unwrap(), "final", "{stream}");
    let run = &app.get("/api/executions")[0];
    assert_eq!(run["effectiveModel"], "fallback", "{run}");
    assert_eq!(&run["preflightPolicy"]["revision"], revision, "{run}");
    let operations = app.get(&format!("/api/executions/{}/operations", run["id"].as_str().unwrap()));
    assert!(operations.as_array().unwrap().iter().any(|op| op["result"]["preflightBlocked"] == true
        && &op["result"]["preflightRevision"] == revision));
}

#[test]
fn per_bot_growth_changes_only_that_bots_next_prompt() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, requests) = scripted_provider(move |_| {
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 | 4 | 5 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let agent_a = app.post("/api/agents", &json!({"name":"growth-a","systemPrompt":"A"}));
    let agent_b = app.post("/api/agents", &json!({"name":"growth-b","systemPrompt":"B"}));
    let a_id = agent_a["id"].as_str().unwrap();
    let b_id = agent_b["id"].as_str().unwrap();
    let session_a = app.post("/api/sessions", &json!({"title":"a","agentId":a_id}));
    let a_session = session_a["id"].as_str().unwrap();
    for index in 1..=2 {
        let _ = app.chat(&json!({
            "sessionId": a_session,
            "agentId": a_id,
            "input": format!("A 실패 작업 {index}"),
            "autoLearning": false
        }));
    }
    let growth = app.get(&format!("/api/agents/{a_id}/growth"));
    assert!(growth["runs"].as_u64().unwrap_or(0) >= 2, "{growth}");
    assert!(!growth["guidance"].as_array().unwrap_or(&Vec::new()).is_empty(), "{growth}");
    let _ = app.chat(&json!({
        "sessionId": a_session,
        "agentId": a_id,
        "input": "A 세 번째 작업",
        "autoLearning": false
    }));
    let session_b = app.post("/api/sessions", &json!({"title":"b","agentId":b_id}));
    let b_session = session_b["id"].as_str().unwrap();
    let _ = app.chat(&json!({
        "sessionId": b_session,
        "agentId": b_id,
        "input": "B 첫 작업",
        "autoLearning": false
    }));
    let requests = requests.lock().unwrap();
    let prompt_for = |needle: &str| -> String {
        requests
            .iter()
            .find_map(|request| {
                let messages = request["messages"].as_array()?;
                let has_user = messages.iter().any(|message| {
                    message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.contains(needle))
                });
                if !has_user {
                    return None;
                }
                Some(
                    messages
                        .iter()
                        .filter_map(|message| message["content"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            })
            .unwrap_or_default()
    };
    let a_prompt = prompt_for("A 세 번째 작업");
    assert!(a_prompt.contains("Agent growth"), "A growth missing: {a_prompt}");
    assert!(
        a_prompt.contains("tool_failed") || a_prompt.contains("shell_exec"),
        "A failure guidance missing: {a_prompt}"
    );
    let b_prompt = prompt_for("B 첫 작업");
    assert!(
        !b_prompt.contains("Agent growth"),
        "B inherited A's growth: {b_prompt}"
    );
}

#[test]
fn allow_all_recovery_algorithm_retries_the_same_failed_call() {
    let _serial = serial();
    let calls = Arc::new(Mutex::new(0u32));
    let counter = calls.clone();
    let (port, _) = scripted_provider(move |request| {
        if request["stream"].as_bool() != Some(true) {
            return vec![json!({"choices":[{"delta":{"content":"{}"},"finish_reason":"stop"}]})];
        }
        let mut count = counter.lock().unwrap();
        *count += 1;
        match *count {
            1 | 2 => vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":format!("fail-{count}"),"type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]})],
            _ => vec![json!({"choices":[{"delta":{"content":"재시도 후 완료."},"finish_reason":"stop"}]})],
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({
            "kind":"recovery.strategy",
            "name":"allow-all",
            "implementation":"allow-all",
            "parameters":{}
        }),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({"input":"같은 실패를 재시도해","autoLearning":false}));
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
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
    assert_eq!(shells, 2, "allow-all did not retry: {operations}");
}

#[test]
fn parallel_reads_combination_executes_independent_reads_in_one_round() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"두 파일을 읽었습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-1","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-2","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"two.txt\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    std::fs::write(app._root.join("two.txt"), "TWO").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt와 two.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("두 파일을 읽었습니다"), "{stream}");
    let executions = app.get("/api/executions");
    let execution = &executions.as_array().unwrap()[0];
    let operations = {
        let execution_id = execution["id"].as_str().unwrap();
        app.get(&format!("/api/executions/{execution_id}/operations"))
    };
    let reads = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["tool"] == "workspace_read" && operation["status"] == "completed")
        .count();
    assert_eq!(reads, 2, "{operations}");
    let synergy = app.get("/api/synergy");
    let combination = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.combination")
        .expect("combination statistic");
    assert_eq!(combination["uses"], 1, "{combination}");
    assert_eq!(combination["successes"], 1, "{combination}");
}

#[test]
fn parallel_reads_answer_malformed_arguments_so_no_tool_call_is_orphaned() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_error = messages.iter().any(|message| {
            message["role"] == "tool"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("invalid tool arguments"))
        });
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_error {
            return vec![json!({"choices":[{"delta":{"content":"malformed-read-was-answered"},"finish_reason":"stop"}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"missing-error"},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-good","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-bad","type":"function","function":{"name":"workspace_read","arguments":"{bad"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("malformed-read-was-answered"), "{stream}");
}

#[test]
fn parallel_reads_counts_a_failed_tool_as_a_failed_combination() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"읽기 시도 완료"},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"read-good","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"one.txt\"}"}},
            {"index":1,"id":"read-missing","type":"function","function":{"name":"workspace_read","arguments":"{\"path\":\"missing.txt\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    std::fs::write(app._root.join("one.txt"), "ONE").unwrap();
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.combination","name":"parallel-reads","implementation":"parallel-reads","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let stream = app.chat(&json!({"input":"one.txt와 missing.txt를 읽어","autoLearning":false}));
    assert!(stream.contains("읽기 시도 완료"), "{stream}");
    let synergy = app.get("/api/synergy");
    let combination = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.combination")
        .expect("combination statistic");
    assert_eq!(combination["uses"], 1, "{combination}");
    assert_eq!(combination["failures"], 1, "{combination}");
    assert_eq!(combination["successes"], 0, "{combination}");
}

#[test]
fn normalize_paths_arguments_run_through_the_common_dispatch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"생성했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\" ./sub//normalized.txt \",\"content\":\"OK\"}"}}
        ]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"tool.arguments","name":"normalize-paths","implementation":"normalize-paths","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({"input":"normalized.txt를 만들어","autoLearning":false}));
    assert_eq!(
        std::fs::read_to_string(app._root.join("sub/normalized.txt")).unwrap_or_default(),
        "OK"
    );
    let synergy = app.get("/api/synergy");
    let arguments = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "tool.arguments")
        .expect("argument statistic");
    assert_eq!(arguments["uses"], 1, "{arguments}");
    assert_eq!(arguments["successes"], 1, "{arguments}");
}

#[test]
fn room_conflict_resolution_prefers_evidence_backed_claim() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let system = request["messages"][0]["content"].as_str().unwrap_or("");
        let reply = if system.contains("활성 에이전트: Alice (id: a)") {
            "workspace_read 결과를 확인하고 작업을 완료했습니다"
        } else {
            "작업이 실패했습니다"
        };
        vec![json!({"choices":[{"delta":{"content":reply},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        app.post(
            "/api/agents",
            &json!({"id":id,"name":name,"systemPrompt":"독립 참가자","enabled":true}),
        );
    }
    let room = app.post(
        "/api/rooms",
        &json!({"name":"충돌 대화","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}),
    );
    let id = room["id"].as_str().unwrap();
    let sent = app.post(
        &format!("/api/rooms/{id}/messages"),
        &json!({"input":"테스트 실행 결과를 알려줘","clientMessageId":"owner-conflict-001"}),
    );
    let job_id = sent["jobId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        let status = job["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed") {
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let after = app.get(&format!("/api/rooms/{id}"));
    let resolution = &after["conflictResolution"];
    assert_eq!(resolution["conflict"], true, "{after}");
    assert_eq!(resolution["outcome"], "success", "{after}");
    assert_eq!(resolution["winner"]["agentId"], "a", "{after}");
    let synergy = app.get("/api/synergy");
    let stat = synergy["algorithmStats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stat| stat["kind"] == "multiagent.conflict")
        .expect("conflict stat");
    assert_eq!(stat["uses"], 1, "{stat}");
    assert_eq!(stat["successes"], 1, "{stat}");
}

#[test]
fn clause_split_decomposition_is_persisted_with_independent_quality() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_request| {
        vec![json!({"choices":[{"delta":{"content":"세 단계로 처리하겠습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let algorithm = app.post(
        "/api/algorithms",
        &json!({"kind":"task.decomposition","name":"clause-split","implementation":"clause-split","parameters":{}}),
    );
    app.post(
        &format!("/api/algorithms/{}/activate", algorithm["id"].as_str().unwrap()),
        &json!({"force": true}),
    );
    let _ = app.chat(&json!({
        "input":"설정 파일을 읽고 그리고 테스트를 실행한 뒤 결과를 보고해",
        "autoLearning":false
    }));
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| {
            execution["decomposition"]["kernel"].as_str() == Some("clause-split")
        })
        .expect("decomposition execution");
    let subtasks = execution["subtasks"].as_array().unwrap();
    assert_eq!(subtasks.len(), 3, "{execution}");
    assert_eq!(execution["decomposition"]["quality"]["orderPreserved"], true);
    assert!(
        execution["decomposition"]["qualityScore"].as_f64().unwrap() >= 0.75,
        "{execution}"
    );
}

#[test]
fn chat_fails_over_from_native_api_to_compat_provider_in_one_turn() {
    let _serial = serial();
    let (fallback_port, fallback_requests) = scripted_provider(|request| {
        let has_native_tools = request.get("tools").is_some();
        assert!(!has_native_tools, "compat provider received native tools: {request}");
        vec![json!({"choices":[{"delta":{"content":"대체 제공자 응답"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(fallback_port, "compat");
    for provider in app.get("/api/providers").as_array().cloned().unwrap_or_default() {
        if let Some(id) = provider["id"].as_str() {
            app.delete(&format!("/api/providers/{id}"));
        }
    }
    let primary = app.post(
        "/api/providers",
        &json!({"name":"cloud-native","baseUrl":"http://127.0.0.1:1/v1","model":"cloud","enabled":true,"toolMode":"native"}),
    );
    let fallback = app.post(
        "/api/providers",
        &json!({"name":"local-compat","baseUrl":format!("http://127.0.0.1:{fallback_port}/v1"),"model":"local","enabled":true,"toolMode":"compat"}),
    );
    let agent = app.post("/api/agents", &json!({
        "name":"failover agent","systemPrompt":"FAILOVER_IDENTITY",
        "providerId":primary["id"]
    }));
    let stream = app.chat(&json!({
        "input":"파일 도구로 상태를 확인해",
        "agentId":agent["id"],
        "providerId":primary["id"],
        "toolsEnabled":true,
        "autoLearning":false
    }));
    assert!(stream.contains("대체 제공자 응답"), "{stream}");
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| {
            execution["effectiveProviderId"].as_str() == fallback["id"].as_str()
        })
        .expect("failover execution");
    assert!(execution["failoverAt"].is_string(), "{execution}");
    let saved = app.get("/api/agents")
        .as_array()
        .unwrap()
        .iter()
        .find(|saved| saved["id"] == agent["id"])
        .cloned()
        .expect("agent survives failover");
    assert_eq!(saved["providerId"], fallback["id"], "agent must store the provider that answered: {saved}");
    assert_eq!(saved["model"], "local", "agent must store the fallback model: {saved}");
    let captured = fallback_requests.lock().unwrap();
    assert!(!captured.is_empty(), "fallback provider was not called");
    assert!(captured.iter().all(|body| body.get("tools").is_none()));
    assert!(
        captured[0]["messages"][0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("Available tools:"),
        "{}",
        captured[0]
    );
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// Minimal OpenAI-compatible SSE server.
fn start_mock_provider() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let _ = handle(stream);
        }
    });
    port
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    // Read headers.
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer).to_string();
    let length = text
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let header_end = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(buffer.len());
    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    if text.contains("/chat/completions") {
        // Slow enough that a second concurrent turn overlaps the first.
        std::thread::sleep(Duration::from_millis(600));
    }
    let body = if text.contains("/chat/completions") {
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello-e2e\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"total_tokens\":11}}\n\n\
         data: [DONE]\n\n"
    } else {
        "{}"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

fn wait_health(port: u16, seconds: u64) {
    let client = reqwest::blocking::Client::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/api/health"))
            .timeout(Duration::from_secs(1))
            .send()
        {
            if response.status().is_success() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("server did not become healthy");
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn temp_root() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("dominion-e2e-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("web")).expect("web dir");
    std::fs::write(dir.join("web/index.html"), "<html>ok</html>").expect("index");
    dir
}

#[test]
fn chat_sse_persistence_and_origin_guard() {
    let _serial = serial();
    let provider_port = start_mock_provider();
    let port = free_port();
    let root = temp_root();
    let binary = env!("CARGO_BIN_EXE_dominion-rust");

    let child = Command::new(binary)
        .env("DOMINION_PORT", port.to_string())
        .env("DOMINION_DATA", root.join("runtime"))
        .env("DOMINION_WEB", root.join("web"))
        .env("DOMINION_WORKSPACE", &root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let _guard = ChildGuard(child);
    wait_health(port, 20);

    let client = reqwest::blocking::Client::new();
    let base = format!("http://127.0.0.1:{port}");

    let health: serde_json::Value = client
        .get(format!("{base}/api/health"))
        .send()
        .expect("health")
        .json()
        .expect("health json");
    assert_eq!(health["runtime"], "rust");

    let provider: serde_json::Value = client
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "mock",
            "baseUrl": format!("http://127.0.0.1:{provider_port}/v1"),
            "apiKey": "test",
            "model": "mock",
            "enabled": true
        }))
        .send()
        .expect("provider")
        .json()
        .expect("provider json");
    assert!(provider["hasKey"].as_bool().unwrap_or(true));

    let session: serde_json::Value = client
        .post(format!("{base}/api/sessions"))
        .json(&serde_json::json!({ "title": "e2e" }))
        .send()
        .expect("session")
        .json()
        .expect("session json");
    let session_id = session["id"].as_str().expect("session id").to_string();

    let stream = client
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "sessionId": session_id,
            "input": "ping",
            "toolsEnabled": false
        }))
        .send()
        .expect("chat")
        .text()
        .expect("chat body");
    assert!(stream.contains("hello-e2e"), "stream: {stream}");
    assert!(stream.contains("\"done\""));

    let messages: serde_json::Value = client
        .get(format!("{base}/api/sessions/{session_id}/messages"))
        .send()
        .expect("messages")
        .json()
        .expect("messages json");
    let roles = messages
        .as_array()
        .expect("array")
        .iter()
        .map(|row| row["role"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    assert_eq!(roles, vec!["user", "assistant"]);

    let executions: serde_json::Value = client
        .get(format!("{base}/api/executions"))
        .send()
        .expect("executions")
        .json()
        .expect("executions json");
    assert_eq!(executions[0]["tokens"], 11);

    // Cross-origin writes are rejected; same-origin writes are allowed.
    let evil = client
        .post(format!("{base}/api/policy"))
        .header("Origin", "http://evil.example")
        .json(&serde_json::json!({ "autoApproveAll": true }))
        .send()
        .expect("evil");
    assert_eq!(evil.status(), reqwest::StatusCode::FORBIDDEN);

    let local = client
        .post(format!("{base}/api/policy"))
        .header("Origin", format!("http://127.0.0.1:{port}"))
        .json(&serde_json::json!({ "autoApproveAll": false }))
        .send()
        .expect("local");
    assert_eq!(local.status(), reqwest::StatusCode::OK);
}

#[test]
fn concurrent_turns_on_one_session_are_rejected() {
    let _serial = serial();
    let provider_port = start_mock_provider();
    let port = free_port();
    let root = temp_root();
    let binary = env!("CARGO_BIN_EXE_dominion-rust");

    let child = Command::new(binary)
        .env("DOMINION_PORT", port.to_string())
        .env("DOMINION_DATA", root.join("runtime"))
        .env("DOMINION_WEB", root.join("web"))
        .env("DOMINION_WORKSPACE", &root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let _guard = ChildGuard(child);
    wait_health(port, 20);

    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    client
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "mock",
            "baseUrl": format!("http://127.0.0.1:{provider_port}/v1"),
            "apiKey": "test",
            "model": "mock",
            "enabled": true
        }))
        .send()
        .expect("provider");
    let session: serde_json::Value = client
        .post(format!("{base}/api/sessions"))
        .json(&serde_json::json!({ "title": "race" }))
        .send()
        .expect("session")
        .json()
        .expect("json");
    let session_id = session["id"].as_str().unwrap().to_string();

    let base_a = base.clone();
    let session_a = session_id.clone();
    let first = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base_a}/api/chat"))
            .json(&serde_json::json!({
                "sessionId": session_a, "input": "one", "toolsEnabled": false
            }))
            .send()
            .map(|response| response.text().unwrap_or_default())
            .unwrap_or_default()
    });
    std::thread::sleep(Duration::from_millis(30));
    let second = client
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "sessionId": session_id, "input": "two", "toolsEnabled": false
        }))
        .send()
        .map(|response| response.text().unwrap_or_default())
        .unwrap_or_default();
    let first = first.join().unwrap_or_default();

    let combined = format!("{first}{second}");
    assert!(
        combined.contains("Session is already running"),
        "expected a duplicate-session rejection, got: {combined}"
    );
}

#[test]
fn ordinary_chat_rejects_word_only_completion() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"gate-claim.txt 파일을 저장했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"claim-only"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-claim.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("실행 증거가 부족"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    assert!(!app._root.join("gate-claim.txt").exists());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn ordinary_chat_accepts_real_tool_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let saw_tool = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        if saw_tool {
            vec![json!({"choices":[{"delta":{"content":"파일을 만들고 확인했습니다."},"finish_reason":"stop"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write-1","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"gate-real.txt\",\"content\":\"evidence\"}"}}]},"finish_reason":"tool_calls"}]})]
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"real-evidence"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-real.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-real.txt")).unwrap(),
        "evidence"
    );
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let final_roles = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .count();
    assert!(final_roles >= 1);
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn native_job_prompt_runs_and_client_cannot_forge_status() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"job-done"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let created = app.post(
        "/api/jobs",
        &json!({
            "kind": "prompt",
            "input": {"prompt": "job prompt", "toolsEnabled": false},
            "status": "completed",
            "result": {"forged": true}
        }),
    );
    assert_eq!(created["status"], "queued");
    assert!(created["result"].is_null());
    let id = created["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut job = created;
    while Instant::now() < deadline {
        job = app.get(&format!("/api/jobs/{id}"));
        if matches!(job["status"].as_str(), Some("completed" | "failed")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    assert_eq!(job["status"], "completed", "{job}");
    let session_id = job["result"]["sessionId"]
        .as_str()
        .expect("job result session")
        .to_string();
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    assert!(messages
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["role"] == "assistant" && row["content"] == "job-done"));
}

#[test]
fn executions_are_read_only_and_scoped_cancel_only_hits_running() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"exec-done"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let forged = app
        .client
        .post(format!("{}/api/executions", app.base))
        .json(&json!({"status":"completed","tokens":999,"evidence":[{"fake":true}]}))
        .send()
        .unwrap();
    assert!(
        matches!(
            forged.status(),
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
        ),
        "unexpected status {}",
        forged.status()
    );
    assert_eq!(app.get("/api/executions").as_array().unwrap().len(), 0);

    let session = app.post("/api/sessions", &json!({"title":"exec"}));
    app.chat(&json!({"sessionId":session["id"],"input":"hello","autoLearning":false}));
    let executions = app.get("/api/executions");
    let execution = &executions[0];
    assert_eq!(execution["status"], "completed");
    let cancel = app
        .client
        .post(format!(
            "{}/api/executions/{}/cancel",
            app.base,
            execution["id"].as_str().unwrap()
        ))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancel.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn cancelling_a_running_execution_marks_it_cancelled() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(4));
        vec![json!({"choices":[{"delta":{"content":"late"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"exec-cancel"}));
    let base = app.base.clone();
    let chat_id = session["id"].as_str().unwrap().to_string();
    let turn = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base}/api/chat"))
            .json(&json!({"sessionId":chat_id,"input":"slow","autoLearning":false}))
            .send()
            .unwrap()
            .text()
            .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut execution_id = None;
    while Instant::now() < deadline {
        let executions = app.get("/api/executions");
        if let Some(execution) = executions
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["status"] == "running")
        {
            execution_id = execution["id"].as_str().map(str::to_string);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let id = execution_id.expect("running execution");
    let cancelled = app
        .client
        .post(format!("{}/api/executions/{id}/cancel", app.base))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
    let stream = turn.join().unwrap();
    assert!(stream.contains("중단") || stream.contains("error"), "{stream}");
    let stored = app.get(&format!("/api/executions/{id}"));
    assert_eq!(stored["status"], "cancelled");
}

