use crate::engine::jobs::JobContext;
use crate::engine::{chat::TurnRequest, run_quiet};
use serde_json::{json, Value};

/// Autonomous goal loop. The model works step by step with real tools until it
/// declares `DONE`, the iteration budget runs out, or the user cancels.
pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    let state = &context.state;
    let goal_id = job
        .get("input")
        .and_then(|value| value.get("goalId"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let mut goal = state
        .store
        .get("goal", goal_id)
        .ok_or_else(|| "목표를 찾을 수 없습니다.".to_string())?;

    let text = goal
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("목표 내용이 비어 있습니다.".into());
    }

    let executor = goal.get("executor").cloned().unwrap_or_else(|| json!({}));
    let max_iterations = goal
        .get("maxIterations")
        .and_then(|value| value.as_i64())
        .unwrap_or(5)
        .clamp(1, 12) as usize;

    goal["status"] = json!("running");
    goal["startedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let goal = state.store.put("goal", &goal).unwrap_or(goal);
    context.emit(json!({ "type": "goal_started", "goalId": goal_id }));

    let session_id = match goal.get("sessionId").and_then(|value| value.as_str()) {
        Some(id) if state.store.get("session", id).is_some() => id.to_string(),
        _ => {
            let created = state
                .store
                .put(
                    "session",
                    &json!({
                        "title": format!("Goal · {}", text.chars().take(36).collect::<String>()),
                        "agentId": executor.get("agentId"),
                    }),
                )
                .ok_or("목표 세션을 만들 수 없습니다.")?;
            created
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string()
        }
    };

    let mut transcript = String::new();
    let mut iterations = 0usize;
    let mut completed = false;
    let mut cancelled = false;

    for index in 0..max_iterations {
        if state.is_cancelled(&session_id) {
            cancelled = true;
            break;
        }
        let progress = if transcript.is_empty() {
            "(아직 없음)".to_string()
        } else {
            transcript.chars().rev().take(4000).collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        };
        let prompt = format!(
            "자율 작업: {text}\n\n지금까지 진행:\n{progress}\n\n\
             다음 한 단계를 실제로 수행하세요. 도구가 필요하면 지금 호출하세요. \
             작업이 완전히 끝났다면 마지막 줄에 DONE 이라고만 쓰세요."
        );

        let request = TurnRequest {
            session_id: Some(session_id.clone()),
            input: prompt,
            agent_id: executor
                .get("agentId")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            provider_id: executor
                .get("providerId")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            model: executor
                .get("model")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            tools_enabled: true,
            thinking: None,
            reasoning_effort: None,
            system_override: Some(
                "You are executing a durable autonomous goal. Act with real tools, \
                 report honestly, and only write DONE when the goal is verified complete."
                    .to_string(),
            ),
        };

        let message = match run_quiet(state, &request).await {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(error) => {
                if let Some(mut record) = state.store.get("goal", goal_id) {
                    record["status"] = json!("failed");
                    record["error"] = json!(error);
                    record["sessionId"] = json!(session_id);
                    record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
                    state.store.put("goal", &record);
                }
                return Err(error);
            }
        };

        let content = message
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        transcript.push_str(&format!("\n[단계 {}] {content}\n", index + 1));
        iterations = index + 1;

        let mut record = state.store.get("goal", goal_id).unwrap_or_else(|| json!({}));
        record["iterations"] = json!(iterations);
        record["lastOutput"] = json!(content.chars().take(2000).collect::<String>());
        record["sessionId"] = json!(session_id);
        state.store.put("goal", &record);
        context.patch(json!({ "stepIndex": iterations, "stepTotal": max_iterations }));
        context.emit(json!({
            "type": "goal_progress",
            "goalId": goal_id,
            "iteration": iterations,
            "done": content.trim().ends_with("DONE"),
        }));

        if content.trim().ends_with("DONE") {
            completed = true;
            break;
        }
    }

    let status = if cancelled {
        "cancelled"
    } else if completed {
        "completed"
    } else {
        "needs_review"
    };

    if let Some(mut record) = state.store.get("goal", goal_id) {
        record["status"] = json!(status);
        record["iterations"] = json!(iterations);
        record["sessionId"] = json!(session_id);
        record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("goal", &record);
    }
    state
        .store
        .audit("goal.finished", json!({ "goalId": goal_id, "status": status, "iterations": iterations }));
    context.emit(json!({ "type": "goal_finished", "goalId": goal_id, "status": status }));

    Ok(json!({
        "goalId": goal_id,
        "status": status,
        "sessionId": session_id,
        "iterations": iterations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::state::AppState;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-goals-{}", uuid::Uuid::new_v4()));
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

    #[tokio::test]
    async fn missing_goal_fails_cleanly() {
        let state = temp_state();
        let job = crate::engine::jobs::enqueue(&state, "goal", json!({ "goalId": "missing" }))
            .expect("job");
        let ctx = crate::engine::jobs::JobContext::new(state.clone(), "job".into());
        let error = run(&ctx, &job).await.unwrap_err();
        assert!(error.contains("목표"));
    }
}
