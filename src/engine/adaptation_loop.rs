//! Section 11-1 of design 2.4: observation -> adaptation inside the running
//! loop. A failing tool call is recorded as an observation with a hypothesis
//! and a changed-action intervention. The runtime also blocks an unchanged
//! repeat of that failed call so the model has to pick a different action.
use crate::state::AppState;
use serde_json::{json, Value};

/// Records a tool failure and the adaptation it implies. Bounded so a long run
/// cannot grow the execution record without limit.
pub fn record_tool_failure(
    state: &AppState,
    execution_id: &str,
    round: usize,
    call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let digest = crate::engine::memory::stable_id(&[tool, &arguments.to_string()]);
    let adaptation = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, call_id, &digest])
        ),
        "runId": execution_id,
        "stepId": round,
        "operationId": call_id,
        "observation": {
            "kind": "tool_failure",
            "tool": tool,
            "argsDigest": digest,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "hypothesis": {
            "claim": "the same call will repeat the same failure",
            "evidenceIds": [call_id],
        },
        "intervention": {
            "change": "different tool, arguments, or approach",
            "instruction": "do not repeat the identical failed call; choose a changed action or explain the blocker",
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(adaptation.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = adaptation.clone();
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(adaptation)
}

pub fn repeated_failure_message(tool: &str) -> String {
    format!(
        "변경 없는 `{tool}` 호출이 이미 실패했습니다. 같은 호출을 다시 실행하지 않았습니다. \
         다른 도구, 다른 인자, 또는 원인을 확인하는 접근을 선택하세요."
    )
}
