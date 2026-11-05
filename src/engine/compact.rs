use crate::state::AppState;
use serde_json::{json, Value};

const KEEP_RECENT: usize = 8;

/// Summarizes older messages into a checkpoint. The original messages stay in
/// the database (the UI still lists them); the engine uses the summary plus the
/// recent window on later turns.
pub async fn run(
    state: &AppState,
    session_id: &str,
    provider_id: &str,
    model: &str,
) -> Result<Value, String> {
    if state.store.get("session", session_id).is_none() {
        return Err("세션을 찾을 수 없습니다.".into());
    }
    if provider_id.is_empty() || model.is_empty() {
        return Err("컨텍스트 압축에는 연결된 모델이 필요합니다.".into());
    }
    let provider = state
        .store
        .get("provider", provider_id)
        .ok_or("제공자를 찾을 수 없습니다.")?;
    let base_url = provider
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let api_key = state
        .read_secret(&format!("provider:{provider_id}"))
        .unwrap_or_default();

    let history = state.store.messages(session_id);
    let prior = state.store.get("session_compaction", session_id);
    let prior_id = prior
        .as_ref()
        .and_then(|value| value.get("throughMessageId"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let prior_index = prior_id
        .as_ref()
        .and_then(|id| {
            history
                .iter()
                .position(|row| row.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
        })
        .map(|index| index as isize)
        .unwrap_or(-1);

    let mut cut = history.len().saturating_sub(KEEP_RECENT).max(1);
    while cut > (prior_index + 1) as usize
        && history
            .get(cut.saturating_sub(1))
            .and_then(|row| row.get("role"))
            .and_then(|value| value.as_str())
            == Some("tool")
    {
        cut -= 1;
    }
    if cut <= (prior_index + 1) as usize {
        return Err("압축할 만큼 새 메시지가 충분하지 않습니다.".into());
    }

    let candidates = &history[(prior_index + 1) as usize..cut];
    let transcript = candidates
        .iter()
        .map(|row| {
            let role = row
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("?");
            let content = row
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .chars()
                .take(3200)
                .collect::<String>();
            format!("[{role}] {content}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let transcript: String = transcript
        .chars()
        .rev()
        .take(110_000)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    let previous_summary = prior
        .as_ref()
        .and_then(|value| value.get("summary"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let request = json!({
        "model": model,
        "stream": false,
        "messages": [
            {
                "role": "system",
                "content": "Summarize prior conversation for continuation. Preserve user intent, key decisions, files, constraints, open tasks, and important tool outcomes. Do not invent results. Be concise and write in the same language as the conversation."
            },
            {
                "role": "user",
                "content": format!(
                    "{}{}Conversation to compact:\n{transcript}",
                    if previous_summary.is_empty() { String::new() } else { format!("Previous summary:\n{previous_summary}\n\n") },
                    ""
                )
            }
        ]
    });

    let response = reqwest::Client::new()
        .post(format!("{base_url}/chat/completions"))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&request)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|error| format!("모델 연결 실패: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("모델 호출 실패: HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    let summary = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if summary.is_empty() {
        return Err("모델이 빈 요약을 반환했습니다.".into());
    }
    let summary: String = summary.chars().take(18_000).collect();

    let through = candidates
        .last()
        .and_then(|row| row.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    state.store.put(
        "session_compaction",
        &json!({
            "id": session_id,
            "throughMessageId": through,
            "summary": summary,
            "updatedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state.store.audit(
        "session.compacted",
        json!({ "sessionId": session_id, "summarized": candidates.len() }),
    );

    Ok(json!({
        "sessionId": session_id,
        "summarized": candidates.len(),
        "remaining": history.len() - cut,
        "summary": summary,
    }))
}
