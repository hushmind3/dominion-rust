use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet};
use serde_json::{json, Value};

/// Roundtable runner: each participant speaks for the configured number of
/// rounds, sees the shared goal and the previous speaker's output, and writes
/// into one shared session. Every turn is a real model call through the Rust
/// engine (tools included).
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let room_id = input
        .get("roomId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let goal = input
        .get("goal")
        .or_else(|| input.get("input"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if goal.is_empty() {
        return Err("회의 의제(goal)가 비어 있습니다.".into());
    }

    let state = &context.state;
    let room = state
        .store
        .get("room", room_id)
        .ok_or_else(|| "회의실을 찾을 수 없습니다.".to_string())?;
    let agent_ids = room
        .get("agentIds")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if agent_ids.is_empty() {
        return Err("회의실에 참여 에이전트가 없습니다.".into());
    }

    let rounds = room
        .get("rounds")
        .and_then(|value| value.as_i64())
        .unwrap_or(1)
        .clamp(1, 4) as usize;
    let max_turns = room
        .get("maxTurns")
        .and_then(|value| value.as_i64())
        .unwrap_or(12)
        .clamp(1, 60) as usize;

    let session_id = match room.get("sessionId").and_then(|value| value.as_str()) {
        Some(id) if state.store.get("session", id).is_some() => id.to_string(),
        _ => {
            let created = state
                .store
                .put(
                    "session",
                    &json!({
                        "title": room.get("name").cloned().unwrap_or(json!("회의실")),
                        "roomId": room_id,
                    }),
                )
                .ok_or("회의실 세션을 만들 수 없습니다.")?;
            let id = created
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let mut updated = room.clone();
            updated["sessionId"] = json!(id);
            state.store.put("room", &updated);
            id
        }
    };

    let total = (rounds * agent_ids.len()).min(max_turns);
    let mut previous = String::new();
    let mut turn = 0usize;

    'outer: for round in 0..rounds {
        for agent_id in &agent_ids {
            if turn >= max_turns {
                break 'outer;
            }
            let Some(agent) = state.store.get("agent", agent_id) else {
                context.emit(json!({ "type": "skipped", "agentId": agent_id }));
                continue;
            };
            let name = agent
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("참가자")
                .to_string();
            context.emit(json!({
                "type": "speaker",
                "agentId": agent_id,
                "name": name,
                "round": round + 1,
                "step": turn + 1,
                "stepTotal": total,
            }));

            let prompt = if turn == 0 {
                format!("회의 의제: {goal}")
            } else if previous.is_empty() {
                format!("회의 의제: {goal}\n지금까지의 논의를 바탕으로 독립적인 관점을 제시하세요.")
            } else {
                format!(
                    "회의 의제: {goal}\n\n직전 발언:\n{previous}\n\n직전 발언에 반응하면서 자신의 독립적인 관점을 제시하세요."
                )
            };

            let request = TurnRequest {
                session_id: Some(session_id.clone()),
                input: prompt,
                agent_id: Some(agent_id.clone()),
                provider_id: agent
                    .get("providerId")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                model: agent
                    .get("model")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                tools_enabled: true,
                thinking: None,
        auto_learning: false,
                reasoning_effort: None,
            reasoning_language: None,
                system_override: Some(format!(
                    "You are participant {name} in a roundtable. Address the shared goal, \
                     react to prior speakers, and contribute an independent perspective. \
                     Do not impersonate other agents."
                )),
            };

            let message = run_quiet(state, &request)
                .await?
                .ok_or_else(|| format!("{name} 의 발언이 비어 있습니다."))?;
            previous = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            turn += 1;
            context.patch(json!({ "stepIndex": turn, "stepTotal": total }));
        }
    }

    state.store.audit(
        "meeting.completed",
        json!({ "roomId": room_id, "sessionId": session_id, "turns": turn }),
    );

    Ok(json!({
        "roomId": room_id,
        "sessionId": session_id,
        "turns": turn,
    }))
}
