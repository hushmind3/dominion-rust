//! One canonical room ledger with durable, bounded transport outboxes.
//! A transport never creates nine independent identities or a second history.
use crate::{
    engine::{
        discord::{ids, text},
        memory,
    },
    state::AppState,
};
use serde_json::{json, Value};
use std::future::Future;

pub fn ingest(
    state: &AppState,
    config: &Value,
    message_id: &str,
    author_id: &str,
    author_name: &str,
    content: &str,
    author_bot: bool,
) -> Result<Value, String> {
    if author_bot {
        return Err("봇 발언은 방 주인 명령으로 수집하지 않습니다.".into());
    }
    if config["enabled"] != true {
        return Err("채널이 비활성 상태입니다.".into());
    }
    if !current(state, config) {
        return Err("채널 설정이 변경되었거나 연결이 중지되었습니다.".into());
    }
    if is_sent(state, text(config, "id"), message_id) {
        return Ok(json!({"ignored":"own outbound message"}));
    }
    let allowed = ids(config, "allowedUserIds");
    if !allowed.is_empty() && !allowed.iter().any(|id| id == author_id) {
        return Err("허용되지 않은 채널 사용자입니다.".into());
    }
    let can_use_tools = config["toolsEnabled"] == true && allowed.iter().any(|id| id == author_id);
    let origin = json!({"kind":config["kind"],"channelBindingId":config["id"],"messageId":message_id,"authorId":author_id,"authorName":author_name,"authorKind":"human","authorIsBot":false});
    crate::api::rooms::submit_internal(state,text(config,"roomId"),&json!({"input":content,"clientMessageId":format!("bridge-{}",memory::stable_id(&[text(config,"kind"),text(config,"guildId"),text(config,"channelId"),text(config,"chatId"),message_id])),"thinking":false,"reasoningEffort":"low"}),Some(origin),can_use_tools).map_err(|(_,error)|error)
}
pub fn is_sent(state: &AppState, binding: &str, remote_id: &str) -> bool {
    state
        .store
        .get("channel_sent", &format!("{binding}:{remote_id}"))
        .is_some()
}
fn same_binding(origin: &Value, binding: &str) -> bool {
    origin["channelBindingId"].as_str() == Some(binding)
}
fn eligible(state: &AppState, config: &Value, message: &Value) -> bool {
    if message["role"] == "user" {
        return message["metadata"]["authorKind"] == "owner"
            && config["syncRoomMessages"] == true
            && !same_binding(&message["metadata"]["origin"], text(config, "id"));
    }
    if message["role"] != "assistant"
        || message["tool_calls"].is_array()
        || message["content"].as_str().unwrap_or("").trim().is_empty()
    {
        return false;
    }
    let source = message["metadata"]["replyToMessageId"]
        .as_str()
        .and_then(|id| state.store.message(id));
    config["syncRoomMessages"] == true
        || source
            .as_ref()
            .is_some_and(|source| same_binding(&source["metadata"]["origin"], text(config, "id")))
}
fn split(content: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut units = 0;
    for ch in content.chars() {
        if units + ch.len_utf16() > limit && !part.is_empty() {
            parts.push(std::mem::take(&mut part));
            units = 0;
        }
        part.push(ch);
        units += ch.len_utf16();
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}
fn formatted(state: &AppState, message: &Value) -> String {
    let label = if message["role"] == "assistant" {
        message["agent_id"]
            .as_str()
            .and_then(|id| state.store.get("agent", id))
            .and_then(|agent| agent["name"].as_str().map(str::to_string))
            .unwrap_or_else(|| "DOMINION".into())
    } else {
        message["metadata"]["origin"]["authorName"]
            .as_str()
            .or_else(|| message["metadata"]["ownerName"].as_str())
            .unwrap_or("방 주인")
            .to_string()
    };
    format!("[{label}]\n{}", message["content"].as_str().unwrap_or("")).replace('@', "＠")
}
fn current(state: &AppState, config: &Value) -> bool {
    state
        .store
        .get("channel", text(config, "id"))
        .is_some_and(|current| {
            current["enabled"] == true && current["revision"] == config["revision"]
        })
}
/// The callback performs exactly one external request, with no retry on an
/// unknown write outcome. Pending records may run; sending records may not.
pub async fn flush<F, Fut>(
    state: &AppState,
    config: &Value,
    limit: usize,
    mut send: F,
) -> Result<usize, String>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    if text(config, "roomId").is_empty() {
        return Ok(0);
    }
    let key = format!("bridge-send:{}", text(config, "id"));
    if !state.begin_session(&key) {
        return Ok(0);
    }
    let _guard = state.session_guard(&key);
    let session = crate::engine::rooms::ensure_room_session(state, text(config, "roomId"))?;
    let session_id = text(&session, "id");
    let cursor_id = format!("{}:{}", text(config, "id"), text(config, "revision"));
    let cursor = state
        .store
        .get("bridge_cursor", &cursor_id)
        .unwrap_or_else(|| json!({"id":cursor_id,"lastMessageId":config["bridgeStartMessageId"]}));
    let messages = state.store.messages(session_id);
    let start = cursor["lastMessageId"]
        .as_str()
        .and_then(|id| messages.iter().position(|message| message["id"] == id))
        .map(|index| index + 1)
        .unwrap_or(0);
    let mut sent_count = 0;
    for message in messages[start..].iter().take(12) {
        if !current(state, config) || state.is_cancelled(session_id) {
            break;
        }
        if eligible(state, config, message) {
            let parts = split(&formatted(state, message), limit);
            if parts.len() > 20 {
                let id = format!(
                    "outbox-{}",
                    memory::stable_id(&[
                        text(config, "id"),
                        text(config, "revision"),
                        text(message, "id"),
                        "oversize"
                    ])
                );
                state.store.put("channel_outbox",&json!({"id":id,"bindingId":config["id"],"messageId":message["id"],"status":"needs_review","error":"20개 조각을 초과하여 전송하지 않았습니다. 전체 원문은 공용 대화에 보관했습니다."})).ok_or("대용량 전송 검토 상태를 저장할 수 없습니다.")?;
                state.store.put("bridge_cursor",&json!({"id":cursor_id,"lastMessageId":message["id"],"sessionId":session_id})).ok_or("전송 위치를 저장할 수 없습니다.")?;
                continue;
            }
            for (index, part) in parts.into_iter().enumerate() {
                if !current(state, config) || state.is_cancelled(session_id) {
                    return Ok(sent_count);
                }
                let id = format!(
                    "outbox-{}",
                    memory::stable_id(&[
                        text(config, "id"),
                        text(config, "revision"),
                        text(message, "id"),
                        &index.to_string()
                    ])
                );
                let mut record=match state.store.get("channel_outbox",&id){Some(record)=>record,None=>state.store.put_if_absent("channel_outbox",&json!({"id":id,"bindingId":config["id"],"revision":config["revision"],"roomId":config["roomId"],"sessionId":session_id,"messageId":message["id"],"partIndex":index,"content":part,"status":"pending"})).ok_or("전송 대기를 저장할 수 없습니다.")?};
                match text(&record, "status") {
                    "sent" => continue,
                    "needs_review" | "cancelled" => break,
                    "sending" => {
                        record["status"] = json!("needs_review");
                        record["error"]=json!("전송 중 중단되어 실제 전송 여부를 확인해야 합니다. 자동 재전송하지 않습니다.");
                        state
                            .store
                            .put("channel_outbox", &record)
                            .ok_or("전송 확인 상태를 저장할 수 없습니다.")?;
                        break;
                    }
                    _ => {}
                }
                record["status"] = json!("sending");
                record["startedAt"] = json!(chrono::Utc::now().to_rfc3339());
                state
                    .store
                    .put("channel_outbox", &record)
                    .ok_or("전송 시작을 저장할 수 없습니다.")?;
                match send(part).await {
                    Ok(remote_id) => {
                        if remote_id.is_empty() {
                            return Err(
                                "전송 결과 메시지 ID가 없습니다. 전송 기록 검토가 필요합니다."
                                    .into(),
                            );
                        }
                        state.store.put("channel_sent",&json!({"id":format!("{}:{remote_id}",text(config,"id")),"bindingId":config["id"],"remoteMessageId":remote_id,"messageId":message["id"]})).ok_or("전송 메시지 식별자를 저장할 수 없습니다.")?;
                        record["status"] = json!("sent");
                        record["remoteMessageId"] = json!(remote_id);
                        record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
                        sent_count += 1;
                    }
                    Err(error) => {
                        record["status"] = json!("needs_review");
                        record["error"] = json!(error);
                    }
                }
                let saved = state
                    .store
                    .put("channel_outbox", &record)
                    .ok_or("전송 결과를 저장할 수 없습니다.")?;
                state.bus.record("channel_outbox", &saved);
                if saved["status"] == "needs_review" {
                    break;
                }
            }
        }
        state
            .store
            .put(
                "bridge_cursor",
                &json!({"id":cursor_id,"lastMessageId":message["id"],"sessionId":session_id}),
            )
            .ok_or("공용 대화 전송 위치를 저장할 수 없습니다.")?;
    }
    Ok(sent_count)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!("dominion-bridge-{}", uuid::Uuid::new_v4()));
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
        state.store.put(
            "room",
            &json!({"id":"room","agentIds":["a"],"toolsEnabled":true}),
        );
        state
    }
    fn config() -> Value {
        json!({"id":"tg","kind":"telegram","roomId":"room","enabled":true,"revision":"r","toolsEnabled":true,"allowedUserIds":["10"],"syncRoomMessages":false})
    }
    #[tokio::test]
    async fn inbound_is_once_in_canonical_ledger_and_outbound_has_no_echo_or_retry() {
        let state = state();
        let config = config();
        state.store.put("channel", &config);
        let first = ingest(&state, &config, "1", "10", "실제 사람", "안녕", false).unwrap();
        let again = ingest(&state, &config, "1", "10", "실제 사람", "안녕", false).unwrap();
        assert_eq!(first["message"]["id"], again["message"]["id"]);
        let session = text(&first, "sessionId");
        assert_eq!(state.store.messages(session).len(), 1);
        assert_eq!(first["job"]["input"]["roomSnapshot"]["toolsEnabled"], true);
        assert!(ingest(&state, &config, "2", "20", "unknown", "no", false).is_err());
        assert!(ingest(&state, &config, "3", "10", "bot", "no", true).is_err());
        state.store.write_message(&json!({"id":"reply","session_id":session,"role":"assistant","agent_id":"a","content":"실제 답변","metadata":{"authorKind":"agent","replyToMessageId":first["message"]["id"]}}));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        assert_eq!(
            flush(&state, &config, 4000, move |content| {
                assert!(content.starts_with("[참가자]"));
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err("unknown write outcome".into()) }
            })
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            state.store.all("channel_outbox")[0]["status"],
            "needs_review"
        );
        flush(&state, &config, 4000, |_| async {
            panic!("must never retry uncertain send")
        })
        .await
        .unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn explicit_sync_mirrors_other_humans_and_web_speakers_without_historical_blast() {
        let state = state();
        let mut config = config();
        let session = crate::engine::rooms::ensure_room_session(&state, "room").unwrap();
        let session = text(&session, "id");
        state.store.write_message(&json!({"id":"old","session_id":session,"role":"assistant","agent_id":"a","content":"old history"}));
        config["bridgeStartMessageId"] = json!("old");
        config["syncRoomMessages"] = json!(true);
        state.store.put("channel", &config);
        state.store.write_message(&json!({"id":"new","session_id":session,"role":"user","content":"web owner message","metadata":{"authorKind":"owner"}}));
        assert_eq!(
            flush(&state, &config, 4000, |content| {
                assert!(content.contains("web owner message"));
                async { Ok("remote-one".into()) }
            })
            .await
            .unwrap(),
            1
        );
        assert!(is_sent(&state, "tg", "remote-one"));
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
    #[tokio::test]
    async fn crashed_sending_part_is_never_replayed_or_continued_automatically() {
        let state = state();
        let mut config = config();
        config["syncRoomMessages"] = json!(true);
        state.store.put("channel", &config);
        let session = crate::engine::rooms::ensure_room_session(&state, "room").unwrap();
        let session = text(&session, "id");
        state.store.write_message(&json!({"id":"native","session_id":session,"role":"assistant","agent_id":"a","content":"a repeated long response that needs multiple chunks"}));
        let id = format!("outbox-{}", memory::stable_id(&["tg", "r", "native", "0"]));
        state.store.put(
            "channel_outbox",
            &json!({"id":id,"bindingId":"tg","messageId":"native","status":"sending"}),
        );
        flush(&state, &config, 10, |_| async {
            panic!("crashed delivery must not replay any part")
        })
        .await
        .unwrap();
        assert_eq!(
            state.store.get("channel_outbox", &id).unwrap()["status"],
            "needs_review"
        );
        assert_eq!(
            state.store.all("bridge_cursor")[0]["lastMessageId"],
            "native"
        );
        let _ = std::fs::remove_dir_all(&state.config.data_dir);
    }
}
