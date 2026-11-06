//! End-to-end test: runs the real binary, a fake OpenAI-compatible model, and
//! exercises HTTP/SSE, persistence and the cross-origin write guard.

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
    fn delete(&self, path: &str) {
        let _ = self
            .client
            .delete(format!("{}{path}", self.base))
            .send();
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
    // Repeating a read call carries information, so the harness lets the
    // model continue until the round budget; it still must fail and never
    // save a raw tool-call fragment as the final answer.
    assert!(stream.contains("tool_start") || stream.contains("tool_error"), "{stream}");
    assert!(!stream.contains("assistant_saved"), "{stream}");
    let calls = requests.lock().unwrap();
    assert!(calls.len() >= 8, "round budget was not reached: {} calls", calls.len());
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
fn a_room_greeting_gets_one_direct_reply_not_a_panel() {
    let _serial = serial();
    let (port, requests) = scripted_provider(|_| {
        vec![json!({"choices":[{"delta":{"content":"안녕하세요, 주인님."},"finish_reason":"stop"}]})]
    });
    let app = TestApp::new(port, "native");
    for (id, name) in [("a", "Alice"), ("b", "Bob")] {
        app.post(
            "/api/agents",
            &json!({"id":id,"name":name,"systemPrompt":"독립 참가자","enabled":true}),
        );
    }
    app.post("/api/policy", &json!({"autoApproveAll":true}));
    let room = app.post(
        "/api/rooms",
        &json!({"name":"인사방","agentIds":["a","b"],"routingMode":"discussion","rounds":1,"maxTurns":9,"toolsEnabled":true,"autoLearning":false}),
    );
    let id = room["id"].as_str().unwrap();
    let sent = app.post(
        &format!("/api/rooms/{id}/messages"),
        &json!({"input":"안녕","clientMessageId":"owner-greeting-001"}),
    );
    let job_id = sent["jobId"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = app.get(&format!("/api/jobs/{job_id}"));
        let status = job["status"].as_str().unwrap_or("");
        if matches!(status, "completed" | "failed") {
            assert_eq!(status, "completed", "{job}");
            break;
        }
        assert!(Instant::now() < deadline, "{job}");
        std::thread::sleep(Duration::from_millis(25));
    }
    let messages = app.get(&format!("/api/rooms/{id}/messages"));
    let assistants = messages
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["role"] == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(assistants.len(), 1, "{messages}");
    assert_eq!(assistants[0]["agent_id"], "a", "{messages}");
    assert_eq!(assistants[0]["content"], "안녕하세요, 주인님.");
    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 1, "a greeting called the whole panel");
    let system = captured[0]["messages"][0]["content"]
        .as_str()
        .unwrap_or("");
    assert!(system.contains("first responder"), "{system}");
    assert!(system.contains("do not invent a topic"), "{system}");
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
    let started = Instant::now();
    let stream = app.chat(&json!({"input":"병렬 프로그램을 실행해","autoLearning":false}));
    let elapsed = started.elapsed();
    assert!(!stream.contains("\"type\":\"error\""), "{stream}");
    assert!(
        elapsed < Duration::from_millis(3_500),
        "parallel tools did not run concurrently (serial 4s minimum): {elapsed:?}"
    );
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
        &json!({}),
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
    let execution = &app.get("/api/executions")[0];
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
                messages
                    .iter()
                    .find(|message| message["role"] == "system")
                    .and_then(|message| message["content"].as_str())
                    .map(str::to_string)
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
        &json!({}),
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
        &json!({}),
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
        &json!({}),
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
        &json!({}),
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
        &json!({}),
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
        let reply = if system.contains("Active agent: Alice (id: a)") {
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
        &json!({}),
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
    let stream = app.chat(&json!({
        "input":"파일 도구로 상태를 확인해",
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
