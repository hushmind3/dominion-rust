//! Room-level semantic knowledge.
//!
//! A completed multi-agent exchange is one learning unit, not one unit per
//! speaker. The extractor may read the owner and every participant, but the
//! stored record keeps the source message, agent and model for each statement.
//! Provenance is evidence: it is displayed and superseded, never used as a
//! model/provider applicability filter. Any model in the same room can recall
//! the same knowledge.
use crate::{
    engine::{jobs, jobs::JobContext, llm, memory, program},
    state::AppState,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_MESSAGES: usize = 80;
const MAX_CONTENT_CHARS: usize = 1_200;
const MAX_PROMPT_CHARS: usize = 24_000;
const MAX_ITEMS: usize = 24;
const MAX_TEXT_CHARS: usize = 1_400;

const SYSTEM_PROMPT: &str = "You extract durable knowledge from a multi-agent room transcript. Output one JSON object and nothing else. Rules: (1) Use only the supplied transcript; never add outside facts. (2) Types are claim (an unverified assertion), hypothesis (a testable proposal), conclusion (a supported answer), correction (an explicit replacement of an earlier statement). (3) Every item needs sourceMessageIds and at least one evidenceQuote copied exactly from those messages. (4) corrections need correctsMessageIds and explicit corrective wording. (5) Agent statements are not owner facts. Use owner_confirmed only when an owner/user message confirms the item; tool_evidence only when a successful tool operation is shown; otherwise debate or unverified. (6) topicKey is a short stable lowercase slug. (7) If nothing is durable, return {\"knowledge\":[]}. Schema: {\"knowledge\":[{\"type\":\"claim|hypothesis|conclusion|correction\",\"topicKey\":\"...\",\"text\":\"...\",\"confidence\":0.0,\"sourceMessageIds\":[\"...\"],\"evidenceQuotes\":[\"...\"],\"correctsMessageIds\":[\"...\"],\"verification\":\"owner_confirmed|tool_evidence|debate|unverified\"}]}";

fn knowledge_id(room_id: &str, topic: &str, kind: &str, text: &str) -> String {
    format!(
        "room-knowledge-{}",
        memory::stable_id(&[room_id, topic, kind, &normalize_text(text)])
    )
}

fn checkpoint_id(room_id: &str) -> String {
    format!(
        "room-learning-checkpoint-{}",
        memory::stable_id(&[room_id])
    )
}

fn state_id(room_id: &str) -> String {
    format!("room-learning-state-{}", memory::stable_id(&[room_id]))
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalize_quote(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A question or request is not owner confirmation of the extracted claim.
fn is_question(text: &str) -> bool {
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace());
    trimmed.ends_with('?') || trimmed.ends_with('？')
}

fn contains_quote(content: &str, quote: &str) -> bool {
    let quote = normalize_quote(quote);
    !quote.is_empty() && normalize_quote(content).contains(&quote)
}

fn topic_key(value: &Value, text: &str) -> String {
    let raw = value
        .as_str()
        .unwrap_or("")
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.')
        .take(80)
        .collect::<String>();
    if raw.is_empty() {
        format!("topic-{}", memory::stable_id(&[&normalize_text(text)]))
    } else {
        raw
    }
}

fn agent_name(state: &AppState, agent_id: &str) -> String {
    state
        .store
        .get("agent", agent_id)
        .and_then(|agent| agent["name"].as_str().map(str::to_string))
        .unwrap_or_else(|| agent_id.to_string())
}

fn tool_evidence(state: &AppState, execution_id: &str) -> Vec<Value> {
    state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|operation| operation["runId"].as_str() == Some(execution_id))
        .filter(|operation| operation["status"].as_str() == Some("completed"))
        .filter_map(|operation| {
            Some(json!({
                "tool": operation["tool"],
                "status": operation["status"],
                "effectState": operation["effectState"],
            }))
        })
        .take(8)
        .collect()
}

/// A message with the provenance needed for both the prompt and the durable
/// record. `content` is the full original text used for quote validation.
fn transcript_item(state: &AppState, message: &Value) -> Value {
    let agent_id = message["agent_id"].as_str().unwrap_or("");
    let execution_id = message["metadata"]["executionId"].as_str().unwrap_or("");
    let execution = if execution_id.is_empty() {
        None
    } else {
        state.store.get("execution", execution_id)
    };
    let model = execution
        .as_ref()
        .and_then(|execution| {
            execution["effectiveModel"]
                .as_str()
                .or_else(|| execution["model"].as_str())
        })
        .unwrap_or("");
    let provider_id = execution
        .as_ref()
        .and_then(|execution| execution["providerId"].as_str())
        .unwrap_or("");
    let tools = if execution_id.is_empty() {
        Vec::new()
    } else {
        tool_evidence(state, execution_id)
    };
    json!({
        "messageId": message["id"],
        "role": message["role"],
        "authorKind": message["metadata"]["authorKind"],
        "agentId": if agent_id.is_empty() { Value::Null } else { json!(agent_id) },
        "agentName": if agent_id.is_empty() {
            json!("방 주인")
        } else {
            json!(agent_name(state, agent_id))
        },
        "model": if model.is_empty() { Value::Null } else { json!(model) },
        "providerId": if provider_id.is_empty() { Value::Null } else { json!(provider_id) },
        "content": message["content"].as_str().unwrap_or("").chars().take(MAX_CONTENT_CHARS).collect::<String>(),
        "tools": tools,
    })
}

fn source_entry(state: &AppState, message: &Value, quote: &str) -> Value {
    let item = transcript_item(state, message);
    json!({
        "messageId": item["messageId"],
        "role": item["role"],
        "authorKind": item["authorKind"],
        "agentId": item["agentId"],
        "agentName": item["agentName"],
        "model": item["model"],
        "providerId": item["providerId"],
        "quote": quote.chars().take(500).collect::<String>(),
    })
}

fn has_tool_evidence(sources: &[Value]) -> bool {
    sources
        .iter()
        .any(|source| source["tools"].as_array().is_some_and(|tools| !tools.is_empty()))
}

fn rank(kind: &str) -> u8 {
    match kind {
        "correction" => 0,
        "conclusion" => 1,
        "claim" => 2,
        "hypothesis" => 3,
        _ => 4,
    }
}

fn supersede(
    state: &AppState,
    room_id: &str,
    replacement_id: &str,
    topic: &str,
    kind: &str,
    corrects: &BTreeSet<String>,
) -> usize {
    let mut count = 0;
    for mut record in state.store.all("room_knowledge") {
        if record["roomId"].as_str() != Some(room_id)
            || record["status"].as_str() != Some("active")
            || record["id"].as_str() == Some(replacement_id)
        {
            continue;
        }
        let old_kind = record["type"].as_str().unwrap_or("");
        let message_hit = record["sources"].as_array().is_some_and(|sources| {
            sources.iter().any(|source| {
                source["messageId"]
                    .as_str()
                    .is_some_and(|id| corrects.contains(id))
            })
        });
        let topic_hit = record["topicKey"].as_str() == Some(topic)
            && (kind == "correction"
                || (kind == "conclusion" && matches!(old_kind, "claim" | "hypothesis")));
        if !(message_hit || topic_hit) {
            continue;
        }
        record["status"] = json!("superseded");
        record["supersededBy"] = json!(replacement_id);
        record["supersededAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(saved) = state.store.put("room_knowledge", &record) {
            state.bus.record("room_knowledge", &saved);
            count += 1;
        }
    }
    count
}

/// Validates one extraction result and persists it. Kept separate from the
/// model call so the evidence rules can be tested without a network provider.
fn apply_extraction(
    state: &AppState,
    room_id: &str,
    session_id: &str,
    batch: &[Value],
    extracted: &Value,
) -> Result<Value, String> {
    let by_id = batch
        .iter()
        .filter_map(|message| {
            Some((
                message["id"].as_str()?.to_string(),
                message.clone(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let candidates = extracted["knowledge"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let (mut inserted, mut updated, mut superseded, mut rejected, mut promoted) = (0, 0, 0, 0, 0);
    for candidate in candidates.into_iter().take(MAX_ITEMS) {
        let kind = candidate["type"].as_str().unwrap_or("").trim();
        if !matches!(kind, "claim" | "hypothesis" | "conclusion" | "correction") {
            rejected += 1;
            continue;
        }
        let text = candidate["text"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(MAX_TEXT_CHARS)
            .collect::<String>();
        if text.is_empty() {
            rejected += 1;
            continue;
        }
        let source_ids = candidate["sourceMessageIds"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .filter(|id| by_id.contains_key(id))
            .collect::<Vec<_>>();
        if source_ids.is_empty() {
            rejected += 1;
            continue;
        }
        let quotes = candidate["evidenceQuotes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        let mut sources = Vec::new();
        for id in &source_ids {
            let Some(message) = by_id.get(id) else {
                continue;
            };
            let Some(quote) = quotes.iter().find(|quote| {
                contains_quote(message["content"].as_str().unwrap_or(""), quote)
            }) else {
                continue;
            };
            let mut source = source_entry(state, message, quote);
            let tools = tool_evidence(
                state,
                message["metadata"]["executionId"].as_str().unwrap_or(""),
            );
            source["tools"] = json!(tools);
            sources.push(source);
        }
        if sources.is_empty() {
            rejected += 1;
            continue;
        }
        let corrects = candidate["correctsMessageIds"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .filter(|id| by_id.contains_key(id))
            .collect::<BTreeSet<_>>();
        let owner_confirmed = sources.iter().any(|source| {
            source["role"] == "user"
                && source["quote"]
                    .as_str()
                    .map(|quote| !is_question(quote))
                    .unwrap_or(false)
        });
        let tool_confirmed = has_tool_evidence(&sources);
        let mut kind = kind.to_string();
        let (verification, authority) = if owner_confirmed {
            (
                "owner_confirmed",
                if kind == "correction" {
                    "owner_correction"
                } else {
                    "owner_confirmed"
                },
            )
        } else if tool_confirmed {
            ("tool_evidence", "tool_evidence")
        } else {
            ("debate", "debate")
        };
        let mut verification = verification.to_string();
        let mut authority = authority.to_string();
        // A single agent's confident sentence is not a conclusion.
        if kind == "conclusion" && verification == "debate" && sources.len() < 2 {
            kind = "claim".into();
            verification = "unverified".into();
            authority = "agent_claim".into();
        }
        // A correction without an explicit target or owner confirmation is
        // stored as a claim, not as a replacement.
        if kind == "correction" && corrects.is_empty() && !owner_confirmed {
            kind = "claim".into();
            verification = "unverified".into();
            authority = "agent_claim".into();
        }
        if kind == "claim" && verification == "debate" {
            verification = "unverified".into();
            authority = "agent_claim".into();
        }
        if kind == "hypothesis" && verification == "debate" {
            authority = "agent_hypothesis".into();
        }
        if kind == "correction" && authority == "debate" {
            authority = "agent_correction".into();
        }
        let topic = topic_key(&candidate["topicKey"], &text);
        let id = knowledge_id(room_id, &topic, &kind, &text);
        let confidence = candidate["confidence"]
            .as_f64()
            .unwrap_or(if owner_confirmed { 0.9 } else { 0.5 })
            .clamp(0.0, 1.0);
        superseded += supersede(state, room_id, &id, &topic, &kind, &corrects);
        if let Some(mut existing) = state.store.get("room_knowledge", &id) {
            let mut existing_sources = existing["sources"].as_array().cloned().unwrap_or_default();
            let mut added = false;
            for source in &sources {
                let key = (
                    source["messageId"].as_str().unwrap_or("").to_string(),
                    source["quote"].as_str().unwrap_or("").to_string(),
                );
                let present = existing_sources.iter().any(|item| {
                    item["messageId"].as_str() == Some(key.0.as_str())
                        && item["quote"].as_str() == Some(key.1.as_str())
                });
                if !present {
                    existing_sources.push(source.clone());
                    added = true;
                }
            }
            existing_sources.truncate(24);
            existing["sources"] = json!(existing_sources);
            existing["text"] = json!(text);
            existing["type"] = json!(kind);
            existing["topicKey"] = json!(topic);
            existing["authority"] = json!(authority);
            existing["verification"] = json!(verification);
            existing["confidence"] = json!(confidence);
            // Re-processing an old extraction must not resurrect knowledge that
            // a later correction already replaced.
            if existing["status"].as_str() != Some("superseded") {
                existing["status"] = json!("active");
            }
            existing["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            if added {
                existing["occurrences"] =
                    json!(existing["occurrences"].as_u64().unwrap_or(1) + 1);
            }
            if let Some(saved) = state.store.put("room_knowledge", &existing) {
                state.bus.record("room_knowledge", &saved);
                if promote_global(state, &saved) {
                    promoted += 1;
                }
                updated += 1;
            }
            continue;
        }
        let record = json!({
            "id": id,
            "roomId": room_id,
            "sessionId": session_id,
            "type": kind,
            "topicKey": topic,
            "text": text,
            "status": "active",
            "authority": authority,
            "verification": verification,
            "confidence": confidence,
            "sources": sources,
            "correctsMessageIds": corrects,
            "supersedes": [],
            "supersededBy": Value::Null,
            "occurrences": 1,
            "createdAt": chrono::Utc::now().to_rfc3339(),
            "updatedAt": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(saved) = state.store.put("room_knowledge", &record) {
            state.bus.record("room_knowledge", &saved);
            if promote_global(state, &saved) {
                promoted += 1;
            }
            inserted += 1;
        }
    }
    Ok(json!({
        "inserted": inserted,
        "updated": updated,
        "superseded": superseded,
        "rejected": rejected,
        "promoted": promoted,
    }))
}

/// Validated room conclusions become global, model-agnostic assets. They are
/// still provenance-linked to the room but are no longer limited to it; chat
/// and other rooms retrieve them through the experience context.
fn promote_global(state: &AppState, knowledge: &Value) -> bool {
    if knowledge["status"].as_str() != Some("active") {
        return false;
    }
    if !matches!(
        knowledge["verification"].as_str(),
        Some("owner_confirmed" | "tool_evidence")
    ) {
        return false;
    }
    if !matches!(knowledge["type"].as_str(), Some("conclusion" | "correction")) {
        return false;
    }
    let Some(text) = knowledge["text"].as_str().filter(|text| !text.trim().is_empty()) else {
        return false;
    };
    let id = format!(
        "room-global-{}",
        memory::stable_id(&[&normalize_text(text)])
    );
    let models = knowledge["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| source["model"].as_str().filter(|value| !value.is_empty()))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let agents = knowledge["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|source| source["agentId"].as_str().filter(|value| !value.is_empty()))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut principle = state
        .store
        .get("experience_principle", &id)
        .unwrap_or_else(|| {
            json!({
                "id": id,
                "kind": "room_validated",
                "origin": "room_knowledge",
                "scope": "global",
                "status": "active",
                "principle": text,
                "topicKey": knowledge["topicKey"],
                "evidenceGrade": "E2_verified",
                "confidence": knowledge["confidence"],
                "supportingCases": [],
                "supportingModels": [],
                "supportingAgents": [],
                "sourceRooms": [],
                "counterexamples": [],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            })
        });
    let mut cases = principle["supportingCases"].as_array().cloned().unwrap_or_default();
    if !cases.iter().any(|case| case.as_str() == knowledge["id"].as_str()) {
        cases.push(knowledge["id"].clone());
    }
    cases.truncate(50);
    principle["supportingCases"] = json!(cases);
    let mut support_models = principle["supportingModels"].as_array().cloned().unwrap_or_default();
    for model in models {
        if !support_models.iter().any(|value| value.as_str() == Some(model.as_str())) {
            support_models.push(json!(model));
        }
    }
    principle["supportingModels"] = json!(support_models);
    let mut support_agents = principle["supportingAgents"].as_array().cloned().unwrap_or_default();
    for agent in agents {
        if !support_agents.iter().any(|value| value.as_str() == Some(agent.as_str())) {
            support_agents.push(json!(agent));
        }
    }
    principle["supportingAgents"] = json!(support_agents);
    let mut rooms = principle["sourceRooms"].as_array().cloned().unwrap_or_default();
    if let Some(room) = knowledge["roomId"].as_str() {
        if !rooms.iter().any(|value| value.as_str() == Some(room)) {
            rooms.push(json!(room));
        }
    }
    principle["sourceRooms"] = json!(rooms);
    principle["principle"] = json!(text);
    principle["confidence"] = json!(knowledge["confidence"]);
    principle["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("experience_principle", &principle) {
        state.bus.record("experience_principle", &saved);
        state.store.audit(
            "room_knowledge.promoted",
            json!({"id": saved["id"], "knowledgeId": knowledge["id"], "roomId": knowledge["roomId"]}),
        );
        true
    } else {
        false
    }
}

/// Registers one completed room exchange. The job id is content-addressed by
/// the session and the last message, so repeated completion is idempotent.
pub fn enqueue(
    state: &AppState,
    room_id: &str,
    session_id: &str,
    through_message_id: &str,
) -> Option<Value> {
    if room_id.is_empty() || session_id.is_empty() || through_message_id.is_empty() {
        return None;
    }
    let id = format!(
        "room-learn-{}",
        memory::stable_id(&[room_id, session_id, through_message_id])
    );
    jobs::enqueue_once(
        state,
        &id,
        "learn",
        json!({
            "sessionId": session_id,
            "roomId": room_id,
            "throughMessageId": through_message_id,
            "semantic": true,
            "roomLearning": true,
        }),
    )
}

/// Take the oldest unprocessed messages first. Processing newest-first would
/// advance the checkpoint past the older backlog and drop it permanently.
fn select_batch(messages: &[Value]) -> Vec<Value> {
    let mut selected = Vec::new();
    let mut chars = 0usize;
    for message in messages {
        if selected.len() >= MAX_MESSAGES {
            break;
        }
        let size = message["content"]
            .as_str()
            .map(|content| content.chars().count().min(MAX_CONTENT_CHARS) + 128)
            .unwrap_or(128);
        if !selected.is_empty() && chars + size > MAX_PROMPT_CHARS {
            break;
        }
        chars += size;
        selected.push(message.clone());
    }
    selected
}

fn build_prompt(state: &AppState, batch: &[Value]) -> String {
    let transcript = batch
        .iter()
        .map(|message| transcript_item(state, message))
        .collect::<Vec<_>>();
    json!({
        "messages": transcript,
        "instruction": "Extract only durable knowledge. Every quote must be an exact substring. Separate claims, hypotheses, conclusions and corrections. Preserve agent/model/message provenance through sourceMessageIds.",
    })
    .to_string()
}

/// Worker entry point. A missing learning provider is not a reason to drop the
/// transcript: the job fails, the raw messages remain, and the existing
/// learning retry path requeues it later.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let state = &context.state;
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let room_id = input["roomId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("방 학습에 roomId가 없습니다.")?;
    let session_id = input["sessionId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("방 학습에 sessionId가 없습니다.")?;
    let through = input["throughMessageId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("방 학습에 throughMessageId가 없습니다.")?;
    let all = state.store.messages(session_id);
    let end = all
        .iter()
        .position(|message| message["id"].as_str() == Some(through))
        .map(|index| index + 1)
        .ok_or("방 학습 스냅샷의 원문 메시지가 삭제되었습니다.")?;
    let checkpoint = state
        .store
        .get("room_learning_checkpoint", &checkpoint_id(room_id));
    let start = checkpoint
        .as_ref()
        .and_then(|checkpoint| checkpoint["throughMessageId"].as_str())
        .and_then(|through| {
            all.iter()
                .position(|message| message["id"].as_str() == Some(through))
        })
        .map(|index| index + 1)
        .unwrap_or(0);
    if start >= end {
        return Ok(json!({"status":"already_learned","knowledge":0}));
    }
    let batch = select_batch(&all[start..end]);
    if batch.is_empty() {
        return Ok(json!({"status":"empty","knowledge":0}));
    }
    let Some((provider_id, model)) = llm::learning_route(state) else {
        let _ = state.store.put(
            "room_learning_state",
            &json!({
                "id": state_id(room_id),
                "roomId": room_id,
                "sessionId": session_id,
                "status": "unprocessed",
                "reason": "learning model is not configured",
                "throughMessageId": through,
                "updatedAt": chrono::Utc::now().to_rfc3339(),
            }),
        );
        return Err("방 학습 모델이 설정되지 않았습니다. 원문은 보존되었습니다.".into());
    };
    let prompt = build_prompt(state, &batch);
    let (raw, used_provider, used_model) =
        llm::complete_with_fallback(state, &provider_id, &model, SYSTEM_PROMPT, &prompt)
            .await
            .map_err(|error| format!("방 학습 모델 호출 실패: {error}"))?;
    let extracted = program::parse_json_object(&raw).ok_or_else(|| {
        format!(
            "방 학습 응답에 JSON 객체가 없습니다: {}",
            raw.chars().take(240).collect::<String>()
        )
    })?;
    let report = apply_extraction(state, room_id, session_id, &batch, &extracted)?;
    if report["inserted"].as_u64().unwrap_or(0) + report["updated"].as_u64().unwrap_or(0) > 0 {
        state.bus.emit(json!({
            "type": "room_event",
            "roomId": room_id,
            "sessionId": session_id,
            "agentId": Value::Null,
            "turnId": "learning",
            "event": {
                "type": "knowledge",
                "inserted": report["inserted"],
                "updated": report["updated"],
                "promoted": report["promoted"],
            }
        }));
    }
    crate::engine::learning::record_review(
        state,
        session_id,
        None,
        "knowledge",
        0,
        false,
        Some((
            report["inserted"].as_u64().unwrap_or(0),
            report["updated"].as_u64().unwrap_or(0),
            report["promoted"].as_u64().unwrap_or(0),
        )),
    );
    let last_id = batch
        .last()
        .and_then(|message| message["id"].as_str())
        .unwrap_or(through)
        .to_string();
    let processed_end = all
        .iter()
        .position(|message| message["id"].as_str() == Some(last_id.as_str()))
        .map(|index| index + 1)
        .unwrap_or(end);
    let processed_messages = processed_end.saturating_sub(start);
    let remaining_messages = end.saturating_sub(processed_end);
    let partial = remaining_messages > 0;
    state
        .store
        .put(
            "room_learning_checkpoint",
            &json!({
                "id": checkpoint_id(room_id),
                "roomId": room_id,
                "sessionId": session_id,
                "throughMessageId": last_id,
                "updatedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("방 학습 체크포인트를 저장할 수 없습니다.")?;
    // More messages were waiting than one batch can hold. Queue the next
    // window with a checkpoint-specific id so no transcript segment is lost.
    if last_id != through {
        let id = format!(
            "room-learn-{}",
            memory::stable_id(&[room_id, session_id, through, &last_id])
        );
        jobs::enqueue_once(
            state,
            &id,
            "learn",
            json!({
                "sessionId": session_id,
                "roomId": room_id,
                "throughMessageId": through,
                "semantic": true,
                "roomLearning": true,
            }),
        );
    }
    state
        .store
        .put(
            "room_learning_state",
            &json!({
                "id": state_id(room_id),
                "roomId": room_id,
                "sessionId": session_id,
                "status": if partial { "partial" } else { "processed" },
                "throughMessageId": last_id,
                "targetMessageId": through,
                "processedMessages": processed_messages,
                "remainingMessages": remaining_messages,
                "progress": processed_messages as f64 / (processed_messages + remaining_messages).max(1) as f64,
                "providerId": used_provider,
                "model": used_model,
                "report": report,
                "updatedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("방 학습 상태를 저장할 수 없습니다.")?;
    state.store.audit(
        "room.knowledge_learned",
        json!({
            "roomId": room_id,
            "sessionId": session_id,
            "providerId": used_provider,
            "model": used_model,
            "report": report,
        }),
    );
    Ok(json!({
        "status": "completed",
        "roomId": room_id,
        "providerId": used_provider,
        "model": used_model,
        "report": report,
    }))
}

/// Knowledge visible to every participant of a room, regardless of which
/// model or agent created it.
fn relevance(record: &Value, query: &str) -> f64 {
    let text = format!(
        "{} {} {}",
        record["text"].as_str().unwrap_or(""),
        record["topicKey"].as_str().unwrap_or(""),
        record["type"].as_str().unwrap_or("")
    )
    .to_lowercase();
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && !('\u{ac00}'..='\u{d7a3}').contains(&c))
        .filter(|token| token.chars().count() >= 2)
        .map(|token| {
            if text.contains(token) {
                1.0
            } else {
                let prefix = token.chars().take(2).collect::<String>();
                if prefix.chars().count() == 2 && text.contains(&prefix) {
                    0.5
                } else {
                    0.0
                }
            }
        })
        .sum()
}

pub fn list(state: &AppState, room_id: &str) -> Vec<Value> {
    let mut items = state
        .store
        .all("room_knowledge")
        .into_iter()
        .filter(|record| record["roomId"].as_str() == Some(room_id))
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        let status = |record: &Value| u8::from(record["status"].as_str() != Some("active"));
        status(a)
            .cmp(&status(b))
            .then_with(|| {
                rank(a["type"].as_str().unwrap_or(""))
                    .cmp(&rank(b["type"].as_str().unwrap_or("")))
            })
            .then_with(|| {
                b["updatedAt"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(a["updatedAt"].as_str().unwrap_or(""))
            })
    });
    items
}

/// Prompt block for the room's shared knowledge. The model/provider is shown
/// as provenance only; there is deliberately no per-model selection here.
pub fn context_block(state: &AppState, room_id: &str, query: &str) -> Option<String> {
    let mut active = list(state, room_id)
        .into_iter()
        .filter(|record| record["status"].as_str() == Some("active"))
        .collect::<Vec<_>>();
    if !query.trim().is_empty() {
        active.sort_by(|a, b| {
            relevance(b, query)
                .partial_cmp(&relevance(a, query))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    rank(a["type"].as_str().unwrap_or(""))
                        .cmp(&rank(b["type"].as_str().unwrap_or("")))
                })
                .then_with(|| {
                    b["updatedAt"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(a["updatedAt"].as_str().unwrap_or(""))
                })
        });
        active.retain(|record|relevance(record,query)>0.0);
    }
    let items = active.into_iter().take(6).collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    let mut block = String::from(
        "Shared room knowledge (model-agnostic; source and verification are provenance, not a model filter):",
    );
    for item in items {
        let sources = item["sources"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|source| {
                format!(
                    "{}/{} ({})",
                    source["agentName"].as_str().unwrap_or("?"),
                    source["model"].as_str().unwrap_or("model unknown"),
                    source["messageId"].as_str().unwrap_or("message?")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        block.push_str(&format!(
            "\n- [{}] {} (verification: {}; confidence {:.2}; sources: {})",
            item["type"].as_str().unwrap_or("claim").to_uppercase(),
            item["text"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(500)
                .collect::<String>(),
            item["verification"].as_str().unwrap_or("unverified"),
            item["confidence"].as_f64().unwrap_or(0.0),
            sources
        ));
    }
    Some(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-room-knowledge-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn message(id: &str, role: &str, agent: Option<&str>, content: &str) -> Value {
        json!({
            "id": id,
            "session_id": "room:r1",
            "role": role,
            "agent_id": agent,
            "content": content,
            "metadata": {"authorKind": if role == "user" { "owner" } else { "agent" }},
        })
    }

    #[test]
    fn batch_starts_with_the_oldest_unprocessed_messages() {
        let messages = (0..120)
            .map(|index| {
                json!({
                    "id": format!("m{index}"),
                    "content": format!("message {index}"),
                })
            })
            .collect::<Vec<_>>();
        let batch = select_batch(&messages);
        assert_eq!(batch.len(), MAX_MESSAGES);
        assert_eq!(batch.first().unwrap()["id"], "m0");
        assert_eq!(batch.last().unwrap()["id"], "m79");
    }

    #[test]
    fn batch_respects_the_prompt_budget() {
        let long = "x".repeat(MAX_CONTENT_CHARS);
        let messages = (0..30)
            .map(|index| json!({"id": format!("m{index}"), "content": long}))
            .collect::<Vec<_>>();
        let batch = select_batch(&messages);
        let per_message = MAX_CONTENT_CHARS + 128;
        assert_eq!(batch.len(), MAX_PROMPT_CHARS / per_message);
        assert_eq!(batch.first().unwrap()["id"], "m0");
    }

    #[test]
    fn questions_do_not_count_as_owner_confirmation() {
        assert!(is_question("이 방식이 맞아?"));
        assert!(is_question("정말？ "));
        assert!(!is_question("이 방식이 맞습니다."));
        assert!(!is_question("좋아, 그걸로 확정해"));
    }

    #[test]
    fn relevance_prefers_the_question_topic() {
        let corpus = json!({"text":"database rollback runbook","topicKey":"database","type":"conclusion"});
        let other = json!({"text":"캐시 만료 정책","topicKey":"cache","type":"claim"});
        assert!(relevance(&corpus, "database rollback") > relevance(&other, "database rollback"));
        assert!(relevance(&other, "캐시 정책") > relevance(&corpus, "캐시 정책"));
    }

    #[test]
    fn extraction_keeps_provenance_and_supersedes_a_correction_once() {
        let (state, dir) = state();
        state.store.put("agent", &json!({"id":"a","name":"A"}));
        state.store.put("agent", &json!({"id":"b","name":"B"}));
        state.store.put(
            "session",
            &json!({"id":"room:r1","roomId":"r1","agentId":null}),
        );
        let owner = message("m1", "user", None, "배포 방식 정하자");
        let claim = message("m2", "assistant", Some("a"), "배포는 X 방식으로 하자");
        let correction = message(
            "m3",
            "assistant",
            Some("b"),
            "정정: X는 실패했다. 배포는 Y 방식으로 해야 한다.",
        );
        let confirm = message("m4", "user", None, "Y가 맞다");
        for item in [&owner, &claim, &correction, &confirm] {
            state.store.write_message(item);
        }
        let batch = vec![owner, claim, correction, confirm];
        let extraction = json!({"knowledge":[
            {
                "type":"conclusion","topicKey":"deploy","text":"배포는 X 방식",
                "sourceMessageIds":["m2"],"evidenceQuotes":["배포는 X 방식으로 하자"],
                "confidence":0.6,"verification":"debate"
            },
            {
                "type":"correction","topicKey":"deploy","text":"배포는 Y 방식",
                "sourceMessageIds":["m3","m4"],
                "evidenceQuotes":["배포는 Y 방식으로 해야 한다.","Y가 맞다"],
                "correctsMessageIds":["m2"],"confidence":0.95,
                "verification":"owner_confirmed"
            }
        ]});
        let first = apply_extraction(&state, "r1", "room:r1", &batch, &extraction).unwrap();
        assert_eq!(first["inserted"], 2, "{first}");
        let second = apply_extraction(&state, "r1", "room:r1", &batch, &extraction).unwrap();
        assert_eq!(second["inserted"], 0, "{second}");
        let items = list(&state, "r1");
        assert_eq!(items.len(), 2, "{items:?}");
        let correction = items
            .iter()
            .find(|item| item["type"] == "correction")
            .expect("correction");
        assert_eq!(correction["verification"], "owner_confirmed");
        let promoted = state.store.all("experience_principle");
        assert_eq!(promoted.len(), 1, "{promoted:?}");
        assert_eq!(promoted[0]["origin"], "room_knowledge");
        assert_eq!(promoted[0]["scope"], "global");
        assert_eq!(promoted[0]["principle"], "배포는 Y 방식");
        assert_eq!(correction["status"], "active");
        let claim = items
            .iter()
            .find(|item| item["type"] == "claim")
            .expect("claim");
        assert_eq!(claim["status"], "superseded", "{claim}");
        assert_eq!(claim["supersededBy"], correction["id"]);
        let sources = correction["sources"].as_array().unwrap();
        assert!(sources.iter().any(|source| source["agentName"] == "B"));
        assert!(sources.iter().any(|source| source["messageId"] == "m3"));
        let block = context_block(&state, "r1", "").unwrap();
        assert!(block.contains("배포는 Y 방식"), "{block}");
        assert!(block.contains("owner_confirmed"), "{block}");
        assert!(block.contains("B/"), "{block}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unsupported_quotes_and_single_agent_conclusions_are_rejected_or_demoted() {
        let (state, dir) = state();
        let owner = message("m1", "user", None, "질문");
        let claim = message("m2", "assistant", Some("a"), "내 주장은 X다");
        state.store.write_message(&owner);
        state.store.write_message(&claim);
        let extraction = json!({"knowledge":[
            {
                "type":"conclusion","topicKey":"x","text":"X",
                "sourceMessageIds":["m2"],"evidenceQuotes":["내 주장은 X다"],
                "verification":"debate"
            },
            {
                "type":"conclusion","topicKey":"bad","text":"없는 인용",
                "sourceMessageIds":["m2"],"evidenceQuotes":["이 문장은 원문에 없다"],
                "verification":"debate"
            }
        ]});
        let report = apply_extraction(&state, "r1", "room:r1", &[owner, claim], &extraction).unwrap();
        assert_eq!(report["rejected"], 1, "{report}");
        let items = list(&state, "r1");
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["type"], "claim", "{items:?}");
    }

    #[test]
    fn room_knowledge_is_shared_without_a_model_or_agent_filter() {
        let (state, dir) = state();
        state.store.put(
            "room_knowledge",
            &json!({
                "id":"k1","roomId":"r1","type":"conclusion","topicKey":"t",
                "text":"공유 결론","status":"active","verification":"debate",
                "confidence":0.6,"sources":[{"agentName":"A","model":"local-4b","messageId":"m1"}],
                "updatedAt":"2026-01-01T00:00:00Z"
            }),
        );
        let block = context_block(&state, "r1", "").unwrap();
        assert!(block.contains("공유 결론"), "{block}");
        assert!(block.contains("local-4b"), "{block}");
        assert!(context_block(&state, "r2", "").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
