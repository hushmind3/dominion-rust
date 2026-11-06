//! Deterministic distillation of stored conversation/tool records.
//!
//! Conversation itself is a model task. Learning from what the conversation
//! left in SQLite is not: corrections, failed tools, later successful
//! alternatives and explicit preferences can all be extracted without calling
//! any model. Extraction is incremental and idempotent by message/execution id.
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

fn checkpoint_id(scope: &MemoryScope) -> String {
    format!(
        "distill-checkpoint-{}",
        crate::engine::memory::stable_id(&[
            scope.session_id.as_str(),
            scope.partition_key.as_str(),
            scope.agent_id.as_deref().unwrap_or("default"),
        ])
    )
}

fn evidence_recorded(state: &AppState, evidence: &[String]) -> bool {
    if evidence.is_empty() {
        return false;
    }
    state.store.all("experience").iter().any(|record| {
        record["evidenceMessageIds"].as_array().is_some_and(|ids| {
            evidence.iter().all(|id| ids.iter().any(|item| item.as_str() == Some(id)))
        })
    })
}

/// Runs one incremental distillation pass for a session. `force` ignores the
/// checkpoint (used by tests and explicit rebuilds); evidence dedup still
/// prevents double-counting.
pub fn distill_incremental(
    state: &AppState,
    session_id: &str,
    agent_id: Option<&str>,
    force: bool,
) -> Value {
    let scope = MemoryScope::for_session(&state.store, session_id, agent_id);
    let id = checkpoint_id(&scope);
    let checkpoint = if force {
        Value::Null
    } else {
        state.store.get("distill_checkpoint", &id).unwrap_or(Value::Null)
    };
    let messages = state.store.messages(session_id);
    let start = if force {
        0
    } else {
        checkpoint["throughMessageId"]
            .as_str()
            .and_then(|through| {
                messages
                    .iter()
                    .position(|message| message["id"].as_str() == Some(through))
            })
            .map(|index| index + 1)
            .unwrap_or(0)
    };
    let executions = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| execution["sessionId"].as_str() == Some(session_id))
        .collect::<Vec<_>>();
    let processed_executions = checkpoint["executionIds"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();

    let mut memories = 0;
    let mut lessons = 0;
    let mut last_message_id = checkpoint["throughMessageId"].clone();
    for message in messages.iter().skip(start) {
        last_message_id = message["id"].clone();
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
            // Link the correction to the most recent execution before it,
            // not to every failed tool in the whole session.
            let execution = executions
                .iter()
                .filter(|execution| !execution["finishedAt"].is_null())
                .max_by(|a, b| {
                    a["finishedAt"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(b["finishedAt"].as_str().unwrap_or(""))
                });
            let Some(execution) = execution else {
                continue;
            };
            let run_id = execution["id"].as_str().unwrap_or("");
            let mut failed_tools = Vec::new();
            let mut evidence = vec![message_id.to_string()];
            for operation in state.store.all("execution_operation") {
                if operation["runId"].as_str() != Some(run_id)
                    || operation["status"].as_str() != Some("failed")
                {
                    continue;
                }
                if let Some(tool) = operation["tool"].as_str() {
                    failed_tools.push(tool.to_string());
                }
                if let Some(op_id) = operation["id"].as_str() {
                    evidence.push(op_id.to_string());
                }
            }
            failed_tools.sort();
            failed_tools.dedup();
            if failed_tools.is_empty() || evidence_recorded(state, &evidence) {
                continue;
            }
            let request = execution["request"].as_str().unwrap_or("");
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

    // A failed tool followed by a different successful tool, in that order,
    // and only when the run itself succeeded.
    let mut processed = processed_executions;
    for execution in &executions {
        let run_id = execution["id"].as_str().unwrap_or("");
        if run_id.is_empty() || processed.contains(run_id) {
            continue;
        }
        if execution["taskOutcome"].as_str() != Some("succeeded") {
            processed.insert(run_id.to_string());
            continue;
        }
        let mut operations = state
            .store
            .all("execution_operation")
            .into_iter()
            .filter(|operation| operation["runId"].as_str() == Some(run_id))
            .collect::<Vec<_>>();
        operations.sort_by_key(|operation| operation["opSeq"].as_u64().unwrap_or(0));
        let mut recovery = None;
        for (index, operation) in operations.iter().enumerate() {
            if operation["status"].as_str() != Some("failed") {
                continue;
            }
            let Some(failed_tool) = operation["tool"].as_str() else {
                continue;
            };
            if let Some(next) = operations[index + 1..].iter().find(|next| {
                next["status"].as_str() == Some("completed")
                    && next["tool"].as_str() != Some(failed_tool)
            }) {
                recovery = Some((failed_tool.to_string(), next.clone()));
                break;
            }
        }
        processed.insert(run_id.to_string());
        let Some((failed_tool, alternative)) = recovery else {
            continue;
        };
        let Some(alternative_tool) = alternative["tool"].as_str() else {
            continue;
        };
        let evidence = vec![
            operations
                .iter()
                .find(|operation| operation["tool"].as_str() == Some(failed_tool.as_str()))
                .and_then(|operation| operation["id"].as_str())
                .unwrap_or("")
                .to_string(),
            alternative["id"].as_str().unwrap_or("").to_string(),
        ];
        if evidence.iter().any(String::is_empty) || evidence_recorded(state, &evidence) {
            continue;
        }
        let lesson = format!(
            "`{failed_tool}` 실패 후 `{alternative_tool}`로 전환해 작업을 완료했습니다."
        );
        if experience::record(
            state,
            &scope,
            execution["request"].as_str().unwrap_or(""),
            "failure",
            &lesson,
            &failed_tool,
            &evidence,
            &[failed_tool.clone()],
            &[],
        )
        .is_some()
        {
            lessons += 1;
        }
    }

    state.store.put(
        "distill_checkpoint",
        &json!({
            "id": id,
            "sessionId": session_id,
            "agentId": scope.agent_id,
            "partitionKey": scope.partition_key,
            "throughMessageId": last_message_id,
            "executionIds": processed.iter().cloned().collect::<Vec<_>>(),
            "updatedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );

    json!({
        "sessionId": session_id,
        "agentId": scope.agent_id,
        "memories": memories,
        "lessons": lessons,
        "modelCalls": 0,
    })
}

/// Explicit rebuild: ignores the checkpoint but still deduplicates evidence.
#[allow(dead_code)]
pub fn distill_session(state: &AppState, session_id: &str, agent_id: Option<&str>) -> Value {
    distill_incremental(state, session_id, agent_id, true)
}

/// Background pass over every session. No model call; only new records since
/// the last checkpoint are examined.
pub fn sweep(state: &AppState) -> usize {
    let mut sessions = state
        .store
        .all("session")
        .into_iter()
        .filter_map(|session| session["id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    sessions.sort();
    sessions.dedup();
    let mut processed = 0;
    for session_id in sessions {
        let agent = state
            .store
            .get("session", &session_id)
            .and_then(|session| session["agentId"].as_str().map(str::to_string));
        let result = distill_incremental(state, &session_id, agent.as_deref(), false);
        if result["memories"].as_u64().unwrap_or(0) > 0
            || result["lessons"].as_u64().unwrap_or(0) > 0
        {
            processed += 1;
        }
    }
    processed
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
                "request":"파일을 만들어","taskOutcome":"succeeded",
                "finishedAt": chrono::Utc::now().to_rfc3339()
            }),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-1","runId":"run-1","tool":"shell_exec","status":"failed","opSeq":1}),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-2","runId":"run-1","tool":"workspace_write","status":"completed","opSeq":2}),
        );
        let result = distill_session(&state, "s", Some("a"));
        assert_eq!(result["modelCalls"], 0);
        assert!(result["memories"].as_u64().unwrap() >= 1, "{result}");
        assert!(result["lessons"].as_u64().unwrap() >= 1, "{result}");
        assert!(!state.store.all("memory").is_empty());
        assert!(!state.store.all("experience").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn repeated_sweeps_do_not_inflate_experience_occurrences() {
        let (state, dir) = state();
        state
            .store
            .write_message(&json!({
                "session_id":"s","role":"user","agent_id":"a",
                "content":"그게 아니라 다음부터는 다르게 해"
            }))
            .unwrap();
        state.store.put(
            "execution",
            &json!({
                "id":"run-1","sessionId":"s","agentId":"a",
                "request":"파일을 만들어","taskOutcome":"succeeded",
                "finishedAt": chrono::Utc::now().to_rfc3339()
            }),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-1","runId":"run-1","tool":"shell_exec","status":"failed","opSeq":1}),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-2","runId":"run-1","tool":"workspace_write","status":"completed","opSeq":2}),
        );
        distill_session(&state, "s", Some("a"));
        let first = state
            .store
            .all("experience")
            .iter()
            .map(|record| record["occurrences"].as_u64().unwrap_or(1))
            .sum::<u64>();
        for _ in 0..5 {
            distill_session(&state, "s", Some("a"));
        }
        let after = state
            .store
            .all("experience")
            .iter()
            .map(|record| record["occurrences"].as_u64().unwrap_or(1))
            .sum::<u64>();
        assert_eq!(first, after, "reprocessing inflated occurrences");
        let _ = std::fs::remove_dir_all(dir);
    }
}
