//! Room speaker and model routing: deterministic, user-controlled policies.
//! The job engine remains responsible for execution, checkpoints and budgets.
use super::*;

/// Choose initial responders. Discussion visits the roster, round robin picks
/// the next member, and mentioned waits for a real @name/@id. Explicit owner
/// selections override the initial routing, without changing room membership.
pub(super) fn is_deep_request(input: &str) -> bool {
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
pub(super) fn speaker_window(
    state: &AppState,
    room: &Value,
    input: &str,
    roster: &[String],
) -> Vec<String> {
    // Small rooms are cheap enough to keep everyone; trimming them loses more
    // perspective than it saves.
    let adaptive = room["economyMode"] == "adaptive";
    if roster.len() <= 3 && !adaptive {
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
    // Adaptive mode avoids paying for an entire panel to answer a greeting.
    // Explicit mentions, owner-picked speakers, and "everyone" above still win.
    let count = (if adaptive {
        if is_deep_request(input) { 3 } else if input.chars().count() <= 60 { 1 } else { 2 }
    } else if is_deep_request(input) {
        6
    } else if input.chars().count() <= 60 {
        2
    } else {
        3
    }).min(roster.len());
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
pub(super) fn pool_slot(roster: &[String], agent_id: &str, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    roster
        .iter()
        .position(|id| id == agent_id)
        .unwrap_or(0)
        % len
}

pub(super) fn auto_reasoning_effort(input: &str) -> String {
    if is_deep_request(input) {
        "medium".into()
    } else if input.chars().count() <= 60 {
        "none".into()
    } else {
        "low".into()
    }
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
pub(super) fn model_pool(state: &crate::state::AppState, room: &Value) -> Vec<(String, String, Option<String>)> {
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

/// Never guess model prices from names. The owner labels low-cost and frontier
/// routes; without labels we preserve the legacy stable rotation.
pub(super) fn tiered_model_pool(
    room: &Value, owner_text: &str, pool: Vec<(String, String, Option<String>)>,
) -> Vec<(String, String, Option<String>)> {
    if room["economyMode"] != "adaptive" || pool.is_empty() {
        return pool;
    }
    let desired = if is_deep_request(owner_text) { "frontier" } else { "economy" };
    let eligible = room["models"].as_array().into_iter().flatten()
        .filter(|entry| entry["tier"].as_str() == Some(desired))
        .filter_map(|entry| Some((entry["providerId"].as_str()?,entry["model"].as_str()?)))
        .collect::<HashSet<_>>();
    let matched = pool.iter()
        .filter(|(provider,model,_)|eligible.contains(&(provider.as_str(),model.as_str())))
        .cloned().collect::<Vec<_>>();
    if matched.is_empty() { pool } else { matched }
}

