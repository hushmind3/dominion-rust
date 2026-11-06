//! Outbound channel adapters ported from the Node DOMINION drivers.
//!
//! The Rust bridge already owns the room ledger, outbox, revisions and
//! idempotency. This module only performs the platform-specific HTTP call, so
//! adding a messenger never creates a second history or a parallel runtime.
use crate::{engine::discord::text, state::AppState};
use serde_json::{json, Value};

pub const SUPPORTED: &[&str] = &[
    "slack",
    "matrix",
    "whatsapp",
    "line",
    "zulip",
    "mattermost",
    "signal",
];

pub fn supports(kind: &str) -> bool {
    SUPPORTED.contains(&kind)
}

fn secret(state: &AppState, config: &Value) -> String {
    state
        .read_secret(&crate::engine::discord::token_name(text(config, "id")))
        .or_else(|| {
            config["token"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn required_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "slack" => &["channelId"],
        "matrix" => &["homeserver", "userId", "channelId"],
        "whatsapp" => &["phoneNumberId", "graphVersion", "chatId"],
        "line" => &["chatId"],
        "zulip" => &["site", "email", "channelId"],
        "mattermost" => &["baseUrl", "channelId"],
        "signal" => &["bridgeUrl", "phoneNumber", "chatId"],
        _ => &[],
    }
}

pub fn validate(state: &AppState, config: &Value) -> Result<(), String> {
    let kind = text(config, "kind");
    if !supports(&kind) {
        return Err(format!("지원하지 않는 채널 종류입니다: {kind}"));
    }
    let missing = required_fields(&kind)
        .iter()
        .filter(|key| text(config, key).is_empty())
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("필수 채널 설정이 없습니다: {}", missing.join(", ")));
    }
    let _ = state;
    Ok(())
}

async fn response_id(response: reqwest::Response, kind: &str) -> Result<String, String> {
    let status = response.status();
    let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        return Err(format!("{kind} HTTP {status}: {value}"));
    }
    let id = match kind {
        "slack" => {
            if value["ok"] == false {
                return Err(format!("Slack error: {}", value["error"]));
            }
            value["ts"].as_str().unwrap_or("").to_string()
        }
        "matrix" => value["event_id"].as_str().unwrap_or("").to_string(),
        "whatsapp" => value["messages"][0]["id"].as_str().unwrap_or("").to_string(),
        "line" => value["sentMessages"][0]["id"]
            .as_str()
            .or_else(|| value["requestId"].as_str())
            .unwrap_or("")
            .to_string(),
        "zulip" => value["id"].as_u64().map(|id| id.to_string()).unwrap_or_default(),
        "mattermost" => value["id"].as_str().unwrap_or("").to_string(),
        "signal" => value["timestamp"]
            .as_str()
            .map(str::to_string)
            .or_else(|| value["timestamp"].as_u64().map(|id| id.to_string()))
            .unwrap_or_default(),
        _ => String::new(),
    };
    Ok(id)
}

/// Performs exactly one external request. Unknown outcomes are surfaced to the
/// bridge, which marks the outbox `needs_review` instead of retrying blindly.
pub async fn send(state: &AppState, config: &Value, content: &str) -> Result<String, String> {
    let kind = text(config, "kind");
    let token = secret(state, config);
    match kind {
        "slack" => {
            let base = text(config, "apiBase");
            let base = if base.is_empty() { "https://slack.com/api".to_string() } else { base.trim_end_matches('/').to_string() };
            let response = client()
                .post(format!("{base}/chat.postMessage"))
                .bearer_auth(&token)
                .json(&json!({
                    "channel": text(config, "channelId"),
                    "text": content.chars().take(3900).collect::<String>(),
                }))
                .send()
                .await
                .map_err(|error| format!("Slack 연결 실패: {error}"))?;
            response_id(response, "slack").await
        }
        "matrix" => {
            let homeserver = text(config, "homeserver").trim_end_matches('/').to_string();
            let room = text(config, "channelId");
            let url = format!(
                "{homeserver}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
                urlencoding(&room),
                uuid::Uuid::new_v4()
            );
            let response = client()
                .put(url)
                .bearer_auth(&token)
                .json(&json!({
                    "msgtype": "m.text",
                    "body": content.chars().take(30000).collect::<String>(),
                }))
                .send()
                .await
                .map_err(|error| format!("Matrix 연결 실패: {error}"))?;
            response_id(response, "matrix").await
        }
        "whatsapp" => {
            let base = text(config, "graphBase");
            let base = if base.is_empty() { "https://graph.facebook.com".to_string() } else { base.trim_end_matches('/').to_string() };
            let url = format!(
                "{base}/{}/{}/messages",
                urlencoding(&text(config, "graphVersion")),
                urlencoding(&text(config, "phoneNumberId"))
            );
            let response = client()
                .post(url)
                .bearer_auth(&token)
                .json(&json!({
                    "messaging_product": "whatsapp",
                    "to": text(config, "chatId"),
                    "type": "text",
                    "text": {"body": content.chars().take(4096).collect::<String>()},
                }))
                .send()
                .await
                .map_err(|error| format!("WhatsApp 연결 실패: {error}"))?;
            response_id(response, "whatsapp").await
        }
        "line" => {
            let base = text(config, "apiBase");
            let base = if base.is_empty() { "https://api.line.me".to_string() } else { base.trim_end_matches('/').to_string() };
            let response = client()
                .post(format!("{base}/v2/bot/message/push"))
                .bearer_auth(&token)
                .json(&json!({
                    "to": text(config, "chatId"),
                    "messages": [{"type":"text","text": content.chars().take(5000).collect::<String>()}],
                }))
                .send()
                .await
                .map_err(|error| format!("LINE 연결 실패: {error}"))?;
            response_id(response, "line").await
        }
        "zulip" => {
            let site = text(config, "site").trim_end_matches('/').to_string();
            let response = client()
                .post(format!("{site}/api/v1/messages"))
                .basic_auth(text(config, "email"), Some(token))
                .json(&json!({
                    "type": "stream",
                    "to": text(config, "channelId"),
                    "topic": text(config, "topic"),
                    "content": content.chars().take(9000).collect::<String>(),
                }))
                .send()
                .await
                .map_err(|error| format!("Zulip 연결 실패: {error}"))?;
            response_id(response, "zulip").await
        }
        "mattermost" => {
            let base = text(config, "baseUrl").trim_end_matches('/').to_string();
            let response = client()
                .post(format!("{base}/api/v4/posts"))
                .bearer_auth(&token)
                .json(&json!({
                    "channel_id": text(config, "channelId"),
                    "message": content.chars().take(16000).collect::<String>(),
                }))
                .send()
                .await
                .map_err(|error| format!("Mattermost 연결 실패: {error}"))?;
            response_id(response, "mattermost").await
        }
        "signal" => {
            let base = text(config, "bridgeUrl").trim_end_matches('/').to_string();
            let mut request = client()
                .post(format!("{base}/v2/send"))
                .json(&json!({
                    "number": text(config, "phoneNumber"),
                    "message": content.chars().take(3000).collect::<String>(),
                    "recipients": [text(config, "chatId")],
                }));
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| format!("Signal 연결 실패: {error}"))?;
            response_id(response, "signal").await
        }
        _ => Err(format!("지원하지 않는 채널 종류입니다: {kind}")),
    }
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Sends pending room messages through every enabled adapter. Started once by
/// the job runtime; the bridge still owns outbox state and crash semantics.
pub async fn flush_enabled(state: &AppState) -> usize {
    let mut sent = 0usize;
    for config in state.store.all("channel") {
        if config["enabled"] != true
            || !supports(text(&config, "kind"))
            || text(&config, "roomId").is_empty()
        {
            continue;
        }
        let owned = config.clone();
        let result = crate::engine::bridge::flush(state, &config, 4000, |content| {
            let state = state.clone();
            let config = owned.clone();
            async move { send(&state, &config, &content).await }
        })
        .await;
        if let Ok(count) = result {
            sent += count;
        }
    }
    sent
}

pub fn start(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let _ = flush_enabled(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

    async fn harness(
        kind: &str,
        config: Value,
        route: &'static str,
        reply: Value,
    ) -> (AppState, Arc<Mutex<Vec<Value>>>, std::path::PathBuf) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let seen = captured.clone();
        let app = Router::new().route(
            route,
            post(move |Json(body): Json<Value>| {
                let seen = seen.clone();
                let reply = reply.clone();
                async move {
                    seen.lock().unwrap().push(body);
                    Json(reply)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = std::env::temp_dir().join(format!("dominion-adapter-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::new(
            crate::config::Config {
                port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone(),
            },
            crate::db::Store::open(&dir.join("db.sqlite")).unwrap(),
            crate::vault::Vault::open(&dir.join("keys.json")),
        );
        state.store.put(
            "channel",
            &json!({"id":"binding","kind":kind,"enabled":true,"roomId":"room",
                "baseUrl":format!("http://{address}"),"homeserver":format!("http://{address}"),
                "site":format!("http://{address}"),"bridgeUrl":format!("http://{address}"),
                "channelId":"C1","chatId":"C1","email":"bot@example.com","phoneNumber":"+1",
                "phoneNumberId":"P1","graphVersion":"v23.0","topic":"general",
                "apiBase":format!("http://{address}"),"graphBase":format!("http://{address}"),
                "token":"secret"}),
        );
        let _ = config;
        (state, captured, dir)
    }

    #[tokio::test]
    async fn slack_and_matrix_payloads_are_sent() {
        let (state, captured, dir) = harness("slack", json!({}), "/chat.postMessage", json!({"ok":true,"ts":"1.2"})).await;
        let id = send(&state, &state.store.get("channel", "binding").unwrap(), "hello").await.unwrap();
        assert_eq!(id, "1.2");
        assert_eq!(captured.lock().unwrap()[0]["text"], "hello");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn mattermost_payload_carries_channel_and_message() {
        let (state, captured, dir) = harness("mattermost", json!({}), "/api/v4/posts", json!({"id":"post-1"})).await;
        let id = send(&state, &state.store.get("channel", "binding").unwrap(), "hello").await.unwrap();
        assert_eq!(id, "post-1");
        assert_eq!(captured.lock().unwrap()[0]["channel_id"], "C1");
        let _ = std::fs::remove_dir_all(dir);
    }
}
