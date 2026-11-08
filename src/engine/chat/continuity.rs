//! Durable transport identity per participant and model route. A matching
//! prefix is a cache request, never evidence that the provider actually hit it.
use super::{ProviderRef, TurnRequest};
use crate::{engine::memory::stable_id, state::AppState};
use serde_json::{json, Value};

pub(super) fn prepare(
    state: &AppState, provider: &ProviderRef, request: &TurnRequest,
    session_id: &str, instructions: &str, reasoning: &Value,
) -> Result<Value, String> {
    let scope = stable_id(&[
        "model-conversation-v1", session_id, request.agent_id.as_deref().unwrap_or(""),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""),
    ]);
    let id = format!("conversation-{scope}");
    let prefix = stable_id(&[instructions, &reasoning.to_string()]);
    let key = format!("dominion_{}", stable_id(&[&scope, &prefix]));
    let previous = state.store.get("model_conversation", &id);
    let same_prefix = previous.as_ref().is_some_and(|record|record["cacheKey"] == key);
    let mut record = previous.unwrap_or_else(||json!({
        "id":id,"sessionId":session_id,"agentId":request.agent_id,
        "providerId":provider.id,"model":provider.model,
        "createdAt":chrono::Utc::now().to_rfc3339(),
    }));
    record["transportSessionId"] = json!(scope);
    record["cacheKey"] = json!(key);
    record["prefixDigest"] = json!(prefix);
    record["lastRequestedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("model_conversation", &record)
        .ok_or("봇의 지속 대화 상태를 저장할 수 없습니다.")?;
    Ok(json!({
        "id":id,"transport":"codex-responses","mode":"prefix-reuse",
        "transportSessionId":scope,"cacheKey":key,"prefixDigest":prefix,
        "samePrefixAsPreviousRequest":same_prefix,
        "previousReplyMessageId":record["lastReplyMessageId"],
    }))
}

/// Only a durably stored visible answer advances the bot's continuation cursor.
pub(super) fn commit(state: &AppState, continuity: &Value, message: &Value) {
    let Some(id) = continuity["id"].as_str() else { return };
    let Some(saved) = message["id"].as_str().and_then(|id|state.store.message(id)) else { return };
    let Some(record) = state.store.get("model_conversation", id) else { return };
    if saved["role"] != "assistant" || saved["session_id"] != record["sessionId"]
        || saved["agent_id"] != record["agentId"] { return; }
    let _ = state.store.patch_if("model_conversation", id, "cacheKey", &continuity["cacheKey"], &json!({
        "lastReplyMessageId":saved["id"],
        "lastSourceMessageId":saved["metadata"]["replyToMessageId"],
        "lastCompletedAt":chrono::Utc::now().to_rfc3339(),
        "lastUsage":saved["metadata"]["usage"],
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(root: &std::path::Path) -> AppState {
        let config = crate::config::Config { port:0, data_dir:root.into(), web_dir:root.join("web"), workspace:root.into() };
        AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()))
    }
    fn route() -> ProviderRef {
        ProviderRef { id:"p".into(),name:"test".into(),base_url:"https://example.test/codex".into(),api_key:"never-store-this".into(),model:"model-a".into(),tool_mode:"native".into(),reasoning_protocol:"openai".into(),local:false,context_tokens:None,max_output_tokens:None,requests_per_minute:None,kind:"codex".into(),account_id:Some("account-a".into()) }
    }
    #[test]
    fn continuity_survives_restart_and_separates_bots_routes_and_prefix_revisions() {
        let root=std::env::temp_dir().join(format!("dominion-continuity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let request=TurnRequest::from_value(&json!({"input":"first","agentId":"a"})).unwrap();
        let provider=route();
        let first=prepare(&state,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["samePrefixAsPreviousRequest"],false);
        let mut next=request.clone();next.input="second, different content".into();
        let second=prepare(&state,&provider,&next,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],second["cacheKey"]);
        assert_eq!(second["samePrefixAsPreviousRequest"],true);
        commit(&state,&first,&json!({"id":"not-persisted"}));
        let id=first["id"].as_str().unwrap();
        assert!(state.store.get("model_conversation",id).unwrap()["lastReplyMessageId"].is_null());
        let message=state.store.write_message(&json!({"session_id":"room:one","agent_id":"a","role":"assistant","content":"my previous answer","metadata":{"usage":{"cachedPromptTokens":0}}})).unwrap();
        commit(&state,&first,&message);
        let other=|provider:&ProviderRef,request:&TurnRequest,session:&str,prefix:&str|prepare(&state,provider,request,session,prefix,&json!({"effort":"low"})).unwrap();
        let mut peer=request.clone();peer.agent_id=Some("b".into());
        assert_ne!(first["cacheKey"],other(&provider,&peer,"room:one","full persona")["cacheKey"]);
        assert_ne!(first["cacheKey"],other(&provider,&request,"room:two","full persona")["cacheKey"]);
        for change in ["model","account","endpoint"] {
            let mut changed=provider.clone();
            match change {"model"=>changed.model="model-b".into(),"account"=>changed.account_id=Some("account-b".into()),_=>changed.base_url="https://other.test".into()}
            assert_ne!(first["cacheKey"],other(&changed,&request,"room:one","full persona")["cacheKey"]);
        }
        assert_ne!(first["cacheKey"],other(&provider,&request,"room:one","edited persona")["cacheKey"]);
        assert!(!state.store.get("model_conversation",id).unwrap().to_string().contains("never-store-this"));
        drop(state);
        let restored=super::tests::state(&root);
        let resumed=prepare(&restored,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],resumed["cacheKey"]);
        assert_eq!(resumed["previousReplyMessageId"],message["id"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
