//! Owner-authored step-boundary steering for a live private chat.
//!
//! A steering instruction is durably recorded *with* its queue record in one
//! SQLite transaction. Neither model output nor a skill can enqueue one.
//! The runner can claim it only for the execution it targeted. A late message
//! remains visible as an owner message and is marked deferred, not silently
//! attributed to a run that never observed it.
use crate::state::AppState;
use serde_json::{json, Value};

const MAX_STEER_CHARS: usize = 16_000;
const MAX_STEERS_PER_CLAIM: usize = 8;

pub(crate) fn enqueue(state: &AppState, session_id: &str, text: &str) -> Result<Value, String> {
    let session = state.store.get("session", session_id)
        .ok_or("지시를 전달할 세션을 찾을 수 없습니다.")?;
    if session["roomId"].is_string() || session["channelContext"].is_object()
        || session["rsiRun"] == true || session["goalId"].is_string()
    {
        return Err("실행 중 지시 변경은 현재 일반 개인 채팅에서만 지원합니다.".into());
    }
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_STEER_CHARS {
        return Err("지시는 1~16,000자로 입력하세요.".into());
    }
    // An explicit running execution owns the interjection. Never enqueue for
    // an idle or already-finished run and never retarget it to another run.
    let execution = state.store.all("execution").into_iter()
        .find(|run| run["sessionId"].as_str() == Some(session_id)
            && run["status"] == "running")
        .ok_or("현재 실행 중인 에이전트가 없습니다. 새 메시지로 요청하세요.")?;
    let execution_id = execution["id"].as_str()
        .ok_or("실행 식별자가 없습니다.")?;
    let message_id = uuid::Uuid::new_v4().to_string();
    let record = json!({
        "id":uuid::Uuid::new_v4().to_string(),
        "sessionId":session_id,
        "executionId":execution_id,
        "messageId":message_id,
        "status":"queued",
        "createdAt":chrono::Utc::now().to_rfc3339(),
        "kind":"owner-steer",
    });
    let message = json!({
        "id":message_id,
        "session_id":session_id,
        "role":"user",
        "content":text,
        "agent_id":null,
        "metadata":{
            "authorKind":"owner",
            "ownerSteer":true,
            "targetExecutionId":execution_id,
        }
    });
    let (saved_message, saved_record) = state.store
        .write_message_with_record(&message, "owner_steer", &record)
        .ok_or("사용자 지시와 실행 큐를 원자적으로 저장하지 못했습니다.")?;
    state.bus.message(&saved_message);
    state.bus.record("owner_steer", &saved_record);
    state.store.audit("owner.steer.queued",json!({
        "sessionId":session_id,"executionId":execution_id,"messageId":message_id
    }));
    Ok(saved_record)
}

/// Returns authentic owner messages for the exact running execution.
/// Compare-and-patch prevents a second poll or competing worker from claiming
/// the same instruction. Only an owner-authored, session-matching message is
/// accepted; no model-created role text is promoted to an owner directive.
pub(crate) fn claim(state: &AppState, session_id: &str, execution_id: &str) -> Vec<Value> {
    let mut queued = state.store.all("owner_steer").into_iter()
        .filter(|item| item["sessionId"].as_str() == Some(session_id)
            && item["executionId"].as_str() == Some(execution_id)
            && item["status"] == "queued")
        .collect::<Vec<_>>();
    queued.sort_by(|a,b| a["createdAt"].as_str().cmp(&b["createdAt"].as_str())
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut claimed = Vec::new();
    for item in queued.into_iter().take(MAX_STEERS_PER_CLAIM) {
        let Some(message_id) = item["messageId"].as_str() else {continue};
        let Some(message) = state.store.message(message_id) else {continue};
        if message["session_id"].as_str() != Some(session_id)
            || message["role"] != "user"
            || message["metadata"]["authorKind"] != "owner"
            || message["metadata"]["ownerSteer"] != true
            || message["metadata"]["targetExecutionId"].as_str() != Some(execution_id) {
            continue;
        }
        let Some(id) = item["id"].as_str() else {continue};
        let Some(saved) = state.store.patch_if("owner_steer",id,"status",
            &json!("queued"),&json!({
                "status":"claimed",
                "claimedAt":chrono::Utc::now().to_rfc3339(),
            })) else {continue};
        state.bus.record("owner_steer", &saved);
        state.store.audit("owner.steer.claimed",json!({
            "sessionId":session_id,"executionId":execution_id,
            "messageId":message_id,
        }));
        claimed.push(message);
    }
    claimed
}

/// A run which ended before its queued message was observed cannot claim it
/// succeeded. The owner-authored message stays in history for the next turn.
pub(crate) fn defer_unclaimed(state: &AppState, execution_id: &str) -> usize {
    let mut count = 0;
    for item in state.store.all("owner_steer") {
        if item["executionId"].as_str() != Some(execution_id)
            || item["status"] != "queued" {continue}
        let Some(id) = item["id"].as_str() else {continue};
        if let Some(saved)=state.store.patch_if("owner_steer",id,"status",&json!("queued"),
            &json!({"status":"deferred",
                "reason":"execution_finished_before_next_model_step"})){
            state.bus.record("owner_steer",&saved);
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state() -> (AppState,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-owner-inbox-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=AppState::new(cfg,Store::open(&dir.join("db.sqlite")).unwrap(),
            Vault::open(&dir.join("key")));
        (state,dir)
    }
    #[test]
    fn owner_can_steer_a_running_turn_once_with_durable_provenance(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("execution",&json!({"id":"r1","sessionId":"s","status":"running"}));
        let queued=enqueue(&state,"s","다음 단계에서 새 파일을 만들어라").unwrap();
        assert_eq!(queued["status"],"queued");
        let commands=claim(&state,"s","r1");
        assert_eq!(commands.len(),1);
        assert_eq!(commands[0]["role"],"user");
        assert_eq!(commands[0]["metadata"]["authorKind"],"owner");
        assert!(claim(&state,"s","r1").is_empty());
        assert!(claim(&state,"s","r2").is_empty());
        assert_eq!(defer_unclaimed(&state,"r1"),0);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn no_live_target_or_room_can_receive_a_steering_instruction(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"}));
        assert!(enqueue(&state,"s","hello").is_err());
        state.store.put("execution",&json!({"id":"r","sessionId":"s","status":"running"}));
        state.store.put("session",&json!({"id":"room","roomId":"r"}));
        assert!(enqueue(&state,"room","override").is_err());
        assert!(enqueue(&state,"s","  ").is_err());
        assert!(enqueue(&state,"s",&"x".repeat(MAX_STEER_CHARS+1)).is_err());
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn late_steering_is_marked_deferred_not_claimed_by_another_run(){
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("execution",&json!({"id":"r1","sessionId":"s","status":"running"}));
        let record=enqueue(&state,"s","이전 지시를 변경해라").unwrap();
        state.store.patch_if("execution","r1","status",&json!("running"),
            &json!({"status":"completed"})).unwrap();
        assert_eq!(defer_unclaimed(&state,"r1"),1);
        assert!(claim(&state,"s","r1").is_empty());
        assert_eq!(state.store.get("owner_steer",record["id"].as_str().unwrap()).unwrap()["status"],"deferred");
        assert_eq!(state.store.messages("s").len(),1);
        let _=std::fs::remove_dir_all(dir);
    }
}
