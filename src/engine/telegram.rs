//! Native Telegram Bot API polling. Only an explicitly enabled binding opens a
//! connection; credentials stay in the vault and are never reported in errors.
use crate::{
    engine::{
        bridge,
        discord::{ids, text, token_name},
    },
    state::AppState,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;
const API: &str = "https://api.telegram.org";
pub fn numeric_id(id: &str) -> bool {
    id.parse::<i64>().is_ok_and(|id| id != 0) && id.len() <= 21
}
pub fn validate(state: &AppState, config: &Value) -> Result<(), String> {
    if config["kind"] != "telegram" || !numeric_id(text(config, "chatId")) {
        return Err("Telegram chatId(그룹은 음수 ID)를 확인하세요.".into());
    }
    if text(config, "roomId").is_empty()
        || state.store.get("room", text(config, "roomId")).is_none()
    {
        return Err("Telegram에 연결할 공용 그룹 대화를 선택하세요.".into());
    }
    if ids(config, "allowedUserIds")
        .iter()
        .any(|id| !numeric_id(id) || id.starts_with('-'))
    {
        return Err("허용 사용자에는 양의 Telegram 사용자 ID를 입력하세요.".into());
    }
    if config["toolsEnabled"] == true && ids(config, "allowedUserIds").is_empty() {
        return Err("원격 도구 실행에는 명시적인 allowedUserIds가 필요합니다.".into());
    }
    Ok(())
}
#[derive(Clone)]
struct Client {
    http: reqwest::Client,
    token: String,
    base: String,
}
impl Client {
    fn new(token: String) -> Result<Self, String> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "Telegram HTTP 클라이언트를 만들 수 없습니다.")?,
            token,
            base: API.into(),
        })
    }
    async fn call(&self, method: &str, body: Value) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/bot{}/{}", self.base, self.token, method))
            .json(&body)
            .send()
            .await
            .map_err(|_| "Telegram 네트워크 요청의 결과를 확인할 수 없습니다.".to_string())?;
        let status = response.status();
        let result: Value = response
            .json()
            .await
            .map_err(|_| "Telegram 응답 형식을 확인할 수 없습니다.")?;
        if !status.is_success() || result["ok"] != true {
            return Err(format!(
                "Telegram HTTP {} / API {}: 권한·설정 또는 요청 제한을 확인하세요.",
                status.as_u16(),
                result["error_code"].as_i64().unwrap_or(0)
            ));
        }
        Ok(result["result"].clone())
    }
}
fn client(state: &AppState, config: &Value) -> Result<Client, String> {
    Client::new(
        state
            .read_secret(&token_name(text(config, "id")))
            .ok_or("Telegram Bot Token이 없습니다.")?,
    )
}
pub async fn diagnose(state: &AppState, config: &Value) -> Result<Value, String> {
    validate(state, config)?;
    let client = client(state, config)?;
    let me = client.call("getMe", json!({})).await?;
    if me["is_bot"] != true {
        return Err("Telegram Bot Token을 확인하세요.".into());
    }
    let chat = client
        .call("getChat", json!({"chat_id":text(config,"chatId")}))
        .await?;
    Ok(
        json!({"verified":true,"botId":me["id"],"username":me["username"],"chatTitle":chat["title"],"message":"Telegram 봇 인증과 그룹 접근을 확인했습니다. 메시지 수신은 봇의 그룹 개인정보 보호 설정에 따릅니다."}),
    )
}
fn accepts(
    state: &AppState,
    config: &Value,
    message: &Value,
    bot_id: &str,
    username: &str,
) -> bool {
    let author = &message["from"];
    let author_id = author["id"]
        .as_i64()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let chat = message["chat"]["id"]
        .as_i64()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let remote = message["message_id"]
        .as_i64()
        .map(|id| id.to_string())
        .unwrap_or_default();
    if chat != text(config, "chatId")
        || author_id.is_empty()
        || author_id == bot_id
        || author["is_bot"] == true
        || message["sender_chat"].is_object()
        || text(message, "text").trim().is_empty()
        || bridge::is_sent(state, text(config, "id"), &remote)
    {
        return false;
    }
    let users = ids(config, "allowedUserIds");
    if !users.is_empty() && !users.contains(&author_id) {
        return false;
    }
    if config["mentionOnly"] == true && message["chat"]["type"] != "private" {
        let mentioned = !username.is_empty()
            && text(message, "text")
                .to_lowercase()
                .contains(&format!("@{}", username.to_lowercase()));
        let reply = message["reply_to_message"]["from"]["id"]
            .as_i64()
            .map(|id| id.to_string())
            .as_deref()
            == Some(bot_id);
        if !mentioned && !reply {
            return false;
        }
    }
    true
}
async fn poll(state: &AppState, config: &Value) -> Result<(), String> {
    poll_with(state, config, &client(state, config)?).await
}
async fn poll_with(state: &AppState, config: &Value, client: &Client) -> Result<(), String> {
    validate(state, config)?;
    let token_lock = format!(
        "telegram-token:{}",
        crate::engine::memory::stable_id(&[&client.token])
    );
    if !state.begin_session(&token_lock) {
        return Ok(());
    }
    let _token_guard = state.session_guard(&token_lock);
    let id = format!("{}:{}", text(config, "id"), text(config, "revision"));
    let runtime = state.store.get("telegram_cursor", &id);
    let identity = if let Some(runtime) = &runtime {
        runtime["identity"].clone()
    } else {
        client.call("getMe", json!({})).await?
    };
    if identity["is_bot"] != true {
        return Err("Telegram 봇 인증이 필요합니다.".into());
    }
    let bot_id = identity["id"]
        .as_i64()
        .ok_or("Telegram 봇 ID가 없습니다.")?
        .to_string();
    // getUpdates consumes a bot-wide stream; two independent cursors must not
    // race one token and silently acknowledge each other's groups.
    for other in state.store.all("channel_identity") {
        if other["kind"] == "telegram"
            && other["botId"] == bot_id
            && other["id"] != config["id"]
            && other["id"]
                .as_str()
                .and_then(|id| state.store.get("channel", id))
                .is_some_and(|binding| binding["enabled"] == true)
        {
            return Err("하나의 Telegram 봇은 하나의 활성 연결에서만 수신할 수 있습니다.".into());
        }
    }
    state
        .store
        .put(
            "channel_identity",
            &json!({"id":config["id"],"kind":"telegram","botId":bot_id}),
        )
        .ok_or("Telegram 봇 식별자를 저장할 수 없습니다.")?;
    let offset = runtime
        .as_ref()
        .and_then(|runtime| runtime["offset"].as_i64())
        .unwrap_or(-1);
    let updates=client.call("getUpdates",json!({"offset":offset,"limit":if offset<0{1}else{100},"timeout":if offset<0{0}else{3},"allowed_updates":["message"]})).await?;
    let updates = updates
        .as_array()
        .ok_or("Telegram 수신 목록 형식이 올바르지 않습니다.")?;
    let mut next = if offset < 0 { 0 } else { offset };
    for update in updates {
        let update_id = update["update_id"]
            .as_i64()
            .ok_or("Telegram update_id가 없습니다.")?;
        if offset >= 0 && update_id < offset {
            continue;
        }
        if offset >= 0 {
            let message = &update["message"];
            if accepts(state, config, message, &bot_id, text(&identity, "username")) {
                let user = &message["from"];
                let author_id = user["id"].as_i64().unwrap_or(0).to_string();
                let remote = message["message_id"].as_i64().unwrap_or(0).to_string();
                let name = format!("{} {}", text(user, "first_name"), text(user, "last_name"))
                    .trim()
                    .to_string();
                bridge::ingest(
                    state,
                    config,
                    &remote,
                    &author_id,
                    &name,
                    text(message, "text"),
                    false,
                )?;
            }
        }
        next = next.max(update_id.saturating_add(1));
    }
    state
        .store
        .put(
            "telegram_cursor",
            &json!({"id":id,"offset":next,"identity":identity}),
        )
        .ok_or("Telegram 수신 위치를 저장할 수 없습니다.")?;
    bridge::flush(state,config,4000,|text|{
        let client=client.clone();let chat_id=text_field(config,"chatId");
        async move{let sent=client.call("sendMessage",json!({"chat_id":chat_id,"text":text,"link_preview_options":{"is_disabled":true}})).await?;sent["message_id"].as_i64().filter(|id|*id>0).map(|id|id.to_string()).ok_or("Telegram 전송 결과 ID가 없습니다.".into())}
    }).await?;
    Ok(())
}
fn text_field(config: &Value, key: &str) -> String {
    text(config, key).to_string()
}
pub fn start(state: AppState) {
    tokio::spawn(async move {
        loop {
            let configs = state
                .store
                .all("channel")
                .into_iter()
                .filter(|config| {
                    config["kind"] == "telegram"
                        && config["driver"] == "dominion-telegram"
                        && config["enabled"] == true
                })
                .collect::<Vec<_>>();
            futures_util::stream::iter(configs)
                .for_each_concurrent(8, |config| {
                    let state = state.clone();
                    async move {
                        let key = format!("telegram-poll:{}", text(&config, "id"));
                        if !state.begin_session(&key) {
                            return;
                        }
                        let _guard = state.session_guard(&key);
                        let result = poll(&state, &config).await;
                        let patch = match result {
                            Ok(_) => json!({"status":"connected","runtimeError":null}),
                            Err(error) => json!({"status":"error","runtimeError":error}),
                        };
                        if let Some(saved) = state.store.patch_if(
                            "channel",
                            text(&config, "id"),
                            "revision",
                            &config["revision"],
                            &patch,
                        ) {
                            state.bus.record("channel", &saved);
                        }
                    }
                })
                .await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!("dominion-telegram-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root,
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        state.store.put("agent", &json!({"id":"a","name":"참가자"}));
        state
            .store
            .put("room", &json!({"id":"room","agentIds":["a"]}));
        state
    }
    #[tokio::test]
    async fn official_http_flow_preserves_offsets_shared_room_and_real_outbox_results() {
        use axum::{
            extract::{Path, State},
            routing::post,
            Json, Router,
        };
        #[derive(Clone)]
        struct Mock {
            requests: std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
            updates: std::sync::Arc<std::sync::Mutex<usize>>,
        }
        async fn handler(
            State(mock): State<Mock>,
            Path((_token, method)): Path<(String, String)>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            mock.requests
                .lock()
                .unwrap()
                .push((method.clone(), body.clone()));
            let result = match method.as_str() {
                "getMe" => json!({"id":99,"is_bot":true,"username":"fixture_bot"}),
                "getUpdates" => {
                    let mut count = mock.updates.lock().unwrap();
                    *count += 1;
                    if *count == 1 {
                        json!([{"update_id":1}])
                    } else {
                        json!([{"update_id":2,"message":{"message_id":10,"chat":{"id":-100},"from":{"id":7,"first_name":"사람","is_bot":false},"text":"안녕"}}])
                    }
                }
                "sendMessage" => json!({"message_id":20}),
                _ => json!({}),
            };
            Json(json!({"ok":true,"result":result}))
        }
        let mock = Mock {
            requests: Default::default(),
            updates: Default::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/:token/:method", post(handler))
            .with_state(mock.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let state = state();
        let config = json!({"id":"tg","kind":"telegram","driver":"dominion-telegram","enabled":true,"roomId":"room","chatId":"-100","allowedUserIds":["7"],"revision":"one","syncRoomMessages":true});
        state.store.put("channel", &config);
        let client = Client {
            http: reqwest::Client::new(),
            token: "fixture".into(),
            base: format!("http://{addr}"),
        };
        poll_with(&state, &config, &client).await.unwrap();
        assert_eq!(state.store.count("job"), 0);
        poll_with(&state, &config, &client).await.unwrap();
        assert_eq!(state.store.count("job"), 1);
        let session = crate::engine::rooms::ensure_room_session(&state, "room").unwrap();
        let messages = state.store.messages(text(&session, "id"));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["metadata"]["origin"]["authorName"], "사람");
        state.store.write_message(&json!({"session_id":session["id"],"role":"assistant","agent_id":"a","content":"공용 답변","metadata":{"replyToMessageId":messages[0]["id"]}}));
        poll_with(&state, &config, &client).await.unwrap();
        assert_eq!(state.store.count("job"), 1);
        assert_eq!(state.store.all("channel_outbox")[0]["status"], "sent");
        assert!(bridge::is_sent(&state, "tg", "20"));
        let requests = mock.requests.lock().unwrap();
        assert!(requests
            .iter()
            .any(|(method, body)| method == "getUpdates" && body["offset"] == 2));
        assert_eq!(
            requests
                .iter()
                .filter(|(method, _)| method == "sendMessage")
                .count(),
            1
        );
        server.abort();
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
}
