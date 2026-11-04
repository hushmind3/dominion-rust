use crate::engine::jobs::JobContext;
use crate::engine::{jobs, llm};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::HashSet;

/// Queues the post-turn learning pass (memories + reusable skill drafts).
pub fn enqueue(state: &AppState, session_id: &str, provider_id: &str, model: &str) -> Option<Value> {
    jobs::enqueue(
        state,
        "learn",
        json!({
            "sessionId": session_id,
            "providerId": provider_id,
            "model": model,
            "automatic": true,
        }),
    )
}

/// Extracts durable memories and a skill draft from a finished conversation.
/// This is the Rust replacement for Node's learnFromSession pipeline.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let state = &context.state;
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let session_id = input
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return Err("학습할 세션이 없습니다.".into());
    }
    let provider_id = input
        .get("providerId")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let model = input
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let messages = state.store.messages(&session_id);
    if messages.len() < 2 {
        return Ok(json!({ "memories": 0, "skill": false, "reason": "not enough messages" }));
    }
    let start = messages.len().saturating_sub(30);
    let transcript = messages[start..]
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?;
            let content = message
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(1200)
                .collect::<String>();
            if content.trim().is_empty() {
                return None;
            }
            Some(format!("[{role}] {content}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        return Ok(json!({ "memories": 0, "skill": false }));
    }

    let raw = llm::complete(
        state,
        &provider_id,
        &model,
        "너는 DOMINION의 학습기다. 대화와 실제 도구 실행 결과에서 오래 쓸 지식만 뽑아라. \
         JSON 하나만 출력: {\"memories\":[{\"text\":\"...\",\"type\":\"fact|preference|decision|procedure|lesson\",\"importance\":0.0,\"scope\":\"shared\"}],\"skill\":{\"name\":\"...\",\"instructions\":\"...\"}|null}. \
         text는 반드시 한국어. 확실한 사용자 선호, 검증된 결정, 실제 결과에서 얻은 교훈만 저장한다. \
         시도만 한 도구 호출이나 검증 안 된 주장은 저장하지 않는다. 비밀·임시정보·추측은 저장하지 않는다. \
         저장할 게 없으면 memories는 빈 배열, skill은 null.",
        &format!("대화:\n{transcript}"),
    )
    .await?;

    let parsed = parse_learning(&raw);
    let mut inserted = 0usize;
    let existing = state
        .store
        .all("memory")
        .into_iter()
        .filter_map(|memory| memory.get("text").and_then(|value| value.as_str()).map(normalize))
        .collect::<HashSet<_>>();
    let mut seen = existing;

    for memory in parsed.memories.iter().take(8) {
        if memory.text.trim().is_empty() || !seen.insert(normalize(&memory.text)) {
            continue;
        }
        state.store.put(
            "memory",
            &json!({
                "text": memory.text,
                "type": memory.kind,
                "importance": memory.importance,
                "scope": memory.scope,
                "source": "learn",
                "sessionId": session_id,
                "createdAt": chrono::Utc::now().to_rfc3339(),
            }),
        );
        inserted += 1;
    }

    let mut skill_saved = false;
    if let Some(skill) = &parsed.skill {
        if !skill.name.trim().is_empty() && !skill.instructions.trim().is_empty() {
            let exists = state.store.all("skill").iter().any(|record| {
                record
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(|name| name.eq_ignore_ascii_case(&skill.name))
                    .unwrap_or(false)
            });
            if !exists {
                state.store.put(
                    "skill",
                    &json!({
                        "name": skill.name,
                        "instructions": skill.instructions,
                        "description": skill.instructions.chars().take(160).collect::<String>(),
                        "enabled": false,
                        "status": "draft",
                        "source": "learn",
                        "sessionId": session_id,
                        "createdAt": chrono::Utc::now().to_rfc3339(),
                    }),
                );
                skill_saved = true;
            }
        }
    }

    state
        .store
        .audit("learning.completed", json!({
            "sessionId": session_id,
            "memories": inserted,
            "skill": skill_saved,
        }));
    context.patch(json!({ "result": { "memories": inserted, "skill": skill_saved } }));
    Ok(json!({ "memories": inserted, "skill": skill_saved }))
}

struct LearnedMemory {
    text: String,
    kind: String,
    importance: f64,
    scope: String,
}

struct LearnedSkill {
    name: String,
    instructions: String,
}

#[derive(Default)]
struct Learning {
    memories: Vec<LearnedMemory>,
    skill: Option<LearnedSkill>,
}

fn parse_learning(text: &str) -> Learning {
    let Some(start) = text.find('{') else {
        return Learning::default();
    };
    let Some(end) = text.rfind('}') else {
        return Learning::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text[start..=end]) else {
        return Learning::default();
    };
    let memories = value
        .get("memories")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(LearnedMemory {
                        text: item.get("text")?.as_str()?.to_string(),
                        kind: item
                            .get("type")
                            .and_then(|value| value.as_str())
                            .unwrap_or("fact")
                            .to_string(),
                        importance: item
                            .get("importance")
                            .and_then(|value| value.as_f64())
                            .unwrap_or(0.5)
                            .clamp(0.0, 1.0),
                        scope: item
                            .get("scope")
                            .and_then(|value| value.as_str())
                            .unwrap_or("shared")
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let skill = value.get("skill").and_then(|item| {
        if item.is_null() {
            return None;
        }
        Some(LearnedSkill {
            name: item.get("name")?.as_str()?.to_string(),
            instructions: item.get("instructions")?.as_str()?.to_string(),
        })
    });
    Learning { memories, skill }
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memories_and_skill() {
        let learning = parse_learning(
            "```json\n{\"memories\":[{\"text\":\"사용자는 답변을 짧게 원한다\",\"type\":\"preference\",\"importance\":0.9,\"scope\":\"shared\"}],\"skill\":{\"name\":\"Short answers\",\"instructions\":\"Always answer in 3 lines.\"}}\n```",
        );
        assert_eq!(learning.memories.len(), 1);
        assert_eq!(learning.memories[0].kind, "preference");
        assert_eq!(learning.memories[0].importance, 0.9);
        assert!(learning.skill.is_some());
    }

    #[test]
    fn ignores_invalid_payloads() {
        assert!(parse_learning("not json").memories.is_empty());
        assert!(parse_learning("{\"memories\":[],\"skill\":null}").skill.is_none());
    }

    #[test]
    fn normalization_dedupes_spacing_and_case() {
        assert_eq!(normalize("Hello World!"), normalize("hello   world"));
    }
}
