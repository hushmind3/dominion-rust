//! Experience bank: concrete task cases with outcomes, lessons and usage.
//! Unlike free-form memories, these are retrieved by task similarity and fed
//! into planning so failures are avoided and successful procedures reused.
use crate::engine::memory::{self, MemoryScope};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

fn signature(request: &str) -> String {
    let mut seen = HashSet::new();
    tokens(request)
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .take(10)
        .collect::<Vec<_>>()
        .join(" ")
}

fn visible(record: &Value, scope: &MemoryScope) -> bool {
    if record["partitionKey"].as_str() != Some(scope.partition_key.as_str()) {
        return false;
    }
    record["scope"].as_str() == Some("shared")
        || record["agentId"].as_str() == scope.agent_id.as_deref()
}

pub fn record(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
) -> Option<Value> {
    record_with_scope(
        state, scope, request, outcome, lesson, avoid, evidence_ids, tools, gaps, false,
    )
}

/// Shared cases are visible to every agent in the same partition.
pub fn record_shared(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
) -> Option<Value> {
    record_with_scope(
        state, scope, request, outcome, lesson, avoid, evidence_ids, tools, gaps, true,
    )
}

#[allow(clippy::too_many_arguments)]
fn record_with_scope(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
    shared: bool,
) -> Option<Value> {
    let request = request.trim().chars().take(2000).collect::<String>();
    let lesson = lesson.trim().chars().take(2000).collect::<String>();
    if request.is_empty() || lesson.is_empty() || !matches!(outcome, "success" | "failure") {
        return None;
    }
    let sig = signature(&request);
    let id = format!(
        "experience-{}",
        memory::stable_id(&[
            &scope.partition_key,
            if shared {
                "shared"
            } else {
                scope.agent_id.as_deref().unwrap_or("default")
            },
            &sig,
            outcome,
            &normalize(&lesson),
        ])
    );
    if let Some(mut existing) = state.store.get("experience", &id) {
        existing["occurrences"] = json!(existing["occurrences"].as_u64().unwrap_or(1) + 1);
        existing["lastSeenAt"] = json!(chrono::Utc::now().to_rfc3339());
        let merged = existing["evidenceMessageIds"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(evidence_ids.iter().cloned().map(Value::String))
            .collect::<Vec<_>>();
        existing["evidenceMessageIds"] = json!(merged.into_iter().take(12).collect::<Vec<_>>());
        let saved = state.store.put("experience", &existing)?;
        state.bus.record("experience", &saved);
        return Some(saved);
    }
    let saved = state.store.put(
        "experience",
        &json!({
            "id": id,
            "scope": if shared || scope.agent_id.is_none() { "shared" } else { "agent" },
            "agentId": scope.agent_id,
            "partitionKey": scope.partition_key,
            "sessionId": scope.session_id,
            "signature": sig,
            "request": request,
            "outcome": outcome,
            "lesson": lesson,
            "avoid": avoid.chars().take(1000).collect::<String>(),
            "gaps": gaps.iter().take(8).cloned().collect::<Vec<_>>(),
            "tools": tools.iter().take(20).cloned().collect::<Vec<_>>(),
            "evidenceMessageIds": evidence_ids.iter().take(12).cloned().collect::<Vec<_>>(),
            "occurrences": 1,
            "uses": 0,
            "status": "active",
            "createdAt": chrono::Utc::now().to_rfc3339(),
            "lastSeenAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    if let Some(saved) = &saved {
        state.bus.record("experience", saved);
        state
            .store
            .audit("experience.recorded", json!({"id": saved["id"], "outcome": outcome}));
    }
    saved
}

/// Retrieves similar cases and bumps their usage counters.
pub fn retrieve(state: &AppState, scope: &MemoryScope, query: &str, limit: usize) -> Vec<Value> {
    let query_tokens = tokens(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let mut scored = Vec::new();
    for record in state.store.all("experience") {
        if record["status"].as_str() != Some("active") || !visible(&record, scope) {
            continue;
        }
        let haystack = format!(
            "{} {} {}",
            record["signature"].as_str().unwrap_or(""),
            record["request"].as_str().unwrap_or(""),
            record["lesson"].as_str().unwrap_or("")
        )
        .to_lowercase();
        let overlap = query_tokens
            .iter()
            .filter(|token| haystack.contains(token.as_str()))
            .count();
        if overlap == 0 {
            continue;
        }
        let boost = if record["outcome"] == "failure" { 2.0 } else { 1.0 };
        let occurrences = record["occurrences"].as_u64().unwrap_or(1) as f64;
        scored.push((overlap as f64 * 3.0 + boost + occurrences * 0.1, record));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let selected = scored
        .into_iter()
        .take(limit.clamp(1, 8))
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    for record in &selected {
        if let Some(mut stored) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
            stored["uses"] = json!(stored["uses"].as_u64().unwrap_or(0) + 1);
            stored["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("experience", &stored);
        }
    }
    selected
}

pub fn context_block(state: &AppState, scope: &MemoryScope, query: &str) -> Option<String> {
    let cases = retrieve(state, scope, query, 5);
    if cases.is_empty() {
        return None;
    }
    let mut block = String::from("Similar past task cases (failure cases are warnings):");
    for case in cases {
        let outcome = case["outcome"].as_str().unwrap_or("");
        let request = case["request"].as_str().unwrap_or("").chars().take(220).collect::<String>();
        let lesson = case["lesson"].as_str().unwrap_or("").chars().take(320).collect::<String>();
        let avoid = case["avoid"].as_str().unwrap_or("").chars().take(220).collect::<String>();
        block.push_str(&format!("\n- [{outcome}] task: {request}\n  lesson: {lesson}"));
        if !avoid.is_empty() {
            block.push_str("\n  avoid: ");
            block.push_str(&avoid);
        }
    }
    Some(block)
}

/// Merges duplicates and archives the least useful cases. Called by the
/// learning worker so the bank does not grow without bound.
pub fn consolidate(state: &AppState) -> Value {
    let mut seen: HashMap<String, Value> = HashMap::new();
    let mut archived = 0usize;
    for record in state.store.all("experience") {
        if record["status"].as_str() != Some("active") {
            continue;
        }
        let key = format!(
            "{}:{}:{}:{}",
            record["partitionKey"].as_str().unwrap_or(""),
            if record["scope"].as_str() == Some("shared") {
                "shared"
            } else {
                record["agentId"].as_str().unwrap_or("default")
            },
            record["signature"].as_str().unwrap_or(""),
            record["outcome"].as_str().unwrap_or("")
        );
        match seen.get(&key) {
            Some(existing)
                if existing["occurrences"].as_u64().unwrap_or(1)
                    >= record["occurrences"].as_u64().unwrap_or(1) =>
            {
                if let Some(mut old) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
                    old["status"] = json!("archived");
                    old["archivedAt"] = json!(chrono::Utc::now().to_rfc3339());
                    state.store.put("experience", &old);
                    archived += 1;
                }
            }
            _ => {
                seen.insert(key, record);
            }
        }
    }
    let mut active = state
        .store
        .all("experience")
        .into_iter()
        .filter(|record| record["status"] == "active")
        .collect::<Vec<_>>();
    active.sort_by(|a, b| {
        b["occurrences"].as_u64().unwrap_or(0)
            .cmp(&a["occurrences"].as_u64().unwrap_or(0))
            .then_with(|| {
                b["lastSeenAt"].as_str().unwrap_or("").cmp(a["lastSeenAt"].as_str().unwrap_or(""))
            })
    });
    for record in active.into_iter().skip(300) {
        if let Some(mut old) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
            old["status"] = json!("archived");
            old["archivedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("experience", &old);
            archived += 1;
        }
    }
    json!({ "archived": archived })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-exp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn records_dedupes_and_retrieves_similar_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let first = record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m1".into()], &["workspace_write".into()], &["file_changed".into()],
        ).unwrap();
        record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m2".into()], &[], &[],
        ).unwrap();
        let stored = state.store.get("experience", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["occurrences"], 2);
        let cases = retrieve(&state, &scope, "README.md 파일 작성", 5);
        assert_eq!(cases.len(), 1);
        assert!(context_block(&state, &scope, "README.md 파일 작성")
            .unwrap()
            .contains("말로만"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scopes_do_not_leak_between_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record(&state, &a, "파일 작성", "success", "A의 절차", "", &[], &[], &[]).unwrap();
        assert!(retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_cases_reach_other_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record_shared(
            &state, &a, "파일 작성", "success", "공유 절차를 재사용한다", "", &[], &[], &[],
        )
        .unwrap();
        assert!(!retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consolidation_archives_duplicate_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let base = state.store.put("experience", &json!({
            "id":"e1","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":5,"status":"active","lastSeenAt":"2026-01-01"
        })).unwrap();
        state.store.put("experience", &json!({
            "id":"e2","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":1,"status":"active","lastSeenAt":"2026-01-02"
        })).unwrap();
        consolidate(&state);
        assert_eq!(state.store.get("experience", base["id"].as_str().unwrap()).unwrap()["status"], "active");
        assert_eq!(state.store.get("experience", "e2").unwrap()["status"], "archived");
        let _ = std::fs::remove_dir_all(dir);
    }
}
