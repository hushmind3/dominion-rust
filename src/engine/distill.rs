//! Deterministic distillation of stored conversation/tool records.
//!
//! Conversation itself is a model task. Learning from what the conversation
//! left in SQLite is not: corrections, failed tools, later successful
//! alternatives and explicit preferences can all be extracted without calling
//! any model.
use crate::engine::experience;
use crate::engine::memory::{self, MemoryScope};
use crate::state::AppState;
use serde_json::{json, Value};

fn explicit_preference(text: &str) -> bool {
    let lower = text.to_lowercase();
    let markers = [
        "앞으로", "다음부터", "항상", "하지 마", "하지마", "대신", "선호",
        "from now", "always", "never", "prefer",
    ];
    markers.iter().any(|marker| lower.contains(marker))
}

fn correction(text: &str) -> bool {
    crate::engine::reflect::looks_like_correction(text)
}

/// Extracts durable lessons from one bot's stored session. No model call.
pub fn distill_session(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
) -> Value {
    let scope = MemoryScope::for_session(&state.store, session_id, agent_id);
    let messages = state.store.messages(session_id);
    let executions = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| execution["sessionId"].as_str() == Some(session_id))
        .collect::<Vec<_>>();

    let mut memories = 0;
    let mut lessons = 0;
    for message in messages.iter() {
        if message["role"].as_str() != Some("user") {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        let is_preference = explicit_preference(content);
        let is_correction = correction(content);
        if !is_preference && !is_correction {
            continue;
        }
        let Some(message_id) = message["id"].as_str() else {
            continue;
        };
        if is_preference {
            let key = format!(
                "distilled-preference-{}",
                crate::engine::memory::stable_id(&[agent_id.unwrap_or("default"), content])
            );
            if memory::save_grounded(
                &state.store,
                &scope,
                content,
                "preference",
                0.8,
                Some(&key),
                "conversation_distill",
                &[message_id.to_string()],
                content,
            )
            .is_ok()
            {
                memories += 1;
            }
        }
        if is_correction {
            let mut evidence = vec![message_id.to_string()];
            let mut failed_tools = Vec::new();
            for execution in &executions {
                let run_id = execution["id"].as_str().unwrap_or("");
                for operation in state.store.all("execution_operation") {
                    if operation["runId"].as_str() != Some(run_id)
                        || operation["status"].as_str() != Some("failed")
                    {
                        continue;
                    }
                    if let Some(tool) = operation["tool"].as_str() {
                        failed_tools.push(tool.to_string());
                    }
                    if let Some(id) = operation["id"].as_str() {
                        evidence.push(id.to_string());
                    }
                }
            }
            failed_tools.sort();
            failed_tools.dedup();
            if !failed_tools.is_empty() {
                let request = executions
                    .iter()
                    .find_map(|execution| execution["request"].as_str())
                    .unwrap_or("");
                if experience::record(
                    state,
                    &scope,
                    request,
                    "failure",
                    content,
                    &failed_tools.join(", "),
                    &evidence,
                    &failed_tools,
                    &[content.to_string()],
                )
                .is_some()
                {
                    lessons += 1;
                }
            }
        }
    }

    // A failed tool followed by a different successful tool in the same run is
    // a concrete, model-free recovery lesson.
    for execution in &executions {
        let run_id = execution["id"].as_str().unwrap_or("");
        let operations = state
            .store
            .all("execution_operation")
            .into_iter()
            .filter(|operation| operation["runId"].as_str() == Some(run_id))
            .collect::<Vec<_>>();
        let mut failed = Vec::new();
        let mut succeeded = Vec::new();
        for operation in &operations {
            let Some(tool) = operation["tool"].as_str() else {
                continue;
            };
            match operation["status"].as_str() {
                Some("failed") => failed.push(tool.to_string()),
                Some("completed") => succeeded.push(tool.to_string()),
                _ => {}
            }
        }
        let alternative = succeeded
            .into_iter()
            .find(|tool| !failed.iter().any(|failed| failed == tool));
        let Some(alternative) = alternative else {
            continue;
        };
        let Some(failed_tool) = failed.first() else {
            continue;
        };
        let lesson = format!(
            "`{failed_tool}` 실패 후 `{alternative}`로 전환해 작업을 완료했습니다."
        );
        let evidence = operations
            .iter()
            .filter_map(|operation| operation["id"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        if experience::record(
            state,
            &scope,
            execution["request"].as_str().unwrap_or(""),
            "failure",
            &lesson,
            failed_tool,
            &evidence,
            &failed,
            &[],
        )
        .is_some()
        {
            lessons += 1;
        }
    }

    json!({
        "sessionId": session_id,
        "agentId": scope.agent_id,
        "memories": memories,
        "lessons": lessons,
        "modelCalls": 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-distill-{}", uuid::Uuid::new_v4()));
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
    fn corrections_and_failed_tools_become_lessons_without_a_model() {
        let (state, dir) = state();
        state
            .store
            .write_message(&json!({
                "session_id":"s","role":"user","agent_id":"a",
                "content":"그게 아니라 앞으로는 workspace_write를 먼저 써"
            }))
            .unwrap();
        state.store.put(
            "execution",
            &json!({
                "id":"run-1","sessionId":"s","agentId":"a",
                "request":"파일을 만들어","taskOutcome":"failed"
            }),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-1","runId":"run-1","tool":"shell_exec","status":"failed"}),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-2","runId":"run-1","tool":"workspace_write","status":"completed"}),
        );
        let result = distill_session(&state, "s", Some("a"));
        assert_eq!(result["modelCalls"], 0);
        assert!(result["memories"].as_u64().unwrap() >= 1, "{result}");
        assert!(result["lessons"].as_u64().unwrap() >= 1, "{result}");
        assert!(!state.store.all("memory").is_empty());
        assert!(!state.store.all("experience").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
