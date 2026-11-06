use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::path::PathBuf;

/// The registry distinguishes DOMINION-owned executable drivers from planned connectors.
fn manifests() -> PathBuf {
    if let Some(assets) = std::env::var_os("DOMINION_ASSETS") {
        return PathBuf::from(assets).join("channels");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/channels")
}

pub fn catalog() -> Vec<Value> {
    let mut entries = Vec::new();
    let Ok(dirs) = std::fs::read_dir(manifests()) else {
        return entries;
    };
    for dir in dirs.flatten() {
        let path = dir.path().join("manifest.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let available = value["id"]
            .as_str()
            .is_some_and(|id| matches!(id, "discord" | "telegram")
                || crate::engine::channel_adapter::supports(id));
        value["available"] = json!(available);
        value["status"] = json!(if available {
            "available"
        } else {
            "unavailable"
        });
        value["requiresPublicUrl"] = json!(value
            .get("mode")
            .and_then(|mode| mode.as_str())
            .map(|mode| mode == "webhook")
            .unwrap_or(false));
        entries.push(value);
    }
    entries.sort_by(|a, b| {
        let left = (
            a.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            a.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        let right = (
            b.get("category").and_then(|v| v.as_str()).unwrap_or(""),
            b.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        );
        left.cmp(&right)
    });
    entries
}

pub fn connected_accounts(state: &AppState) -> Vec<Value> {
    state
        .store
        .all("oauth_config")
        .into_iter()
        .filter(|record| record.get("connected").and_then(|v| v.as_bool()) == Some(true))
        .map(|record| {
            let id = record
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            json!({
                "id": id,
                "provider": id,
                "displayName": record.get("account").cloned().unwrap_or(json!("연결된 계정")),
                "scopes": record.get("scopes").cloned().unwrap_or(json!("")),
                "hasToken": state.store.has_secret(&format!("oauth:{id}:token")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> (AppState, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dominion-channel-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        for index in 0..9 {
            state.store.put(
                "agent",
                &json!({"id":format!("a{index}"),"name":format!("참가자{index}")}),
            );
        }
        (state, dir)
    }

    #[tokio::test]
    async fn native_save_encrypts_tokens_and_accepts_nine_participants() {
        let (state, dir) = test_state();
        state.store.put(
            "room",
            &json!({"id":"room9","agentIds":(0..9).map(|i|format!("a{i}")).collect::<Vec<_>>()}),
        );
        let body = json!({"id":"binding","kind":"discord","guildId":"10","channelId":"20","roomId":"room9","token":"private-token","credentials":{"token":"nested-secret"},"enabled":false});
        let response = save_config(&state, None, body);
        assert_eq!(response.status(), StatusCode::OK);
        let public = String::from_utf8(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(!public.contains("private-token"));
        assert!(!public.contains("nested-secret"));
        let saved = state.store.get("channel", "binding").unwrap();
        assert!(saved.get("token").is_none());
        assert!(saved.get("credentials").is_none());
        assert_eq!(saved["driver"], "dominion-discord");
        assert_eq!(saved["replyMentionBots"], false);
        let secret = crate::engine::discord::token_name("binding");
        assert_eq!(state.read_secret(&secret).as_deref(), Some("private-token"));
        assert_ne!(
            state.store.get_secret(&secret).as_deref(),
            Some("private-token")
        );
        let response = save_config(
            &state,
            Some("binding"),
            json!({"name":"변경","token":"","replyMentionBots":true}),
        );
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.read_secret(&secret).as_deref(), Some("private-token"));
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn public_legacy_records_hide_tokens_and_require_native_activation() {
        let (state, dir) = test_state();
        let public = public_channel(
            &state,
            &json!({"id":"legacy","kind":"discord","enabled":true,"status":"connected","token":"legacy-secret","credentials":{"token":"nested-secret"}}),
        );
        assert!(!public.to_string().contains("secret"));
        assert_eq!(public["status"], "not_started");
        drop(state);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn telegram_configuration_uses_vault_room_binding_and_explicit_shared_sync() {
        let (state, dir) = test_state();
        state.store.put(
            "room",
            &json!({"id":"room","name":"공용","agentIds":["a0"]}),
        );
        let session = crate::engine::rooms::ensure_room_session(&state, "room").unwrap();
        state.store.write_message(
            &json!({"id":"old","session_id":session["id"],"role":"user","content":"기존 기록"}),
        );
        let response = save_config(
            &state,
            None,
            json!({"id":"tg","kind":"telegram","chatId":"-100","roomId":"room","allowedUserIds":["7"],"token":"fixture-token","enabled":false,"syncRoomMessages":true}),
        );
        assert_eq!(response.status(), StatusCode::OK);
        let config = state.store.get("channel", "tg").unwrap();
        assert_eq!(config["driver"], "dominion-telegram");
        assert_eq!(config["bridgeStartMessageId"], "old");
        assert!(config.get("token").is_none());
        assert_eq!(
            state
                .read_secret(&crate::engine::discord::token_name("tg"))
                .as_deref(),
            Some("fixture-token")
        );
        let public = public_channel(&state, &config);
        assert_eq!(public["chatId"], "-100");
        assert_eq!(public["syncRoomMessages"], true);
        assert!(!public.to_string().contains("fixture-token"));
        let response = save_config(&state, Some("tg"), json!({"allowedUserIds":[7]}));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = save_config(
            &state,
            Some("tg"),
            json!({"toolsEnabled":true,"allowedUserIds":[]}),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slack_adapter_is_saved_with_its_driver_and_required_fields() {
        let (state, dir) = test_state();
        let saved = save_config(
            &state,
            Some("slack-check"),
            json!({
                "kind":"slack","name":"Slack check","channelId":"C1",
                "enabled":false,"syncRoomMessages":false,"token":"xoxb-test"
            }),
        );
        assert_eq!(saved.status(), StatusCode::OK);
        let record = state.store.get("channel", "slack-check").unwrap();
        assert_eq!(record["driver"], "dominion-slack");
        assert_eq!(record["kind"], "slack");
        let missing = save_config(
            &state,
            Some("slack-bad"),
            json!({"kind":"slack","name":"bad","enabled":false,"token":"xoxb-test"}),
        );
        assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn catalog_exposes_only_implemented_drivers() {
        let entries = catalog();
        assert!(entries.len() >= 20, "expected the full channel registry");
        let discord = entries
            .iter()
            .find(|entry| entry["id"] == "discord")
            .expect("discord entry");
        assert_eq!(discord["available"], true);
        let implemented = ["discord", "telegram", "slack", "matrix", "whatsapp",
            "line", "zulip", "mattermost", "signal"];
        for kind in implemented {
            let entry = entries.iter().find(|e| e["id"] == kind)
                .unwrap_or_else(|| panic!("missing {kind}"));
            assert_eq!(entry["available"], true, "{entry}");
        }
        assert!(entries
            .iter()
            .filter(|e| !implemented.contains(&e["id"].as_str().unwrap_or("")))
            .all(|e| e["available"] == false), "{entries:?}");
        assert!(discord["fields"].as_array().is_some());
    }
}

pub async fn catalog_handler() -> Json<Value> {
    Json(serde_json::json!(catalog()))
}

pub async fn connections(State(state): State<AppState>) -> Json<Value> {
    Json(serde_json::json!(connected_accounts(&state)))
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}

/// Never return a submitted token, including legacy records written by the
/// generic collection API. Public configuration uses an explicit field list.
pub fn public_channel(state: &AppState, record: &Value) -> Value {
    let mut public = json!({});
    for key in [
        "id",
        "name",
        "kind",
        "agentId",
        "providerId",
        "model",
        "enabled",
        "mentionOnly",
        "roomId",
        "roomPolicy",
        "guildId",
        "channelId",
        "chatId",
        "syncRoomMessages",
        "allowedUserIds",
        "allowedBotIds",
        "replyMentionBots",
        "toolsEnabled",
        "driver",
        "status",
        "runtimeError",
        "revision",
        "createdAt",
        "updatedAt",
    ] {
        if let Some(value) = record.get(key) {
            public[key] = value.clone();
        }
    }
    let present = state.store.has_secret(&crate::engine::discord::token_name(
        crate::engine::discord::text(record, "id"),
    )) || record
        .get("token")
        .and_then(Value::as_str)
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let required = if record["kind"] == "telegram" {
        vec!["chatId", "roomId"]
    } else if let Some(kind) = record["kind"].as_str()
        .filter(|kind| crate::engine::channel_adapter::supports(kind))
    {
        crate::engine::channel_adapter::required_fields(kind)
            .iter()
            .copied()
            .chain(std::iter::once("roomId"))
            .collect()
    } else {
        vec!["guildId", "channelId"]
    };
    let missing = required
        .into_iter()
        .filter(|k| crate::engine::discord::text(record, k).is_empty())
        .collect::<Vec<_>>();
    public["credentialsPresent"] = json!({"token":present});
    public["configured"] = json!(present && missing.is_empty());
    public["missing"] = json!(missing);
    let outbox = state
        .store
        .all("channel_outbox")
        .into_iter()
        .filter(|item| item["bindingId"] == record["id"])
        .collect::<Vec<_>>();
    public["delivery"] = json!({"pending":outbox.iter().filter(|item|item["status"]=="pending").count(),"sending":outbox.iter().filter(|item|item["status"]=="sending").count(),
        "sent":outbox.iter().filter(|item|item["status"]=="sent").count(),"needsReview":outbox.iter().filter(|item|item["status"]=="needs_review").count(),
        "lastError":outbox.iter().find(|item|item["status"]=="needs_review").map(|item|item["error"].clone())});
    if record.get("enabled").and_then(Value::as_bool) != Some(true) {
        public["status"] = json!("disabled");
    } else if !matches!(
        (record["kind"].as_str(), record["driver"].as_str()),
        (Some("discord"), Some("dominion-discord")) | (Some("telegram"), Some("dominion-telegram"))
    ) && !record["kind"].as_str().is_some_and(|kind| {
        crate::engine::channel_adapter::supports(kind)
            && record["driver"].as_str() == Some(format!("dominion-{kind}").as_str())
    }) {
        public["status"] = json!("not_started");
        public["runtimeError"] = json!("Rust 연결 설정에서 저장·활성화가 필요합니다.");
    }
    public
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state
        .store
        .all("channel")
        .iter()
        .map(|c| public_channel(&state, c))
        .collect::<Vec<_>>()))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("channel", &id) {
        Some(c) => Json(public_channel(&state, &c)).into_response(),
        None => error(StatusCode::NOT_FOUND, "연결을 찾을 수 없습니다."),
    }
}

pub async fn save(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    save_config(&state, None, body)
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    save_config(&state, Some(&id), body)
}

fn save_config(state: &AppState, path_id: Option<&str>, body: Value) -> Response {
    if !body.is_object() {
        return error(StatusCode::BAD_REQUEST, "연결 설정 객체가 필요합니다.");
    }
    let id = path_id
        .map(str::to_string)
        .or_else(|| {
            body.get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if id.len() > 128 || id.chars().any(|c| c.is_control() || c == '/' || c == ':') {
        return error(StatusCode::BAD_REQUEST, "연결 ID가 올바르지 않습니다.");
    }
    let existing = state.store.get("channel", &id);
    let mut config = json!({"id":id,"kind":"discord","agentId":"general","enabled":false,"mentionOnly":true,"replyMentionBots":false,"toolsEnabled":false,"syncRoomMessages":false,"roomPolicy":"all_or_named"});
    for key in [
        "name",
        "kind",
        "agentId",
        "providerId",
        "model",
        "enabled",
        "mentionOnly",
        "roomId",
        "roomPolicy",
        "guildId",
        "channelId",
        "chatId",
        "syncRoomMessages",
        "allowedUserIds",
        "allowedBotIds",
        "replyMentionBots",
        "toolsEnabled",
        "homeserver",
        "userId",
        "phoneNumberId",
        "graphVersion",
        "site",
        "email",
        "baseUrl",
        "phoneNumber",
        "bridgeUrl",
        "topic",
        "apiBase",
        "graphBase",
    ] {
        if let Some(value) = body
            .get(key)
            .or_else(|| existing.as_ref().and_then(|v| v.get(key)))
        {
            config[key] = value.clone();
        }
    }
    for key in [
        "enabled",
        "mentionOnly",
        "replyMentionBots",
        "toolsEnabled",
        "syncRoomMessages",
    ] {
        if !config[key].is_boolean() {
            return error(
                StatusCode::BAD_REQUEST,
                format!("{key}는 true/false여야 합니다."),
            );
        }
    }
    for key in ["allowedUserIds", "allowedBotIds"] {
        if let Some(value) = config.get(key) {
            if !value.is_string()
                && !value
                    .as_array()
                    .is_some_and(|ids| ids.iter().all(Value::is_string))
            {
                return error(
                    StatusCode::BAD_REQUEST,
                    format!("{key}에는 문자열 ID 목록이 필요합니다."),
                );
            }
        }
    }
    let kind = config["kind"].as_str().unwrap_or("");
    config["driver"] = json!(if kind == "telegram" {
        "dominion-telegram".to_string()
    } else if crate::engine::channel_adapter::supports(kind) {
        format!("dominion-{kind}")
    } else {
        "dominion-discord".to_string()
    });
    config["revision"] = json!(uuid::Uuid::new_v4().to_string());
    config["status"] = json!(
        if config.get("enabled").and_then(Value::as_bool) == Some(true) {
            "starting"
        } else {
            "disabled"
        }
    );
    let kind = config["kind"].as_str().unwrap_or("");
    let validation = if kind == "telegram" {
        crate::engine::telegram::validate(state, &config)
    } else if crate::engine::channel_adapter::supports(kind) {
        crate::engine::channel_adapter::validate(state, &config)
    } else {
        crate::engine::discord::validate(state, &config)
    };
    if let Err(message) = validation {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && !s.starts_with("••"))
        .or_else(|| {
            existing
                .as_ref()
                .and_then(|v| v.get("token"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        });
    let secret_name = crate::engine::discord::token_name(&id);
    if token.is_none() && !state.store.has_secret(&secret_name) {
        return error(StatusCode::BAD_REQUEST, "채널 토큰을 입력하세요.");
    }
    if let Some(token) = token {
        if token.len() > 512 || token.chars().any(char::is_whitespace) {
            return error(
                StatusCode::BAD_REQUEST,
                "Bot Token 형식이 올바르지 않습니다.",
            );
        }
        state.save_secret(&secret_name, token);
    }
    if !crate::engine::discord::text(&config, "roomId").is_empty() {
        match crate::engine::rooms::ensure_room_session(
            state,
            crate::engine::discord::text(&config, "roomId"),
        ) {
            Ok(session) => {
                config["bridgeStartMessageId"] = state
                    .store
                    .messages(crate::engine::discord::text(&session, "id"))
                    .last()
                    .map(|message| message["id"].clone())
                    .unwrap_or(Value::Null)
            }
            Err(message) => return error(StatusCode::BAD_REQUEST, message),
        }
    }
    match state.store.put("channel", &config) {
        Some(record) => {
            let public = public_channel(state, &record);
            state.bus.record("channel", &public);
            state.store.audit(
                "channel.saved",
                json!({"connectionId":id,"kind":config["kind"],"enabled":config.get("enabled")}),
            );
            Json(public).into_response()
        }
        None => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "연결 설정을 저장할 수 없습니다.",
        ),
    }
}

pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("channel", &id);
    state
        .store
        .remove_secret(&crate::engine::discord::token_name(&id));
    state
        .bus
        .emit(json!({"type":"record","kind":"channel","removed":id}));
    Json(json!({"ok":true}))
}

pub async fn diagnose(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(config) = state.store.get("channel", &id) else {
        return error(StatusCode::NOT_FOUND, "연결을 찾을 수 없습니다.");
    };
    let result = match config["kind"].as_str() {
        Some("telegram") => crate::engine::telegram::diagnose(&state, &config).await,
        Some("discord") => crate::engine::discord::diagnose(&state, &config).await,
        Some(kind) if crate::engine::channel_adapter::supports(kind) => {
            let public = public_channel(&state, &config);
            let configured = public["configured"] == true;
            return Json(json!({
                "verified": false,
                "configured": configured,
                "status": public["status"],
                "missing": public["missing"],
                "message": if configured {
                    "필수 설정을 확인했습니다. 이 채널의 원격 인증과 전송 결과는 아직 확인하지 않았습니다."
                } else {
                    "필수 설정 또는 인증 키가 비어 있습니다. 연결 설정을 확인하세요."
                },
            })).into_response();
        }
        _ => return error(StatusCode::NOT_IMPLEMENTED, "이 채널의 진단을 지원하지 않습니다."),
    };
    match result {
        Ok(result) => Json(result).into_response(),
        Err(message) => Json(json!({"verified":false,"message":message})).into_response(),
    }
}
