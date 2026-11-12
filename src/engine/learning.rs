use crate::engine::jobs::JobContext;
use crate::engine::memory::{self, MemoryScope};
use crate::engine::{jobs, llm};
use crate::state::AppState;
use serde_json::{json, Value};

const BATCH_MESSAGES: usize = 30;

fn effective_agent(state: &AppState, session_id: &str) -> Option<String> {
    state
        .store
        .messages(session_id)
        .iter()
        .rev()
        .find_map(|message| message["agent_id"].as_str().map(str::to_string))
        .or_else(|| {
            state
                .store
                .get("session", session_id)
                .and_then(|session| session["agentId"].as_str().map(str::to_string))
        })
}

/// Posts a visible self-improvement report into the conversation. This is a
/// durable transcript entry, not just a toast, so chat, rooms and bridged
/// channels (Telegram/Discord) all show the same event.
pub(crate) fn record_review(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    source: &str,
    memories: u64,
    skill: bool,
    knowledge: Option<(u64, u64, u64)>,
) -> Option<Value> {
    let content = if let Some((inserted, updated, promoted)) = knowledge {
        if inserted + updated + promoted == 0 {
            return None;
        }
        let mut text = format!("💾 방 공유 학습: 공유 지식 업데이트됨 (+{inserted}");
        if updated > 0 {
            text.push_str(&format!(" · 갱신 {updated}"));
        }
        if promoted > 0 {
            text.push_str(&format!(" · 경험 원칙 +{promoted}"));
        }
        text.push(')');
        text
    } else {
        if memories == 0 && !skill {
            return None;
        }
        let mut text = format!("💾 자기 개선 리뷰: 메모리 업데이트됨 (기억 +{memories}");
        if skill {
            text.push_str(" · 스킬 갱신");
        }
        text.push(')');
        text
    };
    // Debounce repeats for the same learner, without hiding another bot's
    // real updates behind the first participant's report.
    let state_id = format!(
        "learning-review-{}",
        memory::stable_id(&[session_id, source, agent_id.unwrap_or("room")])
    );
    let now = chrono::Utc::now();
    if let Some(previous) = state
        .store
        .get("learning_review_state", &state_id)
        .and_then(|value| {
            value["at"]
                .as_str()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        })
    {
        if (now - previous.with_timezone(&chrono::Utc)).num_seconds() < 45 {
            return None;
        }
    }
    state.store.put(
        "learning_review_state",
        &json!({
            "id":state_id,"sessionId":session_id,"source":source,
            "at":now.to_rfc3339(),
        }),
    );
    // Room-wide knowledge has no individual author. Never attribute the
    // team's update to whichever participant happens to be first in a roster.
    let author = agent_id.map(str::to_string);
    let saved = state.store.write_message(&json!({
        "session_id":session_id,
        "role":"assistant",
        "agent_id":author,
        "content":content,
        "metadata":{
            "authorKind":"system",
            "kind":"learning_report",
            "source":source,
            "learningScope":if agent_id.is_some(){"agent"}else if state.store.get("session",session_id)
                .is_some_and(|s|s["roomId"].is_string()){"room"}else{"session"},
            "memories":memories,
            "skill":skill,
            "knowledge":knowledge.map(|(inserted,updated,promoted)| json!({
                "inserted":inserted,"updated":updated,"promoted":promoted
            })),
        },
    }))?;
    state.bus.message(&saved);
    Some(saved)
}

/// Deterministic and semantic learning advance independently. Sharing one
/// checkpoint let automatic model-free learning suppress a later semantic
/// extraction for the same message.
fn checkpoint_id(scope: &MemoryScope, semantic: bool) -> String {
    memory::stable_id(&[
        &scope.session_id,
        &scope.partition_key,
        scope.agent_id.as_deref().unwrap_or("default"),
        if semantic { "semantic" } else { "deterministic" },
    ])
}

/// Snapshot the completed transcript and deduplicate identical queued work.
/// Distinct snapshots stay immutable even while a worker is claiming a job.
pub fn enqueue(
    state: &AppState,
    session_id: &str,
    provider_id: &str,
    model: &str,
) -> Option<Value> {
    let actor = effective_agent(state, session_id);
    enqueue_for_agent(state, session_id, actor.as_deref(), provider_id, model)
}

/// Explicit actor snapshots are needed for a room's manual learn-all action;
/// its last speaker's provider must not be reused for every other participant.
pub fn enqueue_for_agent(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    provider_id: &str,
    model: &str,
) -> Option<Value> {
    enqueue_for_agent_through(state, session_id, agent_id, provider_id, model, None)
}

/// Manual learning restores the Node behavior: an explicit user action may use
/// the model for semantic extraction. Automatic chat learning stays model-free.
pub fn enqueue_semantic_for_agent_through(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    provider_id: &str,
    model: &str,
    through_message_id: Option<&str>,
) -> Option<Value> {
    enqueue_for_agent_through_mode(
        state, session_id, agent_id, provider_id, model, through_message_id, true,
    )
}

pub fn enqueue_semantic(
    state: &AppState,
    session_id: &str,
    provider_id: &str,
    model: &str,
) -> Option<Value> {
    let actor = effective_agent(state, session_id);
    enqueue_for_agent_through_mode(
        state, session_id, actor.as_deref(), provider_id, model, None, true,
    )
}

/// Room-wide manual learning needs an explicit watermark per participant so
/// each agent learns its own turns up to the message it actually produced.
pub fn enqueue_for_agent_through(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    provider_id: &str,
    model: &str,
    through_message_id: Option<&str>,
) -> Option<Value> {
    enqueue_for_agent_through_mode(
        state, session_id, agent_id, provider_id, model, through_message_id, false,
    )
}

fn enqueue_for_agent_through_mode(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    provider_id: &str,
    model: &str,
    through_message_id: Option<&str>,
    semantic: bool,
) -> Option<Value> {
    let session = state.store.get("session", session_id)?;
    // A deleted or synthetic agent id may still own historical messages.
    // Learning must not fail just because the agent record is gone.
    let agent = agent_id.and_then(|id| state.store.get("agent", id));
    if let Some(room_id) = session["roomId"].as_str() {
        let room = state.store.get("room", room_id)?;
        if agent_id.is_none()
            || !room["agentIds"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id.as_str() == agent_id))
        {
            return None;
        }
    }
    if state
        .store
        .get("session", session_id)
        .is_some_and(|session| session["channelContext"]["authorIsBot"].as_bool() == Some(true))
    {
        return None;
    }
    let messages = state.store.messages(session_id);
    let through = through_message_id
        .map(str::to_string)
        .or_else(|| messages.last()?.get("id").and_then(Value::as_str).map(str::to_string))?;
    let scope = MemoryScope::for_session(&state.store, session_id, agent_id);
    if state
        .store
        .get("learning_checkpoint", &checkpoint_id(&scope, semantic))
        .is_some_and(|checkpoint| checkpoint["throughMessageId"].as_str() == Some(through.as_str()))
    {
        return None;
    }
    let resolved_provider = if !provider_id.is_empty() {
        Some(provider_id.to_string())
    } else {
        agent
            .as_ref()
            .and_then(|agent| agent["providerId"].as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .or_else(|| {
                if session["roomId"].is_null() {
                    session["providerId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .or_else(|| {
                state
                    .store
                    .all("provider")
                    .iter()
                    .find(|provider| provider["enabled"] != false)
                    .and_then(|provider| provider["id"].as_str().map(str::to_string))
            })
    }
    .unwrap_or_default();
    let resolved_model = if !model.is_empty() {
        Some(model.to_string())
    } else {
        agent
            .as_ref()
            .filter(|agent| {
                agent["providerId"]
                    .as_str()
                    .is_none_or(|id| id == resolved_provider)
            })
            .and_then(|agent| agent["model"].as_str())
            .filter(|model| !model.is_empty())
            .map(str::to_string)
            .or_else(|| {
                if session["roomId"].is_null()
                    && session["providerId"].as_str() == Some(resolved_provider.as_str())
                    && session["agentId"].as_str() == agent_id
                {
                    session["model"]
                        .as_str()
                        .filter(|model| !model.is_empty())
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .or_else(|| {
                state
                    .store
                    .get("provider", &resolved_provider)
                    .and_then(|provider| provider["model"].as_str().map(str::to_string))
            })
    }
    .unwrap_or_default();
    let input = json!({
        "sessionId":session_id,"agentId":agent_id,"throughMessageId":through.clone(),
        "providerId":resolved_provider,
        "model":resolved_model,
        "automatic":true,
        "semantic":semantic,
    });
    for job in state.store.all("job") {
        if job["kind"] == "learn"
            && matches!(job["status"].as_str(), Some("queued" | "running"))
            && job["input"]["sessionId"] == session_id
            && job["input"]["agentId"] == input["agentId"]
            && job["input"]["throughMessageId"] == input["throughMessageId"]
        {
            if semantic && job["input"]["semantic"].as_bool() != Some(true) {
                let mut upgraded = job.clone();
                upgraded["input"]["semantic"] = json!(true);
                upgraded["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
                return state.store.put("job", &upgraded);
            }
            return Some(job);
        }
    }
    let id = format!(
        "learn-{}",
        memory::stable_id(&[
            session_id,
            &scope.partition_key,
            agent_id.unwrap_or("default"),
            through.as_str(),
            if semantic { "semantic" } else { "deterministic" },
        ])
    );
    jobs::enqueue_once(state, &id, "learn", input)
}

/// Promote only quotes from unprocessed user messages or successful tools.
/// Assistant claims never establish a fact. Checkpoints advance after writes.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let state = &context.state;
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    if input["roomLearning"].as_bool() == Some(true) {
        return crate::engine::room_knowledge::run(context, job).await;
    }
    let session_id = input["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("학습할 세션이 없습니다.")?;
    if state
        .store
        .get("session", session_id)
        .is_some_and(|session| session["channelContext"]["authorIsBot"].as_bool() == Some(true))
    {
        return Ok(
            json!({"memories":0,"skill":false,"reason":"bot-authored source is not durable user evidence"}),
        );
    }
    let agent_id = if input.get("agentId").is_some() {
        input["agentId"].as_str().map(str::to_string)
    } else {
        effective_agent(state, session_id)
    };
    let scope = MemoryScope::for_session(&state.store, session_id, agent_id.as_deref());
    // Multiple durable workers may claim overlapping snapshots. Serialize
    // this actor's checkpoint independently of its foreground chat turn.
    let semantic = input["semantic"].as_bool() == Some(true);
    let lock_id = format!("learning:{}", checkpoint_id(&scope, semantic));
    if !state.begin_session(&lock_id) {
        return Err("deferred: learning checkpoint is already in use".into());
    }
    let _learning_guard = state.session_guard(&lock_id);
    let all = state.store.messages(session_id);
    if all.is_empty() {
        return Ok(json!({"memories":0,"skill":false,"reason":"no messages"}));
    }
    let snapshot_end = if let Some(through) = input["endMessageId"]
        .as_str()
        .or_else(|| input["throughMessageId"].as_str())
    {
        all.iter()
            .position(|message| message["id"].as_str() == Some(through))
            .ok_or("학습 스냅샷의 원문 메시지가 삭제되었습니다.")?
            + 1
    } else {
        all.len()
    };
    let snapshot_last_id = all[snapshot_end - 1]["id"]
        .as_str()
        .map(str::to_string)
        .ok_or("학습 스냅샷의 마지막 메시지 ID가 없습니다.")?;
    let checkpoint = state
        .store
        .get("learning_checkpoint", &checkpoint_id(&scope, semantic));
    let start = checkpoint
        .as_ref()
        .and_then(|checkpoint| checkpoint["throughMessageId"].as_str())
        .and_then(|through| {
            all.iter()
                .position(|message| message["id"].as_str() == Some(through))
        })
        .map(|index| index + 1)
        .unwrap_or(0);
    if start >= snapshot_end {
        return Ok(json!({"memories":0,"skill":false,"reason":"already learned"}));
    }
    let end = (start + BATCH_MESSAGES).min(snapshot_end);
    let batch = &all[start..end];
    let (transcript, selected_through) =
        select_page(batch, scope.agent_id.as_deref(), 1_000);
    // Advance only through the last message actually sent to the model. If
    // the budget cut the batch short, the remainder job starts there.
    let through = selected_through
        .or_else(|| all[end - 1]["id"].as_str().map(str::to_string))
        .ok_or("학습 메시지 ID가 없습니다.")?;
    let session = state
        .store
        .get("session", session_id)
        .unwrap_or(Value::Null);
    let provider_id = input["providerId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| session["providerId"].as_str())
        .unwrap_or("");
    let model = input["model"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| session["model"].as_str())
        .unwrap_or("");
    let mut existing_keys = state.store.all("memory").into_iter()
        .filter(|record|memory::visible(record,&scope) && record["key"].is_string())
        .take(2).map(|record|json!({"key":record["key"],"quote":record["text"].as_str().unwrap_or("").chars().take(60).collect::<String>()}))
        .collect::<Vec<_>>();
    existing_keys.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    let (preferred_provider, preferred_model) = llm::learning_route(state)
        .unwrap_or_else(|| (provider_id.to_string(), model.to_string()));
    let deterministic = crate::engine::distill::distill_incremental(
        state,
        session_id,
        scope.agent_id.as_deref(),
        false,
    );
    // Automatic learning is model-free by default. Deterministic distillation
    // already stores preferences, corrections and failed->alternative lessons;
    // calling an LLM here only burns local GPU/CPU and is never model-weight
    // training. Semantic extraction stays opt-in via input.semantic=true.
    if input["semantic"].as_bool() != Some(true) {
        let mut result = json!({
            "sessionId": session_id,
            "agentId": scope.agent_id,
            "memories": deterministic["memories"],
            "lessons": deterministic["lessons"],
            "skill": false,
            "rejected": 0,
            "items": [],
            "deterministicDistill": deterministic,
            "learningProvider": Value::Null,
            "learningModel": Value::Null,
            "modelCalls": 0,
            "reason": "deterministic learning; no model call",
        });
        let consolidation = crate::engine::experience::consolidate(state);
        result["experienceConsolidation"] = consolidation;
        state
            .store
            .put(
                "learning_checkpoint",
                &json!({
                    "id":checkpoint_id(&scope, semantic),"sessionId":session_id,"agentId":scope.agent_id,
                    "partitionKey":scope.partition_key,"throughMessageId":snapshot_last_id,
                    "updatedAt":chrono::Utc::now().to_rfc3339(),
                }),
            )
            .ok_or("학습 체크포인트 저장에 실패했습니다.")?;
        state.store.audit(
            "learning.completed",
            json!({
                "sessionId":session_id,"agentId":scope.agent_id,
                "throughMessageId":snapshot_last_id,"result":result,
                "deterministicOnly":true,
            }),
        );
        let feedback = json!({
            "id":format!("{}:{snapshot_last_id}",context.job_id),
            "type":"learning_feedback","source":"learn","sessionId":session_id,
            "agentId":scope.agent_id,"inserted":result["memories"],"rejected":0,
            "skill":false,"items":[],"watermark":snapshot_last_id,
            "jobId":context.job_id,"at":chrono::Utc::now().to_rfc3339(),
            "effect":"deterministic memory learning; no model weight update",
        });
        if let Some(saved) = state.store.put("learning_feedback", &feedback) {
            state.bus.emit(saved);
        }
        let _ = record_review(
            state,
            session_id,
            scope.agent_id.as_deref(),
            "memory",
            result["memories"].as_u64().unwrap_or(0),
            result["skill"].as_bool().unwrap_or(false),
            None,
        );
        if let Some(room_id) = state
            .store
            .get("session", session_id)
            .and_then(|session| session["roomId"].as_str().map(str::to_string))
        {
            state.bus.emit(json!({
                "type": "room_event", "roomId": room_id, "sessionId": session_id,
                "agentId": scope.agent_id, "turnId": "learning",
                "event": {"type": "learning", "memories": result["memories"], "skill": result["skill"]}
            }));
        }
        context.patch(json!({"result":result}));
        return Ok(result);
    }
    let completion = llm::complete_with_fallback_for_session(state,&preferred_provider,&preferred_model,
        "너는 증거 기반 학습기다. 아래 messages에서 사용자가 명시한 선호·사실·결정을 추출한다. assistant 발언은 근거로 쓰지 않는다. 각 기억은 반드시 해당 user 메시지의 id와, 그 메시지 내용에서 띄어쓰기까지 그대로 복사한 evidenceQuote를 포함한다. 인용이 정확하지 않으면 저장하지 않는다. JSON 하나만 출력한다. 예시 형식: {\"memories\":[{\"text\":\"사용자는 앞으로 답변을 한국어로만 받길 원한다\",\"type\":\"preference\",\"importance\":0.8,\"key\":\"answer-language\",\"sourceMessageIds\":[\"u1\"],\"evidenceQuote\":\"앞으로 답변은 항상 한국어로만 해\"}],\"skill\":null,\"strategies\":[],\"toolPolicies\":[],\"harnessProposal\":null}. 위 예시는 형식 설명이다. 실제 messages의 id와 원문을 사용하라. 추출할 것이 없으면 memories를 빈 배열로 둔다.",
        &json!({"messages":transcript,"existingFactKeys":existing_keys}).to_string(), session_id).await;
    let (raw, learning_provider, learning_model) = match completion {
        Ok(value) => value,
        Err(error) => {
            return Err(format!(
                "deferred: learning model unavailable after deterministic distillation ({error})"
            ));
        }
    };
    let parsed = parse_learning(&raw)?;
    let included_ids = transcript
        .iter()
        .filter_map(|message| message["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let included_batch = batch
        .iter()
        .filter(|message| {
            message["id"]
                .as_str()
                .is_some_and(|id| included_ids.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut result = apply_learning(state, &scope, &included_batch, &parsed)?;
    result["deterministicDistill"] = deterministic;
    result["learningProvider"] = json!(learning_provider);
    result["learningModel"] = json!(learning_model);
    let consolidation = crate::engine::experience::consolidate(state);
    result["experienceConsolidation"] = consolidation;
    state
        .store
        .put(
            "learning_checkpoint",
            &json!({
                "id":checkpoint_id(&scope, semantic),"sessionId":session_id,"agentId":scope.agent_id,
                "partitionKey":scope.partition_key,"throughMessageId":through,
                "updatedAt":chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("학습 체크포인트 저장에 실패했습니다.")?;
    state.store.audit("learning.completed",json!({"sessionId":session_id,"agentId":scope.agent_id,"throughMessageId":through,"result":result}));
    let feedback = json!({"id":format!("{}:{through}",context.job_id),"type":"learning_feedback","source":"learn","sessionId":session_id,"agentId":scope.agent_id,
        "inserted":result["memories"],"rejected":result["rejected"],"skill":result["skill"],"items":result["items"],"watermark":through,
        "jobId":context.job_id,"at":chrono::Utc::now().to_rfc3339(),"effect":"future scoped context and draft procedures; no model weight update"});
    if let Some(saved) = state.store.put("learning_feedback", &feedback) {
        state.bus.emit(saved);
    }
    let _ = record_review(
        state,
        session_id,
        scope.agent_id.as_deref(),
        "memory",
        result["memories"].as_u64().unwrap_or(0),
        result["skill"].as_bool().unwrap_or(false),
        None,
    );
    if let Some(room_id) = state
        .store
        .get("session", session_id)
        .and_then(|session| session["roomId"].as_str().map(str::to_string))
    {
        state.bus.emit(json!({
            "type": "room_event", "roomId": room_id, "sessionId": session_id,
            "agentId": scope.agent_id, "turnId": "learning",
            "event": {"type": "learning", "memories": result["memories"], "skill": result["skill"]}
        }));
    }
    context.patch(json!({"result":result}));
    let included_end = all
        .iter()
        .position(|message| message["id"].as_str() == Some(through.as_str()))
        .map(|index| index + 1)
        .unwrap_or(end);
    if included_end < snapshot_end {
        // Process the rest from the last message actually sent, never from the
        // end of the snapshot.
        let mut remainder = input.clone();
        remainder["throughMessageId"] = json!(through);
        remainder["endMessageId"] = json!(snapshot_last_id);
        remainder["agentId"] = json!(scope.agent_id);
        jobs::enqueue(state, "learn", remainder).ok_or("추가 학습 작업 저장에 실패했습니다.")?;
    }
    Ok(result)
}

fn select_page(
    batch: &[Value],
    agent_id: Option<&str>,
    budget: usize,
) -> (Vec<Value>, Option<String>) {
    let mut transcript = Vec::new();
    let mut used_chars = 0usize;
    let mut through = None;
    for message in batch {
        // Room sessions contain multiple agents; learn each agent's own turns.
        if message["agent_id"].as_str().is_some()
            && message["agent_id"].as_str() != agent_id
        {
            continue;
        }
        let content = message["content"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();
        let size = content.chars().count() + 96;
        if used_chars + size > budget && !transcript.is_empty() {
            break;
        }
        used_chars += size;
        transcript.push(json!({
            "id":message["id"],"role":message["role"],"toolCallId":message["tool_call_id"],"origin":message["metadata"]["origin"],
            "content":content,
        }));
        through = message["id"].as_str().map(str::to_string);
    }
    (transcript, through)
}

fn apply_learning(
    state: &AppState,
    scope: &MemoryScope,
    batch: &[Value],
    learning: &Learning,
) -> Result<Value, String> {
    let allowed_ids = batch
        .iter()
        .filter(|message| {
            message["agent_id"].as_str().is_none()
                || message["agent_id"].as_str() == scope.agent_id.as_deref()
        })
        .filter_map(|message| message["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut inserted = 0;
    let mut rejected = 0;
    let mut memory_items: Vec<String> = Vec::new();
    let mut skill_item: Option<String> = None;
    let mut strategy_items: Vec<String> = Vec::new();
    let mut policy_items: Vec<String> = Vec::new();
    let mut harness_item: Option<String> = None;
    for candidate in learning.memories.iter().take(8) {
        if !candidate
            .source_ids
            .iter()
            .all(|id| allowed_ids.contains(id.as_str()))
        {
            rejected += 1;
            continue;
        }
        let before = state.store.count("memory");
        match memory::save_grounded(
            &state.store,
            scope,
            &candidate.text,
            &candidate.kind,
            candidate.importance,
            candidate.key.as_deref(),
            "learn",
            &candidate.source_ids,
            &candidate.quote,
        ) {
            Ok(record) => {
                let added = state.store.count("memory") > before;
                inserted += usize::from(added);
                if added {
                    memory_items.push(candidate.text.clone());
                }
                state.bus.record("memory", &record);
            }
            Err(reason) => {
                if reason.ends_with("실패했습니다.") {
                    return Err(reason);
                }
                rejected += 1;
                state.store.audit(
                    "learning.rejected",
                    json!({"sessionId":scope.session_id,"reason":reason}),
                );
            }
        }
    }
    let mut skill_saved = false;
    if let Some(skill) = &learning.skill {
        let tool_evidence = skill
            .source_ids
            .iter()
            .filter_map(|id| {
                batch
                    .iter()
                    .find(|message| message["id"].as_str() == Some(id))
            })
            .filter(|message| {
                message["role"] == "tool"
                    && allowed_ids.contains(message["id"].as_str().unwrap_or(""))
            })
            .collect::<Vec<_>>();
        // Reusable drafts require successful tool evidence and remain disabled.
        let grounded = tool_evidence
            .iter()
            .any(|message| memory::tool_message_succeeded(&state.store, scope, message));
        if grounded && !skill.name.trim().is_empty() && !skill.instructions.trim().is_empty() {
            let id = format!(
                "learned-skill-{}",
                memory::stable_id(&[
                    &scope.partition_key,
                    scope.agent_id.as_deref().unwrap_or("default"),
                    &memory::normalize(&skill.name)
                ])
            );
            if state.store.get("skill", &id).is_none() {
                let saved=state.store.put("skill",&json!({
                    "id":id,"name":skill.name,"instructions":skill.instructions,
                    "description":skill.instructions.chars().take(160).collect::<String>(),
                    "enabled":false,"status":"draft","source":"learn",
                    "sessionId":scope.session_id,"agentId":scope.agent_id,"partitionKey":scope.partition_key,
                    "sourceMessageIds":skill.source_ids,"createdAt":chrono::Utc::now().to_rfc3339(),
                })).ok_or("스킬 초안 저장에 실패했습니다.")?;
                state.bus.record("skill", &saved);
                skill_saved = true;
                skill_item = Some(skill.name.clone());
            }
        }
    }
    let allowed = |ids: &[String]| -> Vec<String> {
        ids.iter()
            .filter(|id| allowed_ids.contains(id.as_str()))
            .cloned()
            .collect()
    };
    let mut improvements = 0usize;
    for strategy in &learning.strategies {
        let evidence = allowed(&strategy.source_ids);
        if evidence.is_empty() {
            continue;
        }
        if let Some(record) = crate::engine::improve::propose(
            state,
            scope,
            "strategy",
            &strategy.text,
            &strategy.rationale,
            &evidence,
            None,
            &strategy.scope,
        ) {
            if record["status"] == "adopted" {
                improvements += 1;
                strategy_items.push(strategy.text.clone());
            }
            let verification = crate::engine::procedure::normalize(&strategy.verification);
            if verification["type"].as_str() != Some("unverified") {
                crate::engine::improve::attach_verification(
                    state,
                    record["id"].as_str().unwrap_or(""),
                    &verification,
                );
            }
        }
    }
    for policy in &learning.tool_policies {
        let evidence = allowed(&policy.source_ids);
        if evidence.is_empty() {
            continue;
        }
        if let Some(record) = crate::engine::improve::propose(
            state,
            scope,
            "tool_policy",
            &policy.text,
            &policy.rationale,
            &evidence,
            Some(&policy.tool),
            &policy.scope,
        ) {
            if record["status"] == "adopted" {
                improvements += 1;
                policy_items.push(policy.text.clone());
            }
        }
    }
    let mut harness_proposed = false;
    if let Some(harness) = &learning.harness {
        let title = harness["title"].as_str().unwrap_or("하네스 개선");
        let problem = harness["problem"].as_str().unwrap_or("");
        let change = harness["change"].as_str().unwrap_or("");
        let test_plan = harness["testPlan"].as_str().unwrap_or("");
        let text = format!("{title}: {change}");
        let evidence = allowed(&[]);
        if let Some(record) = crate::engine::improve::propose(
            state,
            scope,
            "harness",
            &text,
            &format!("{problem}\n검증 계획: {test_plan}"),
            &evidence,
            None,
            "shared",
        ) {
            harness_proposed = record["status"] == "proposed";
            if harness_proposed {
                harness_item = Some(text.clone());
            }
        }
    }
    Ok(json!({"memories":inserted,"rejected":rejected,"skill":skill_saved,"improvements":improvements,"harnessProposal":harness_proposed,"items":{"memories":memory_items,"skill":skill_item,"strategies":strategy_items,"toolPolicies":policy_items,"harness":harness_item}}))
}

struct LearnedMemory {
    text: String,
    kind: String,
    importance: f64,
    key: Option<String>,
    source_ids: Vec<String>,
    quote: String,
}
struct LearnedSkill {
    name: String,
    instructions: String,
    source_ids: Vec<String>,
}
struct LearnedStrategy {
    text: String,
    rationale: String,
    scope: String,
    source_ids: Vec<String>,
    verification: Value,
}

struct LearnedToolPolicy {
    tool: String,
    text: String,
    rationale: String,
    scope: String,
    source_ids: Vec<String>,
}

struct Learning {
    memories: Vec<LearnedMemory>,
    skill: Option<LearnedSkill>,
    strategies: Vec<LearnedStrategy>,
    tool_policies: Vec<LearnedToolPolicy>,
    harness: Option<Value>,
}

fn source_ids(value: &Value) -> Vec<String> {
    value["sourceMessageIds"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|id| id.as_str().map(str::to_string))
                .take(8)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_learning(text: &str) -> Result<Learning, String> {
    let start = text
        .find('{')
        .ok_or("학습 모델이 JSON을 반환하지 않았습니다.")?;
    let end = text
        .rfind('}')
        .filter(|end| *end >= start)
        .ok_or("학습 JSON이 완전하지 않습니다.")?;
    let value: Value =
        serde_json::from_str(&text[start..=end]).map_err(|_| "학습 JSON을 해석할 수 없습니다.")?;
    let items = value["memories"]
        .as_array()
        .ok_or("학습 JSON에 memories 배열이 필요합니다.")?;
    let memories = items
        .iter()
        .filter_map(|item| {
            Some(LearnedMemory {
                text: item["text"].as_str()?.chars().take(1601).collect(),
                kind: item["type"].as_str().unwrap_or("fact").into(),
                importance: item["importance"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0),
                key: item["key"]
                    .as_str()
                    .map(|key| key.chars().take(120).collect()),
                source_ids: source_ids(item),
                quote: item["evidenceQuote"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(1600)
                    .collect(),
            })
        })
        .take(8)
        .collect();
    let skill = if value["skill"].is_object() {
        let item = &value["skill"];
        Some(LearnedSkill {
            name: item["name"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect(),
            instructions: item["instructions"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(12000)
                .collect(),
            source_ids: source_ids(item),
        })
    } else {
        None
    };
    let strategies = value["strategies"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LearnedStrategy {
                        text: item["text"].as_str()?.chars().take(2000).collect(),
                        rationale: item["rationale"].as_str().unwrap_or("").chars().take(1000).collect(),
                        scope: if item["scope"].as_str() == Some("agent") { "agent".into() } else { "shared".into() },
                        source_ids: source_ids(item),
                        verification: item["verification"].clone(),
                    })
                })
                .take(4)
                .collect()
        })
        .unwrap_or_default();
    let tool_policies = value["toolPolicies"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LearnedToolPolicy {
                        tool: item["tool"].as_str()?.chars().take(80).collect(),
                        text: item["text"].as_str()?.chars().take(2000).collect(),
                        rationale: item["rationale"].as_str().unwrap_or("").chars().take(1000).collect(),
                        scope: if item["scope"].as_str() == Some("agent") { "agent".into() } else { "shared".into() },
                        source_ids: source_ids(item),
                    })
                })
                .take(4)
                .collect()
        })
        .unwrap_or_default();
    let harness = value["harnessProposal"]
        .is_object()
        .then(|| value["harnessProposal"].clone());
    Ok(Learning { memories, skill, strategies, tool_policies, harness })
}

#[cfg(test)]
include!("../../tests/support/engine/learning.tests.rs");
