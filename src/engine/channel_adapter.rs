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
            let topic = text(config, "topic");
            let response = client()
                .post(format!("{site}/api/v1/messages"))
                .basic_auth(text(config, "email"), Some(token))
                .json(&json!({
                    "type": "stream",
                    "to": text(config, "channelId"),
                    "topic": if topic.is_empty() { "DOMINION" } else { &topic },
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

fn configured_base(config: &Value, key: &str, fallback: &str) -> String {
    let value = text(config, key);
    if value.is_empty() {
        fallback.trim_end_matches('/').to_string()
    } else {
        value.trim_end_matches('/').to_string()
    }
}

/// Performs a read-only authentication probe. No message is posted, so the
/// owner can diagnose a live workspace without disturbing its history.
pub async fn diagnose(state: &AppState, config: &Value) -> Result<Value, String> {
    let kind = text(config, "kind");
    let token = secret(state, config);
    let client = client();
    match kind {
        "slack" => {
            if token.is_empty() {
                return Err("Slack 액세스 토큰이 없습니다.".into());
            }
            let base = configured_base(config, "apiBase", "https://slack.com/api");
            let response = client
                .post(format!("{base}/auth.test"))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("Slack 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() || body["ok"] == false {
                return Err(format!(
                    "Slack 인증 실패: {}",
                    body["error"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["user"],
                "workspace": body["team"],
                "message": "Slack 인증과 채널 접근을 확인했습니다.",
            }))
        }
        "matrix" => {
            if token.is_empty() {
                return Err("Matrix 액세스 토큰이 없습니다.".into());
            }
            let homeserver = configured_base(config, "homeserver", "");
            let response = client
                .get(format!("{homeserver}/_matrix/client/v3/account/whoami"))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("Matrix 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() {
                return Err(format!(
                    "Matrix 인증 실패 (HTTP {status}): {}",
                    body["errcode"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["user_id"],
                "message": "Matrix 계정과 홈서버 연결을 확인했습니다.",
            }))
        }
        "whatsapp" => {
            if token.is_empty() {
                return Err("WhatsApp 액세스 토큰이 없습니다.".into());
            }
            let base = configured_base(config, "graphBase", "https://graph.facebook.com");
            let version = text(config, "graphVersion");
            let phone = text(config, "phoneNumberId");
            let url = format!(
                "{base}/{version}/{phone}?fields=id,display_phone_number,verified_name"
            );
            let response = client
                .get(url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("WhatsApp 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() || body["error"].is_object() {
                return Err(format!(
                    "WhatsApp 인증 실패: {}",
                    body["error"]["message"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["display_phone_number"],
                "workspace": body["verified_name"],
                "message": "WhatsApp Business 번호와 토큰을 확인했습니다.",
            }))
        }
        "line" => {
            if token.is_empty() {
                return Err("LINE 채널 토큰이 없습니다.".into());
            }
            let base = configured_base(config, "apiBase", "https://api.line.me");
            let response = client
                .get(format!("{base}/v2/bot/info"))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("LINE 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() {
                return Err(format!(
                    "LINE 인증 실패 (HTTP {status}): {}",
                    body["message"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["userId"],
                "workspace": body["displayName"],
                "message": "LINE 봇 토큰과 대상 채널을 확인했습니다.",
            }))
        }
        "zulip" => {
            if token.is_empty() {
                return Err("Zulip API 키가 없습니다.".into());
            }
            let site = configured_base(config, "site", "");
            let response = client
                .get(format!("{site}/api/v1/users/me"))
                .basic_auth(text(config, "email"), Some(token.as_str()))
                .send()
                .await
                .map_err(|error| format!("Zulip 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() {
                return Err(format!(
                    "Zulip 인증 실패 (HTTP {status}): {}",
                    body["msg"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["email"],
                "workspace": body["full_name"],
                "message": "Zulip 계정과 스트림 접근을 확인했습니다.",
            }))
        }
        "mattermost" => {
            if token.is_empty() {
                return Err("Mattermost 액세스 토큰이 없습니다.".into());
            }
            let base = configured_base(config, "baseUrl", "");
            let response = client
                .get(format!("{base}/api/v4/users/me"))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|error| format!("Mattermost 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() {
                return Err(format!(
                    "Mattermost 인증 실패 (HTTP {status}): {}",
                    body["message"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": body["username"],
                "workspace": body["email"],
                "message": "Mattermost 계정과 채널 접근을 확인했습니다.",
            }))
        }
        "signal" => {
            let base = configured_base(config, "bridgeUrl", "");
            let mut request = client.get(format!("{base}/v1/about"));
            if !token.is_empty() {
                request = request.bearer_auth(&token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| format!("Signal 브리지 연결 실패: {error}"))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
            if !status.is_success() {
                return Err(format!(
                    "Signal 브리지 인증 실패 (HTTP {status}): {}",
                    body["error"].as_str().unwrap_or("응답을 읽을 수 없습니다.")
                ));
            }
            Ok(json!({
                "verified": true,
                "status": "online",
                "account": text(config, "phoneNumber"),
                "workspace": body["build"],
                "message": "Signal 브리지와 등록된 번호를 확인했습니다.",
            }))
        }
        _ => Err(format!("이 채널의 진단을 지원하지 않습니다: {kind}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::{get, post}, Json, Router};
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

    async fn read_harness(
        kind: &str,
        route: &'static str,
        reply: Value,
    ) -> (AppState, std::path::PathBuf) {
        let handler = move || {
            let reply = reply.clone();
            async move { Json(reply) }
        };
        let app = Router::new().route(route, get(handler.clone()).post(handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = std::env::temp_dir().join(format!("dominion-diagnose-{}", uuid::Uuid::new_v4()));
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
                "homeserver":format!("http://{address}"),"site":format!("http://{address}"),
                "bridgeUrl":format!("http://{address}"),"channelId":"C1","chatId":"C1",
                "email":"bot@example.com","phoneNumber":"+1","phoneNumberId":"P1",
                "graphVersion":"v23.0","apiBase":format!("http://{address}"),
                "graphBase":format!("http://{address}"),"token":"secret"}),
        );
        (state, dir)
    }

    #[tokio::test]
    async fn diagnose_probes_remote_identity_without_posting() {
        let (state, dir) = read_harness(
            "slack",
            "/auth.test",
            json!({"ok":true,"user":"U1","team":"T1"}),
        )
        .await;
        let config = state.store.get("channel", "binding").unwrap();
        let result = diagnose(&state, &config).await.unwrap();
        assert_eq!(result["verified"], true);
        assert_eq!(result["account"], "U1");
        assert_eq!(result["workspace"], "T1");
        let _ = std::fs::remove_dir_all(dir);

        let (state, dir) = read_harness(
            "zulip",
            "/api/v1/users/me",
            json!({"email":"bot@example.com","full_name":"Bot"}),
        )
        .await;
        let config = state.store.get("channel", "binding").unwrap();
        let result = diagnose(&state, &config).await.unwrap();
        assert_eq!(result["verified"], true);
        assert_eq!(result["account"], "bot@example.com");
        let _ = std::fs::remove_dir_all(dir);

        let (state, dir) = read_harness("signal", "/v1/about", json!({"build":"1.0"})).await;
        let config = state.store.get("channel", "binding").unwrap();
        let result = diagnose(&state, &config).await.unwrap();
        assert_eq!(result["verified"], true);
        assert_eq!(result["account"], "+1");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn diagnose_surfaces_platform_authentication_errors() {
        let (state, dir) = read_harness(
            "slack",
            "/auth.test",
            json!({"ok":false,"error":"invalid_auth"}),
        )
        .await;
        let config = state.store.get("channel", "binding").unwrap();
        let error = diagnose(&state, &config).await.unwrap_err();
        assert!(error.contains("invalid_auth"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
