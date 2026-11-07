//! Deterministic request routing.
//!
//! No extra model call is made: the classifier uses the request shape, length,
//! attachments and transcript depth. Auto-selected chats use the cheapest
//! capable route for light questions and a credentialed/high-capability route
//! for deep work; explicit provider selections are never overridden.

use crate::engine::gateway;
use crate::state::AppState;
use serde_json::Value;

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
            });
        }
        Complexity::Standard => {
            candidates.sort_by(|a, b| {
                has_key(state, a)
                    .cmp(&has_key(state, b))
                    .then_with(|| {
                        score(b)
                            .partial_cmp(&score(a))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });
        }
    }
    candidates.into_iter().next()
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
