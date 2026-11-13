//! Deterministic request routing.
//!
//! No extra model call is made: the classifier uses the request shape, length,
//! attachments and transcript depth. Auto-selected chats use the cheapest
//! capable route for light questions and a credentialed/high-capability route
//! for deep work; explicit provider selections are never overridden.

use crate::engine::gateway;
use crate::state::AppState;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Light,
    Standard,
    Deep,
}

const DEEP_MARKERS: &[&str] = &[
    "분석", "설계", "아키텍처", "조사", "비교", "증명", "디버그", "리팩터",
    "계획", "배포", "보안", "최적화", "구현", "마이그레이션", "벤치마크",
    "research", "analyze", "design", "debug", "refactor", "deploy",
    "security", "compare", "plan", "implement", "migrate", "benchmark",
];

/// Classify without calling a model. Long prompts, attachments, deep-work
/// vocabulary and long transcripts are deep; short everyday turns are light.
pub fn classify(input: &str, attachments: usize, messages: usize) -> Complexity {
    let text = input.trim().to_lowercase();
    let chars = text.chars().count();
    let marker = DEEP_MARKERS.iter().any(|marker| text.contains(marker));
    if attachments > 0 || chars > 500 || messages > 20 || marker {
        return Complexity::Deep;
    }
    if chars <= 80 && !marker && !text.contains("```") && !text.contains('\n') {
        return Complexity::Light;
    }
    Complexity::Standard
}

/// Explicit owner-entered KRW per million tokens. Unknown prices are not
/// silently converted into free, and comparisons are limited to known pairs.
fn estimated_krw_per_million(provider: &Value) -> Option<f64> {
    let input=provider["costPerMillionInputKrw"].as_f64()?;
    let output=provider["costPerMillionOutputKrw"].as_f64()?;
    if !input.is_finite() || !output.is_finite()
        || input<0.0 || output<0.0 || input>100_000_000.0 || output>100_000_000.0 {
        return None;
    }
    // Conservative conversational shape (four input tokens / one output).
    Some(input * 0.8 + output * 0.2)
}

fn compare_known_prices(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (estimated_krw_per_million(a), estimated_krw_per_million(b)) {
        (Some(a),Some(b))=>a.total_cmp(&b),
        _=>std::cmp::Ordering::Equal,
    }
}

fn has_key(state: &AppState, provider: &Value) -> bool {
    provider["id"]
        .as_str()
        .is_some_and(|id| state.store.has_secret(&format!("provider:{id}")))
}

/// Pick a provider for an auto route. Explicit user/agent selections bypass
/// this function entirely.
pub fn select_provider(state: &AppState, complexity: Complexity) -> Option<Value> {
    let mut candidates = state
        .store
        .all("provider")
        .into_iter()
        .filter(gateway::provider_eligible)
        .filter(|provider| provider["enabled"] != false)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let score = |provider: &Value| gateway::capability_score(provider);
    match complexity {
        Complexity::Light => {
            // Free/keyless first, then the smallest capable route.
            candidates.sort_by(|a, b| {
                has_key(state, a)
                    .cmp(&has_key(state, b))
                    .then_with(|| compare_known_prices(a,b))
                    .then_with(|| {
                        score(a)
                            .partial_cmp(&score(b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| {
                        let local = |provider: &Value| provider["local"] == true;
                        local(b).cmp(&local(a))
                    })
            });
        }
        Complexity::Deep => {
            // A rate-limited anonymous free route is a poor fit for long work.
            candidates.sort_by(|a, b| {
                has_key(state, b)
                    .cmp(&has_key(state, a))
                    .then_with(|| {
                        score(b)
                            .partial_cmp(&score(a))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| compare_known_prices(a,b))
                    .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
            });
        }
        Complexity::Standard => {
            candidates.sort_by(|a, b| {
                has_key(state, a)
                    .cmp(&has_key(state, b))
                    .then_with(|| {
                        // For near-equal capability, pay the lower declared
                        // token price. A large quality gap is not traded away.
                        ((score(a)-score(b)).abs() >= 0.1)
                            .then(||score(b).total_cmp(&score(a)))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| compare_known_prices(a,b))
                    .then_with(|| score(b).total_cmp(&score(a)))
                    .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
            });
        }
    }
    candidates.into_iter().next()
}

/// Persist a successful fallback route onto the agent, so the next turn does
/// not retry a rate-limited/dead model and the group rotates naturally.
pub fn rotate_agent_route(
    state: &AppState,
    agent_id: &str,
    provider_id: &str,
    model: &str,
) -> Option<Value> {
    let mut agent = state.store.get("agent", agent_id)?;
    if agent["providerId"].as_str() == Some(provider_id)
        && agent["model"].as_str() == Some(model)
    {
        return None;
    }
    agent["providerId"] = json!(provider_id);
    agent["model"] = json!(model);
    agent["routeRotatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("agent", &agent)?;
    state.bus.record("agent", &saved);
    state.store.audit(
        "agent.route_rotated",
        json!({"agentId": agent_id, "providerId": provider_id, "model": model}),
    );
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;
    use serde_json::json;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-router-{}", uuid::Uuid::new_v4()));
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

    #[test]
    fn short_small_talk_is_light_and_deep_work_is_deep() {
        assert_eq!(classify("안녕", 0, 1), Complexity::Light);
        assert_eq!(classify("지금 뭐 해", 0, 3), Complexity::Light);
        assert_eq!(
            classify("이 아키텍처를 분석하고 마이그레이션 계획을 설계해줘", 0, 2),
            Complexity::Deep
        );
        assert_eq!(classify("첨부한 로그를 봐줘", 1, 2), Complexity::Deep);
        assert_eq!(classify("오늘 있었던 일을 조금 길게 정리해 주세요. 어떤 상황이었는지, 누가 있었는지, 다음에는 어떻게 했으면 하는지까지 순서대로 적어 주시면 좋겠습니다. 너무 딱딱하지 않게 편한 말투로 부탁드립니다.", 0, 2), Complexity::Standard);
    }

    #[test]
    fn successful_fallback_rotates_the_agent_route_once() {
        let (state, dir) = state();
        state.store.put(
            "agent",
            &json!({"id":"a","name":"A","providerId":"old","model":"old-model"}),
        );
        let saved = rotate_agent_route(&state, "a", "new", "new-model").unwrap();
        assert_eq!(saved["providerId"], "new");
        assert_eq!(saved["model"], "new-model");
        assert!(rotate_agent_route(&state, "a", "new", "new-model").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn light_auto_route_uses_declared_price_only_when_both_are_known() {
        let (state, dir) = state();
        state.store.put("provider",&json!({"id":"expensive","baseUrl":"https://paid.example/v1",
            "model":"high","enabled":true,"costPerMillionInputKrw":10000,
            "costPerMillionOutputKrw":10000}));
        state.store.put("provider",&json!({"id":"cheap","baseUrl":"https://cheap.example/v1",
            "model":"high","enabled":true,"costPerMillionInputKrw":500,
            "costPerMillionOutputKrw":900}));
        assert_eq!(select_provider(&state, Complexity::Light).unwrap()["id"],"cheap");
        assert_eq!(estimated_krw_per_million(&json!({"costPerMillionInputKrw":5})),None);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn light_prefers_free_and_deep_prefers_credentialed_routes() {
        let (state, dir) = state();
        state.store.put(
            "provider",
            &json!({"id":"free","name":"free","baseUrl":"https://free/v1","model":"small","enabled":true}),
        );
        state.store.put(
            "provider",
            &json!({"id":"paid","name":"paid","baseUrl":"https://paid/v1","model":"large","enabled":true,"contextTokens":128000}),
        );
        state.save_secret("provider:paid", "test-key");
        let light = select_provider(&state, Complexity::Light).unwrap();
        assert_eq!(light["id"], "free");
        let deep = select_provider(&state, Complexity::Deep).unwrap();
        assert_eq!(deep["id"], "paid");
        let _ = std::fs::remove_dir_all(dir);
    }
}
