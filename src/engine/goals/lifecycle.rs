//! Goal persistence, completion CAS, learning and model role resolution.
use super::*;

pub(super) fn role_model(
    state: &AppState,
    role: &Value,
    default_provider: &str,
    default_model: &str,
) -> (String, String) {
    let agent = role["agentId"]
        .as_str()
        .and_then(|id| state.store.get("agent", id))
        .unwrap_or(json!({}));
    (
        role["providerId"]
            .as_str()
            .or(agent["providerId"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(default_provider)
            .into(),
        role["model"]
            .as_str()
            .or(agent["model"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(default_model)
            .into(),
    )
}


pub(super) fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
pub(super) fn tail(text: &str, limit: usize) -> String {
    text.chars()
        .rev()
        .take(limit)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
pub(super) fn patch_goal(state: &AppState, id: &str, patch: Value) {
    if let Some(record) = state.store.get("goal", id) {
        if record["status"] == "cancelled" && patch["status"] != "cancelled" {
            return;
        }
        if let Some(saved) = state
            .store
            .patch_if("goal", id, "status", &record["status"], &patch)
        {
            state.bus.record("goal", &saved);
        }
    }
}
pub(super) fn finish(
    context: &JobContext,
    id: &str,
    requested_status: &str,
    original_gaps: Vec<String>,
    error: Option<String>,
) -> Value {
    // A concurrent direct owner correction can arrive during verification.
    // The terminal status must therefore commit against the exact owner
    // revision observed by the verifier, not just the old "running" status.
    let state = &context.state;
    let mut latest = state.store.get("goal", id).unwrap_or(Value::Null);
    let mut result: Option<(Value, Vec<String>, String)> = None;
    for _ in 0..16 {
        if !latest.is_object() {
            break;
        }
        let job_cancelled = state.store.get("job", &context.job_id)
            .is_some_and(|job| job["cancelRequested"] == true);
        let already_cancelled = latest["status"] == "cancelled";
        // A different worker/owner may already have sealed this goal. Never
        // turn its terminal decision back into "running" or overwrite it.
        if !matches!(latest["status"].as_str(), Some("queued"|"running")) {
            let status = latest["status"].as_str().unwrap_or("needs_review").to_owned();
            let gaps = latest["gaps"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).map(str::to_string).collect();
            result = Some((latest.clone(), gaps, status));
            break;
        }
        let mut status = if job_cancelled || already_cancelled {
            "cancelled"
        } else {
            requested_status
        };
        let mut gaps = original_gaps.clone();
        // Re-evaluate the filesystem evidence against the same record
        // revision that we are about to compare-and-swap into a terminal state.
        let final_report = report::build(state, &latest);
        if status == "completed" && crate::engine::goal_steering::pending(
            state, id, latest["appliedPlanRevision"].as_u64().unwrap_or(0)
        ).is_some() {
            status = "needs_review";
            gaps.push("사용자의 최신 변경 지시가 아직 실행되지 않았습니다. 기존 계획으로 완료 처리하지 않습니다.".into());
        }
        if status == "completed" && final_report["accepted"] != true {
            status = "needs_review";
            gaps.extend(final_report["gaps"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).map(str::to_string));
        }
        gaps.sort();
        gaps.dedup();
        let patch = json!({
            "status": status,
            "phase": if status == "completed" {"complete"} else {status},
            "gaps": gaps,
            "error": error,
            "finalReport": final_report,
            "finishedAt": chrono::Utc::now().to_rfc3339()
        });
        if let Some(saved) = state.store.patch_if_fields("goal", id,
            &[("status", &latest["status"]),
              ("planRevision", &latest["planRevision"])],
            &patch
        ) {
            state.bus.record("goal", &saved);
            result = Some((saved, gaps, status.to_owned()));
            break;
        }
        // An owner instruction or stop command raced with the verifier.
        // Refresh and retry; never assert completion using stale evidence.
        latest = state.store.get("goal", id).unwrap_or(Value::Null);
    }
    let Some((goal, gaps, status)) = result else {
        state.store.audit("goal.finish_revision_conflict", json!({
            "goalId":id,"jobId":context.job_id
        }));
        return json!({"goalId":id,"status":"needs_review",
            "error":"목표 완료 중 사용자 지시가 변경됐습니다. 최신 상태를 다시 조회하세요."});
    };
    // Runtime task evidence contributes to the next RSI research hypothesis,
    // but is never recorded as an independently evaluated RSI improvement.
    crate::engine::goal_feedback::record(&context.state,&context.job_id,&goal);
    if let Some(task_id) = goal["taskId"].as_str().filter(|id| !id.is_empty()) {
        let final_status = goal["status"].as_str().unwrap_or(&status);
        crate::engine::episode::set_root_outcome(
            &context.state,
            task_id,
            crate::engine::episode::goal_outcome(final_status),
            &gaps,
        );
    }
    context.state.store.audit(
        "goal.finished",
        json!({"goalId":id,"status":status,"iterations":goal.get("iterations"),
            "planRevision":goal["planRevision"],
            "appliedPlanRevision":goal["appliedPlanRevision"]}),
    );
    if goal["status"] == "completed" {
        if let Some(session_id) = goal["sessionId"].as_str().filter(|id| !id.is_empty()) {
            let executor = goal.get("executor").cloned().unwrap_or_else(|| json!({}));
            let agent_id = executor["agentId"].as_str().filter(|id| !id.is_empty());
            let provider_id = executor["providerId"].as_str().unwrap_or("");
            let model = executor["model"].as_str().unwrap_or("");
            let through = context
                .state
                .store
                .messages(session_id)
                .last()
                .and_then(|message| message["id"].as_str().map(str::to_string));
            if let Some(agent_id) = agent_id {
                crate::engine::learning::enqueue_semantic_for_agent_through(
                    &context.state,
                    session_id,
                    Some(agent_id),
                    provider_id,
                    model,
                    through.as_deref(),
                );
            } else {
                crate::engine::learning::enqueue_semantic(
                    &context.state,
                    session_id,
                    provider_id,
                    model,
                );
            }
        }
    }
    context.emit(json!({"type":"goal_finished","goalId":id,"status":goal["status"]}));
    json!({"goalId":id,"status":goal["status"],"sessionId":goal.get("sessionId"),
        "iterations":goal.get("iterations"),"evidence":goal.get("evidence"),
        "gaps":gaps,"finalReport":goal.get("finalReport")})
}
