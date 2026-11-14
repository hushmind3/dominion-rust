//! Durable tool evidence boundary for autonomous goal completion.
//! Pre-commit UI events can never substitute for a stored execution receipt.
//! This module intentionally has no model / provider dependency.
use crate::state::AppState;
use serde_json::Value;
use std::collections::HashSet;

/// Only a *specific*, completed and verified server-side tool operation may
/// satisfy an acceptance predicate. callId can repeat across model rounds and
/// sessions; operationId is immutable and globally unique for a given run.
pub(super) fn hydrate(state:&AppState,session:Option<&str>,events:Vec<Value>)->Vec<Value>{
    let mut seen=HashSet::new();
    events.into_iter().filter_map(|event|{
        let id=event["operationId"].as_str()?;
        if !seen.insert(id.to_string()){return None;}
        let stored=state.store.get("execution_operation",id)?;
        if stored["status"]!="completed"
            || stored["callId"]!=event["callId"]
            || stored["tool"]!=event["tool"]
            || !crate::engine::verification::successful_evidence(&stored)
        {return None;}
        if session.is_some_and(|id|stored["sessionId"].as_str()!=Some(id)){
            return None;
        }
        Some(stored)
    }).collect()
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    use serde_json::json;

    fn setup()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-tool-proof-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        (state,dir)
    }

    #[test]
    fn forged_or_uncommitted_ui_events_are_not_completion_receipts(){
        let (state,dir)=setup();
        let receipt=state.store.put("execution_operation",&json!({
            "id":"op-good","runId":"exec-42","sessionId":"goal-a",
            "tool":"workspace_write","callId":"reused-call",
            "opSeq":7,"startedAt":"2026-11-14T01:00:01Z",
            "finishedAt":"2026-11-14T01:00:02Z","status":"completed",
            "arguments":{"path":"src/lib.rs"},
            "result":{"written":true}
        })).unwrap();
        let event=json!({"operationId":"op-good","tool":"workspace_write",
            "callId":"reused-call",
            "result":{"error":"forged client error"},
            "arguments":{"path":"elsewhere"}});
        let evidence=hydrate(&state,Some("goal-a"),vec![event.clone(),event.clone()]);
        assert_eq!(evidence.len(),1,"one stable operationId is one proof");
        assert_eq!(evidence[0],receipt,
            "persisted operation wins over untrusted event body");
        assert_eq!(evidence[0]["runId"],"exec-42");
        assert_eq!(evidence[0]["opSeq"],7);
        assert!(hydrate(&state,Some("another-room"),vec![event.clone()]).is_empty());
        assert!(hydrate(&state,Some("goal-a"),vec![json!({
            "operationId":"never-committed","tool":"workspace_write",
            "callId":"reused-call"})]).is_empty());
        assert!(hydrate(&state,Some("goal-a"),vec![json!({
            "operationId":"op-good","tool":"workspace_write",
            "callId":"different-invocation"})]).is_empty());
        assert!(hydrate(&state,Some("goal-a"),vec![json!({
            "operationId":"op-good","tool":"terminal_exec",
            "callId":"reused-call"})]).is_empty());
        state.store.put("execution_operation",&json!({
            "id":"op-inflight","runId":"exec-42","sessionId":"goal-a",
            "tool":"workspace_write","callId":"next","status":"running",
            "result":{"written":true}
        })).unwrap();
        assert!(hydrate(&state,Some("goal-a"),vec![json!({
            "operationId":"op-inflight","tool":"workspace_write",
            "callId":"next"})]).is_empty());
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
