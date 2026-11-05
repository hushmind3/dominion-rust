//! Non-streaming model calls for planner, reviewer, reflection and learning.
//! Provider resolution, capability ordering and local admission live in
//! `engine::gateway`; this module only builds prompts and parses responses.
use crate::engine::gateway;
use crate::state::AppState;
use serde_json::{json, Value};
use std::time::Duration;

async fn try_complete(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let route = gateway::resolve(state, provider_id, Some(model))?;
    // Thinking models can consume the entire completion budget on reasoning
    // and return empty content. Start with a practical budget and retry once
    // with a larger one before reporting an empty response.
    let mut max_tokens = 8192u64;
    let client = reqwest::Client::new();
    let mut last = String::new();
    for attempt in 0..2 {
        let mut body = json!({
            "model": route.model,
            "stream": false,
            "max_tokens": max_tokens,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ]
        });
        if route.disable_thinking {
            body["think"] = json!(false);
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        let response = gateway::send(&route, &client, &body, Duration::from_secs(240)).await?;
        let response = response.response;
        if !response.status().is_success() {
            return Err(format!("모델 호출 실패: HTTP {}", response.status()));
        }
        let value: Value = response.json().await.map_err(|error| error.to_string())?;
        gateway::record_usage(state, &route, gateway::usage_from(&value), "helper", None);
        let choice = value
            .get("choices")
            .and_then(|choices| choices.get(0))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let content = choice["message"]["content"].as_str().unwrap_or("");
        if !content.trim().is_empty() {
            return Ok(content.to_string());
        }
        let finish = choice["finish_reason"].as_str().unwrap_or("unknown");
        let reasoning = choice["message"]["reasoning_content"]
            .as_str()
            .or_else(|| choice["message"]["reasoning"].as_str())
            .unwrap_or("");
        last = format!(
            "모델이 빈 응답을 반환했습니다 (finish={finish}, reasoning={}자, attempt={})",
            reasoning.chars().count(),
            attempt + 1
        );
        if finish != "length" {
            break;
        }
        max_tokens = (max_tokens * 2).min(16384);
    }
    Err(last)
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

/// Learning and reflection should not fail just because the conversation used
/// a very small thinking model. Try the same model first, then up to two
/// capability-ordered alternatives.
pub async fn complete_with_fallback(
    state: &AppState,
    provider_id: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> Result<(String, String, String), String> {
    let alternatives = gateway::ordered_alternatives(state, provider_id);
    let mut last_error = String::new();
    for route in std::iter::once(None)
        .chain(alternatives.into_iter().take(2).map(Some))
    {
        let (candidate_id, candidate_model) = match &route {
            Some(route) => (route.id.clone(), route.model.clone()),
            None => (provider_id.to_string(), model.to_string()),
        };
        match try_complete(
            state,
            &candidate_id,
            &candidate_model,
            system,
            prompt,
        )
        .await
        {
            Ok(content) => return Ok((content, candidate_id, candidate_model)),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-llm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn learning_route_prefers_the_declared_teacher() {
        let state = state();
        state.store.put(
            "provider",
            &json!({"id":"local","name":"local","baseUrl":"http://127.0.0.1/v1","model":"small"}),
        );
        state.store.put(
            "provider",
            &json!({"id":"teacher","name":"teacher","baseUrl":"https://api/v1","model":"large","learning":true}),
        );
        assert_eq!(
            learning_route(&state),
            Some(("teacher".into(), "large".into()))
        );
    }

    #[test]
    fn an_explicit_text_disabled_provider_is_not_a_fallback() {
        let state = state();
        state.store.put(
            "provider",
            &json!({"id":"bad","name":"bad","baseUrl":"http://bad/v1","model":"m","capabilities":{"text":false}}),
        );
        assert!(gateway::ordered_alternatives(&state, "primary").is_empty());
    }
}
