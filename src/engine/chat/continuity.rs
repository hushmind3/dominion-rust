//! Durable transport identity per participant and model route. A matching
//! prefix is a cache request, never evidence that the provider actually hit it.
use super::{ProviderRef, TurnRequest};
use crate::{engine::memory::stable_id, state::AppState};
use serde_json::{json, Value};

pub(crate) struct Projection {
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

fn evidence_digest(kind: &str, record: &Value) -> String {
    // Usage counters and last-used timestamps are not semantic revisions.
    let mut meaning = serde_json::Map::new();
    if kind=="room_state" {
        meaning.insert("facts".into(),json!(record["facts"].as_array().into_iter().flatten()
            .filter(|fact|fact["status"]=="active")
            .map(|fact|json!({"id":fact["id"],"kind":fact["kind"],"text":fact["text"],"sourceMessageId":fact["sourceMessageId"]}))
            .collect::<Vec<_>>()));
    }
    for field in ["text","content","instructions","description","name","lesson","avoid","request",
        "principle","trigger","intervention","preconditions","exceptions","status","enabled",
        "scope","partitionKey","shareWithRooms","shareWithChannels","source","type","expiresAt",
        "agentId","sessionId","roomId","revision","verification","confidence","importance",
        "sourceAuthors","sourceRole","sourceMessageIds","correctsMessageIds","evidenceMessageIds","sources","authority","outcome","supportingModels",
        "supportingCases","parameters","applicability"] {
        // Ranking/confidence updates do not edit the memory shown to the
        // model. Treating these as content changes could flush working views
        // even when their remembered text and provenance were identical.
        if matches!(kind, "memory" | "room_knowledge")
            && matches!(field, "confidence" | "importance" | "revision" | "supportingCases") {
            continue;
        }
        if let Some(value) = record.get(field) { meaning.insert(field.into(),value.clone()); }
    }
    stable_id(&[&Value::Object(meaning).to_string()])
}

pub(crate) fn dependencies(state: &AppState, items: &[Value]) -> Vec<Value> {
    items.iter().filter_map(|item| {
        let kind = match item["kind"].as_str()? {
            "memory"=>"memory", "experience"=>"experience", "skill"=>"skill", "improvement"=>"improvement",
            "room_knowledge"=>"room_knowledge", "room_knowledge_global"=>"experience_principle",
            "learned_method"=>"experience_rule", "owner_state"=>"room_state", _=>return None,
        };
        let id=item["id"].as_str()?;
        let record=state.store.get(kind,id)?;
        Some(json!({"kind":kind,"id":id,"digest":evidence_digest(kind, &record)}))
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
pub(crate) fn project(
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
    // Rotate legacy batch-based views once; otherwise old aggregate evidence
    // and the new record-based frames could coexist after an upgrade.
    let signature = stable_id(&["record-evidence-v2", &wire_message(&messages[0]).to_string(), &json!(tools).to_string()]);
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
                        "memory"=>crate::engine::memory::recallable(&state.store,&record,&scope),
                        "skill"=>crate::engine::memory::visible(&record,&scope),
                        "experience"=>crate::engine::experience::visible(&record,&scope),
                        "experience_rule"=>crate::engine::experience::rule_visible(&record,&scope),
                        _=>true,
                    };
                    !visible || json!(evidence_digest(dependency["kind"].as_str().unwrap_or(""), &record)) != dependency["digest"]
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

pub(crate) fn commit_projection(state: &AppState, plan: &Value, message: &Value, mut projection: Projection) {
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

pub(crate) fn prepare(
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
    // Cache affinity belongs to the bot, not the room. Transport cursors
    // above remain conversation-local so a cache hint cannot mix histories.
    let identity = super::context::identity_prefix(state, request).0;
    let affinity = stable_id(&[
        "agent-cache-v2", request.agent_id.as_deref().unwrap_or(session_id),
        &provider.id, &provider.base_url, &provider.model,
        provider.account_id.as_deref().unwrap_or(""), &provider.kind,
        &provider.reasoning_protocol, &identity,
    ]);
    let key = format!("dominion_{affinity}");
    let previous = state.store.get("model_conversation", &id);
    let same_prefix = previous.as_ref().is_some_and(|record|record["prefixDigest"] == prefix);
    let mut record = previous.unwrap_or_else(||json!({
        "id":id,"sessionId":session_id,"agentId":request.agent_id,
        "providerId":provider.id,"model":provider.model,
        "createdAt":chrono::Utc::now().to_rfc3339(),
    }));
    record["transportSessionId"] = json!(scope);
    record["cacheAffinityId"] = json!(affinity);
    record["cacheKey"] = json!(key);
    record["prefixDigest"] = json!(prefix);
    record["lastRequestedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put("model_conversation", &record)
        .ok_or("봇의 지속 대화 상태를 저장할 수 없습니다.")?;
    Ok(json!({
        "id":id,"participantId":participant_id,"transport":provider.kind,"mode":"prefix-reuse",
        "transportSessionId":scope,"cacheAffinityId":affinity,"cacheKey":key,"prefixDigest":prefix,
        "samePrefixAsPreviousRequest":same_prefix,
        "previousReplyMessageId":participant.get("lastReplyMessageId").unwrap_or(&record["lastReplyMessageId"]),
        "participantBaseReplyMessageId":participant["lastReplyMessageId"],
        "previousRouteReplyMessageId":record["lastReplyMessageId"],
    }))
}

/// Only a durably stored visible answer advances the bot's continuation cursor.
pub(crate) fn commit(state: &AppState, continuity: &Value, message: &Value) {
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

pub(super) fn rejects_hint(status: reqwest::StatusCode, text: &str, field: &str) -> bool {
    if !matches!(status.as_u16(),400|422) {return false}
    let lower=text.to_ascii_lowercase();
    let parsed=serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let parameter=parsed["error"]["param"].as_str().or_else(||parsed["param"].as_str());
    (parameter.is_some_and(|param|param.contains(field)) || lower.contains(field))
        && ["unsupported", "not supported", "unrecognized", "unknown", "not permitted", "not allowed", "extra_forbidden", "extra inputs"].iter().any(|word|lower.contains(word))
}

fn feature_id(provider: &ProviderRef, wire: CacheWire) -> String {
    format!("wire-{}", stable_id(&[
        &provider.id, &provider.base_url, &provider.kind, &provider.model,
        provider.account_id.as_deref().unwrap_or(""), wire.field(),
    ]))
}

pub(super) fn note_hint(state: &AppState, provider: &ProviderRef, wire: CacheWire, accepted: bool, status: u16) {
    state.store.put("provider_wire_feature", &json!({
        "id":feature_id(provider,wire), "field":wire.field(), "accepted":accepted,
        "observedAt":chrono::Utc::now().to_rfc3339(), "httpStatus":status,
        "expiresAt":if accepted {Value::Null} else {json!(chrono::Utc::now().timestamp()+86400)},
    }));
}

/// Both HTTP and WebSocket must send the same negotiated cache identity.
pub(super) fn hinted_body(state: &AppState, provider: &ProviderRef, original: &Value, plan: &mut Value, wire: CacheWire) -> Value {
    let known=state.store.get("provider_wire_feature", &feature_id(provider,wire));
    let disabled=known.as_ref().is_some_and(|record|record["accepted"]==false
        && record["expiresAt"].as_i64().is_some_and(|until|until>chrono::Utc::now().timestamp()));
    let mut body=original.clone();
    let hinted=!disabled && apply_hint(&mut body,plan,wire);
    plan["cacheHint"]=json!(wire.field());
    plan["wireHintSent"]=json!(hinted);
    plan["cacheHintAcceptance"]=if disabled {json!(false)} else {Value::Null};
    body
}

/// A protocol hint is negotiated once from a specific rejection, never from
/// a model name. Failure of an optional hint must not remove conversation state.
pub(super) async fn send(
    state:&AppState, provider:&ProviderRef, builder:reqwest::RequestBuilder,
    original:&Value, plan:&mut Value, wire:CacheWire,
) -> Result<reqwest::Response,String> {
    let mut body=hinted_body(state,provider,original,plan,wire);
    let mut hinted=plan["wireHintSent"]==true;
    let opencode_go=state.store.get("provider",&provider.id)
        .is_some_and(|record|record["integration"]=="opencode-go");
    for attempt in 1..=2 {
        plan["httpRequests"]=json!(attempt);
        let mut request=builder.try_clone().ok_or("모델 요청을 복제할 수 없습니다.")?;
        if let Some(id)=plan["cacheAffinityId"].as_str().or_else(||plan["transportSessionId"].as_str()) {
            request=request.header("x-dominion-conversation",id);
            // OpenCode Go's direct endpoint uses the same stable conversation
            // affinity under its own header. No local proxy is involved.
            if opencode_go { request=request.header("x-opencode-session",id); }
        }
        let response=request.json(&body).send().await.map_err(|e|format!("{} 연결 실패: {e}",provider.name))?;
        let status=response.status();
        if hinted && attempt==1 && matches!(status.as_u16(),400|422) {
            let headers=response.headers().clone();let version=response.version();
            let bytes=response.bytes().await.map_err(|e|e.to_string())?;
            if rejects_hint(status,&String::from_utf8_lossy(&bytes),wire.field()) {
                note_hint(state,provider,wire,false,status.as_u16());
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
            note_hint(state,provider,wire,true,status.as_u16());
        }
        return Ok(response)
    }
    unreachable!("one optional compatibility retry")
}

#[cfg(test)]
#[path = "../../../tests/unit/engine_chat_continuity_tests.rs"]
mod tests;
