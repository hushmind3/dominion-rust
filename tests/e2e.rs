//! End-to-end test: runs the real binary, a fake OpenAI-compatible model, and
//! exercises HTTP/SSE, persistence and the cross-origin write guard.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
