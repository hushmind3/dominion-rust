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
        let available = value.get("id").and_then(Value::as_str) == Some("discord");
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
    fn catalog_exposes_only_implemented_drivers() {
        let entries = catalog();
        assert!(entries.len() >= 20, "expected the full channel registry");
        let discord = entries
            .iter()
            .find(|entry| entry["id"] == "discord")
            .expect("discord entry");
        assert_eq!(discord["available"], true);
        assert!(entries
            .iter()
            .filter(|e| e["id"] != "discord")
            .all(|e| e["available"] == false));
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
    let missing = ["guildId", "channelId"]
        .into_iter()
        .filter(|k| crate::engine::discord::text(record, k).is_empty())
        .collect::<Vec<_>>();
    public["credentialsPresent"] = json!({"token":present});
    public["configured"] = json!(present && missing.is_empty());
    public["missing"] = json!(missing);
    if record.get("enabled").and_then(Value::as_bool) != Some(true) {
        public["status"] = json!("disabled");
    } else if crate::engine::discord::text(record, "driver") != "dominion-discord" {
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
    let mut config = json!({"id":id,"kind":"discord","agentId":"general","enabled":false,"mentionOnly":true,"replyMentionBots":false,"toolsEnabled":false,"roomPolicy":"all_or_named"});
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
        "allowedUserIds",
        "allowedBotIds",
        "replyMentionBots",
        "toolsEnabled",
    ] {
        if let Some(value) = body
            .get(key)
            .or_else(|| existing.as_ref().and_then(|v| v.get(key)))
        {
            config[key] = value.clone();
        }
    }
    config["driver"] = json!("dominion-discord");
    config["revision"] = json!(uuid::Uuid::new_v4().to_string());
    config["status"] = json!(
        if config.get("enabled").and_then(Value::as_bool) == Some(true) {
            "starting"
        } else {
            "disabled"
        }
    );
    if let Err(message) = crate::engine::discord::validate(state, &config) {
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
        return error(StatusCode::BAD_REQUEST, "Discord Bot Token을 입력하세요.");
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
    match state.store.put("channel", &config) {
        Some(record) => {
            let public = public_channel(state, &record);
            state.bus.record("channel", &public);
            state.store.audit(
                "channel.saved",
                json!({"connectionId":id,"kind":"discord","enabled":config.get("enabled")}),
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
    match crate::engine::discord::diagnose(&state, &config).await {
        Ok(result) => Json(result).into_response(),
        Err(message) => Json(json!({"verified":false,"message":message})).into_response(),
    }
}
