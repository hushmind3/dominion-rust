use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

/// Non-streaming completion without tools. Used by the planner, reviewer and
/// the learning pipeline — all DOMINION-owned logic.
async fn try_complete(
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
    // One local model instance cannot serve four job workers at once. Queue
    // non-streaming local calls so learning/reflection do not time out.
    static LOCAL_LLM_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let local = base.contains("127.0.0.1")
        || base.contains("localhost")
        || provider["local"].as_bool() == Some(true);
    let mut body = json!({
        "model": model,
        "stream": false,
        "max_tokens": 4096,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt }
        ]
    });
    if local {
        // Qwen3 and similar local thinking models otherwise spend the whole
        // output budget on reasoning and return empty JSON.
        body["think"] = json!(false);
        body["chat_template_kwargs"] = json!({"enable_thinking": false});
    }
    let client = reqwest::Client::new();
    let url = format!("{base}/chat/completions");
    let _local_guard = if local {
        Some(LOCAL_LLM_LOCK.lock().await)
    } else {
        None
    };
    let mut last_error = String::new();
    let mut response = None;
    // Local runtimes occasionally drop an idle connection while loading a
    // model. One retry avoids losing the whole learning/reflection job.
    for attempt in 0..2 {
        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {key}"))
            .json(&body)
            .timeout(Duration::from_secs(240))
            .send()
            .await
        {
            Ok(value) => {
                response = Some(value);
                break;
            }
            Err(error) => {
                last_error = error.to_string();
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
            }
        }
    }
    let response = response.ok_or(last_error)?;
    if !response.status().is_success() {
        return Err(format!("모델 호출 실패: HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    let choice = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let content = choice["message"]["content"].as_str().unwrap_or("");
    if content.trim().is_empty() {
        let finish = choice["finish_reason"].as_str().unwrap_or("unknown");
        let reasoning = choice["message"]["reasoning_content"]
            .as_str()
            .or_else(|| choice["message"]["reasoning"].as_str())
            .unwrap_or("");
        return Err(format!(
            "모델이 빈 응답을 반환했습니다 (finish={finish}, reasoning={}자)",
            reasoning.chars().count()
        ));
    }
    Ok(content.to_string())
}

pub async fn complete(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    try_complete(state, provider_id, model, system, prompt).await
}

/// A provider explicitly marked for learning is used as the teacher model,
/// even when the conversation itself runs on a smaller local model.
pub fn learning_route(state: &AppState) -> Option<(String, String)> {
    state
        .store
        .all("provider")
        .into_iter()
        .find(|provider| {
            provider["learning"].as_bool() == Some(true)
                && provider["enabled"].as_bool() != Some(false)
        })
        .and_then(|provider| {
            Some((
                provider["id"].as_str()?.to_string(),
                provider["model"].as_str()?.to_string(),
            ))
        })
}

fn model_rank(model: &str) -> f64 {
    model
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .filter_map(|part| part.parse::<f64>().ok())
        .fold(0.0, f64::max)
}

/// Learning and reflection should not fail just because the conversation used
/// a very small thinking model. Try the same model first, then a larger local
/// model, then any configured provider.
pub async fn complete_with_fallback(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<(String, String, String), String> {
    let mut candidates = Vec::new();
    for provider in state.store.all("provider") {
        if provider["enabled"].as_bool() == Some(false) {
            continue;
        }
        let id = provider["id"].as_str().unwrap_or("").to_string();
        let candidate_model = provider["model"].as_str().unwrap_or("").to_string();
        if id.is_empty() || candidate_model.is_empty() || id == provider_id {
            continue;
        }
        let base = provider["baseUrl"].as_str().unwrap_or("");
        let local = provider["local"].as_bool() == Some(true)
            || base.contains("127.0.0.1")
            || base.contains("localhost");
        let learning = provider["learning"].as_bool() == Some(true);
        let rank = model_rank(&candidate_model);
        candidates.push((id, candidate_model, local, rank, learning));
    }
    candidates.sort_by(|a, b| {
        b.4.cmp(&a.4)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut last_error = String::new();
    for (candidate_id, candidate_model) in std::iter::once((provider_id.to_string(), model.to_string()))
        .chain(
            candidates
                .into_iter()
                .take(2)
                .map(|(id, model, _, _, _)| (id, model)),
        )
    {
        match try_complete(state, &candidate_id, &candidate_model, system, prompt).await {
            Ok(content) => return Ok((content, candidate_id, candidate_model)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}
