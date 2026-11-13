//! session: isolated native chat runtime responsibility; preserve executable RSI behavior.
use super::*;

pub(super) fn mark_budget_exhausted(state: &AppState, execution_id: &str) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["budgetExhausted"] = json!(true);
        state.store.put("execution", &execution);
    }
}

pub(super) fn mark_run_signals(state: &AppState, record: &Value) {
    let Some(execution_id) = record["id"].as_str() else {
        return;
    };
    if let Some(entries) = record["strategyCompliance"].as_array() {
        let applicable = entries
            .iter()
            .filter(|entry| {
                entry["applicable"].as_bool() == Some(true)
                    && entry["result"]["triggered"].as_bool() == Some(true)
                    && entry["result"]["verified"].as_bool() == Some(true)
            })
            .collect::<Vec<_>>();
        if !applicable.is_empty() {
            let followed = applicable.iter().all(|entry| {
                entry["result"]["verified"].as_bool() == Some(true)
                    && entry["result"]["followed"].as_bool() == Some(true)
            });
            crate::engine::algorithms::mark_signal(
                state, execution_id, "strategy.selection", followed,
            );
        }
    }
    if let Some(execution) = state.store.get("execution", execution_id) {
        if execution["budgetChecked"].as_bool() == Some(true) {
            crate::engine::algorithms::mark_signal(
                state,
                execution_id,
                "execution.budget",
                execution["budgetExhausted"].as_bool() != Some(true),
            );
        }
        if let Some(available) = execution["discoveredTools"].as_array() {
            let used = state
                .store
                .all("execution_operation")
                .into_iter()
                .filter(|operation| operation["runId"].as_str() == Some(execution_id))
                .filter_map(|operation| operation["tool"].as_str().map(str::to_string))
                .collect::<Vec<_>>();
            if !used.is_empty() && !available.is_empty() {
                let ok = used.iter().all(|tool| {
                    available.iter().any(|name| name.as_str() == Some(tool.as_str()))
                });
                crate::engine::algorithms::mark_signal(
                    state, execution_id, "tool.discovery", ok,
                );
            }
        }
        if let Some(score) = execution["decomposition"]["qualityScore"].as_f64() {
            crate::engine::algorithms::mark_signal(
                state,
                execution_id,
                "task.decomposition",
                score >= 0.75,
            );
        }
    }
    if record["contextChecked"].as_bool() == Some(true) {
        let ok = record["contextOverflow"].as_bool() != Some(true);
        crate::engine::algorithms::mark_signal(
            state, execution_id, "context.selection", ok,
        );
    }
    if let Some(ids) = record["retrievedExperienceIds"].as_array() {
        if !ids.is_empty() {
            // Retrieval exposure is observable; whether the retrieved case
            // helped is not. Tool overlap in particular is not an answer
            // quality signal, so it must not create success/failure counters.
            crate::engine::algorithms::mark_unrated(
                state,
                execution_id,
                "memory.retrieval",
                "retrieval effect is not independently measured; exposure is not success",
            );
        }
    }
}

pub(super) fn retitle_if_placeholder(state: &AppState, session: &Value, text: &str) {
    let title = session
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    if !title.is_empty() && title != "새 대화" {
        return;
    }
    let mut updated = session.clone();
    updated["title"] = json!(text.chars().take(40).collect::<String>());
    if let Some(saved) = state.store.put("session", &updated) {
        state.bus.record("session", &saved);
    }
}

pub(super) fn ensure_session(state: &AppState, request: &TurnRequest) -> Result<Value, String> {
    if let Some(id) = &request.session_id {
        if let Some(session) = state.store.get("session", id) {
            return Ok(session);
        }
        return Err("세션을 찾을 수 없습니다.".into());
    }
    let title = request.input.chars().take(40).collect::<String>();
    let session = state.store.put(
        "session",
        &json!({
            "title": title,
            "agentId": request.agent_id,
            "providerId": request.provider_id,
            "model": request.model,
        }),
    );
    session.ok_or_else(|| "세션을 만들 수 없습니다.".to_string())
}

pub(super) fn record_tool_stat(state: &AppState, session_id: &str, agent_id: Option<&str>, name: &str, ok: bool) {
    let Some(stat) = state.store.increment_counters(
        "tool_stat",
        name,
        &["attempts", if ok { "successes" } else { "failures" }],
    ) else {
        return;
    };
    let attempts = stat["attempts"].as_u64().unwrap_or(0);
    let failures = stat["failures"].as_u64().unwrap_or(0);
    if attempts >= 3 && failures * 2 >= attempts {
        let scope = crate::engine::memory::MemoryScope::for_session(&state.store, session_id, agent_id);
        let text = format!(
            "{name} 도구의 실패가 반복됩니다. 가능하면 다른 도구를 먼저 사용하고, 사용할 때는 인자를 단순화하세요."
        );
        let _ = crate::engine::improve::propose(
            state,
            &scope,
            "tool_policy",
            &text,
            "도구별 성공/실패 통계에서 반복 실패를 확인했습니다.",
            &[format!("tool-stat:{name}")],
            Some(name),
            "shared",
        );
    }
}

