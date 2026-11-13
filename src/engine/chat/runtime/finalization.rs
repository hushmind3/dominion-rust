//! finalization responsibilities of the chat runtime.
use super::*;

/// Overall compliance for an attempt. A single violated standing strategy must
/// not be masked by another followed strategy in the same bundle.
/// Evaluator version is separate from the payload hash, so re-running the same
/// terminal attempt with a new evaluator is a new evaluation revision rather
/// than a conflicting payload.
const FINALIZATION_EVALUATOR_VERSION: &str = "finalize-semantic-v1";

/// Immutable semantic content of a terminal attempt. Counters, timings and
/// `finishedAt` are excluded: a retried finalization of the same attempt must
/// produce the same hash even though those mutable fields differ.
fn finalization_semantic_payload(record: &Value, operations: &[Value]) -> Value {
    let mut normalized = operations
        .iter()
        .map(|operation| {
            json!({
                "id": operation["id"],
                "runId": operation["runId"],
                "tool": operation["tool"],
                "arguments": operation["arguments"],
                "origin": operation["origin"],
                "procedureNodeId": operation["procedureNodeId"],
                "procedureStrategyId": operation["procedureStrategyId"],
                "procedureRevision": operation["procedureRevision"],
                "status": operation["status"],
                "result": operation["result"],
                "outcomeUnknown": operation["outcomeUnknown"],
            })
        })
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or("")
            .cmp(right["id"].as_str().unwrap_or(""))
    });
    json!({
        "schemaVersion": 1,
        "evaluatorVersion": FINALIZATION_EVALUATOR_VERSION,
        "executionId": record["id"],
        "sessionId": record["sessionId"],
        "agentId": record["agentId"],
        "request": record["request"],
        "status": record["status"],
        "taskOutcome": record["taskOutcome"],
        "failureClass": record["failureClass"],
        "acceptancePassed": record["acceptancePassed"],
        "acceptanceText": record["acceptanceText"],
        "appliedImprovementIds": record["appliedImprovementIds"],
        "strategyCompliance": record["strategyCompliance"],
        "finalMessageId": record["finalMessageId"],
        "operations": normalized,
    })
}

pub(super) fn finalization_payload_hash(record: &Value, operations: &[Value]) -> String {
    crate::engine::memory::stable_id(&[
        &finalization_semantic_payload(record, operations).to_string(),
    ])
}

/// Overall compliance for an attempt. Contract:
/// 1. any confirmed `violated` wins — never masked by another arm's `followed`
///    or by an `unknown` sibling;
/// 2. then `unknown/invalid` (evidence not trustworthy);
/// 3. then `followed`; 4. `not_triggered`; 5. otherwise `not_applicable`.
pub(super) fn compliance_summary(
    entries: &[Value],
) -> crate::engine::adaptation::types::Compliance {
    use crate::engine::adaptation::types::Compliance;
    if entries.iter().any(|entry| {
        entry["applicable"] == true
            && entry["result"]["triggered"] == true
            && entry["result"]["followed"] != true
            && !matches!(entry["result"]["status"].as_str(), Some("unknown" | "invalid"))
    }) {
        return Compliance::Violated;
    }
    if entries.iter().any(|entry| {
        matches!(entry["result"]["status"].as_str(), Some("unknown" | "invalid"))
    }) {
        return Compliance::Unknown;
    }
    if entries.iter().any(|entry| {
        entry["applicable"] == true
            && entry["result"]["triggered"] == true
            && entry["result"]["followed"] == true
    }) {
        return Compliance::Followed;
    }
    if entries.iter().any(|entry| entry["applicable"] == true) {
        return Compliance::NotTriggered;
    }
    Compliance::NotApplicable
}

/// Lossless per-verdict counts stored next to the single summary so a violated
/// arm stays visible even when another arm is unknown.
pub(super) fn compliance_detail(entries: &[Value]) -> Value {
    let mut violated = Vec::new();
    let mut unknown = Vec::new();
    let mut followed = Vec::new();
    let mut not_triggered = Vec::new();
    let mut not_applicable = Vec::new();
    for entry in entries {
        let Some(id) = entry["strategyId"].as_str() else {
            continue;
        };
        let status = entry["result"]["status"].as_str();
        if matches!(status, Some("unknown" | "invalid")) {
            unknown.push(json!(id));
        } else if entry["applicable"] != true {
            not_applicable.push(json!(id));
        } else if entry["result"]["triggered"] != true {
            not_triggered.push(json!(id));
        } else if entry["result"]["followed"] == true {
            followed.push(json!(id));
        } else {
            violated.push(json!(id));
        }
    }
    json!({
        "summary": compliance_summary(entries),
        "total": entries.len(),
        "violated": violated,
        "unknown": unknown,
        "followed": followed,
        "notTriggered": not_triggered,
        "notApplicable": not_applicable,
    })
}

fn standing_violation(execution: &Value) -> bool {
    execution["strategyCompliance"]
        .as_array()
        .is_some_and(|items| {
            items.iter().any(|item| {
                item["authority"].as_str() == Some("standing")
                    && item["applicable"].as_bool() == Some(true)
                    && item["result"]["verified"].as_bool() == Some(true)
                    && item["result"]["triggered"].as_bool() == Some(true)
                    && item["result"]["followed"].as_bool() == Some(false)
            })
        })
}

pub(super) fn determine_task_outcome(execution: &Value) -> TaskOutcome {
    match execution["status"].as_str() {
        Some("cancelled") => return TaskOutcome::Cancelled,
        Some("completed") => {}
        _ => return TaskOutcome::Failed,
    }
    if standing_violation(execution) {
        return TaskOutcome::Failed;
    }
    if execution["acceptancePassed"] == true {
        return if crate::engine::verification::has_result_verifier(&execution["acceptanceChecks"]) {
            TaskOutcome::Succeeded
        } else {
            TaskOutcome::AnsweredUnverified
        };
    }
    if execution["acceptanceText"].is_null() {
        return TaskOutcome::AnsweredUnverified;
    }
    TaskOutcome::Failed
}

pub(super) fn record_success_experience(
    state: &AppState,
    record: &Value,
    execution_id: &str,
    task_outcome: TaskOutcome,
) -> Option<Value> {
    if task_outcome != TaskOutcome::Succeeded {
        return None;
    }
    // Deterministic execution is a shortcut, not evidence that the harness
    // learned to solve a new task.
    if record["deterministicShortcut"].is_object() {
        return None;
    }
    let acceptance_text = record["acceptanceText"].as_str()?;
    let final_message_id = record["finalMessageId"].as_str()?;
    // One verified success sample per root task. Nine room speakers reusing
    // the same owner message are not nine independent successes.
    let task_id = record["taskId"].as_str().unwrap_or(execution_id);
    let marker_id = crate::engine::episode::success_marker_id(task_id);
    let marker = json!({
        "id": marker_id,
        "taskId": task_id,
        "executionId": execution_id,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    if state
        .store
        .put_if_absent("success_experience_marker", &marker)
        .is_none()
    {
        return None;
    }
    let scope = crate::engine::memory::MemoryScope::for_session(
        &state.store,
        record["sessionId"].as_str().unwrap_or(""),
        record["agentId"].as_str(),
    );
    let lesson = format!(
        "검증 조건을 실제 도구 결과로 충족함. 결과 요약: {}",
        record["finalAnswer"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect::<String>()
    );
    let tools = record["toolsUsed"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    crate::engine::experience::record_shared(
        state,
        &scope,
        acceptance_text,
        "success",
        &lesson,
        "",
        &[final_message_id.to_string(), execution_id.to_string()],
        &tools,
        &[],
    )
}
