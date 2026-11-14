//! Native, durable owner-reviewed goal recovery.
//! Shared by HTTP and the first-party in-process Rust SDK. A new job is
//! created transactionally, and uncertain external effects require explicit
//! acknowledgement before any further model/tool call.
use crate::state::AppState;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalResumeError {
    Invalid(String),
    Missing(String),
    Conflict(String),
    Storage(String),
}

impl std::fmt::Display for GoalResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Invalid(message) | Self::Missing(message) |
            Self::Conflict(message) | Self::Storage(message) => message,
        };
        f.write_str(message)
    }
}
impl std::error::Error for GoalResumeError {}

/// An interrupted external operation is never blindly replayed. Only an
/// owner-reviewed environment reconciliation can activate a new job.
pub fn resume_goal(
    state: &AppState,
    id: &str,
    body: &Value,
) -> Result<Value, GoalResumeError> {
    use GoalResumeError::{Conflict, Invalid, Missing, Storage};
    if !body.is_object() {
        return Err(Invalid("복구 설정 객체가 필요합니다.".into()));
    }
    let lock_id = format!("goal-resume:{id}");
    if !state.begin_session(&lock_id) {
        return Err(Conflict("목표 복구가 진행 중입니다.".into()));
    }
    let _guard = state.session_guard(&lock_id);
    let goal = state.store.get("goal", id)
        .ok_or_else(|| Missing("목표를 찾을 수 없습니다.".into()))?;
    // A queued goal cancelled before its first worker turn has no worktree
    // yet: allow the next worker to create it. Once a worktree was created,
    // its original identity must still validate before recovery.
    if goal["isolateWorktree"] == true
        && goal["gitWorktree"].is_object()
        && crate::engine::goal_workspace::workspace_for_goal(state, &goal).is_err() {
        return Err(Conflict(
            "이 목표의 Git 작업공간이 제거되거나 변경되어 자동 재개할 수 없습니다.".into()
        ));
    }
    if !matches!(goal["status"].as_str(), Some("needs_review" | "failed" | "cancelled")) {
        return Err(Conflict("검토·실패·중단된 목표만 이어서 실행할 수 있습니다.".into()));
    }
    let previous = goal["jobId"].as_str().and_then(|key| state.store.get("job", key))
        .or_else(|| {
            let mut jobs = state.store.all("job").into_iter().filter(|job|
                job["kind"] == "goal" && job["input"]["goalId"] == id
            ).collect::<Vec<_>>();
            jobs.sort_by(|a,b| b["queuedAt"].as_str().cmp(&a["queuedAt"].as_str()));
            jobs.into_iter().next()
        }).ok_or_else(|| Conflict("이전 실행의 체크포인트가 없습니다.".into()))?;
    if matches!(previous["status"].as_str(), Some("running" | "queued")) {
        return Err(Conflict("이전 목표 작업이 아직 실행 중입니다.".into()));
    }
    let old = &previous["goalState"];
    if goal["isolateWorktree"] == true
        && !goal["gitWorktree"].is_object()
        && (old["inFlight"] == true || old["reviewPending"] == true) {
        return Err(Conflict(
            "미완료 도구 실행의 Git 작업공간 기록이 없습니다. 원본 작업공간을 수동 검토하세요.".into()
        ));
    }
    let session = goal["sessionId"].as_str().unwrap_or("");
    let persisted_unknown = if session.is_empty() { false } else {
        match state.store.session_has_uncertain_effects(session) {
            Ok(value) => value,
            Err(cause) => {
                state.store.audit("goal.resume_inspection_failed",
                    json!({"goalId":id,"reason":cause}));
                return Err(Storage(
                    "도구 실행 상태를 조회할 수 없습니다. 복구 작업을 중단했습니다.".into()
                ));
            }
        }
    };
    let unknown = old["inFlight"] == true || old["reviewPending"] == true
        || persisted_unknown;
    let note = body["resolutionNote"].as_str().unwrap_or("").trim();
    if unknown && (body["acknowledgeExternalEffects"] != true || note.chars().count() < 10) {
        return Err(Invalid(
            "외부 작업 결과가 불확실합니다. 실행 기록을 직접 확인한 뒤 승인과 10자 이상의 처리 기록을 제출하세요.".into()
        ));
    }
    if note.chars().count() > 2000 {
        return Err(Invalid("복구 기록은 2,000자 이하입니다.".into()));
    }
    let prior_step = old["nextIteration"].as_u64()
        .unwrap_or_else(|| goal["iterations"].as_u64().unwrap_or(0));
    let next_step = prior_step.saturating_add(u64::from(old["inFlight"] == true));
    let configured = goal["maxIterations"].as_u64().unwrap_or(16);
    // Do not silently coerce negative, fractional or string limits to defaults.
    let max = match body.get("maxIterations") {
        None | Some(Value::Null) => configured,
        Some(value) => value.as_u64().ok_or_else(||
            Invalid("총 반복 한도는 양의 정수여야 합니다.".into()))?,
    };
    if max == 0 || max > 64 || max <= next_step || max < configured {
        return Err(Invalid(
            "총 반복 한도를 남은 실행 단계보다 크게 지정하세요(최대 64).".into()
        ));
    }
    let previous_cap = goal["maxExecutorTokens"].as_u64().unwrap_or(80_000);
    let new_cap = match body.get("maxExecutorTokens") {
        None | Some(Value::Null) => previous_cap,
        Some(value) => value.as_u64().ok_or_else(||
            Invalid("모델 토큰 한도는 양의 정수여야 합니다.".into()))?,
    };
    if new_cap < previous_cap || new_cap > 2_000_000 || new_cap < 5_000 {
        return Err(Invalid(
            "재개 시 실행 모델 토큰 한도는 기존 한도 이상 2,000,000 이하로 지정하세요.".into()
        ));
    }
    let mut transcript = old["transcript"].as_str().unwrap_or("").to_string();
    if !note.is_empty() {
        transcript.push_str("\n[사용자 복구 확인] ");
        transcript.extend(note.chars().take(2000));
    }
    let mut gaps = goal["gaps"].as_array().cloned().unwrap_or_default();
    if unknown {
        gaps.push(json!("이전 도구의 외부 결과는 사용자가 확인했습니다. 이전 단계를 자동 재실행하지 않고 현 상태를 다시 검사하세요."));
    }
    let mut resume = old.clone();
    if !resume.is_object() { resume = json!({}); }
    resume["sessionId"] = goal["sessionId"].clone();
    resume["nextIteration"] = json!(next_step);
    resume["inFlight"] = json!(false);
    resume["reviewPending"] = json!(false);
    resume["transcript"] = json!(transcript);
    resume["gaps"] = json!(gaps);
    resume["evidence"] = if goal["evidence"].is_array() {goal["evidence"].clone()}
        else if old["evidence"].is_array() {old["evidence"].clone()}
        else {json!([])};
    resume["stalled"] = json!(0);
    resume["replans"] = goal["replans"].clone();
    resume["plan"] = goal["plan"].clone();
    let mut history = goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({
        "previousJobId":previous["id"],
        "previousStatus":previous["status"],
        "unknownEffectsAcknowledged":unknown,
        "resolutionNote":note,
        "nextIteration":next_step,
        "at":chrono::Utc::now().to_rfc3339()
    }));
    let job_id = uuid::Uuid::new_v4().to_string();
    let job = json!({
        "id":job_id,"kind":"goal","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"goalId":id},"attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    let patch = json!({
        "status":"queued","phase":"queued","resumeState":resume,"resumeHistory":history,
        "maxIterations":max,"maxExecutorTokens":new_cap,"error":Value::Null,"gaps":gaps,
        "resumedAt":chrono::Utc::now().to_rfc3339()
    });
    let reconciliation = if persisted_unknown && unknown {
        Some((session,note))
    } else { None };
    let saved = match state.store.resume_goal_atomic(
        id, goal["status"].as_str().unwrap_or(""),
        goal["jobId"].as_str().unwrap_or(""),
        &patch,&job,reconciliation
    ) {
        Ok(Some(saved)) => saved,
        Ok(None) => return Err(Conflict("목표 상태 또는 작업 버전이 변경되었습니다.".into())),
        Err(cause) => {
            state.store.audit("goal.resume_transaction_failed",
                json!({"goalId":id,"reason":cause}));
            return Err(Storage(
                "목표 재개와 작업 등록을 함께 저장하지 못했습니다. 다시 시도할 수 있습니다.".into()
            ));
        }
    };
    state.bus.record("goal",&saved);
    let saved_job = state.store.get("job",&job_id).unwrap_or(job);
    state.bus.record("job",&saved_job);
    if !session.is_empty() {state.clear_cancel(session);}
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    state.store.audit("goal.resumed",json!({
        "goalId":id,"jobId":job_id,"previousJobId":previous["id"],"explicitReview":unknown
    }));
    Ok(json!({
        "goalId":id,"jobId":job_id,"status":"queued",
        "nextIteration":next_step,"reviewAcknowledged":unknown
    }))
}
