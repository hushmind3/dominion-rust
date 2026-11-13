#[test]
fn cancelling_a_running_job_stops_it() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(4));
        vec![json!({"choices":[{"delta":{"content":"late job"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let created = app.post(
        "/api/jobs",
        &json!({"kind":"prompt","input":{"prompt":"slow job","toolsEnabled":false}}),
    );
    let id = created["id"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut running = false;
    while Instant::now() < deadline {
        let job = app.get(&format!("/api/jobs/{id}"));
        if job["status"] == "running" {
            running = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(running, "job never started");
    let cancelled = app
        .client
        .post(format!("{}/api/jobs/{id}/cancel", app.base))
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
    std::thread::sleep(Duration::from_millis(800));
    let job = app.get(&format!("/api/jobs/{id}"));
    assert_eq!(job["status"], "cancelled", "{job}");
    assert_eq!(job["cancelRequested"], true);
}

#[test]
fn room_manual_learning_enqueues_each_participant_with_its_own_watermark() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"참가자 발언"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let agent_a = app.post(
        "/api/agents",
        &json!({"name":"A","systemPrompt":"A","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let agent_b = app.post(
        "/api/agents",
        &json!({"name":"B","systemPrompt":"B","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let room = app.post(
        "/api/rooms",
        &json!({"name":"learn-room","agentIds":[agent_a,agent_b],"rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}),
    );
    let room_id = room["id"].as_str().unwrap().to_string();
    let submitted = app.post(
        &format!("/api/rooms/{room_id}/messages"),
        &json!({"input":"참가자별 학습 확인"}),
    );
    let job_id = submitted["jobId"].as_str().unwrap().to_string();
    let session_id = submitted["sessionId"].as_str().unwrap().to_string();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut status = String::new();
    while Instant::now() < deadline {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        status = job["status"].as_str().unwrap_or("").to_string();
        if matches!(status.as_str(), "completed" | "failed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(status, "completed", "room job did not finish");
    let messages = app.get(&format!("/api/sessions/{session_id}/messages"));
    let last_for = |agent: &str| {
        messages
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|message| message["agent_id"].as_str() == Some(agent))
            .and_then(|message| message["id"].as_str().map(str::to_string))
    };
    let through_a = last_for(&agent_a).expect("agent A spoke");
    let through_b = last_for(&agent_b).expect("agent B spoke");

    let learn = app.post(&format!("/api/sessions/{session_id}/learn"), &json!({}));
    assert_eq!(learn["status"], "queued", "{learn}");
    let jobs = learn["jobs"].as_array().expect("per-agent jobs");
    assert_eq!(jobs.len(), 2, "{learn}");
    for job in jobs {
        let agent = job["agentId"].as_str().unwrap();
        let expected = if agent == agent_a { &through_a } else { &through_b };
        let record = app.get(&format!("/api/jobs/{}", job["jobId"].as_str().unwrap()));
        assert_eq!(record["input"]["agentId"].as_str().unwrap(), agent);
        assert_eq!(record["input"]["throughMessageId"].as_str().unwrap(), expected);
        assert_eq!(record["input"]["providerId"].as_str().unwrap(), provider);
    }
}

#[test]
fn token_budget_is_enforced_from_stream_usage() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        if request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
        {
            return vec![json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})];
        }
        vec![json!({
            "choices":[{"delta":{"content":"big"},"finish_reason":"stop"}],
            "usage":{"total_tokens":300000}
        })]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"budget"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "budget check",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("토큰 예산"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn reviewer_repairs_a_claim_with_a_real_tool() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let has_tools = request.get("tools").and_then(|tools| tools.as_array()).is_some();
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");

        if !has_tools {
            if system.contains("strict reviewer") {
                return vec![json!({"choices":[{"delta":{"content":"{\"complete\":false,\"gaps\":[\"gate-review.txt가 없습니다\"],\"nextAction\":\"workspace_write로 gate-review.txt를 생성\"}"}}]})];
            }
            return vec![json!({"choices":[{"delta":{"content":"1. gate-review.txt 생성\n2. 존재 확인"}}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"파일을 만들고 확인했습니다."},"finish_reason":"stop"}]})];
        }
        if last_user.contains("다음 전략") || last_user.contains("자동 검증") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"review-write","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"gate-review.txt\",\"content\":\"reviewed\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"gate-review.txt 파일을 저장했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"reviewer-repair"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-review.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-review.txt")).unwrap(),
        "reviewed"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
    let improvements = app.get("/api/improvements");
    assert!(
        improvements.as_array().unwrap().iter().any(|item| {
            item["kind"] == "strategy"
                && item["status"] == "adopted"
                && item["text"].as_str().unwrap_or("").contains("workspace_write")
        }),
        "reviewer strategy was not adopted: {improvements}"
    );
}

#[test]
fn verified_work_completes_without_waiting_for_the_model_to_stop() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let has_tools = request.get("tools").and_then(|tools| tools.as_array()).is_some();
        if !has_tools {
            if system.contains("planner") {
                return vec![json!({"choices":[{"delta":{"content":"1. 파일 생성\\n2. 확인"}}]})];
            }
            return vec![json!({"choices":[{"delta":{"content":"완료했습니다."},"finish_reason":"stop"}]})];
        }
        let arguments = json!({"path":"gate-limit.txt","content":"limit-ok"}).to_string();
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write-limit","type":"function","function":{"name":"workspace_write","arguments":arguments}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"post-tool"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "gate-limit.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("모델 반복 제한"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("gate-limit.txt")).unwrap(),
        "limit-ok"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn compat_mode_executes_a_bare_json_tool_call_from_a_weak_model() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let saw_tool = messages.iter().any(|message| {
            message["role"] == "tool"
                || message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("Tool observation"))
        });
        if system.contains("planner") {
            return vec![json!({"choices":[{"delta":{"content":"1. weak.txt 생성\\n2. 확인"}}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"파일을 실제로 만들었습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"{\"tool\":\"workspace_write\",\"args\":{\"path\":\"weak.txt\",\"content\":\"4b-ok\"}}"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "compat");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"weak-model"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "weak.txt 파일을 만들어줘",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("weak.txt")).unwrap(),
        "4b-ok"
    );
    assert_eq!(app.get("/api/executions")[0]["status"], "completed");
}

#[test]
fn delegate_task_runs_a_real_child_agent_and_returns_its_result() {
    let _serial = serial();
    use std::sync::{Arc, Mutex};
    let child_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let slot = child_slot.clone();
    let (port, _) = scripted_provider(move |request| {
        let messages = request["messages"].as_array().unwrap();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
        if system.contains("CHILD-AGENT") {
            return vec![json!({"choices":[{"delta":{"content":"child result 42"},"finish_reason":"stop"}]})];
        }
        if saw_tool {
            return vec![json!({"choices":[{"delta":{"content":"자식 결과를 받아 완료했습니다."},"finish_reason":"stop"}]})];
        }
        let child = slot.lock().unwrap().clone().unwrap_or_default();
        let arguments = json!({
            "agentId": child,
            "prompt": "compute the answer",
            "wait": true,
            "timeoutSeconds": 60
        })
        .to_string();
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"delegate-1","type":"function","function":{"name":"delegate_task","arguments":arguments}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    let provider = app.get("/api/providers")[0]["id"].as_str().unwrap().to_string();
    let parent = app.post(
        "/api/agents",
        &json!({"name":"Parent","systemPrompt":"PARENT-AGENT","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let child = app.post(
        "/api/agents",
        &json!({"name":"Child","systemPrompt":"CHILD-AGENT","providerId":provider,"model":"mock"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    *child_slot.lock().unwrap() = Some(child.clone());

    let session = app.post(
        "/api/sessions",
        &json!({"title":"delegation","agentId":parent}),
    );
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "agentId": parent,
        "input": "자식 에이전트에게 계산을 위임해",
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    assert!(!stream.contains("실행 증거가 부족"), "{stream}");
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let tool_text = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| message["content"].as_str().unwrap_or(""))
        .collect::<String>();
    assert!(tool_text.contains("child result 42"), "{tool_text}");
    let child_sessions = app.get("/api/sessions");
    let child_session = child_sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"].as_str().unwrap_or("").starts_with("Delegate ·"))
        .expect("child session");
    let child_messages = app.get(&format!(
        "/api/sessions/{}/messages",
        child_session["id"].as_str().unwrap()
    ));
    assert!(child_messages
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "assistant"
            && message["content"] == "child result 42"));
    assert!(app
        .get("/api/jobs")
        .as_array()
        .unwrap()
        .iter()
        .any(|job| job["kind"] == "prompt" && job["status"] == "completed"));
}

#[test]
fn correction_turn_failure_still_enqueues_reflection() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let is_reflection = messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("실패를 분석하는 회고 분석기")
        });
        if is_reflection {
            return vec![json!({"choices":[{"delta":{"content":
                "{\"rootCause\":\"도구를 쓰지 않고 말로만 끝냈다\",\"expected\":\"파일 생성\",\"actual\":\"설명만 함\",\"lesson\":\"행동 요청은 도구 결과로 검증한다\",\"avoid\":\"도구 호출 없이 완료 선언\",\"strategy\":null,\"skill\":null,\"memory\":null}"
            },"finish_reason":"stop"}]})];
        }
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("")
            .to_string();
        if last_user.contains("틀렸") {
            // The corrective turn itself fails: reflection must still be queued.
            vec![json!({"choices":[{"delta":{},"finish_reason":"content_filter"}]})]
        } else {
            vec![json!({"choices":[{"delta":{"content":"설명만 했습니다."},"finish_reason":"stop"}]})]
        }
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"reflect on failure"}));
    let session_id = session["id"].as_str().unwrap();
    let first = app.chat(&json!({
        "sessionId": session_id, "input": "러스트의 장점을 한 문장으로 말해",
        "toolsEnabled": false, "autoLearning": false
    }));
    assert!(first.contains("assistant_saved"), "{first}");

    let second = app.chat(&json!({
        "sessionId": session_id, "input": "아니 그게 아니라, 틀렸어. 다시 해.",
        "toolsEnabled": false, "autoLearning": true
    }));
    assert!(second.contains("error"), "{second}");

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reflect_status = String::new();
    while Instant::now() < deadline {
        if let Some(job) = app
            .get("/api/jobs")
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["kind"] == "reflect")
        {
            reflect_status = job["status"].as_str().unwrap_or("").to_string();
            if matches!(reflect_status.as_str(), "completed" | "failed") {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        reflect_status, "completed",
        "reflection did not complete after failed correction turn"
    );
    let experience = app.get("/api/experience");
    assert!(
        !experience.as_array().unwrap().is_empty(),
        "reflection produced no experience"
    );
}

#[test]
fn completed_chat_without_verification_is_not_a_success() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"안녕하세요."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"plain chat"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "간단히 인사해",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions[0]["status"], "completed");
    assert_eq!(executions[0]["taskOutcome"], "answered_unverified");
    assert!(app.get("/api/experience").as_array().unwrap().is_empty());
}

#[test]
fn tools_disabled_action_is_failed_not_succeeded() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"만들었습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"disabled tools"}));
    let stream = app.chat(&json!({
        "sessionId": session["id"],
        "input": "report.txt 파일을 만들어줘",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(stream.contains("error"), "{stream}");
    let executions = app.get("/api/executions");
    assert_eq!(executions[0]["status"], "failed");
    assert_eq!(executions[0]["taskOutcome"], "failed");
}

#[test]
fn benchmark_identity_reports_the_isolated_roots() {
    let _serial = serial();
    let port = start_mock_provider();
    let app = TestApp::new_with_env(
        port,
        "native",
        &[
            ("DOMINION_BENCHMARK", "1"),
            ("DOMINION_ENGINE_INSTANCE", "bench-test"),
        ],
    );
    let identity = app.get("/api/benchmark/identity");
    assert_eq!(identity["benchmark"], true);
    assert_eq!(identity["instanceId"], "bench-test");
    let workspace = app._root.canonicalize().unwrap();
    assert_eq!(
        identity["workspace"].as_str().unwrap(),
        workspace.to_string_lossy()
    );
    let data = app._root.join("runtime").canonicalize().unwrap();
    assert_eq!(
        identity["dataRoot"].as_str().unwrap(),
        data.to_string_lossy()
    );
}

#[test]
fn procedure_violation_blocks_the_target_and_repairs_with_the_prerequisite() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("회고 분석기") {
            let reflection = json!({
                "rootCause": "audit를 먼저 만들지 않고 report를 작성했다",
                "expected": "audit 먼저",
                "actual": "report만 작성",
                "lesson": "report를 쓰기 전에 audit을 먼저 쓴다",
                "avoid": "선행 작업을 생략하지 않는다",
                "strategy": {
                    "text": "report를 쓰기 전에 audit을 먼저 쓴다",
                    "rationale": "주인의 교정에서 확인된 선행 조건",
                    "verification": {
                        "type": "procedure",
                        "enforce": true,
                        "appliesTo": { "requestContains": ["report.txt"] },
                        "before": [{
                            "when": { "tool": "workspace_write", "path": "report.txt" },
                            "require": { "tool": "workspace_write", "path": "audit.txt" }
                        }],
                        "after": [{
                            "kind": "file_contains",
                            "path": "audit.txt",
                            "text": "audit-ok"
                        }]
                    }
                },
                "skill": null,
                "memory": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":reflection},"finish_reason":"stop"}]})];
        }
        if system.contains("증거 기반 학습기") {
            let empty = json!({
                "memories": [],
                "skill": null,
                "strategies": [],
                "toolPolicies": [],
                "harnessProposal": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":empty},"finish_reason":"stop"}]})];
        }
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "user")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_user.contains("아니") {
            return vec![json!({"choices":[{"delta":{"content":"알겠습니다."},"finish_reason":"stop"}]})];
        }
        let last_tool = messages
            .iter()
            .rev()
            .find(|message| message["role"] == "tool")
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if last_tool.contains("procedureViolation") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"audit-call","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"audit.txt\",\"content\":\"audit-ok\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("audit.txt") && !last_tool.contains("report.txt") {
            return vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"report-call","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"report.txt\",\"content\":\"ok\"}"}}]},"finish_reason":"tool_calls"}]})];
        }
        if last_tool.contains("report.txt") {
            return vec![json!({"choices":[{"delta":{"content":"절차를 지켜 완료했습니다."},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"first-report","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"report.txt\",\"content\":\"ok\"}"}}]},"finish_reason":"tool_calls"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll": true}));

    let session = app.post("/api/sessions", &json!({"title":"procedure"}));
    let session_id = session["id"].as_str().unwrap();
    let baseline = app.chat(&json!({
        "sessionId": session_id,
        "input": "report.txt 파일을 만들고 내용을 ok로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(baseline.contains("assistant_saved"), "{baseline}");

    let correction = app.chat(&json!({
        "sessionId": session_id,
        "input": "아니, report.txt를 쓰기 전에 audit.txt를 먼저 만들어야 해. 다시 해.",
        "toolsEnabled": false,
        "autoLearning": true
    }));
    assert!(correction.contains("error"), "{correction}");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reflected = false;
    while Instant::now() < deadline {
        if app
            .get("/api/jobs")
            .as_array()
            .unwrap()
            .iter()
            .any(|job| job["kind"] == "reflect" && job["status"] == "completed")
        {
            reflected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(reflected, "reflection did not complete");

    let _ = std::fs::remove_file(app._root.join("report.txt"));
    let _ = std::fs::remove_file(app._root.join("audit.txt"));
    let after = app.post("/api/sessions", &json!({"title":"procedure after"}));
    let after_id = after["id"].as_str().unwrap();
    let stream = app.chat(&json!({
        "sessionId": after_id,
        "input": "report.txt 파일을 만들고 내용을 ok로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(
        stream.contains("절차 위반") || stream.contains("procedureViolation"),
        "{stream}"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("audit.txt")).unwrap_or_default(),
        "audit-ok"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("report.txt")).unwrap_or_default(),
        "ok"
    );
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == after_id)
        .expect("post execution");
    assert_eq!(execution["taskOutcome"], "succeeded", "{}", execution);
    let compliance = execution["strategyCompliance"].as_array().expect("compliance");
    assert!(compliance.iter().any(|item| {
        item["applicable"] == true
            && item["result"]["verified"] == true
            && item["result"]["followed"] == true
    }));
}

#[test]
fn harness_assisted_executes_dag_nodes_through_the_common_dispatch() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let messages = request["messages"].as_array().cloned().unwrap_or_default();
        let system = messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.contains("회고 분석기") {
            let reflection = json!({
                "rootCause": "선행 audit 없이 report를 작성했다",
                "expected": "audit 먼저",
                "actual": "report만 작성",
                "lesson": "harness 절차로 audit 후 report를 실행한다",
                "avoid": "선행 작업 생략",
                "strategy": {
                    "text": "harness 절차로 audit 후 report를 실행한다",
                    "rationale": "주인의 교정에서 확인된 선행 조건",
                    "verification": {
                        "type":"procedure",
                        "appliesTo":{"paths":["harness_report.txt"]},
                        "bindings":{},
                        "nodes":[
                            {"id":"audit","tool":"workspace_write","arguments":{"path":"harness_audit.txt","content":"audit-harness"}},
                            {"id":"report","tool":"workspace_write","arguments":{"path":"harness_report.txt","content":"REPORT-OK"}}
                        ],
                        "edges":[{"from":"audit","to":"report"}],
                        "after":[{"kind":"file_contains","path":"harness_audit.txt","text":"audit-harness"}]
                    }
                },
                "skill": null,
                "memory": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":reflection},"finish_reason":"stop"}]})];
        }
        if system.contains("증거 기반 학습기") {
            let empty = json!({
                "memories": [], "skill": null, "strategies": [],
                "toolPolicies": [], "harnessProposal": null
            })
            .to_string();
            return vec![json!({"choices":[{"delta":{"content":empty},"finish_reason":"stop"}]})];
        }
        vec![json!({"choices":[{"delta":{"content":"확인했습니다."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll": true}));

    let session = app.post("/api/sessions", &json!({"title":"harness setup"}));
    let session_id = session["id"].as_str().unwrap();
    app.chat(&json!({
        "sessionId": session_id,
        "input": "간단히 설명해",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    let correction = app.chat(&json!({
        "sessionId": session_id,
        "input": "아니, harness_report.txt를 만들기 전에 harness_audit.txt를 먼저 만들어. 다시 해.",
        "toolsEnabled": false,
        "autoLearning": true
    }));
    assert!(correction.contains("error"), "{correction}");
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if app.get("/api/jobs").as_array().unwrap().iter().any(|job| {
            job["kind"] == "reflect" && job["status"] == "completed"
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = std::fs::remove_file(app._root.join("harness_report.txt"));
    let _ = std::fs::remove_file(app._root.join("harness_audit.txt"));
    let disabled = app.post("/api/sessions", &json!({"title":"harness disabled"}));
    let disabled_id = disabled["id"].as_str().unwrap();
    let disabled_stream = app.chat(&json!({
        "sessionId": disabled_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": false,
        "autoLearning": false
    }));
    assert!(!disabled_stream.contains("error"), "{disabled_stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_audit.txt")).unwrap(),
        "audit-harness"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_report.txt")).unwrap(),
        "REPORT-OK"
    );
    let executions = app.get("/api/executions");
    let disabled_execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == disabled_id)
        .expect("disabled execution");
    assert_eq!(disabled_execution["taskOutcome"], "succeeded");
    let disabled_operations = app.get(&format!(
        "/api/executions/{}/operations",
        disabled_execution["id"].as_str().unwrap()
    ));
    assert!(disabled_operations
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| operation["origin"] == "procedure"));

    let _ = std::fs::remove_file(app._root.join("harness_report.txt"));
    let _ = std::fs::remove_file(app._root.join("harness_audit.txt"));
    let blocked = app.post("/api/sessions", &json!({"title":"harness execution blocked"}));
    let blocked_id = blocked["id"].as_str().unwrap();
    let blocked_stream = app.chat(&json!({
        "sessionId": blocked_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": true,
        "toolsExecutionEnabled": false,
        "autoLearning": false
    }));
    assert!(blocked_stream.contains("error"), "{blocked_stream}");
    assert!(!app._root.join("harness_report.txt").exists());
    assert!(!app._root.join("harness_audit.txt").exists());
    let executions = app.get("/api/executions");
    let blocked_execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == blocked_id)
        .expect("blocked execution");
    let blocked_operations = app.get(&format!(
        "/api/executions/{}/operations",
        blocked_execution["id"].as_str().unwrap()
    ));
    assert!(!blocked_operations
        .as_array()
        .unwrap()
        .iter()
        .any(|operation| operation["origin"] == "procedure"));

    let post = app.post("/api/sessions", &json!({"title":"harness post"}));
    let post_id = post["id"].as_str().unwrap();
    let stream = app.chat(&json!({
        "sessionId": post_id,
        "input": "harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.",
        "toolsEnabled": true,
        "autoLearning": false
    }));
    assert!(!stream.contains("error"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_audit.txt")).unwrap(),
        "audit-harness"
    );
    assert_eq!(
        std::fs::read_to_string(app._root.join("harness_report.txt")).unwrap(),
        "REPORT-OK"
    );
    let executions = app.get("/api/executions");
    let execution = executions
        .as_array()
        .unwrap()
        .iter()
        .find(|execution| execution["sessionId"] == post_id)
        .expect("post execution");
    assert_eq!(execution["taskOutcome"], "succeeded");
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        execution["id"].as_str().unwrap()
    ));
    let nodes = operations
        .as_array()
        .unwrap()
        .iter()
        .filter(|operation| operation["origin"] == "procedure")
        .filter_map(|operation| operation["procedureNodeId"].as_str())
        .collect::<Vec<_>>();
    assert!(nodes.contains(&"audit"));
    assert!(nodes.contains(&"report"));
}

fn measure_estimate(text: &str) -> usize {
    let ascii = text.chars().filter(char::is_ascii).count();
    ascii.div_ceil(4) + text.chars().count() - ascii
}

fn rsi_context_session(app:&TestApp)->Value {
    std::fs::create_dir_all(app._root.join("src")).unwrap();
    std::fs::write(app._root.join("src/target.rs"),"pub fn target() {}\n").unwrap();
    let session=app.post("/api/sessions",&json!({"title":"bounded RSI context"}));
    let c=Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    let mut value=session.clone();
    value["rsiRun"]=json!(true);value["rsiObjective"]=json!("기억 검색 개선의 근거를 확인하고, 근거가 없으면 변경 없이 보고하세요.");
    value["rsiCodeIndexId"]=json!("fixture-index");value["rsiSourceDir"]=json!(app._root);
    for (kind,id,data) in [
        ("session",session["id"].as_str().unwrap(),value),
        ("code_index","fixture-index",json!({"id":"fixture-index","sourceDir":app._root,"files":1})),
        ("code_index_file","fixture-file",json!({"id":"fixture-file","indexId":"fixture-index","path":"src/target.rs","symbols":[{"name":"target","kind":"fn","line":1}]})),
    ] {c.execute("INSERT OR REPLACE INTO records(kind,id,data) VALUES(?1,?2,?3)",rusqlite::params![kind,id,data.to_string()]).unwrap();}
    session
}

#[test]
fn rsi_context_is_not_reinterpreted_as_tasks_or_forced_into_a_write() {
    let _serial=serial();let calls=Arc::new(std::sync::atomic::AtomicUsize::new(0));let counter=calls.clone();
    let (port,requests)=scripted_provider(move |body|{
        if body["stream"]!=true {return vec![json!({"choices":[{"delta":{"content":"{}"},"finish_reason":"stop"}]})];}
        let n=counter.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        let delta=match n {
            0=>json!({"tool_calls":(["target","fn target","target missing","missing target"].iter().enumerate().map(|(i,q)|json!({"index":i,"id":format!("search-{i}"),"type":"function","function":{"name":"code_search","arguments":json!({"query":q}).to_string()}})).collect::<Vec<_>>())}),
            1=>json!({"tool_calls":(0..2).map(|i|json!({"index":i,"id":format!("read-after-four-searches-{i}"),"type":"function","function":{"name":"workspace_read_range","arguments":"{\"path\":\"src/target.rs\",\"startLine\":1,\"endLine\":1}"}})).collect::<Vec<_>>()}),
            _=>json!({"content":"근거가 없어 변경 없이 마칩니다."}),
        };
        vec![json!({"choices":[{"delta":delta,"finish_reason":if n<2 {"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110}})]
    });
    let app=TestApp::new(port,"native");let session=rsi_context_session(&app);
    let input=format!("코드와 지도는 참고 자료입니다. SOURCE_CONTEXT_ONCE\n{}",(0..800).map(|i|format!("{i}. quoted fn source_{i}() {{ unwrap(); }}\n")).collect::<String>());
    let stream=app.chat(&json!({"sessionId":session["id"],"input":input,"toolsEnabled":true,"autoLearning":false}));
    assert!(stream.contains("변경 없이"),"{stream}");
    let seen=requests.lock().unwrap();assert_eq!(seen.len(),3,"no hidden planning request: {}",seen.len());
    for body in seen.iter() {
        assert_eq!(body["stream"],true);
        assert_eq!(body["messages"].to_string().matches("SOURCE_CONTEXT_ONCE").count(),1);
        assert!(body["messages"].as_array().unwrap().iter().all(|row|!row["content"].as_str().unwrap_or("").contains("조사는 충분합니다")));
    }
    assert!(seen[1]["tools"].as_array().unwrap().iter().any(|tool|tool["function"]["name"]=="workspace_read_range"));
    let finishing=seen[2]["tools"].as_array().unwrap();
    for proposal in ["rsi_method_propose","algorithm_propose"] {
        assert!(finishing.iter().any(|tool|tool["function"]["name"]==proposal),"research must be able to finish with a reusable method, not only a source write");
    }
    assert!(!finishing.iter().any(|tool|tool["function"]["name"]=="code_search"));
    assert_eq!(std::fs::read_to_string(app._root.join("src/target.rs")).unwrap(),"pub fn target() {}\n");
    let c=Connection::open(app._root.join("runtime/dominion.sqlite")).unwrap();
    let raw:String=c.query_row("SELECT data FROM records WHERE kind='execution' AND json_extract(data,'$.sessionId')=?1",[session["id"].as_str().unwrap()],|r|r.get(0)).unwrap();
    let execution:Value=serde_json::from_str(&raw).unwrap();
    assert!(execution["subtasks"].as_array().unwrap().len()<5,"source text is not a work list");
    assert!(execution["plan"].is_null());assert_eq!(execution["toolCalls"],6);
}

#[test]
fn rsi_turn_budget_blocks_the_next_request_before_spending_it() {
    let _serial=serial();let (port,requests)=scripted_provider(|_|vec![json!({
        "choices":[{"delta":{"tool_calls":[{"index":0,"id":"search","type":"function","function":{"name":"code_search","arguments":"{\"query\":\"target\"}"}}]},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":249000,"completion_tokens":100,"total_tokens":249100}
    })]);
    let app=TestApp::new(port,"native");let session=rsi_context_session(&app);
    let stream=app.chat(&json!({"sessionId":session["id"],"input":"참고 코드 ".repeat(2000),"toolsEnabled":true,"autoLearning":false}));
    assert!(stream.contains("다음 호출 전에 중단"),"{stream}");
    assert_eq!(requests.lock().unwrap().len(),1,"do not discover an exhausted budget after paying for another call");
}

/// Research measurement, not a behavioural guarantee: shows exactly what the
/// harness sends per participant across three owner turns in a nine-person room.
#[test]
fn measure_room_input_growth_across_three_turns() {
    let _serial = serial();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let reply_counter = counter.clone();
    let (port, requests) = scripted_provider(move |_| {
        let n = reply_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        vec![json!({
            "choices":[{"delta":{"content":format!("응답 {n}")},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"total_tokens":105}
        })]
    });
    let root = temp_root();
    let app = TestApp::from_root(port, "native", root.clone(), &[]);
    let persona = "지침 문장입니다. ".repeat(220);
    let mut ids = Vec::new();
    for index in 1..=9 {
        let agent = app.post("/api/agents", &json!({
            "id":format!("m{index}"),
            "name":format!("참가자{index}"),
            "systemPrompt":format!("{persona}나는 참가자{index}입니다.")
        }));
        ids.push(agent["id"].as_str().unwrap().to_string());
    }
    app.post("/api/rooms", &json!({
        "id":"measure-room","name":"측정 방","agentIds":ids,
        "routingMode":"discussion","rounds":1,"maxTurns":10,
        "toolsEnabled":false,"autoLearning":false,"contextBudgetTokens":2000
    }));
    let mut seen = 0usize;
    for (round, text) in ["안녕", "오늘 기분은 어때", "점심은 뭐 먹을까"].iter().enumerate() {
        let sent = app.post("/api/rooms/measure-room/messages", &json!({"input":text}));
        let job_id = sent["jobId"].as_str().unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            let job = app.get(&format!("/api/jobs/{job_id}"));
            if job["status"] == "completed" { break; }
            assert_ne!(job["status"], "failed", "{job}");
            assert!(Instant::now() < deadline, "timeout: {job}");
            std::thread::sleep(Duration::from_millis(30));
        }
        let bodies = requests.lock().unwrap();
        let fresh: Vec<Value> = bodies[seen..].to_vec();
        seen = bodies.len();
        drop(bodies);
        let mut total = 0usize;
        for (index, body) in fresh.iter().enumerate() {
            let tokens = measure_estimate(&body["messages"].to_string());
            total += tokens;
            let prompt = body["messages"][0]["content"].as_str().unwrap_or("");
            let name = prompt.split("활성 에이전트: ").nth(1)
                .and_then(|rest| rest.split(" (id:").next()).unwrap_or("?");
            println!("round {} call {} agent={} estimate={} items={}", round + 1, index + 1, name, tokens, body["messages"].as_array().map(Vec::len).unwrap_or(0));
        }
        println!("round {} summary calls={} total_estimate={}", round + 1, fresh.len(), total);
    }
    let db = rusqlite::Connection::open_with_flags(
        root.join("runtime/dominion.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).unwrap();
    let mut statement = db.prepare("select data from records where kind='participant_conversation'").unwrap();
    let rows = statement.query_map([], |row| row.get::<_, String>(0)).unwrap();
    for row in rows {
        let data: Value = serde_json::from_str(&row.unwrap()).unwrap();
        let view = &data["workingView"];
        let frames = view["frames"].as_array().cloned().unwrap_or_default();
        let mut contents = std::collections::HashMap::new();
        for frame in &frames {
            let text = frame["message"]["content"].as_str().unwrap_or("").to_string();
            if !text.is_empty() { *contents.entry(text).or_insert(0usize) += 1; }
        }
        let duplicates = contents.values().filter(|count| **count > 1).count();
        println!(
            "view agent={} epoch={} frames={} duplicate_contents={} estimate={}",
            data["agentId"].as_str().unwrap_or("?"),
            view["epoch"], frames.len(), duplicates,
            measure_estimate(&json!(frames).to_string())
        );
    }
}


/// Minimal Codex Responses websocket: one chain per connection, exactly like
/// the subscription backend. Records every `response.create` frame it receives
/// so a test can inspect the incremental payload.
fn scripted_websocket_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_fixture_{count}");
                        let answer = format!("WEBSOCKET_ANSWER_{count}");
                        let events = [
                            json!({"type":"response.output_text.delta","delta":answer}),
                            json!({"type":"response.completed","response":{
                                "id":id,
                                "output":[{"type":"message","role":"assistant",
                                    "content":[{"type":"output_text","text":answer}]}],
                                "usage":{"input_tokens":40,
                                    "input_tokens_details":{"cached_tokens":0},
                                    "output_tokens":5,"total_tokens":45}
                            }}),
                        ];
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

/// Codex Responses websocket that answers the first request with a structured
/// `workspace_write` function call and the continuation with a final answer.
/// The continuation must carry the function-call output, not a text transcript.
fn scripted_codex_tool_provider() -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let observed = observed.clone();
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
                        return;
                    };
                    let mut count = 0usize;
                    while let Some(Ok(message)) = socket.next().await {
                        let Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(body) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if body["type"] != "response.create" {
                            continue;
                        }
                        count += 1;
                        observed.lock().unwrap().push(body.clone());
                        let id = format!("resp_ws_tool_{count}");
                        let events = if body.get("previous_response_id").is_none() {
                            let call = json!({
                                "type":"function_call",
                                "id":"fc_ws_tool_1",
                                "call_id":"call_ws_tool_1",
                                "name":"workspace_write",
                                "arguments":"{\"path\":\"ws_tool.txt\",\"content\":\"WS_TOOL_OK\"}"
                            });
                            vec![
                                json!({"type":"response.output_item.done","item":call.clone()}),
                                json!({"type":"response.completed","response":{
                                    "id":id,
                                    "output":[call],
                                    "usage":{"input_tokens":40,
                                        "input_tokens_details":{"cached_tokens":0},
                                        "output_tokens":5,"total_tokens":45}
                                }}),
                            ]
                        } else {
                            let answer = "웹소켓 도구 실행 완료";
                            vec![
                                json!({"type":"response.output_text.delta","delta":answer}),
                                json!({"type":"response.completed","response":{
                                    "id":id,
                                    "output":[{"type":"message","role":"assistant",
                                        "content":[{"type":"output_text","text":answer}]}],
                                    "usage":{"input_tokens":45,
                                        "input_tokens_details":{"cached_tokens":40},
                                        "output_tokens":5,"total_tokens":50}
                                }}),
                            ]
                        };
                        for event in events {
                            if socket
                                .send(Message::Text(event.to_string().into()))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        });
    });
    (port, requests)
}

#[test]
fn codex_websocket_executes_a_structured_tool_call_and_continues_the_chain() {
    let _serial = serial();
    let (port, requests) = scripted_codex_tool_provider();
    let app = TestApp::new(port, "compat");
    app.post("/api/policy", &json!({"autoApproveAll": true}));
    let provider = app.post("/api/providers", &json!({
        "name":"codex tool fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"ws tool participant",
        "systemPrompt":"WEBSOCKET_TOOL_IDENTITY stays verbatim",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"ws tool","agentId":agent["id"]}));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"agentId":agent["id"],
        "providerId":provider["id"],"model":"fixture-model",
        "input":"ws_tool.txt 파일을 만들어줘",
        "toolsEnabled":true,"autoLearning":false
    }));
    assert!(stream.contains("웹소켓 도구 실행 완료"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("ws_tool.txt")).unwrap_or_default(),
        "WS_TOOL_OK"
    );
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 2, "one tool round plus one final answer: {seen:?}");
    let tools = seen[0]["tools"].as_array().expect("native tools");
    assert!(
        tools.iter().any(|tool| {
            tool["type"] == "function"
                && tool["name"] == "workspace_write"
                && tool["parameters"]["type"] == "object"
        }),
        "workspace_write schema was not sent: {tools:?}"
    );
    assert!(seen[0].get("previous_response_id").is_none());
    assert_eq!(seen[1]["previous_response_id"], "resp_ws_tool_1");
    let second = seen[1]["input"].as_array().unwrap();
    assert!(
        second.iter().any(|item| {
            item["type"] == "function_call_output"
                && item["call_id"] == "call_ws_tool_1"
                && item["output"]
                    .as_str()
                    .is_some_and(|output| output.contains("ws_tool.txt"))
        }),
        "the continuation must carry the function-call output: {second:?}"
    );
    assert!(
        !second
            .iter()
            .any(|item| item["role"] == "user" && item["content"].to_string().contains("Tool observation")),
        "tool output stayed structured instead of becoming user text: {second:?}"
    );
    assert!(
        !second
            .iter()
            .any(|item| item.to_string().contains("ws_tool.txt 파일을 만들어줘")),
        "the chained user turn must not be re-sent: {second:?}"
    );
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    let assistant = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .find(|row| row["tool_calls"].as_array().is_some_and(|calls| {
            calls.iter().any(|call| call["id"] == "call_ws_tool_1")
        }))
        .expect("persisted structured tool call");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["name"],
        "workspace_write"
    );
}

#[test]
fn codex_http_fallback_executes_a_structured_tool_call() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|body| {
        let has_output = body["input"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["type"] == "function_call_output")
        });
        if has_output {
            return vec![
                json!({"type":"response.output_text.delta","delta":"HTTP 도구 완료"}),
                json!({"type":"response.completed","response":{
                    "id":"resp_http_tool_2",
                    "output":[{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":"HTTP 도구 완료"}]}],
                    "usage":{"input_tokens":45,"output_tokens":5,"total_tokens":50}
                }}),
            ];
        }
        let call = json!({
            "type":"function_call",
            "id":"fc_http_tool_1",
            "call_id":"call_http_tool_1",
            "name":"workspace_write",
            "arguments":"{\"path\":\"http_tool.txt\",\"content\":\"HTTP_TOOL_OK\"}"
        });
        vec![
            json!({"type":"response.output_item.done","item":call.clone()}),
            json!({"type":"response.completed","response":{
                "id":"resp_http_tool_1",
                "output":[call],
                "usage":{"input_tokens":40,"output_tokens":5,"total_tokens":45}
            }}),
        ]
    });
    let app = TestApp::new_with_env(port, "compat", &[("DOMINION_CODEX_WS", "off")]);
    app.post("/api/policy", &json!({"autoApproveAll": true}));
    let provider = app.post("/api/providers", &json!({
        "name":"codex http tool fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"http tool participant",
        "systemPrompt":"HTTP_TOOL_IDENTITY stays verbatim",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"http tool","agentId":agent["id"]}));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"agentId":agent["id"],
        "providerId":provider["id"],"model":"fixture-model",
        "input":"http_tool.txt 파일을 만들어줘",
        "toolsEnabled":true,"autoLearning":false
    }));
    assert!(stream.contains("HTTP 도구 완료"), "{stream}");
    assert_eq!(
        std::fs::read_to_string(app._root.join("http_tool.txt")).unwrap_or_default(),
        "HTTP_TOOL_OK"
    );
    let seen = requests.lock().unwrap();
    assert!(
        seen.iter().any(|body| {
            body["tools"].as_array().is_some_and(|tools| {
                tools.iter().any(|tool| {
                    tool["type"] == "function" && tool["name"] == "workspace_write"
                })
            })
        }),
        "HTTP fallback must send the tool schema: {seen:?}"
    );
    assert!(
        seen.iter().any(|body| {
            body["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "function_call_output"
                        && item["call_id"] == "call_http_tool_1"
                        && item["output"]
                            .as_str()
                            .is_some_and(|output| output.contains("http_tool.txt"))
                })
            })
        }),
        "HTTP fallback must send the function-call output: {seen:?}"
    );
}

#[test]
fn codex_websocket_chain_reuses_identity_and_sends_only_the_new_suffix() {
    let _serial = serial();
    let (port, requests) = scripted_websocket_provider();
    let app = TestApp::new(port, "compat");
    let provider = app.post("/api/providers", &json!({
        "name":"codex websocket fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture-account","apiKey":"fixture-only",
        "enabled":true,"model":"fixture-model"
    }));
    let agent = app.post("/api/agents", &json!({
        "name":"ws participant",
        "systemPrompt":"WEBSOCKET_IDENTITY_VERBATIM persona stays whole",
        "providerId":provider["id"],"model":"fixture-model"
    }));
    let session = app.post("/api/sessions", &json!({"title":"ws chain","agentId":agent["id"]}));
    for text in ["첫 번째 질문", "두 번째 질문"] {
        let stream = app.chat(&json!({
            "sessionId":session["id"],"agentId":agent["id"],
            "providerId":provider["id"],"model":"fixture-model",
            "input":text,"toolsEnabled":false,"autoLearning":false
        }));
        assert!(stream.contains("WEBSOCKET_ANSWER"), "{stream}");
    }
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 2, "one websocket request per turn: {seen:?}");
    assert!(seen[0]["prompt_cache_key"].is_string(),"WebSocket must carry the HTTP cache identity");
    assert_eq!(seen[0]["prompt_cache_key"],seen[1]["prompt_cache_key"]);
    assert!(seen[0].get("previous_response_id").is_none());
    assert_eq!(seen[1]["previous_response_id"], "resp_ws_fixture_1");
    assert_eq!(seen[1]["client_metadata"]["session_id"], seen[0]["client_metadata"]["session_id"]);
    for request in seen.iter() {
        assert!(
            request["instructions"].as_str().unwrap().contains("WEBSOCKET_IDENTITY_VERBATIM"),
            "identity is re-sent on every websocket call"
        );
    }
    let first = seen[0]["input"].as_array().unwrap();
    let second = seen[1]["input"].as_array().unwrap();
    assert!(
        !second.iter().any(|item| item.to_string().contains("첫 번째 질문")),
        "already-chained items stay on the server: first={first:?} second={second:?}"
    );
    assert!(
        second.iter().any(|item| item.to_string().contains("두 번째 질문")),
        "the new turn is sent: {second:?}"
    );
    let messages = app.get(&format!("/api/sessions/{}/messages", session["id"].as_str().unwrap()));
    let last = messages.as_array().unwrap().iter().rev()
        .find(|row| row["role"] == "assistant").unwrap();
    assert_eq!(
        last["metadata"]["continuity"]["incrementalTransport"]["wire"], "websocket"
    );
    assert_eq!(
        last["metadata"]["continuity"]["incrementalTransport"]["mode"], "delta"
    );
}

#[test]
fn codex_http_rejects_incomplete_and_unterminated_generations() {
    let _serial = serial();
    for terminal in [None, Some(json!({
        "type":"response.incomplete",
        "response":{"incomplete_details":{"reason":"max_output_tokens"}}
    }))] {
        let (port, requests) = scripted_provider(move |_| {
            let mut events = vec![json!({"type":"response.output_text.delta","delta":"PARTIAL_ONLY"})];
            events.extend(terminal.clone());
            events
        });
        let app = TestApp::new(port, "native");
        let provider = app.post("/api/providers", &json!({
            "name":"incomplete fixture","kind":"codex",
            "baseUrl":format!("http://127.0.0.1:{port}/codex"),
            "accountId":"fixture","apiKey":"fixture","enabled":true,"model":"fixture"
        }));
        let session = app.post("/api/sessions", &json!({"title":"incomplete"}));
        let stream = app.chat(&json!({
            "sessionId":session["id"],"providerId":provider["id"],"model":"fixture",
            "input":"hello","toolsEnabled":false,"autoLearning":false
        }));
        assert!(stream.contains("PARTIAL_ONLY"), "{stream}");
        assert!(stream.contains("\"type\":\"error\""), "{stream}");
        assert!(!stream.contains("assistant_saved"), "{stream}");
        assert_eq!(requests.lock().unwrap().len(), 1, "partial output must not be replayed");
        assert_eq!(app.get("/api/executions")[0]["status"], "failed");
    }
}

#[test]
fn codex_receives_uploaded_image_and_file_contents() {
    use base64::Engine as _;
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| vec![
        json!({"type":"response.output_text.delta","delta":"ATTACHMENTS_RECEIVED"}),
        json!({"type":"response.completed","response":{}}),
    ]);
    let app = TestApp::new(port, "native");
    let provider = app.post("/api/providers", &json!({
        "name":"attachment fixture","kind":"codex",
        "baseUrl":format!("http://127.0.0.1:{port}/codex"),
        "accountId":"fixture","apiKey":"fixture","enabled":true,"model":"fixture",
        "capabilities":{"vision":true}
    }));
    let session = app.post("/api/sessions", &json!({"title":"attachments"}));
    let image_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=";
    let image = app.post("/api/attachments", &json!({
        "sessionId":session["id"],"name":"sample.png","mimeType":"image/png","data":image_data
    }));
    let file = app.post("/api/attachments", &json!({
        "sessionId":session["id"],"name":"sample.txt","mimeType":"text/plain",
        "data":base64::engine::general_purpose::STANDARD.encode("ATTACHMENT_SENTINEL")
    }));
    let stream = app.chat(&json!({
        "sessionId":session["id"],"providerId":provider["id"],"model":"fixture",
        "input":"첨부를 분석해","attachmentIds":[image["id"],file["id"]],
        "toolsEnabled":false,"autoLearning":false
    }));
    assert!(stream.contains("assistant_saved"), "{stream}");
    let usage=app.get(&format!("/api/sessions/{}/usage",session["id"].as_str().unwrap()));
    assert!(usage["totalTokens"].as_u64().unwrap()>0,"missing provider usage must not become a free call: {usage}");
    assert!(usage["estimatedCalls"].as_u64().unwrap()>0,"estimates must remain distinguishable: {usage}");
    let seen = requests.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let parts = seen[0]["input"].as_array().unwrap().iter()
        .flat_map(|item| item["content"].as_array().into_iter().flatten()).collect::<Vec<_>>();
    assert!(parts.iter().any(|part| part["type"] == "input_text"
        && part["text"].as_str().is_some_and(|text| text.contains("ATTACHMENT_SENTINEL"))), "{seen:?}");
    assert!(parts.iter().any(|part| part["type"] == "input_image"
        && part["image_url"] == format!("data:image/png;base64,{image_data}")), "{seen:?}");
}
