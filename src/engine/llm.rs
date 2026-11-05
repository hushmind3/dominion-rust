use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

/// Non-streaming completion without tools. Used by the planner, reviewer and
/// the learning pipeline — all DOMINION-owned logic.
pub async fn complete(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let provider = if provider_id.is_empty() {
        state
            .store
            .all("provider")
            .into_iter()
            .find(|p| p.get("enabled").and_then(|v| v.as_bool()) != Some(false))
    } else {
        state.store.get("provider", provider_id)
    }
    .ok_or("제공자를 찾을 수 없습니다.")?;
    let id = provider.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let base = provider
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_end_matches('/');
    let key = state
        .read_secret(&format!("provider:{id}"))
        .unwrap_or_default();
    let model = if model.is_empty() {
        provider
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("gpt-4o-mini")
    } else {
        model
    };
    let response = reqwest::Client::new()
        .post(format!("{base}/chat/completions"))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {key}"))
        .json(&json!({
            "model": model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ]
        }))
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("모델 호출 실패: HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    Ok(value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .unwrap_or("")
        .to_string())
}
