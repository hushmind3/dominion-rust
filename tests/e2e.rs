//! End-to-end test: runs the real binary, a fake OpenAI-compatible model, and
//! exercises HTTP/SSE, persistence and the cross-origin write guard.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scripted_provider(
    reply: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let reply = Arc::new(reply);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                break;
            };
            let observed = observed.clone();
            let reply = reply.clone();
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
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|length| length.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                let body: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
                observed.lock().unwrap().push(body.clone());
                let events = reply(&body);
                let stream_mode = body["stream"].as_bool().unwrap_or(false);
                let response = if stream_mode {
                    let mut payload = events
                        .iter()
                        .map(|event| format!("data: {event}\n\n"))
                        .collect::<String>();
                    payload.push_str("data: [DONE]\n\n");
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len())
                } else {
                    let content = events
                        .iter()
                        .filter_map(|event| event["choices"][0]["delta"]["content"].as_str())
                        .collect::<String>();
                    let payload = json!({"choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}]}).to_string();
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len())
                };
                let _ = stream.write_all(response.as_bytes());
            });
        }
    });
    (port, requests)
}

struct TestApp {
    _child: ChildGuard,
    _root: PathBuf,
    base: String,
    client: reqwest::blocking::Client,
}

impl TestApp {
    fn new(provider_port: u16, tool_mode: &str) -> Self {
        Self::new_with_env(provider_port, tool_mode, &[])
    }
    fn new_with_env(provider_port: u16, tool_mode: &str, extra: &[(&str, &str)]) -> Self {
        Self::from_root(provider_port, tool_mode, temp_root(), extra)
    }
    fn from_root(
        provider_port: u16,
        tool_mode: &str,
        root: PathBuf,
        extra: &[(&str, &str)],
    ) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_dominion-rust"));
        command
            .env("DOMINION_PORT", port.to_string())
            .env("DOMINION_DATA", root.join("runtime"))
            .env("DOMINION_WEB", root.join("web"))
            .env("DOMINION_WORKSPACE", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (key, value) in extra {
            command.env(key, value);
        }
        let child = command.spawn().unwrap();
        let app = Self {
            _child: ChildGuard(child),
            _root: root,
            base: format!("http://127.0.0.1:{port}"),
            client: reqwest::blocking::Client::new(),
        };
        wait_health(port, 20);
        let providers = app.get("/api/providers");
        if providers.as_array().is_none_or(|items| items.is_empty()) {
            app.post("/api/providers",&json!({"name":"mock","baseUrl":format!("http://127.0.0.1:{provider_port}/v1"),"model":"mock","enabled":true,"toolMode":tool_mode}));
        }
        app
    }
    fn post(&self, path: &str, body: &Value) -> Value {
        self.client
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn get(&self, path: &str) -> Value {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn chat(&self, body: &Value) -> String {
        self.client
            .post(format!("{}/api/chat", self.base))
            .json(body)
            .send()
            .unwrap()
            .text()
            .unwrap()
    }

    fn chat_async(&self, body: &Value) -> std::thread::JoinHandle<String> {
        let client = self.client.clone();
        let url = format!("{}/api/chat", self.base);
        let body = body.clone();
        std::thread::spawn(move || {
            client
                .post(url)
                .json(&body)
                .send()
                .map(|response| response.text().unwrap_or_default())
                .unwrap_or_default()
        })
    }

    fn wait_pending_approval(&self, seconds: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            if let Some(record) = self
                .get("/api/approvals")
                .as_array()
                .and_then(|items| items.first())
            {
                return record.clone();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("approval was not requested");
    }
}

#[test]
fn command_failure_persists_actual_evidence() {
    let _serial = serial();
    let (port, _) = scripted_provider(|request| {
        let saw_tool = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool");
        if saw_tool {
            vec![
                json!({"choices":[{"delta":{"content":"명령 실패를 확인했습니다."},"finish_reason":"stop"}]}),
            ]
        } else {
            vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"failed-shell","type":"function","function":{"name":"shell_exec","arguments":"{\"command\":\"exit 7\"}"}}]},"finish_reason":"tool_calls"}]}),
            ]
        }
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let session = app.post("/api/sessions", &json!({"title":"failure"}));
    let stream =
        app.chat(&json!({"sessionId":session["id"],"input":"실패 명령 실행","autoLearning":false}));
    assert!(stream.contains("tool_error"), "{stream}");
    assert!(!stream.contains("\"type\":\"tool_end\""));
    let executions = app.get("/api/executions");
    let operations = app.get(&format!(
        "/api/executions/{}/operations",
        executions[0]["id"].as_str().unwrap()
    ));
    assert_eq!(operations[0]["status"], "failed");
    assert_eq!(operations[0]["arguments"]["command"], "exit 7");
    assert_eq!(operations[0]["result"]["exitCode"], 7);
}

#[test]
fn compatibility_tools_are_available_and_round_limit_is_a_failure() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![
            json!({"choices":[{"delta":{"content":"<tool_call>{\"name\":\"workspace_list\",\"arguments\":{}}</tool_call>"},"finish_reason":"stop"}]}),
        ]
    });
    let app = TestApp::new(port, "compat");
    let session = app.post("/api/sessions", &json!({"title":"limit"}));
    let stream =
        app.chat(&json!({"sessionId":session["id"],"input":"계속 확인","autoLearning":false}));
    // The loop guard stops identical repeated calls instead of burning all 8
    // rounds, and never saves a raw tool-call fragment as the final answer.
    assert!(stream.contains("tool_error"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    let calls = requests.lock().unwrap();
    assert!(calls.len() < 8, "loop guard used {} calls", calls.len());
    assert!(calls[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("workspace_list"));
    assert!(calls[0].get("tools").is_none());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
    assert_eq!(app.get("/api/executions")[0]["taskOutcome"], "failed");
    let messages = app.get(&format!(
        "/api/sessions/{}/messages",
        session["id"].as_str().unwrap()
    ));
    assert!(messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .all(|row| row["tool_calls"].is_array()));
}

#[test]
fn cancelling_a_stalled_model_does_not_wait_for_network_data() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        std::thread::sleep(Duration::from_secs(5));
        vec![json!({"choices":[{"delta":{"content":"too late"},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    let session = app.post("/api/sessions", &json!({"title":"cancel"}));
    let id = session["id"].as_str().unwrap().to_string();
    let base = app.base.clone();
    let chat_id = id.clone();
    let started = Instant::now();
    let turn = std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(format!("{base}/api/chat"))
            .json(&json!({"sessionId":chat_id,"input":"wait","autoLearning":false}))
            .send()
            .unwrap()
            .text()
            .unwrap()
    });
    while app
        .get("/api/executions/active")
        .as_array()
        .unwrap()
        .is_empty()
    {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(20));
    }
    app.post(&format!("/api/sessions/{id}/cancel"), &json!({}));
    let stream = turn.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cancellation stalled"
    );
    assert!(stream.contains("중단"));
    assert!(!stream.contains("too late"));
    assert_eq!(app.get("/api/executions")[0]["status"], "cancelled");
    assert_eq!(app.get("/api/executions")[0]["taskOutcome"], "cancelled");
}

#[test]
fn disabled_tools_cannot_be_enabled_by_the_model() {
    let _serial = serial();
    let (port, _) = scripted_provider(|_| {
        vec![
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"forbidden","type":"function","function":{"name":"workspace_write","arguments":"{\"path\":\"forbidden.txt\",\"content\":\"should never exist\"}"}}]},"finish_reason":"tool_calls"}]}),
        ]
    });
    let app = TestApp::new(port, "native");
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let stream = app.chat(&json!({"input":"대화만","toolsEnabled":false,"autoLearning":false}));
    assert!(stream.contains("도구가 비활성"), "{stream}");
    assert!(!app._root.join("forbidden.txt").exists());
    assert!(app.get("/api/approvals").as_array().unwrap().is_empty());
    assert_eq!(app.get("/api/executions")[0]["status"], "failed");
}

#[test]
fn persistent_room_http_flow_keeps_owner_once_and_shares_peer_context() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|request| {
        let system = request["messages"][0]["content"].as_str().unwrap();
        let reply = if system.contains("Active agent: Alice (id: a)") {
            "Alice의 실제 의견"
        } else {
            "Bob이 Alice의 의견을 확인했습니다"
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
    let room=app.post("/api/rooms",&json!({"name":"공용 대화","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":2,"toolsEnabled":false,"autoLearning":false}));
    let id = room["id"].as_str().unwrap();
    let input = json!({"input":"우리 함께 의견을 나눠보자","clientMessageId":"owner-request-001"});
    let sent = app.post(&format!("/api/rooms/{id}/messages"), &input);
    let retry = app.post(&format!("/api/rooms/{id}/messages"), &input);
    assert_eq!(sent["jobId"], retry["jobId"]);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = app.get(&format!("/api/rooms/{id}/state"));
        let status = state["jobs"][0]["status"].as_str().unwrap();
        if status == "completed" {
            break;
        }
        assert!(matches!(status, "queued" | "running"), "{state}");
        assert!(Instant::now() < deadline, "{state}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let messages = app.get(&format!("/api/rooms/{id}/messages"));
    let rows = messages.as_array().unwrap();
    assert_eq!(rows.iter().filter(|row| row["role"] == "user").count(), 1);
    assert_eq!(rows[0]["metadata"]["authorKind"], "owner");
    let agents = rows
        .iter()
        .filter(|row| row["role"] == "assistant")
        .map(|row| row["agent_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(agents, vec!["a", "b"]);
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert!(captured[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["content"]
            .as_str()
            .unwrap_or("")
            .contains("Alice의 실제 의견")));
    drop(captured);
    // One owner message is one root task; each speaker is an attempt, not a
    // new independent task.
    let session_id = room["sessionId"].as_str().unwrap();
    let executions = app.get("/api/executions");
    let room_runs = executions
        .as_array()
        .unwrap()
        .iter()
        .filter(|execution| execution["sessionId"] == session_id)
        .collect::<Vec<_>>();
    assert_eq!(room_runs.len(), 2, "{executions}");
    let task_id = room_runs[0]["taskId"].as_str().unwrap();
    assert!(room_runs
        .iter()
        .all(|execution| execution["taskId"] == task_id));
    let task = app.get(&format!("/api/tasks/{task_id}"));
    assert_eq!(task["id"], task_id);
    assert_eq!(task["contract"]["family"], "room");
    assert_eq!(task["contract"]["source"], "owner");
    assert_eq!(task["sourceMessageId"], rows[0]["id"]);
    assert_eq!(task["attempts"].as_array().unwrap().len(), 2);
    let speakers = task["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| attempt["agentId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(speakers, vec!["a", "b"]);
    assert_eq!(task["attempts"][0]["verificationOwner"], "self");
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
            assert!(executions.len() >= 4, "{executions:?}");
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
        let saw_tool = messages.iter().any(|message| message["role"] == "tool");
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
                    .contains("실패를 분석하는 반성기")
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
        if system.contains("반성기") {
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
        if system.contains("반성기") {
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
