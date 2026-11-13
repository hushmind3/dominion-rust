use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_room_quiet};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};

pub const MAX_PARTICIPANTS: usize = 32;
/// Safety ceiling only. The room's own rounds/maxTurns decide the budget;
/// operators can raise this without a code change.
fn max_turns_ceiling() -> usize {
    std::env::var("DOMINION_MAX_ROOM_TURNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(100_000)
}

/// One durable transcript belongs to each room. It is shared by the owner and
/// every participant; changing the selected speaker never changes this binding.
pub fn ensure_room_session(state: &AppState, room_id: &str) -> Result<Value, String> {
    let room = state
        .store
        .get("room", room_id)
        .ok_or("대화방을 찾을 수 없습니다.")?;
    let session_id = room
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("room:{room_id}"));
    let session = match state.store.get("session", &session_id) {
        Some(mut session) => {
            if session
                .get("roomId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != room_id)
                || session
                    .get("channelContext")
                    .is_some_and(|value| !value.is_null())
            {
                return Err("다른 대화방 또는 외부 채널의 세션을 연결할 수 없습니다.".into());
            }
            if session.get("roomId").is_none_or(Value::is_null) {
                // A pre-group implementation could save an untyped session.
                // Do not adopt an ordinary private chat merely because its id
                // was entered into a room record.
                let expected = format!("room:{room_id}");
                if session_id != expected {
                    return Err("대화방 소유권이 없는 기존 세션은 연결할 수 없습니다.".into());
                }
                session["roomId"] = json!(room_id);
                state
                    .store
                    .put("session", &session)
                    .ok_or("대화방 세션을 저장할 수 없습니다.")?
            } else {
                session
            }
        }
        None => {
            let record = json!({"id":session_id,"title":room.get("name").cloned().unwrap_or(json!("대화방")),"roomId":room_id,"kind":"room"});
            match state.store.put_if_absent("session", &record) {
                Some(session) => session,
                None => state
                    .store
                    .get("session", &session_id)
                    .filter(|session| session["roomId"].as_str() == Some(room_id))
                    .ok_or("대화방 세션을 만들 수 없습니다.")?,
            }
        }
    };
    if room["sessionId"].as_str() != Some(session_id.as_str()) {
        // Stable GETs must remain read-only: emitting a room record here makes
        // event subscribers refetch /state, which otherwise emits it again.
        // Compare the binding itself so concurrent first reads also avoid a
        // redundant write/event once another reader has established it.
        match state.store.patch_if(
            "room",
            room_id,
            "sessionId",
            room.get("sessionId").unwrap_or(&Value::Null),
            &json!({"sessionId":session_id}),
        ) {
            Some(saved) => state.bus.record("room", &saved),
            None if state
                .store
                .get("room", room_id)
                .is_some_and(|room| room["sessionId"].as_str() == Some(session_id.as_str())) => {}
            None => return Err("대화방 세션 연결을 저장할 수 없습니다.".into()),
        }
    }
    Ok(session)
}

/// Choose initial responders. Discussion visits the roster, round robin picks
/// the next member, and mentioned waits for a real @name/@id. Explicit owner
/// selections override the initial routing, without changing room membership.
fn is_deep_request(input: &str) -> bool {
    let lower = input.to_lowercase();
    input.chars().count() > 240
        || [
            "분석", "설계", "구현", "검토", "조사", "테스트", "버그", "최적화", "리팩터",
            "원인", "해결", "비교", "계획", "문서", "아키텍처", "리뷰", "debug", "refactor",
            "design", "analyze", "investigate",
        ]
        .iter()
        .any(|term| lower.contains(term))
}

/// Chooses the smallest useful cohort for a discussion turn. The owner can
/// still force everyone with "다들/모두/전원"; mentions override this entirely.
fn speaker_window(
    state: &AppState,
    room: &Value,
    input: &str,
    roster: &[String],
) -> Vec<String> {
    // Small rooms are cheap enough to keep everyone; trimming them loses more
    // perspective than it saves.
    if roster.len() <= 3 {
        return roster.to_vec();
    }
    // An explicit total cap of at least one turn per participant is the
    // owner's instruction that everyone should speak. Do not silently trim
    // that room down to two casual responders.
    if room["maxTurns"]
        .as_u64()
        .is_some_and(|turns| turns as usize >= roster.len())
    {
        return roster.to_vec();
    }
    let lower = input.to_lowercase();
    if [
        "다들", "다 같이", "다같이", "모두", "모든", "전원", "전부", "각자",
        "나머지", "나머지도", "아홉", "9명", "인사", "everyone", "all agents",
    ]
        .iter()
        .any(|term| lower.contains(term))
    {
        return roster.to_vec();
    }
    let count = if is_deep_request(input) {
        6.min(roster.len())
    } else if input.chars().count() <= 60 {
        2.min(roster.len())
    } else {
        3.min(roster.len())
    };
    if count >= roster.len() {
        return roster.to_vec();
    }
    let mut terms: Vec<String> = input
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .map(|word| word.to_lowercase())
        .collect();
    if let Some(topic) = room["topic"].as_str() {
        terms.extend(
            topic
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|word| word.chars().count() >= 2)
                .map(|word| word.to_lowercase()),
        );
    }
    terms.sort();
    terms.dedup();
    let mut scored: Vec<(usize, usize, String)> = roster
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let agent = state.store.get("agent", id).unwrap_or(Value::Null);
            let mut text = String::new();
            for key in ["name", "description", "role", "systemPrompt"] {
                if let Some(value) = agent[key].as_str() {
                    text.push_str(value);
                    text.push(' ');
                }
            }
            for alias in agent["aliases"].as_array().into_iter().flatten() {
                if let Some(alias) = alias.as_str() {
                    text.push_str(alias);
                    text.push(' ');
                }
            }
            let text = text.to_lowercase();
            let score = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            (score + usize::from(index == 0), index, id.clone())
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut selected: Vec<usize> = scored.into_iter().take(count).map(|row| row.1).collect();
    if !selected.contains(&0) {
        if let Some(last) = selected.last_mut() {
            *last = 0;
        }
    }
    selected.sort_unstable();
    selected.into_iter().map(|index| roster[index].clone()).collect()
}

/// Stable model assignment by participant order. Different participants get
/// different pool entries whenever the pool is large enough, and the same
/// participant keeps its entry for every turn of the room.
fn pool_slot(roster: &[String], agent_id: &str, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    roster
        .iter()
        .position(|id| id == agent_id)
        .unwrap_or(0)
        % len
}

fn auto_reasoning_effort(input: &str) -> String {
    if is_deep_request(input) {
        "medium".into()
    } else if input.chars().count() <= 60 {
        "none".into()
    } else {
        "low".into()
    }
}

/// No LLM is used to choose a spend budget. Preserve the owner's explicit
/// maxTurns override and the configured model-to-participant binding.
fn effective_rounds(room: &Value, input: &str, requested: usize) -> usize {
    if room["maxTurns"].as_i64().is_some_and(|turns| turns > 0) {
        return requested;
    }
    let deep = is_deep_request(input);
    let cap = match room["costMode"].as_str().unwrap_or("quality") {
        "economy" => if deep { 2 } else { 1 },
        "balanced" => if deep { requested } else if input.chars().count() <= 60 { 1 } else { 2 },
        _ => requested,
    };
    requested.min(cap).max(1)
}

pub fn plan_speakers(
    state: &AppState,
    room: &Value,
    input: &str,
    explicit: Option<&Value>,
) -> Result<Vec<String>, String> {
    let roster = participants(room)?;
    for id in &roster {
        let agent = state
            .store
            .get("agent", id)
            .ok_or_else(|| format!("참가 에이전트를 찾을 수 없습니다: {id}"))?;
        if agent["enabled"].as_bool() == Some(false) {
            return Err(format!(
                "비활성 에이전트가 참가자로 지정되어 있습니다: {id}"
            ));
        }
    }
    let mode = routing_mode(state, room)?;
    if let Some(value) = explicit {
        let selected = id_list(value)?;
        if selected.iter().any(|id| !roster.contains(id)) {
            return Err("응답자는 대화방 참가자 중에서 선택하세요.".into());
        }
        return Ok(selected);
    }
    let mentioned = mentioned_agents(state, &roster, input)?;
    match mode.as_str() {
        "discussion" => {
            if !mentioned.is_empty() {
                return Ok(mentioned);
            }
            Ok(speaker_window(state, room, input, &roster))
        }
        "round_robin" => {
            if !mentioned.is_empty() {
                return Ok(mentioned);
            }
            let next = room["nextSpeakerIndex"].as_u64().unwrap_or(0) as usize % roster.len();
            Ok(vec![roster[next].clone()])
        }
        "mentioned" => Ok(mentioned),
        _ => unreachable!(),
    }
}

/// Each job responds to one actual owner message in the shared transcript.
/// Model instructions never create extra user messages or turn peer claims
/// into owner facts. Autonomous exchanges end at a finite turn budget.
/// Distinct provider/model pairs from the room's participants. This is
/// generic: any room size reuses the same pool and rotates per turn instead
/// of hardcoding named agents.
fn model_pool(state: &crate::state::AppState, room: &Value) -> Vec<(String, String, Option<String>)> {
    let mut pool = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // An explicit owner selection wins over agent defaults. This is how the
    // owner says "use these Fast FreeLLMAPI models, never anything else".
    for entry in room["models"].as_array().into_iter().flatten() {
        let provider = entry["providerId"].as_str().unwrap_or("");
        let model = entry["model"].as_str().unwrap_or("");
        let effort = entry["reasoningEffort"].as_str().filter(|value| !value.is_empty()).map(str::to_string);
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let key = format!("{provider}|{model}");
        if seen.insert(key) {
            pool.push((provider.to_string(), model.to_string(), effort));
        }
    }
    if !pool.is_empty() {
        return pool;
    }
    for id in room["agentIds"].as_array().into_iter().flatten() {
        let Some(agent) = id.as_str().and_then(|id| state.store.get("agent", id)) else {
            continue;
        };
        let provider = agent["providerId"].as_str().unwrap_or("");
        let model = agent["model"].as_str().unwrap_or("");
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        let key = format!("{provider}|{model}");
        if seen.insert(key) {
            pool.push((provider.to_string(), model.to_string(), None));
        }
    }
    pool
}

pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let room_id = input["roomId"].as_str().ok_or("대화방 ID가 필요합니다.")?;
    let lock_id = format!("room-run:{room_id}");
    if !context.state.begin_session(&lock_id) {
        return Err("deferred: room conversation is already running".into());
    }
    let _guard = context.state.session_guard(&lock_id);
    let result = run_locked(context, job, &input, room_id).await;
    if let Err(error) = &result {
        if !error.starts_with("deferred:") {
            let current = context
                .state
                .store
                .get("room", room_id)
                .unwrap_or(Value::Null);
            if current["lastJobId"].as_str() == Some(context.job_id.as_str()) {
                let cancelled = job_cancelled(context);
                patch_room(
                    &context.state,
                    room_id,
                    json!({"status":if cancelled {"paused"} else {"needs_review"},"activeAgentId":null,"pauseReason":if cancelled {"stopped"} else {"execution_failed"},"lastError":error}),
                );
            }
        }
    }
    result
}

async fn run_locked(
    context: &JobContext,
    job: &Value,
    input: &Value,
    room_id: &str,
) -> Result<Value, String> {
    let state = &context.state;
    let current_room = state
        .store
        .get("room", room_id)
        .ok_or("대화방을 찾을 수 없습니다.")?;
    // Permissions and membership are the owner's configuration when this
    // message was submitted, never a later model-supplied policy.
    let room = input
        .get("roomSnapshot")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| current_room.clone());
    if room
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| id != room_id)
    {
        return Err("대화방 설정 스냅샷이 다른 방을 가리킵니다.".into());
    }
    let existing = job.get("roomState").cloned().unwrap_or_else(|| json!({}));
    if existing["inFlight"].as_bool() == Some(true) {
        patch_room(
            state,
            room_id,
            json!({"status":"needs_review","activeAgentId":null,"pauseReason":"uncertain_turn"}),
        );
        return Ok(
            json!({"roomId":room_id,"sessionId":existing.get("sessionId"),"status":"needs_review","reason":"중단된 발언 또는 도구 작업은 확인 없이 반복하지 않습니다."}),
        );
    }
    let session = ensure_room_session(state, room_id)?;
    let session_id = session["id"].as_str().ok_or("대화방 세션 ID가 없습니다.")?;
    if input
        .get("sessionId")
        .and_then(Value::as_str)
        .is_some_and(|id| id != session_id)
    {
        return Err("작업이 다른 대화방 세션을 가리킵니다.".into());
    }
    let generation = input
        .get("generation")
        .or_else(|| existing.get("generation"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if job_cancelled(context) || !state.resume_session_if_generation(session_id, generation) {
        return Ok(finish(
            context,
            room_id,
            session_id,
            "cancelled",
            &[],
            "stopped",
            None,
        ));
    }

    // Internal automations and compatibility callers receive the same frozen
    // policy as the owner-message API, including on a later safe restart.
    let mut bound_input = input.clone();
    bound_input["roomSnapshot"] = room.clone();
    bound_input["sessionId"] = json!(session_id);
    bound_input["generation"] = json!(generation);
    context.checkpoint(json!({"input":bound_input}))?;

    let source = owner_message(context, input, session_id, room_id)?;
    let source_id = source["id"].as_str().ok_or("주인 메시지 ID가 없습니다.")?;
    let owner_text = source["content"].as_str().unwrap_or("");
    // One root task per owner utterance. Every speaker below is an attempt on
    // this root, and peers' replies are observations, not new owner requests.
    let task_id = format!("room-task-{source_id}");
    let root = crate::engine::episode::start_root(
        state,
        crate::engine::episode::RootSpec {
            task_id: &task_id,
            intent: owner_text,
            family: "room",
            source: "owner",
            source_message_id: Some(source_id),
            scope: json!({"partition": "local", "sessionId": session_id, "roomId": room_id}),
            mandatory: source["metadata"]["acceptance"]["checks"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        },
    );
    let task_revision = root
        .as_ref()
        .and_then(crate::engine::episode::contract_revision)
        .map(str::to_string);
    bound_input["taskId"] = json!(task_id);
    bound_input["taskRevision"] = json!(task_revision);
    context.checkpoint(json!({"input":bound_input}))?;
    let roster = if let Some(value) = existing.get("roster") {
        id_list(value)?
    } else {
        participants(&room)?
    };
    if roster.is_empty() {
        return Err("대화방 참가자가 없습니다.".into());
    }
    let mode = existing["routingMode"]
        .as_str()
        .or_else(|| input["routingMode"].as_str())
        .map(str::to_string)
        .unwrap_or(routing_mode(state, &room)?);
    if !matches!(mode.as_str(), "discussion" | "round_robin" | "mentioned") {
        return Err("지원하지 않는 대화방 응답 방식입니다.".into());
    }
    let initial = if let Some(value) = existing.get("initialSpeakers") {
        id_list(value)?
    } else {
        let explicit = input["explicitAgentIds"]
            .as_bool()
            .unwrap_or(input.get("agentIds").is_some());
        let mut routing = room.clone();
        if mode == "round_robin" {
            routing["nextSpeakerIndex"] = current_room["nextSpeakerIndex"].clone();
        }
        let selection = if mode == "round_robin" && !explicit {
            None
        } else {
            input.get("agentIds")
        };
        plan_speakers(state, &routing, owner_text, selection)?
    };
    if initial.iter().any(|id| !roster.contains(id)) {
        return Err("실행 체크포인트의 응답자가 대화방 참가자가 아닙니다.".into());
    }
    let configured_rounds = room["rounds"].as_i64().unwrap_or(3).clamp(1, max_turns_ceiling() as i64) as usize;
    let rounds = effective_rounds(&room, owner_text, configured_rounds);
    // rounds is the primary control: one round means every participant
    // speaks once. maxTurns > 0 is an explicit total cap; 0 means automatic.
    let budget = existing["turnBudget"]
        .as_u64()
        .map(|value| value as usize)
        .unwrap_or_else(|| {
            room["maxTurns"]
                .as_i64()
                .filter(|value| *value > 0)
                .map(|value| value as usize)
                .unwrap_or(roster.len() * rounds)
        })
        .clamp(1, max_turns_ceiling());
    let token_budget = room["tokenBudget"].as_u64().unwrap_or(0);
    let usage_baseline = state.store.session_usage(session_id)["totalTokens"]
        .as_u64()
        .unwrap_or(0);
    let conflict_strategy =
        crate::engine::algorithms::variant_for_agent(state, "multiagent.conflict", None);
    let mut conflict_resolution = existing
        .get("conflictResolution")
        .cloned()
        .unwrap_or(Value::Null);
    let mut outputs = existing["outputs"].as_array().cloned().unwrap_or_default();
    if outputs.len() > budget {
        return Err("대화방 발언 체크포인트가 허용 횟수를 초과했습니다.".into());
    }
    let mut round_slots = 0usize;
    let mut cohort: Vec<String> = Vec::new();
    let mut pending: VecDeque<String> = if let Some(value) = existing.get("pendingSpeakers") {
        id_list_with_repeats(value)?.into()
    } else {
        let count = if mode == "discussion" { rounds } else { 1 };
        (0..count)
            .flat_map(|_| initial.iter().cloned())
            .skip(outputs.len())
            .take(budget.saturating_sub(outputs.len()))
            .collect()
    };
    if pending.iter().any(|id| !roster.contains(id)) {
        return Err("대화방 발언 순서에 없는 참가자가 포함되어 있습니다.".into());
    }
    patch_room(
        state,
        room_id,
        json!({"status":"running","activeAgentId":null,"lastJobId":context.job_id,"lastSourceMessageId":source_id,"pauseReason":null,"lastError":null}),
    );
    checkpoint(
        context, session_id, generation, source_id, &mode, &roster, &initial, &pending, &outputs,
        budget, false, None,
    )?;

    while outputs.len() < budget {
        if mode == "discussion" && (cohort.is_empty() || round_slots >= roster.len()) {
            round_slots = 0;
            // Mentions since the owner's message define who may speak this
            // round. Recomputed each round so a member pulled in later joins.
            let mut text = owner_text.to_string();
            for output in &outputs {
                if let Some(content) = output["content"].as_str() {
                    text.push('\n');
                    text.push_str(content);
                }
            }
            let mentioned = mentioned_agents(state, &roster, &text).unwrap_or_default();
            cohort = if mentioned.is_empty() { roster.clone() } else { mentioned };
        }
        if stopped(context, session_id, generation) {
            return Ok(finish(
                context,
                room_id,
                session_id,
                "cancelled",
                &outputs,
                "stopped",
                None,
            ));
        }
        if let Some(newer) = newer_owner_message(state, session_id, source_id) {
            return Ok(finish(
                context,
                room_id,
                session_id,
                "completed",
                &outputs,
                "new_owner_message",
                Some(&newer),
            ));
        }
        let Some(agent_id) = pending.pop_front() else {
            break;
        };
        round_slots += 1;
        if mode == "discussion" && !cohort.contains(&agent_id) {
            context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"not_mentioned"}));
            continue;
        }
        let agent = state
            .store
            .get("agent", &agent_id)
            .ok_or_else(|| format!("참가 에이전트가 삭제되었습니다: {agent_id}"))?;
        if agent["enabled"].as_bool() == Some(false) {
            return Err(format!("참가 에이전트가 비활성 상태입니다: {agent_id}"));
        }
        let name = agent["name"].as_str().unwrap_or(&agent_id);
        context.emit(json!({"type":"speaker","roomId":room_id,"agentId":agent_id,"name":name,"step":outputs.len()+1,"stepTotal":budget}));
        patch_room(
            state,
            room_id,
            json!({"status":"running","activeAgentId":agent_id}),
        );
        // Include the selected speaker in the saved pending queue until the
        // result is durable; any interrupted inFlight turn requires review.
        let mut saved_pending = pending.clone();
        saved_pending.push_front(agent_id.clone());
        checkpoint(
            context,
            session_id,
            generation,
            source_id,
            &mode,
            &roster,
            &initial,
            &saved_pending,
            &outputs,
            budget,
            true,
            Some(&agent_id),
        )?;
        // An explicit owner selection is itself a rotation list. Choosing
        // models in the room UI is the owner saying "use exactly these".
        let explicit_pool = room["models"]
            .as_array()
            .is_some_and(|models| !models.is_empty());
        let configured_pool = if explicit_pool || room["autoModels"].as_bool().unwrap_or(true) {
            model_pool(state, &room)
        } else {
            Vec::new()
        };
        // Explicit per-model fast/deep tags are an opt-in routing contract.
        // Keep the selected tier stable throughout this owner's room job.
        let pool = model_tier::select(&room, configured_pool, owner_text);
        // Keep one model per agent for the whole room. Stable routes preserve
        // the prompt prefix (provider caching) and make costs predictable;
        // rate limits are still spread because different agents hash to
        // different pool entries.
        let rotated = if pool.is_empty() {
            None
        } else {
            Some(pool[pool_slot(&roster, &agent_id, pool.len())].clone())
        };
        let resolved_provider=optional(input,"providerId").or_else(||rotated.as_ref().map(|(provider,_,_)| provider.clone())).or_else(||optional(&agent,"providerId"));
        let resolved_model=optional(input,"model").or_else(||rotated.as_ref().map(|(_,model,_)| model.clone())).or_else(||optional(&agent,"model"));
        let rotated_effort=rotated.as_ref().and_then(|(_,_,effort)| effort.clone());
        // The owner's room model selection is a routing decision, not a hint.
        // Local runtimes are eligible exactly like cloud providers; the gateway
        // still applies local concurrency admission and normal failover.
        let (provider_id, model) = (resolved_provider, resolved_model);
        // Per-model effort wins, then the message, the room and the agent.
        // "none" turns reasoning off; an explicit level turns it on.
        let codex_route = provider_id.as_deref().and_then(|id|state.store.get("provider",id))
            .is_some_and(|provider|provider["kind"] == "codex");
        let effort_choice=rotated_effort
            .or_else(||optional(input,"reasoningEffort"))
            .or_else(||optional(&room,"reasoningEffort"))
            .or_else(||optional(&agent,"reasoningEffort"))
            .or_else(||provider_id.as_deref().and_then(|id|state.store.get("provider",id)).and_then(|provider|optional(&provider,"reasoningEffort")))
            // Codex's default is model-specific. Do not convert a greeting's
            // heuristic `none` into an unsupported effort on reasoning models.
            .or_else(||(!codex_route).then(||auto_reasoning_effort(owner_text)));
        let reasoning_effort=effort_choice.clone().filter(|effort|codex_route || effort != "none");
        let thinking=input["thinking"].as_bool()
            .or_else(||room["thinking"].as_bool())
            .or_else(||agent["thinking"].as_bool())
            .or_else(||effort_choice.as_deref().map(|effort| effort != "none"))
            .or_else(||(!codex_route).then_some(false));
        let mut request=TurnRequest {
            session_id:Some(session_id.to_string()),input:owner_text.to_string(),agent_id:Some(agent_id.clone()),provider_id,model,attachment_ids: Vec::new(),tools_enabled:room["toolsEnabled"].as_bool().unwrap_or(false),tools_execution_enabled:true,thinking,auto_learning:room["autoLearning"].as_bool().unwrap_or(true),reasoning_effort,reasoning_language: (input["reasoningLanguage"].as_str().unwrap_or("ko") == "ko").then(|| "ko".into()),
            task_id:Some(task_id.clone()),parent_task_id:None,task_revision:task_revision.clone(),verification_owner:Some("self".into()),
            // The invariant room identity, participants, topic, and authority
            // are supplied by room_context::core. The current owner message,
            // source ID, and external speaker attribution stay in history.
            system_override:None,
        };
        if let Some(note) = crate::engine::debate::guidance(&conflict_resolution) {
            request.system_override = Some(note.to_string());
        }
        let message = match run_room_quiet(state, &request, source_id).await {
            Ok(Some(message)) => message,
            Ok(None) => return Err(format!("{name}의 응답이 비어 있습니다.")),
            Err(_) if stopped(context, session_id, generation) => {
                return Ok(finish(
                    context,
                    room_id,
                    session_id,
                    "cancelled",
                    &outputs,
                    "stopped",
                    None,
                ))
            }
            Err(error) if error.contains("빈 응답") => {
                context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"empty"}));
                checkpoint(
                    context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
                    &outputs, budget, false, None,
                )?;
                continue;
            }
            Err(error) => return Err(error),
        };
        let content = message["content"].as_str().unwrap_or("");
        if is_silence(content) {
            context.emit(json!({"type":"silence","roomId":room_id,"agentId":agent_id,"reason":"pass"}));
            checkpoint(
                context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
                &outputs, budget, false, None,
            )?;
            continue;
        }
        let evidence_count = message_evidence_count(state, &message);
        outputs.push(json!({
            "agentId":agent_id,
            "name":name,
            "content":content,
            "messageId":message["id"],
            "evidenceCount":evidence_count,
            "toolCalls":message["tool_calls"].clone(),
        }));
        let resolution =
            crate::engine::debate::resolve(owner_text, &outputs, &conflict_strategy);
        if resolution["conflict"].as_bool() == Some(true)
            && resolution["revision"].as_str() != conflict_resolution["revision"].as_str()
        {
            if let Some(supported) =
                crate::engine::debate::supported_outcome(&resolution)
            {
                crate::engine::algorithms::record_domain_outcome(
                    state,
                    None,
                    "multiagent.conflict",
                    &conflict_strategy,
                    supported,
                );
            }
            state.store.audit(
                "room.conflict_resolved",
                json!({
                    "roomId": room_id,
                    "turns": outputs.len(),
                    "strategy": resolution["strategy"],
                    "outcome": resolution["outcome"],
                    "winnerAgentId": resolution["winner"]["agentId"],
                }),
            );
            conflict_resolution = resolution.clone();
            patch_room(
                state,
                room_id,
                json!({"conflictResolution": conflict_resolution.clone()}),
            );
        }
        if let Some(index) = roster.iter().position(|id| id == &agent_id) {
            patch_room(
                state,
                room_id,
                json!({"nextSpeakerIndex":(index+1)%roster.len()}),
            );
        }
        let mentions = mentioned_agents(state, &roster, content).unwrap_or_default();
        let spoken = outputs
            .iter()
            .filter_map(|output| output["agentId"].as_str())
            .map(str::to_string)
            .collect::<HashSet<_>>();
        route_peers(
            &mut pending,
            &mentions,
            &agent_id,
            outputs.len(),
            budget,
            if mode == "discussion" {
                Some(&spoken)
            } else {
                None
            },
        );
        if token_budget > 0 {
            let used = state.store.session_usage(session_id)["totalTokens"]
                .as_u64()
                .unwrap_or(0)
                .saturating_sub(usage_baseline);
            if used >= token_budget {
                checkpoint(
                    context, session_id, generation, source_id, &mode, &roster, &initial,
                    &pending, &outputs, budget, false, None,
                )?;
                return Ok(finish(
                    context,
                    room_id,
                    session_id,
                    "completed",
                    &outputs,
                    "token_budget",
                    None,
                ));
            }
        }
        checkpoint(
            context, session_id, generation, source_id, &mode, &roster, &initial, &pending,
            &outputs, budget, false, None,
        )?;
    }
    if stopped(context, session_id, generation) {
        return Ok(finish(
            context,
            room_id,
            session_id,
            "cancelled",
            &outputs,
            "stopped",
            None,
        ));
    }
    let reason = if outputs.len() >= budget {
        "turn_limit"
    } else if outputs.is_empty() {
        "waiting_for_mention"
    } else {
        "waiting_for_owner"
    };
    Ok(finish(
        context,
        room_id,
        session_id,
        "completed",
        &outputs,
        reason,
        None,
    ))
}

fn owner_message(
    context: &JobContext,
    input: &Value,
    session_id: &str,
    room_id: &str,
) -> Result<Value, String> {
    let messages = context.state.store.messages(session_id);
    if let Some(id) = input
        .get("messageId")
        .or_else(|| input.get("sourceMessageId"))
        .and_then(Value::as_str)
    {
        return messages
            .into_iter()
            .find(|message| message["id"].as_str() == Some(id) && is_owner(message))
            .ok_or_else(|| "실제 주인 메시지가 대화방 기록에 없습니다.".into());
    }
    // /run compatibility is still one owner utterance, never one per agent.
    let text = input
        .get("goal")
        .or_else(|| input.get("input"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if text.is_empty() {
        return Err("대화 메시지가 비어 있습니다.".into());
    }
    let id = format!("room-owner:{}", context.job_id);
    if let Some(message) = messages
        .into_iter()
        .find(|message| message["id"].as_str() == Some(id.as_str()))
    {
        return Ok(message);
    }
    let saved=context.state.store.write_message(&json!({"id":id,"session_id":session_id,"role":"user","content":text,"agent_id":null,"metadata":{"authorKind":"owner","roomId":room_id}})).ok_or("주인 메시지를 저장할 수 없습니다.")?;
    context.state.bus.message(&saved);
    Ok(saved)
}
fn is_owner(message: &Value) -> bool {
    message["role"] == "user"
        && message["agent_id"].is_null()
        && message
            .pointer("/metadata/authorKind")
            .and_then(Value::as_str)
            == Some("owner")
}
/// Evidence is not only the acceptance-check list. A tool operation that
/// actually completed is evidence too; the previous count ignored it, so a
/// successful tool-using turn could display `evidenceCount: 0`.
fn message_evidence_count(state: &AppState, message: &Value) -> usize {
    let recorded = message["metadata"]["evidence"]
        .as_array()
        .map(|items| items.len())
        .unwrap_or(0);
    let operations = message["metadata"]["executionId"]
        .as_str()
        .and_then(|run_id| state.store.execution_operations(run_id).ok())
        .map(|items| {
            items
                .iter()
                .filter(|operation| operation["status"].as_str() == Some("completed"))
                .count()
        })
        .unwrap_or(0);
    recorded + operations
}

fn newer_owner_message(state: &AppState, session_id: &str, source_id: &str) -> Option<String> {
    let messages = state.store.messages(session_id);
    let index = messages
        .iter()
        .position(|message| message["id"].as_str() == Some(source_id))?;
    messages[index + 1..]
        .iter()
        .rev()
        .find(|message| is_owner(message))
        .and_then(|message| message["id"].as_str())
        .map(str::to_string)
}
fn job_cancelled(context: &JobContext) -> bool {
    context
        .state
        .store
        .get("job", &context.job_id)
        .is_some_and(|job| {
            job["cancelRequested"].as_bool() == Some(true) || job["status"] == "cancelled"
        })
}
fn stopped(context: &JobContext, session_id: &str, generation: u64) -> bool {
    job_cancelled(context)
        || context.state.cancellation_generation(session_id) != generation
        || context.state.is_cancelled(session_id)
}
fn patch_room(state: &AppState, room_id: &str, patch: Value) {
    if let Some(saved) = state
        .store
        .patch_if("room", room_id, "id", &json!(room_id), &patch)
    {
        state.bus.record("room", &saved);
    }
}
#[allow(clippy::too_many_arguments)]
fn checkpoint(
    context: &JobContext,
    session_id: &str,
    generation: u64,
    source_id: &str,
    mode: &str,
    roster: &[String],
    initial: &[String],
    pending: &VecDeque<String>,
    outputs: &[Value],
    budget: usize,
    in_flight: bool,
    active: Option<&str>,
) -> Result<(), String> {
    context.checkpoint(json!({"sessionId":session_id,"stepIndex":outputs.len(),"stepTotal":budget,"roomState":{"sessionId":session_id,"generation":generation,"sourceMessageId":source_id,"routingMode":mode,"roster":roster,"initialSpeakers":initial,"pendingSpeakers":pending,"turnBudget":budget,"outputs":outputs,"inFlight":in_flight,"activeAgentId":active}}))
}
fn finish(
    context: &JobContext,
    room_id: &str,
    session_id: &str,
    status: &str,
    outputs: &[Value],
    reason: &str,
    yielded: Option<&str>,
) -> Value {
    patch_room(
        &context.state,
        room_id,
        json!({"status":if status=="cancelled" {"paused"} else {"idle"},"activeAgentId":null,"pauseReason":reason,"yieldedToMessageId":yielded}),
    );
    context.state.store.audit("room.exchange_finished",json!({"roomId":room_id,"sessionId":session_id,"turns":outputs.len(),"status":status,"reason":reason}));
    context.emit(json!({"type":"room_finished","roomId":room_id,"sessionId":session_id,"status":status,"reason":reason}));
    // One completed exchange is one learning unit. The job is content-
    // addressed by its last message, so retries and duplicate completion do
    // not create duplicate knowledge.
    if status == "completed" && !outputs.is_empty() {
        let learning_enabled = context
            .state
            .store
            .get("room", room_id)
            .is_some_and(|room| room["autoLearning"].as_bool() != Some(false));
        if learning_enabled {
            // Snapshot the exchange this job actually completed. Re-reading the
            // session tail could pick up a newer owner message that arrived
            // while the final turn was finishing.
            if let Some(through) = outputs
                .iter()
                .rev()
                .find_map(|output| output["messageId"].as_str())
            {
                crate::engine::room_knowledge::enqueue_automatic(
                    &context.state,
                    room_id,
                    session_id,
                    through,
                );
            }
        }
    }
    json!({"roomId":room_id,"sessionId":session_id,"turns":outputs.len(),"status":status,"outputs":outputs,"pauseReason":reason,"yieldedToMessageId":yielded})
}
fn route_peers(
    pending: &mut VecDeque<String>,
    mentions: &[String],
    speaker: &str,
    used: usize,
    budget: usize,
    discussion_spoken: Option<&HashSet<String>>,
) {
    for id in mentions.iter().rev().filter(|id| id.as_str() != speaker) {
        // Discussion gives the selected cohort a chance to speak before an
        // already-spoken pair can consume all remaining turns mentioning one
        // another. Mentioned/round-robin conversations remain freely directed.
        if discussion_spoken.is_some_and(|spoken| {
            spoken.contains(id) && pending.iter().any(|next| !spoken.contains(next))
        }) {
            continue;
        }
        if let Some(index) = pending.iter().position(|next| next == id) {
            pending.remove(index);
            pending.push_front(id.clone());
        } else if used + pending.len() < budget {
            pending.push_front(id.clone());
        }
    }
}
fn routing_mode(state: &AppState, room: &Value) -> Result<String, String> {
    if let Some(mode) = room["routingMode"].as_str() {
        if matches!(mode, "discussion" | "round_robin" | "mentioned") {
            return Ok(mode.to_string());
        }
        return Err("응답 방식은 discussion, round_robin, mentioned 중 하나여야 합니다.".into());
    }
    let kernel = crate::engine::algorithms::choose_for_agent(
        state, "multiagent.roles", None, None,
    )
    .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
    .unwrap_or_else(|| "discussion".into());
    Ok(match kernel.as_str() {
        "round-robin" => "round_robin".into(),
        "mentioned" => "mentioned".into(),
        _ => "discussion".into(),
    })
}
fn id_list(value: &Value) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let ids = id_list_with_repeats(value)?
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect::<Vec<_>>();
    if ids.len() > MAX_PARTICIPANTS {
        return Err("대화방 참가자는 최대 32명입니다.".into());
    }
    Ok(ids)
}
fn id_list_with_repeats(value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or("참가 에이전트 ID 배열이 필요합니다.")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|id| !id.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| "유효하지 않은 에이전트 ID입니다.".into())
        })
        .collect()
}
fn participants(room: &Value) -> Result<Vec<String>, String> {
    let ids = id_list(&room["agentIds"])?;
    if ids.is_empty() {
        return Err("대화방 참가자가 없습니다.".into());
    }
    Ok(ids)
}
fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Mentions inside examples/code/quoted blocks are data, not routing commands.
fn mention_text(input: &str) -> String {
    let mut fenced = false;
    let mut inline = false;
    let mut out = String::new();
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            out.push('\n');
            continue;
        }
        if fenced || trimmed.starts_with('>') {
            out.push('\n');
            continue;
        }
        for ch in line.chars() {
            if ch == '`' {
                inline = !inline;
                out.push(' ');
            } else if inline {
                out.push(' ');
            } else {
                out.push(ch);
            }
        }
        inline = false;
        out.push('\n');
    }
    out
}
fn mention_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !ch.is_alphanumeric() && !matches!(ch, '_' | '-' | '.'))
}
fn bare_mention_follows(after: &str) -> bool {
    if mention_boundary(after.chars().next()) {
        return true;
    }
    const PARTICLES: &[&str] = &[
        "야", "아", "님", "씨", "에게", "한테", "께", "가", "이", "는", "은", "도", "랑",
        "와", "과", "의",
    ];
    PARTICLES.iter().any(|particle| after.starts_with(particle))
}
/// Optional silence: a member may decline its turn instead of speaking.
fn is_silence(content: &str) -> bool {
    let text = content.trim();
    text.is_empty()
        || text.eq_ignore_ascii_case("(pass)")
        || text.eq_ignore_ascii_case("(pass.)")
        || text.eq_ignore_ascii_case("pass")
        || text.eq_ignore_ascii_case("pass.")
}

fn mentioned_agents(
    state: &AppState,
    roster: &[String],
    input: &str,
) -> Result<Vec<String>, String> {
    let text = mention_text(input).to_lowercase();
    let mut matches = Vec::new();
    for (order, id) in roster.iter().enumerate() {
        let agent = state
            .store
            .get("agent", id)
            .ok_or_else(|| format!("참가 에이전트를 찾을 수 없습니다: {id}"))?;
        let mut aliases = vec![id.to_lowercase()];
        if let Some(name) = agent["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
        {
            aliases.push(name.to_lowercase());
        }
        aliases.sort();
        aliases.dedup();
        for alias in aliases {
            let marker = format!("@{alias}");
            for (position, _) in text.match_indices(&marker) {
                if mention_boundary(text[..position].chars().next_back())
                    && mention_boundary(text[position + marker.len()..].chars().next())
                {
                    matches.push((position, marker.len(), order, id.clone(), alias.clone()));
                }
            }
            // Owners often address a participant naturally ("샤르티아야",
            // "샤르티아에게") instead of typing an @mention. Treat a bare name
            // followed by a vocative/particle or a boundary as a direct target.
            if alias.chars().count() < 2 {
                continue;
            }
            for (position, _) in text.match_indices(&alias) {
                if text[..position].ends_with('@') {
                    continue;
                }
                if !mention_boundary(text[..position].chars().next_back()) {
                    continue;
                }
                let after = &text[position + alias.len()..];
                if bare_mention_follows(after) {
                    matches.push((position, alias.len(), order, id.clone(), alias.clone()));
                }
            }
        }
    }
    matches.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    for pair in matches.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].4 == pair[1].4 && pair[0].3 != pair[1].3 {
            return Err("같은 이름의 참가자가 있습니다. @에이전트ID로 지정하세요.".into());
        }
    }
    let mut seen = HashSet::new();
    Ok(matches
        .into_iter()
        .filter_map(|(_, _, _, id, _)| seen.insert(id.clone()).then_some(id))
        .collect())
}

#[path = "rooms/model_tier.rs"]
mod model_tier;

#[cfg(test)]
#[path = "rooms/pool_tests.rs"]
mod pool_tests;
#[cfg(test)]
#[path = "rooms/tests.rs"]
mod tests;
