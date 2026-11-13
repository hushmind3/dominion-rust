//! Durable owner-stated room state.
//!
//! A room transcript is retrieval material: the context budget may drop an
//! early message as the conversation grows. Explicit owner facts, decisions
//! and corrections are different — they must survive those rebases. This
//! module keeps the owner's exact wording in a small room-level record and
//! renders it as mandatory context, not as optional history.
use crate::{engine::memory, state::AppState};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// A room-wide read model reduced from committed events, independent of the
/// selected speaker and of natural-language commands. References stay in the
/// store; the model receives the compact state, not N copies of raw history.
pub(crate) fn conversation_context(
    state: &AppState, room: &Value, session_id: &str, source_id: &str, prior: &[Value],
) -> Value {
    let members=room["agentIds"].as_array().cloned().unwrap_or_default();
    let previous=prior.iter().rev().find(|row|row["session_id"]==session_id && row["role"]=="user");
    let previous_id=previous.and_then(|row|row["id"].as_str());
    let cached=&room["sharedContext"];
    if cached["sessionId"]==session_id && cached["sourceMessageId"]==source_id && cached["memberIds"]==json!(members) {
        return cached.clone();
    }
    let mut latest=BTreeMap::<String,Value>::new();
    let mut owner=None;
    let mut previous_replies=BTreeMap::<String,Value>::new();
    for row in prior {
        if row["session_id"]!=session_id || row["metadata"]["kind"]=="learning_report"
            || row["metadata"]["authorKind"]=="system" {continue;}
        if row["role"]=="user" {owner=row["id"].as_str();continue;}
        if row["role"]!="assistant" || row["content"].as_str().is_none_or(|s|s.trim().is_empty())
            || row["tool_calls"].as_array().is_some_and(|calls|!calls.is_empty()) {continue;}
        let Some(agent)=row["agent_id"].as_str().filter(|id|members.iter().any(|m|m.as_str()==Some(id))) else {continue};
        latest.insert(agent.into(),row.clone());
        let reply_to=row["metadata"]["replyToMessageId"].as_str().or(owner);
        if previous_id.is_some() && reply_to==previous_id {previous_replies.insert(agent.into(),row.clone());}
    }
    let mut participants=Vec::new();
    let mut responded=Vec::new();
    let mut missing=Vec::new();
    let mut observed_work=Vec::new();
    for member in &members {
        let Some(id)=member.as_str() else {continue};
        let name=state.store.get("agent",id).and_then(|a|a["name"].as_str().map(str::to_string)).unwrap_or_else(||id.into());
        let reply=latest.get(id);
        let execution=reply.and_then(|r|r["metadata"]["executionId"].as_str()).and_then(|id|state.store.get("execution",id));
        let tool_calls=execution.as_ref().and_then(|e|e["toolCalls"].as_u64());
        if previous_replies.contains_key(id) {responded.push(name.clone());}
        else if previous_id.is_some(){missing.push(name.clone());}
        if let Some(calls)=tool_calls.filter(|calls|*calls>0) {
            observed_work.push(format!("{name}: 도구 호출 {calls}회, 결과 {}",execution.as_ref().unwrap()["taskOutcome"].as_str().unwrap_or("미확정")));
        }
        participants.push(json!({"agentId":id,"name":name,
            "lastReplyMessageId":reply.map(|r|&r["id"]),
            "lastReplyToMessageId":reply.map(|r|&r["metadata"]["replyToMessageId"]),
            "repliedToPreviousRequest":previous_replies.contains_key(id),
            "executionId":execution.as_ref().map(|e|&e["id"]),"toolCalls":tool_calls}));
    }
    let mut text=String::from("[방 공통 상태: 현재 요청 시점까지의 전체 원문 기록에서 엔진이 계산함]\n");
    if previous.is_some() {
        // The actual prior owner message is already selected verbatim by the
        // compiler. Keep its reference here rather than repeat its body.
        text.push_str(&format!("직전 요청의 응답 기록: {}/{}명. 응답함: {}. 응답 기록 없음: {}.\n",
            responded.len(),members.len(),responded.join(", "),if missing.is_empty(){"없음".into()}else{missing.join(", ")}));
    }else{text.push_str("이 방의 첫 주인 요청이며 이전 응답 기록은 없음.\n");}
    text.push_str(&format!("기준 요청: {source_id}. 직전 요청: {}.\n",previous_id.unwrap_or("없음")));
    if !observed_work.is_empty(){text.push_str(&format!("관측된 작업: {}.\n",observed_work.join("; ")));}
    text.push_str("이후 대화는 선택된 발췌다. 발췌에 안 보인다는 이유로 미응답·부재로 판단하지 말 것. 발언과 실제 도구 실행 결과는 구분할 것.");
    let snapshot=json!({"sessionId":session_id,"sourceMessageId":source_id,"memberIds":members,
        "previousOwnerMessageId":previous_id,"participants":participants,"text":text,
        "digest":memory::stable_id(&[&json!([source_id,previous_id,&participants]).to_string()])});
    if let Some(id)=room["id"].as_str(){
        state.store.patch_if("room",id,"id",&json!(id),&json!({"sharedContext":snapshot}));
    }
    snapshot
}

const MAX_ENTRIES: usize = 256;
const MAX_CONTEXT_ENTRIES: usize = 12;
const MAX_CONTEXT_CHARS: usize = 2_400;
const MAX_ENTRY_CHARS: usize = 1_200;

fn state_id(room_id: &str) -> String {
    format!("room-state-{}", memory::stable_id(&[room_id]))
}

fn is_question(text: &str) -> bool {
    let trimmed = text.trim_end();
    trimmed.ends_with('?') || trimmed.ends_with('？')
}

/// Only explicit owner memory/decision wording is promoted. A discussion
/// prompt such as "rollback criteria" is not a confirmed fact.
fn is_durable_owner_text(text: &str) -> bool {
    if text.trim().is_empty() || is_question(text) {
        return false;
    }
    // A recall/imperative message can mention "폐기" or "암호" without stating
    // a new fact. Do not promote the question into the durable state.
    const REQUEST_MARKERS: &[&str] = &[
        "말해",
        "알려",
        "설명해",
        "정리해",
        "확인해",
        "보고해",
        "답해",
        "각각",
        "뭐였지",
        "무엇",
        "어떻게",
        "언제",
        "어디",
        "누구",
    ];
    let lower = text.to_lowercase();
    if REQUEST_MARKERS.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    const MARKERS: &[&str] = &[
        "기억해",
        "기억하",
        "명심",
        "잊지",
        "확정",
        "정정",
        "폐기",
        "약속",
        "앞으로",
        "항상",
        "절대",
        "금지",
        "승인",
        "원한다",
        "원해",
        "담당자는",
        "암호는",
        "비밀번호는",
        "규칙은",
        "원칙은",
        "정책은",
        "방침은",
    ];
    MARKERS
        .iter()
        .any(|marker| text.to_lowercase().contains(marker))
}

fn entry_kind(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if ["정정", "폐기", "틀렸", "아니라"]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        "correction"
    } else if [
        "확정",
        "정한다",
        "정했다",
        "규칙은",
        "원칙은",
        "정책은",
        "방침은",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        "decision"
    } else {
        "fact"
    }
}

fn code_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .filter(|token| token.len() >= 4)
        .filter(|token| token.chars().any(|ch| ch.is_ascii_uppercase()))
        .map(str::to_string)
        .collect()
}

/// Domain keys are intentionally narrow: a correction must not erase
/// unrelated owner facts merely because both contain the word "정정".
/// This is a deterministic fallback, not a substitute for semantic memory.
fn fact_domain(text: &str) -> Option<&'static str> {
    if text.contains("배포 담당") || text.contains("배포담당") {
        Some("deployment_owner")
    } else if text.contains("암호") || text.contains("비밀번호") {
        Some("credential")
    } else if text.contains("담당자") {
        Some("owner")
    } else if text.contains("원칙") {
        Some("principle")
    } else if text.contains("정책") || text.contains("방침") {
        Some("policy")
    } else if text.contains("규칙") {
        Some("rule")
    } else {
        None
    }
}

fn query_relevance(query: &str, text: &str) -> usize {
    if query.trim().is_empty() {
        return 0;
    }
    let mut score = 0usize;
    if let (Some(asked), Some(stated)) = (fact_domain(query), fact_domain(text)) {
        if asked == stated {
            score += 32;
        }
    }
    let query_tokens = memory::terms(query);
    let text_tokens = memory::terms(text);
    score + query_tokens.intersection(&text_tokens).count().min(12)
}

/// Records one owner message if it states a durable fact, decision or
/// correction. Idempotent by source message id; the full original message
/// remains in the transcript.
pub fn observe_owner_message(
    state: &AppState,
    room_id: &str,
    session_id: &str,
    message: &Value,
) -> Option<Value> {
    if room_id.is_empty()
        || message["role"].as_str() != Some("user")
        || !message["agent_id"].is_null()
        || message["metadata"]["authorKind"].as_str() != Some("owner")
        // External humans use the room's user-message ledger, but that does
        // not grant them the workspace owner's standing instruction authority.
        || message["metadata"]["origin"].is_object()
    {
        return None;
    }
    let text = message["content"].as_str().unwrap_or("").trim();
    if !is_durable_owner_text(text) {
        return None;
    }
    let message_id = message["id"].as_str()?.to_string();
    let id = state_id(room_id);
    let mut record = state.store.get("room_state", &id).unwrap_or_else(|| {
        json!({
            "id": id,
            "roomId": room_id,
            "sessionId": session_id,
            "facts": [],
            "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    });
    let mut facts = record["facts"].as_array().cloned().unwrap_or_default();
    if facts
        .iter()
        .any(|fact| fact["sourceMessageId"].as_str() == Some(message_id.as_str()))
    {
        return Some(record);
    }
    let kind = entry_kind(text);
    let tokens = code_tokens(text);
    if kind == "correction" {
        for fact in facts.iter_mut() {
            if fact["status"].as_str() != Some("active") {
                continue;
            }
            let old = fact["text"].as_str().unwrap_or("");
            let same_explicit_code = code_tokens(old)
                .iter()
                .any(|token| tokens.iter().any(|new| new == token));
            let same_domain = fact_domain(text).is_some()
                && fact_domain(text) == fact_domain(old);
            if same_explicit_code || same_domain {
                fact["status"] = json!("superseded");
                fact["supersededBy"] = json!(message_id);
                fact["supersededAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
        }
    }
    facts.push(json!({
        "id": format!("room-state-entry-{}", memory::stable_id(&[room_id, &message_id])),
        "kind": kind,
        "text": text.chars().take(MAX_ENTRY_CHARS).collect::<String>(),
        "sourceMessageId": message_id.clone(),
        "createdAt": message["created_at"].as_str().unwrap_or("").to_string(),
        "status": "active",
    }));
    if facts.len() > MAX_ENTRIES {
        let drop = facts.len() - MAX_ENTRIES;
        facts.drain(..drop);
    }
    record["sessionId"] = json!(session_id);
    record["facts"] = json!(facts);
    record["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("room_state", &record)?;
    state.bus.record("room_state", &saved);
    state.store.audit(
        "room.owner_state_observed",
        json!({
            "roomId": room_id,
            "sessionId": session_id,
            "messageId": message_id,
            "stateId": saved["id"],
            "kind": kind,
        }),
    );
    Some(saved)
}

/// Renders the active owner state as a compact, verbatim block. The caller
/// appends it to the fixed room prompt so a history-budget rebase cannot drop
/// it. Newest entries win when the context cap is reached.
pub fn context_block(state: &AppState, room_id: &str) -> Option<String> {
    context_block_for_query(state, room_id, "")
}

/// Selects the most relevant durable statements for the current question,
/// rather than evicting an early decision merely because later turns are long.
pub fn context_block_for_query(
    state: &AppState,
    room_id: &str,
    query: &str,
) -> Option<String> {
    context_block_at(state,room_id,query,None)
}

fn context_block_at(state:&AppState,room_id:&str,query:&str,visible:Option<&HashSet<String>>)->Option<String>{
    let record = state.store.get("room_state", &state_id(room_id))?;
    let facts = record["facts"].as_array()?;
    let mut ranked = facts
        .iter()
        .enumerate()
        .filter(|(_,fact)|visible.is_none_or(|ids|fact["sourceMessageId"].as_str().is_some_and(|id|ids.contains(id))))
        .filter(|(_, fact)| fact["status"].as_str() == Some("active")
            || visible.is_some_and(|ids|fact["status"]=="superseded"
                && fact["supersededBy"].as_str().is_some_and(|id|!ids.contains(id))))
        .filter(|(_, fact)| {
            !fact["sourceMessageId"]
                .as_str()
                .and_then(|id| state.store.message(id))
                .is_some_and(|source| source["metadata"]["origin"].is_object())
        })
        .filter_map(|(index, fact)| {
            let text = fact["text"].as_str()?.trim();
            (!text.is_empty()).then_some((query_relevance(query, text), index, fact))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut selected = Vec::new();
    let mut used = 0usize;
    for (_, index, fact) in ranked {
        let kind = fact["kind"].as_str().unwrap_or("fact");
        let text = fact["text"].as_str().unwrap_or("").trim();
        let line = format!("- [{kind}] {text}");
        let cost = line.chars().count() + 1;
        if used + cost > MAX_CONTEXT_CHARS {
            continue;
        }
        selected.push((index, line));
        used += cost;
        if selected.len() == MAX_CONTEXT_ENTRIES {
            break;
        }
    }
    if selected.is_empty() {
        return None;
    }
    selected.sort_by_key(|(index, _)| *index);
    Some(selected.into_iter().map(|(_, line)| line).collect::<Vec<_>>().join("\n"))
}

/// All participants evaluate owner facts at the same request cursor, including
/// when a later owner correction has already been queued in the same room.
pub(crate) fn context_snapshot_at(state:&AppState,room_id:&str,query:&str,messages:&[Value])->Option<(String,String)>{
    let visible=messages.iter().filter_map(|m|m["id"].as_str().map(str::to_string)).collect::<HashSet<_>>();
    context_block_at(state,room_id,query,Some(&visible)).map(|block|(state_id(room_id),block))
}

/// Clears the durable owner state when the room is reset.
pub fn clear(state: &AppState, room_id: &str) -> bool {
    state.store.remove("room_state", &state_id(room_id))
}

#[cfg(test)]
#[path = "room_state/tests.rs"]
mod tests;
