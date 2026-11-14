//! Shared, transactional autonomous-goal admission for REST and native Rust SDK.
//! Neither adapter is allowed to invent or separately persist an agent run.
use crate::state::AppState;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEnqueueError {
    Invalid(String),
    Storage(String),
}

impl std::fmt::Display for GoalEnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Storage(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for GoalEnqueueError {}

/// One durable, atomic owner-goal admission path shared by all SDK transports.
/// Validation occurs before either record is created. A persisted goal always
/// has a corresponding job even across storage errors or process restarts.
pub fn enqueue_goal(state: &AppState, body: &Value) -> Result<Value, GoalEnqueueError> {
    let invalid = |message: &str| GoalEnqueueError::Invalid(message.to_string());
    let text = body["goal"].as_str().unwrap_or("").trim();
    if text.is_empty() {
        return Err(invalid("목표 내용이 필요합니다."));
    }
    if body.get("isolateWorktree").is_some_and(|value| !value.is_boolean()) {
        return Err(invalid("Git 작업공간 격리 옵션은 true 또는 false여야 합니다."));
    }
    let criteria = body.get("successCriteria").cloned().unwrap_or_else(|| json!([]));
    super::validate_criteria(&criteria).map_err(GoalEnqueueError::Invalid)?;
    for key in ["executor", "reviewer", "planner"] {
        if body.get(key).is_some_and(|value| !value.is_object()) {
            return Err(invalid("실행자·검토자·계획자는 객체로 지정하세요."));
        }
    }
    for (key, min, max) in [
        ("maxIterations", 1, 64),
        ("maxReplans", 0, 16),
        ("replanAfter", 1, 8),
        ("maxExecutorTokens", 5000, 2_000_000),
    ] {
        if body.get(key).is_some_and(|value| value.as_u64().is_none_or(|number| number < min || number > max)) {
            return Err(invalid("반복·재계획 한도가 유효하지 않습니다."));
        }
    }
    let goal_id = uuid::Uuid::new_v4().to_string();
    let job_id = uuid::Uuid::new_v4().to_string();
    let queued_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    let goal = json!({
        "id":goal_id, "jobId":job_id,
        "taskId":uuid::Uuid::new_v4().to_string(),
        "goal":text, "successCriteria":criteria,
        "executor":body.get("executor").cloned().unwrap_or_else(||json!({})),
        "reviewer":body.get("reviewer").cloned().unwrap_or_else(||json!({})),
        "planner":body.get("planner").cloned().unwrap_or_else(||json!({})),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "replans":0, "planHistory":[], "reviewHistory":[],
        "status":"queued", "iterations":0,
        "maxIterations":body["maxIterations"].as_u64().unwrap_or(16),
        "maxExecutorTokens":body["maxExecutorTokens"].as_u64().unwrap_or(80_000),
        "isolateWorktree":body["isolateWorktree"]==true,
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    let job = json!({
        "id":job_id, "kind":"goal", "status":"queued",
        "queuedAt":queued_at, "input":{"goalId":goal_id},
        "attempts":0, "stepIndex":0, "stepTotal":0, "events":[]
    });
    if let Err(cause) = state.store.transaction(&[
        ("goal".to_string(), goal.clone()),
        ("job".to_string(), job.clone()),
    ]) {
        state.store.audit("goal.enqueue_failed",json!({
            "error":cause, "goalId":goal_id, "jobId":job_id
        }));
        return Err(GoalEnqueueError::Storage(
            "목표와 실행 작업을 함께 저장하지 못했습니다. 재시도할 수 있습니다.".into()
        ));
    }
    let record = state.store.get("goal", &goal_id).unwrap_or(goal);
    let job_record = state.store.get("job", &job_id).unwrap_or(job);
    state.bus.record("goal", &record);
    state.bus.record("job", &job_record);
    state.store.audit("job.queued", json!({"kind":"goal", "jobId":job_id}));
    Ok(record)
}
