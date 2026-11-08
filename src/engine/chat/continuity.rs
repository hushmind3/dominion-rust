//! Durable transport identity per participant and model route. A matching
//! prefix is a cache request, never evidence that the provider actually hit it.
use super::{ProviderRef, TurnRequest};
use crate::{engine::memory::stable_id, state::AppState};
use serde_json::{json, Value};

pub(super) struct Projection {
    frames: Vec<Value>,
    dependencies: Vec<Value>,
    signature: String,
    epoch: u64,
}

/// Transport metadata is engine-owned and must never become API parameters.
fn wire_message(message: &Value) -> Value {
    let mut value = message.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("_dominion_frame");
        object.remove("_dominion_source");
        object.remove("_dominion_dependencies");
    }
    value
}

fn evidence_digest(record: &Value) -> String {
    // Usage counters and last-used timestamps are not semantic revisions.
    let mut meaning = serde_json::Map::new();
    for field in ["text","content","instructions","description","name","lesson","avoid","request",
        "principle","trigger","intervention","preconditions","exceptions","status","enabled",
        "scope","partitionKey","shareWithRooms","shareWithChannels","source","type","expiresAt",
        "agentId","sessionId","roomId","revision","verification","confidence","importance",
        "sourceAuthors","sourceRole","sourceMessageIds","correctsMessageIds","evidenceMessageIds",
        "supportingCases","parameters","applicability"] {
        if let Some(value) = record.get(field) { meaning.insert(field.into(),value.clone()); }
    }
    stable_id(&[&Value::Object(meaning).to_string()])
}

pub(super) fn dependencies(state: &AppState, items: &[Value]) -> Vec<Value> {
    items.iter().filter_map(|item| {
        let kind = match item["kind"].as_str()? {
            "memory"=>"memory", "experience"=>"experience", "skill"=>"skill", "improvement"=>"improvement",
            "room_knowledge"=>"room_knowledge", "room_knowledge_global"=>"experience_principle",
            "learned_method"=>"experience_rule", _=>return None,
        };
        let id=item["id"].as_str()?;
        let record=state.store.get(kind,id)?;
        Some(json!({"kind":kind,"id":id,"digest":evidence_digest(&record)}))
    }).collect()
}

fn frame_key(message: &Value, source: &str, index: usize) -> String {
    if let Some(id) = message["tool_call_id"].as_str() {
        return format!("tool-result:{id}");
    }
    if let Some(calls) = message["tool_calls"].as_array().filter(|calls|!calls.is_empty()) {
        return format!("tool-call:{}", stable_id(&[&json!(calls.iter().map(|call|&call["id"]).collect::<Vec<_>>()).to_string()]));
    }
    message["_dominion_frame"].as_str().map(str::to_string)
        .unwrap_or_else(|| format!("transient-{}", stable_id(&[source, &index.to_string(), &wire_message(message).to_string()])))
}

/// A selected context is a *delta* to a participant's committed working view.
/// Rebuilding a sliding window on every turn changes its first user boundary,
/// defeating prefix reuse even when the persona and cache key are identical.
/// Keep that prefix verbatim until a bounded context epoch must be rebuilt.
pub(super) fn project(
    state: &AppState, provider: &ProviderRef, messages: &[Value], tools: &[Value],
    session_id: &str, plan: &mut Value,
) -> (Vec<Value>, Option<Projection>) {
    let clean = || messages.iter().map(wire_message).collect();
    let Some(source) = messages.first().and_then(|row| row["_dominion_source"].as_str()) else {
        return (clean(), None);
    };
    let Some(participant) = plan["participantId"].as_str()
        .and_then(|id| state.store.get("participant_conversation", id)) else { return (clean(), None) };
    // The source is frozen by the runtime, not inferred from a mutable latest
    // owner message while another room round could already be running.
    let signature = stable_id(&[&wire_message(&messages[0]).to_string(), &json!(tools).to_string()]);
    let previous = &participant["workingView"];
    let scope=crate::engine::memory::MemoryScope::for_session(&state.store, session_id, participant["agentId"].as_str());
    let fresh_dependencies=messages[0]["_dominion_dependencies"].as_array().cloned().unwrap_or_default();
    let mut dependencies=previous["dependencies"].as_array().cloned().unwrap_or_default();
    let fresh = messages.iter().enumerate().skip(1).map(|(index, message)| {
        let body = wire_message(message);
        let key = frame_key(message, source, index);
        json!({"key":key,"message":body})
    }).collect::<Vec<_>>();
    let mut frames = previous["frames"].as_array().cloned().unwrap_or_default();
    let mut reason = if frames.is_empty() { Some("initial") }
        else if previous["signature"] != signature { Some("contract_changed") }
        else if participant["lastReplyMessageId"].as_str().is_some_and(|id|state.store.message(id).is_none()) { Some("transcript_reset") }
        else if dependencies.iter().any(|dependency| {
            state.store.get(dependency["kind"].as_str().unwrap_or(""),dependency["id"].as_str().unwrap_or(""))
                .is_none_or(|record| {
                    let visible=match dependency["kind"].as_str().unwrap_or("") {
                        "memory"|"skill"=>crate::engine::memory::visible(&record,&scope),
                        "experience"=>crate::engine::experience::visible(&record,&scope),
                        "experience_rule"=>crate::engine::experience::rule_visible(&record,&scope),
                        _=>true,
                    };
                    !visible || json!(evidence_digest(&record)) != dependency["digest"]
                })
        }) { Some("context_evidence_changed") }
        else if participant["lastReplyMessageId"] != plan["participantBaseReplyMessageId"] { Some("concurrent_commit") }
        else { None };
    // A source can be edited or re-rendered (e.g. attachment availability).
    // Never silently retain an obsolete version under the same source ID.
    if reason.is_none() && fresh.iter().any(|new| frames.iter()
        .any(|old| old["key"] == new["key"] && old["message"] != new["message"])) {
        reason = Some("source_changed");
    }
    let mut reused = frames.len();
    if reason.is_none() {
        let seen = frames.iter().filter_map(|frame|frame["key"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        frames.extend(fresh.iter().filter(|frame|!seen.contains(frame["key"].as_str().unwrap_or(""))).cloned());
        let session = state.store.get("session", session_id).unwrap_or(Value::Null);
        let room = session["roomId"].as_str().and_then(|id|state.store.get("room", id));
        // This is a *history* allowance in addition to mandatory identity, not
        // a cap that discards persona or the current selected request.
        let allowance = room.as_ref().map(|room|room["contextBudgetTokens"].as_u64().unwrap_or(2_000) as usize)
            .unwrap_or(40_000);
        let fixed = super::room_context::input_estimate(&[wire_message(&messages[0])], tools);
        let allowance = provider.context_tokens.map(|capacity| {
            allowance.min((capacity as usize).saturating_sub(provider.max_output_tokens.unwrap_or(4096) as usize).saturating_sub(fixed))
        }).unwrap_or(allowance);
        let history_cost = |rows:&[Value]| super::room_context::input_estimate(
            &rows.iter().map(|frame|frame["message"].clone()).collect::<Vec<_>>(), &[]);
        if history_cost(&frames) > allowance.max(history_cost(&fresh)) {
            reason = Some("history_budget");
        }
    }
    let mut epoch = previous["epoch"].as_u64().unwrap_or(0);
    if reason.is_some() { frames = fresh; dependencies=fresh_dependencies; reused = 0; epoch += 1; }
    else {
        for dependency in fresh_dependencies {
            if !dependencies.iter().any(|old|old["kind"]==dependency["kind"] && old["id"]==dependency["id"]) {
                dependencies.push(dependency);
            }
        }
    }
    let mut projected = vec![wire_message(&messages[0])];
    projected.extend(frames.iter().map(|frame|frame["message"].clone()));
    plan["workingView"] = json!({
        "epoch":epoch,"reusedFrames":reused,"appendedFrames":frames.len().saturating_sub(reused),
        "rebaseReason":reason,"sourceMessageId":source,
        "inputDigest":stable_id(&[&json!(projected).to_string()]),
        "estimatedInputTokens":super::room_context::input_estimate(&projected, tools),
    });
    (projected, Some(Projection { frames, dependencies, signature, epoch }))
}

pub(super) fn commit_projection(state: &AppState, plan: &Value, message: &Value, mut projection: Projection) {
    let Some(saved) = message["id"].as_str().and_then(|id|state.store.message(id)) else { return };
    let Some(participant_id) = plan["participantId"].as_str() else { return };
    let Some(record) = state.store.get("participant_conversation", participant_id) else { return };
    if record["lastReplyMessageId"] != saved["id"] { return; }
    let rendered = super::context::render_history(state, saved["session_id"].as_str().unwrap_or(""),
        saved["agent_id"].as_str(), &[saved.clone()], Vec::new());
    projection.frames.extend(rendered.iter().map(|row|json!({"key":saved["id"],"message":wire_message(row)})));
    let _ = state.store.patch_if("participant_conversation", participant_id, "lastReplyMessageId", &saved["id"], &json!({
        "workingView":{"signature":projection.signature,"epoch":projection.epoch,"frames":projection.frames,"dependencies":projection.dependencies},
    }));
}

pub(super) fn prepare(
    state: &AppState, provider: &ProviderRef, request: &TurnRequest,
    session_id: &str, instructions: &str, reasoning: &Value,
) -> Result<Value, String> {
    // Conversation ownership never depends on the currently selected model.
    let participant_id = format!("participant-{}", stable_id(&[
        "participant-session-v1", session_id, request.agent_id.as_deref().unwrap_or(""),
    ]));
    let participant = state.store.get("participant_conversation", &participant_id)
        .or_else(||state.store.put_if_absent("participant_conversation", &json!({
            "id":participant_id,"sessionId":session_id,"agentId":request.agent_id,
            "createdAt":chrono::Utc::now().to_rfc3339(),
        }))).ok_or("봇의 공통 대화 상태를 저장할 수 없습니다.")?;
    let scope = stable_id(&[
        "model-conversation-v1", session_id, request.agent_id.as_deref().unwrap_or(""),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""),
        &provider.kind, &provider.reasoning_protocol,
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
        "id":id,"participantId":participant_id,"transport":provider.kind,"mode":"prefix-reuse",
        "transportSessionId":scope,"cacheKey":key,"prefixDigest":prefix,
        "samePrefixAsPreviousRequest":same_prefix,
        "previousReplyMessageId":participant.get("lastReplyMessageId").unwrap_or(&record["lastReplyMessageId"]),
        "participantBaseReplyMessageId":participant["lastReplyMessageId"],
        "previousRouteReplyMessageId":record["lastReplyMessageId"],
    }))
}

/// Only a durably stored visible answer advances the bot's continuation cursor.
pub(super) fn commit(state: &AppState, continuity: &Value, message: &Value) {
    let Some(id) = continuity["id"].as_str() else { return };
    let Some(saved) = message["id"].as_str().and_then(|id|state.store.message(id)) else { return };
    let Some(record) = state.store.get("model_conversation", id) else { return };
    if saved["role"] != "assistant" || saved["session_id"] != record["sessionId"]
        || saved["agent_id"] != record["agentId"] { return; }
    if state.store.patch_if("model_conversation", id, "cacheKey", &continuity["cacheKey"], &json!({
        "lastReplyMessageId":saved["id"],
        "lastSourceMessageId":saved["metadata"]["replyToMessageId"],
        "lastCompletedAt":chrono::Utc::now().to_rfc3339(),
        "lastUsage":saved["metadata"]["usage"],
    })).is_none() {return;}
    if let Some(participant_id) = continuity["participantId"].as_str() {
        let _ = state.store.patch_if("participant_conversation", participant_id, "lastReplyMessageId", &continuity["participantBaseReplyMessageId"], &json!({
            "lastReplyMessageId":saved["id"],"lastSourceMessageId":saved["metadata"]["replyToMessageId"],
            "lastCompletedAt":chrono::Utc::now().to_rfc3339(),"lastRouteId":id,
        }));
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CacheWire { Key, Ephemeral }

impl CacheWire {
    fn field(self) -> &'static str { match self { Self::Key=>"prompt_cache_key", Self::Ephemeral=>"cache_control" } }
}

fn apply_hint(body: &mut Value, plan: &Value, wire: CacheWire) -> bool {
    match wire {
        CacheWire::Key => { body["prompt_cache_key"]=plan["cacheKey"].clone(); true }
        CacheWire::Ephemeral => {
            if let Some(system)=body["system"].as_str().filter(|text| !text.is_empty()) {
                body["system"]=json!([{"type":"text","text":system,"cache_control":{"type":"ephemeral"}}]);
                true
            } else if let Some(last)=body["system"].as_array_mut().and_then(|parts|parts.last_mut()) {
                last["cache_control"]=json!({"type":"ephemeral"}); true
            } else {false}
        }
    }
}

fn rejects_hint(status: reqwest::StatusCode, text: &str, field: &str) -> bool {
    if !matches!(status.as_u16(),400|422) {return false}
    let lower=text.to_ascii_lowercase();
    let parsed=serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let parameter=parsed["error"]["param"].as_str().or_else(||parsed["param"].as_str());
    (parameter.is_some_and(|param|param.contains(field)) || lower.contains(field))
        && ["unsupported", "not supported", "unrecognized", "unknown", "not permitted", "not allowed", "extra_forbidden", "extra inputs"].iter().any(|word|lower.contains(word))
}

/// A protocol hint is negotiated once from a specific rejection, never from
/// a model name. Failure of an optional hint must not remove conversation state.
pub(super) async fn send(
    state:&AppState, provider:&ProviderRef, builder:reqwest::RequestBuilder,
    original:&Value, plan:&mut Value, wire:CacheWire,
) -> Result<reqwest::Response,String> {
    let feature_id=format!("wire-{}",stable_id(&[
        &provider.id,&provider.base_url,&provider.kind,&provider.model,
        provider.account_id.as_deref().unwrap_or(""),wire.field(),
    ]));
    let known=state.store.get("provider_wire_feature",&feature_id);
    let disabled=known.as_ref().is_some_and(|record|record["accepted"]==false
        && record["expiresAt"].as_i64().is_some_and(|until|until>chrono::Utc::now().timestamp()));
    let mut body=original.clone();
    let mut hinted=!disabled && apply_hint(&mut body,plan,wire);
    plan["cacheHint"]=json!(wire.field());
    plan["wireHintSent"]=json!(hinted);
    plan["cacheHintAcceptance"]=Value::Null;
    for attempt in 1..=2 {
        plan["httpRequests"]=json!(attempt);
        let response=builder.try_clone().ok_or("모델 요청을 복제할 수 없습니다.")?
            .json(&body).send().await.map_err(|e|format!("{} 연결 실패: {e}",provider.name))?;
        let status=response.status();
        if hinted && attempt==1 && matches!(status.as_u16(),400|422) {
            let headers=response.headers().clone();let version=response.version();
            let bytes=response.bytes().await.map_err(|e|e.to_string())?;
            if rejects_hint(status,&String::from_utf8_lossy(&bytes),wire.field()) {
                state.store.put("provider_wire_feature",&json!({"id":feature_id,"field":wire.field(),"accepted":false,
                    "observedAt":chrono::Utc::now().to_rfc3339(),"httpStatus":status.as_u16(),
                    "expiresAt":chrono::Utc::now().timestamp()+86400}));
                body=original.clone();hinted=false;
                plan["wireHintSent"]=json!(false);plan["cacheHintAcceptance"]=json!(false);
                continue;
            }
            // Preserve unrelated errors and routing headers for the existing
            // error handling. No retry on authentication or task failures.
            let mut restored=axum::http::Response::new(bytes);
            *restored.status_mut()=status;*restored.headers_mut()=headers;*restored.version_mut()=version;
            return Ok(reqwest::Response::from(restored));
        }
        if hinted && status.is_success() {
            plan["cacheHintAcceptance"]=json!(true);
            state.store.put("provider_wire_feature",&json!({"id":feature_id,"field":wire.field(),"accepted":true,
                "observedAt":chrono::Utc::now().to_rfc3339()}));
        }
        if disabled {plan["cacheHintAcceptance"]=json!(false);}
        return Ok(response)
    }
    unreachable!("one optional compatibility retry")
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
        for kind in ["openai","anthropic","ollama","llamacpp","lmstudio"] {
            let mut changed=provider.clone();changed.kind=kind.into();changed.model=format!("{kind}-test");
            let transferred=other(&changed,&request,"room:one","full persona");
            assert_eq!(transferred["participantId"],first["participantId"]);
            assert_eq!(transferred["previousReplyMessageId"],message["id"]);
            assert_eq!(transferred["transport"],kind);
        }
        drop(state);
        let restored=super::tests::state(&root);
        let resumed=prepare(&restored,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],resumed["cacheKey"]);
        assert_eq!(resumed["previousReplyMessageId"],message["id"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_cache_rejection_must_identify_the_parameter_and_preserve_other_errors() {
        let bad=reqwest::StatusCode::BAD_REQUEST;
        assert!(rejects_hint(bad,r#"{"error":{"message":"Unrecognized request argument supplied: prompt_cache_key"}}"#,"prompt_cache_key"));
        assert!(rejects_hint(bad,r#"{"error":{"param":"system.0.cache_control","message":"not supported"}}"#,"cache_control"));
        assert!(!rejects_hint(bad,r#"{"error":{"message":"model not supported"}}"#,"prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::UNAUTHORIZED,"prompt_cache_key not supported","prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::TOO_MANY_REQUESTS,"prompt_cache_key not supported","prompt_cache_key"));
    }

    #[test]
    fn committed_working_view_keeps_wire_prefix_when_selection_slides_and_routes_change() {
        let root=std::env::temp_dir().join(format!("dominion-projection-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:test","roomId":"test"}));
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let request=TurnRequest::from_value(&json!({"input":"same words","agentId":"a"})).unwrap();
        let provider=route();
        let frame=|id:&str, text:&str|json!({"role":"user","content":text,"_dominion_frame":id});
        let system=|source:&str|json!({"role":"system","content":"full immutable identity","_dominion_source":source});
        let first=vec![system("owner-1"),frame("older","old correction"),frame("owner-1","same words")];
        let mut plan=prepare(&state,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        let (first_wire,pending)=project(&state,&provider,&first,&[],"room:test",&mut plan);
        assert!(!json!(first_wire).to_string().contains("_dominion_"));
        assert!(state.store.get("participant_conversation",plan["participantId"].as_str().unwrap()).unwrap()["workingView"].is_null(),"uncommitted requests cannot change the working view");
        let saved=state.store.write_message(&json!({"session_id":"room:test","agent_id":"a","role":"assistant","content":"I remember","metadata":{"replyToMessageId":"owner-1"}})).unwrap();
        commit(&state,&plan,&saved);
        commit_projection(&state,&plan,&saved,pending.unwrap());
        let mut changed=provider.clone();changed.kind="ollama".into();changed.model="other-model".into();
        let mut second_plan=prepare(&state,&changed,&request,"room:test","full immutable identity",&json!({})).unwrap();
        // The old correction was omitted by the new selector. Identical text
        // from a NEW owner source must still be appended as a new request.
        let mut own_reply=frame(saved["id"].as_str().unwrap(),"I remember");own_reply["role"]=json!("assistant");
        let second=vec![system("owner-2"),own_reply,frame("owner-2","same words")];
        let (second_wire,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(&second_wire[..first_wire.len()],first_wire.as_slice());
        assert_eq!(second_wire.iter().filter(|row|row["content"]=="same words").count(),2);
        assert_eq!(second_plan["workingView"]["reusedFrames"],3);
        assert_eq!(second_plan["workingView"]["appendedFrames"],1);
        // A changed persona invalidates the prefix without dropping the fresh
        // selected evidence, and budget rotation never cuts mandatory input.
        let mut edited=second.clone();edited[0]["content"]=json!("edited identity");
        let (rebased,_)=project(&state,&changed,&edited,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"contract_changed");
        assert_eq!(rebased.len(),edited.len());
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":1}));
        let (bounded,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"history_budget");
        assert_eq!(bounded.len(),second.len());
        drop(state);
        let restored=super::tests::state(&root);
        let mut resumed=prepare(&restored,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        restored.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let (after_restart,_)=project(&restored,&provider,&second,&[],"room:test",&mut resumed);
        assert_eq!(after_restart,second_wire);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replay_does_not_duplicate_tool_pairs_or_retain_revoked_knowledge() {
        let root=std::env::temp_dir().join(format!("dominion-projection-tools-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("skill",&json!({"id":"skill","instructions":"old procedure","enabled":true}));
        let request=TurnRequest::from_value(&json!({"input":"work","agentId":"a"})).unwrap();
        let provider=route();
        let deps=dependencies(&state,&[json!({"kind":"skill","id":"skill"})]);
        let system=json!({"role":"system","content":"identity","_dominion_source":"owner1","_dominion_dependencies":deps});
        let call=json!({"role":"assistant","content":"","tool_calls":[{"id":"call1","type":"function","function":{"name":"read","arguments":"{}"}}]});
        let result=json!({"role":"tool","tool_call_id":"call1","content":"done"});
        let messages=vec![system.clone(),json!({"role":"user","content":"old procedure","_dominion_frame":"owner1"}),call.clone(),result.clone()];
        let mut plan=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (_,pending)=project(&state,&provider,&messages,&[],"s",&mut plan);
        let saved=state.store.write_message(&json!({"session_id":"s","agent_id":"a","role":"assistant","content":"finished","metadata":{"replyToMessageId":"owner1"}})).unwrap();
        commit(&state,&plan,&saved);commit_projection(&state,&plan,&saved,pending.unwrap());
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"lastUsedAt":"new telemetry","useCount":100}));
        let mut replay=vec![system.clone(),call,result,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        replay[0]["_dominion_source"]=json!("owner2");
        replay[1]["_dominion_frame"]=json!("stored-call-row");replay[2]["_dominion_frame"]=json!("stored-result-row");
        let mut next=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (wire,_)=project(&state,&provider,&replay,&[],"s",&mut next);
        assert!(next["workingView"]["rebaseReason"].is_null());
        assert_eq!(wire.iter().filter(|row|row["tool_calls"].is_array()).count(),1);
        assert_eq!(wire.iter().filter(|row|row["role"]=="tool").count(),1);
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"enabled":false}));
        let mut fresh=vec![system,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        fresh[0]["_dominion_source"]=json!("owner2");fresh[0]["_dominion_dependencies"]=json!([]);
        let (wire,_)=project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"context_evidence_changed");
        assert!(!json!(wire).to_string().contains("old procedure"));
        state.store.clear_messages("s");
        project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"transcript_reset");
        std::fs::remove_dir_all(root).unwrap();
    }
}
