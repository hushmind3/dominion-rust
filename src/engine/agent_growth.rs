//! Per-bot growth. The model is chosen by the user and never changed here.
//! What accumulates is the agent's own execution history: success/failure
//! counts, failing tools, recurring failure classes and concrete guidance
//! derived from verified runs. That guidance is injected into the same agent's
//! next turn, so using one bot repeatedly makes that bot better.
use crate::state::AppState;
use serde_json::{json, Value};

fn agent_of(state: &AppState, execution: &Value) -> Option<String> {
    execution["agentId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            execution["sessionId"].as_str().and_then(|session_id| {
                state
                    .store
                    .get("session", session_id)
                    .and_then(|session| session["agentId"].as_str().map(str::to_string))
            })
        })
}

fn stable_guidance(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "revision": crate::engine::memory::stable_id(&[id, text]),
        "text": text,
    })
}

/// Updates the agent's own growth record after an execution. Never writes to
/// another agent's record and never changes the model.
pub fn record(state: &AppState, execution: &Value) -> Option<Value> {
    let agent_id = agent_of(state, execution)?;
    let mut growth = state
        .store
        .get("agent_growth", &agent_id)
        .unwrap_or_else(|| {
            json!({
                "id": agent_id,
                "agentId": agent_id,
                "runs": 0,
                "succeeded": 0,
                "failed": 0,
                "answeredUnverified": 0,
                "toolUses": {},
                "toolFailures": {},
                "failureClasses": {},
                "guidance": [],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            })
        });
    growth["runs"] = json!(growth["runs"].as_u64().unwrap_or(0) + 1);
    match execution["taskOutcome"].as_str() {
        Some("succeeded") => {
            growth["succeeded"] = json!(growth["succeeded"].as_u64().unwrap_or(0) + 1)
        }
        Some("answered_unverified") => {
            growth["answeredUnverified"] =
                json!(growth["answeredUnverified"].as_u64().unwrap_or(0) + 1)
        }
        _ => growth["failed"] = json!(growth["failed"].as_u64().unwrap_or(0) + 1),
    }
    if let Some(class) = execution["failureClass"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        let count = growth["failureClasses"][class].as_u64().unwrap_or(0) + 1;
        growth["failureClasses"][class] = json!(count);
        let hint = execution["recoveryHint"].as_str().unwrap_or("");
        if count >= 2 {
            let text = if hint.is_empty() {
                format!("`{class}` 실패가 {count}회 관찰되었습니다. 같은 접근을 반복하지 말고 원인을 먼저 확인하세요.")
            } else {
                format!("`{class}` 실패가 {count}회 관찰되었습니다. {hint}")
            };
            add_guidance(&mut growth, &format!("failure:{agent_id}:{class}"), &text, count);
        }
    }
    let run_id = execution["id"].as_str().unwrap_or("");
    for operation in state.store.all("execution_operation") {
        if operation["runId"].as_str() != Some(run_id) {
            continue;
        }
        let Some(tool) = operation["tool"].as_str() else {
            continue;
        };
        let uses = growth["toolUses"][tool].as_u64().unwrap_or(0) + 1;
        growth["toolUses"][tool] = json!(uses);
        if operation["status"].as_str() == Some("failed") {
            let failures = growth["toolFailures"][tool].as_u64().unwrap_or(0) + 1;
            growth["toolFailures"][tool] = json!(failures);
            if failures >= 2 {
                add_guidance(
                    &mut growth,
                    &format!("tool:{agent_id}:{tool}"),
                    &format!(
                        "`{tool}` 실패가 {failures}회 관찰되었습니다. 같은 인자를 반복하지 말고 다른 도구나 더 단순한 인자를 사용하세요."
                    ),
                    failures,
                );
            }
        }
    }
    growth["successRate"] = json!(if growth["runs"].as_u64().unwrap_or(0) > 0 {
        growth["succeeded"].as_u64().unwrap_or(0) as f64
            / growth["runs"].as_u64().unwrap_or(1) as f64
    } else {
        0.0
    });
    growth["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("agent_growth", &growth)?;
    state.bus.record("agent_growth", &saved);
    Some(saved)
}

fn add_guidance(growth: &mut Value, id: &str, text: &str, evidence: u64) {
    let mut guidance = growth["guidance"].as_array().cloned().unwrap_or_default();
    if let Some(existing) = guidance
        .iter_mut()
        .find(|item| item["id"].as_str() == Some(id))
    {
        *existing = stable_guidance(id, text);
        existing["evidenceCount"] = json!(evidence);
        existing["confidence"] = json!(
            (evidence as f64 / growth["runs"].as_u64().unwrap_or(1).max(1) as f64)
                .min(1.0)
        );
    } else {
        let mut record = stable_guidance(id, text);
        record["evidenceCount"] = json!(evidence);
        record["confidence"] = json!(
            (evidence as f64 / growth["runs"].as_u64().unwrap_or(1).max(1) as f64)
                .min(1.0)
        );
        guidance.push(record);
    }
    guidance.sort_by(|a, b| {
        b["evidenceCount"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["evidenceCount"].as_u64().unwrap_or(0))
    });
    if guidance.len() > 8 {
        // Keep the highest-evidence guidance. split_off would keep the tail.
        guidance.truncate(8);
    }
    growth["guidance"] = json!(guidance);
}

pub fn growth(state: &AppState, agent_id: &str) -> Value {
    state.store.get("agent_growth", agent_id).unwrap_or_else(|| {
        json!({
            "agentId": agent_id,
            "runs": 0,
            "succeeded": 0,
            "failed": 0,
            "successRate": 0.0,
            "guidance": [],
        })
    })
}

/// Guidance for this agent only. It is injected into the same agent's prompt,
/// never into another bot's.
pub fn context_block(state: &AppState, agent_id: &str) -> Option<(String, Vec<Value>)> {
    let growth = state.store.get("agent_growth", agent_id)?;
    let runs = growth["runs"].as_u64().unwrap_or(0);
    if runs == 0 {
        return None;
    }
    let guidance = growth["guidance"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item["confidence"].as_f64().unwrap_or(0.0) >= 0.15)
        .take(3)
        .collect::<Vec<_>>();
    let mut block = format!(
        "Agent growth (this bot only; from {runs} runs, success {:.0}%):",
        growth["successRate"].as_f64().unwrap_or(0.0) * 100.0
    );
    for item in &guidance {
        block.push_str(&format!(
            "\n- {} (evidence {}, confidence {:.0}%)",
            item["text"].as_str().unwrap_or(""),
            item["evidenceCount"].as_u64().unwrap_or(0),
            item["confidence"].as_f64().unwrap_or(0.0) * 100.0
        ));
    }
    let items = guidance
        .iter()
        .map(|item| {
            json!({
                "kind": "agent_growth",
                "id": item["id"],
                "revision": item["revision"],
                "confidence": item["confidence"],
            })
        })
        .collect();
    Some((block, items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-growth-{}", uuid::Uuid::new_v4()));
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

    fn failed_execution(id: &str, agent: &str) -> Value {
        json!({
            "id": id,
            "sessionId": "s",
            "agentId": agent,
            "taskOutcome": "failed",
            "failureClass": "tool_failed",
            "recoveryHint": "다른 인자를 사용하세요"
        })
    }

    #[test]
    fn repeated_failures_become_guidance_for_that_agent_only() {
        let (state, dir) = state();
        for run in ["r1", "r2"] {
            state.store.put(
                "execution_operation",
                &json!({"id":format!("{run}:1"),"runId":run,"tool":"shell_exec","status":"failed"}),
            );
            record(&state, &failed_execution(run, "agent-a")).unwrap();
        }
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["runs"], 2);
        assert_eq!(growth["failed"], 2);
        assert!(!growth["guidance"].as_array().unwrap().is_empty());
        let (block, items) = context_block(&state, "agent-a").unwrap();
        assert!(block.contains("Agent growth"));
        assert!(block.contains("tool_failed"));
        assert!(!items.is_empty());
        // Another bot never inherits agent-a's private growth.
        assert!(context_block(&state, "agent-b").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_single_success_still_records_the_bot_run() {
        let (state, dir) = state();
        record(
            &state,
            &json!({"id":"ok","sessionId":"s","agentId":"agent-a","taskOutcome":"succeeded"}),
        )
        .unwrap();
        let growth = growth(&state, "agent-a");
        assert_eq!(growth["succeeded"], 1);
        assert_eq!(growth["successRate"], 1.0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
