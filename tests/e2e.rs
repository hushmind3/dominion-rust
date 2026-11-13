//! Shared end-to-end test harness. Scenarios are organized by responsibility.
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
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
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
                let headers = String::from_utf8_lossy(&bytes[..end]).into_owned();
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
                // Websocket upgrades from the Codex adapter carry no JSON body;
                // ignore them instead of panicking the fixture thread.
                let Ok(body) = serde_json::from_slice::<Value>(&bytes[end..end + length]) else {
                    return;
                };
                let mut recorded=body.clone();
                if body["prompt_cache_key"].is_string() {recorded["_testHttpHeaders"]=json!(headers);}
                observed.lock().unwrap().push(recorded);
                let events = reply(&body);
                // Tests may emulate a strict provider's HTTP error before a
                // generation starts; normal scripted streams are unchanged.
                if let Some(status)=events.first().and_then(|event|event["_testHttpStatus"].as_u64()) {
                    let payload=events[0]["body"].to_string();
                    let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len());
                    let _=stream.write_all(response.as_bytes());
                    return;
                }
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

include!("support/e2e/room_context.rs");
include!("support/e2e/scheduler_and_policies.rs");
include!("support/e2e/adaptation_and_programs.rs");
include!("support/e2e/workflow_and_governance.rs");
include!("support/e2e/model_transport.rs");
