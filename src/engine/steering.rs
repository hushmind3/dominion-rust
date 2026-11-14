//! Durable native chat steering at the next model-step boundary.
//!
//! Inspired by DeepSeek Harness's step-boundary inbox,
//! implemented independently against DOMINION's own SQLite store and Rust
//! model loop. Only an explicit owner/SDK/API call can enqueue a correction.
//! A model output, tool result or retrieved document cannot submit a steer.
use crate::state::AppState;
use serde_json::{json, Value};

/// Queue the owner's literal update. A running turn reads it before its next
/// model request; an idle session reads it during its next turn.
pub fn enqueue(state: &AppState, session_id: &str, instruction: &str) -> Result<Value, String> {
    let session = state.store.get("session", session_id)
        .ok_or_else(|| "세션을 찾을 수 없습니다.".to_string())?;
    if session["roomId"].is_string() || session["channelContext"].is_object() {
        return Err("그룹·외부 채널의 지시는 각 채널 사용자 입력에서 수정하세요.".into());
    }
    if session["archived"] == true {
        return Err("보관한 세션은 새 지시를 받을 수 없습니다.".into());
    }
    if instruction.trim().is_empty() || instruction.len() > 64_000 {
        return Err("새 지시는 1..64000바이트여야 합니다.".into());
    }
    let queued = json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "sessionId": session_id,
        "status": "queued",
        "instruction": instruction,
        "source": "owner",
        "queuedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
    });
    let saved = state.store.put_if_absent("chat_steer", &queued)
        .ok_or("새 지시를 영구 저장하지 못했습니다.")?;
    state.bus.record("chat_steer", &saved);
    Ok(saved)
}

/// Claim each queued steer with an idempotently persisted user message.
/// The same record ID is the message ID, so a crash between message commit
/// and status transition cannot duplicate the owner's instruction on replay.
/// This is called at the next **real model step**, never by the model itself.
pub fn drain_at_step(state: &AppState, session_id: &str) -> Result<Vec<Value>, String> {
    Ok(drain_at_step_with_ids(state,session_id)?
        .into_iter().map(|(_,message)|message).collect())
}

/// Return stable identity separately from the provider-facing message.
/// A crash after the message insert but before status commit must not append
/// the already-historical instruction a second time in the same model prompt.
pub fn drain_at_step_with_ids(
    state: &AppState,session_id:&str
) -> Result<Vec<(String,Value)>, String> {
    let mut queued = state.store.all("chat_steer").into_iter()
        .filter(|item| item["sessionId"].as_str() == Some(session_id)
            && item["status"] == "queued")
        .collect::<Vec<_>>();
    queued.sort_by(|a,b| {
        a["queuedAt"].as_str().unwrap_or("").cmp(b["queuedAt"].as_str().unwrap_or(""))
            .then_with(||a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")))
    });
    let mut injected = Vec::new();
    for item in queued {
        let id = item["id"].as_str().ok_or("지시 ID가 없습니다.")?;
        let instruction = item["instruction"].as_str().ok_or("지시 원문이 없습니다.")?;
        let persisted = match state.store.message(id) {
            Some(message) => message,
            None => state.store.write_message(&json!({
                "id": id, "session_id": session_id, "role": "user",
                "content": instruction,
                "metadata": {
                    "authorKind": "owner", "source": "sdk_chat_steer",
                    "steerId": id, "queuedAt": item["queuedAt"],
                },
            })).ok_or("새 지시 메시지를 저장하지 못했습니다.")?,
        };
        if persisted["session_id"].as_str() != Some(session_id)
            || persisted["role"] != "user"
            || persisted["content"].as_str() != Some(instruction)
        {
            return Err("저장된 지시의 원문 또는 세션이 일치하지 않습니다.".into());
        }
        let delivered = state.store.patch_if(
            "chat_steer", id, "status", &json!("queued"), &json!({
                "status": "delivered",
                "deliveredAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            }),
        ).ok_or("새 지시 수신 상태를 저장하지 못했습니다.")?;
        state.bus.record("chat_steer", &delivered);
        state.bus.message(&persisted);
        state.store.audit("chat.owner_steered", json!({"sessionId":session_id,"steerId":id}));
        injected.push((id.to_string(),json!({"role":"user","content":instruction})));
    }
    Ok(injected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    use std::path::PathBuf;

    fn fixture() -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-steer-{}", uuid::Uuid::new_v4()));
        let config = Config {
            port: 0,
            data_dir: root.join("data"),
            workspace: root.clone(),
            web_dir: root.join("web"),
        };
        let state = AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        state.store.put("session", &json!({"id":"owner-session"})).unwrap();
        (state, root)
    }

    #[test]
    fn owner_steer_is_verbatim_durable_and_delivered_exactly_once() {
        let (state, root) = fixture();
        let literal = "  지금 방향을 바꾸세요.\n기존 계획보다 이 지시를 우선하세요.  ";
        let record = enqueue(&state, "owner-session", literal).unwrap();
        let id = record["id"].as_str().unwrap();
        assert_eq!(record["instruction"], literal);
        let first = drain_at_step(&state, "owner-session").unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0], json!({"role":"user","content":literal}));
        assert!(drain_at_step(&state, "owner-session").unwrap().is_empty());
        assert_eq!(state.store.message(id).unwrap()["content"], literal);
        assert_eq!(state.store.get("chat_steer",id).unwrap()["status"], "delivered");
        assert!(state.store.messages("owner-session").len() == 1);
        let restarted = AppState::new(
            state.config.clone(), state.store.clone(),
            Vault::open(&state.config.key_path()),
        );
        assert!(drain_at_step(&restarted,"owner-session").unwrap().is_empty());
        drop(restarted);
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn in_flight_crash_recovery_does_not_duplicate_owner_instruction() {
        let (state, root) = fixture();
        let record = enqueue(&state,"owner-session","새 목표").unwrap();
        let id = record["id"].as_str().unwrap();
        state.store.write_message(&json!({
            "id":id,"session_id":"owner-session",
            "role":"user","content":"새 목표",
            "metadata":{"authorKind":"owner","source":"sdk_chat_steer"}
        })).unwrap();
        let delivered = drain_at_step(&state,"owner-session").unwrap();
        assert_eq!(delivered.len(),1);
        assert_eq!(state.store.messages("owner-session").len(),1);
        assert_eq!(state.store.get("chat_steer",id).unwrap()["status"],"delivered");
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn identical_text_in_two_separate_owner_commands_remains_two_commands() {
        let (state,root)=fixture();
        let first=enqueue(&state,"owner-session","새 지시").unwrap();
        let second=enqueue(&state,"owner-session","새 지시").unwrap();
        assert_ne!(first["id"],second["id"]);
        let delivered=drain_at_step_with_ids(&state,"owner-session").unwrap();
        assert_eq!(delivered.len(),2);
        assert_ne!(delivered[0].0,delivered[1].0);
        assert_eq!(delivered[0].1,delivered[1].1);
        assert_eq!(state.store.messages("owner-session").len(),2);
        assert!(drain_at_step_with_ids(&state,"owner-session").unwrap().is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cannot_inject_private_chat_instruction_into_room_or_archived_session() {
        let (state,root)=fixture();
        state.store.put("session",&json!({"id":"room","roomId":"r"})).unwrap();
        state.store.put("session",&json!({"id":"archived","archived":true})).unwrap();
        assert!(enqueue(&state,"room","방의 지시 위조").is_err());
        assert!(enqueue(&state,"archived","보관한 지시 위조").is_err());
        assert!(drain_at_step_with_ids(&state,"room").unwrap().is_empty());
        drop(state);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn cannot_steer_unrelated_or_missing_session() {
        let (state, root) = fixture();
        assert!(enqueue(&state, "wrong-session","hello").is_err());
        assert!(enqueue(&state, "owner-session"," \n ").is_err());
        let record = enqueue(&state,"owner-session","original").unwrap();
        assert!(drain_at_step(&state, "wrong-session").unwrap().is_empty());
        assert_eq!(state.store.get("chat_steer",record["id"].as_str().unwrap()).unwrap()["status"],"queued");
        drop(state);
        let _ = std::fs::remove_dir_all(root);
    }
}
