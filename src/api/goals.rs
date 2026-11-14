use crate::engine;
use crate::state::AppState;
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use super::response::error;

pub async fn list_goals(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("goal")))
}

pub async fn get_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("goal", &id) {
        Some(goal) => Json(goal).into_response(),
        None => error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다."),
    }
}

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return error(StatusCode::CONFLICT,"실행 중인 목표는 먼저 중단하세요.");
    }
    if goal["gitWorktree"].is_object() && goal["gitWorktree"]["rolledBack"]!=true
        && goal["gitWorktree"]["applied"]!=true {
        return error(StatusCode::CONFLICT,
            "격리 작업공간이 남아 있습니다. 먼저 변경을 반영하거나 폐기하세요.");
    }
    state.store.remove("goal",&id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
}

/// Owner changes are accepted only before the first agent/tool turn.
pub async fn edit_plan(
    State(state):State<AppState>, Path(id):Path<String>, Json(body):Json<Value>,
)->Response {
    let Some(plan)=body["plan"].as_str().map(str::trim)
        .filter(|value|!value.is_empty()&&value.chars().count()<=6000) else {
        return error(StatusCode::BAD_REQUEST,"계획은 1~6000자여야 합니다.");
    };
    if body.as_object().is_none_or(|map|map.len()!=1) {
        return error(StatusCode::BAD_REQUEST,"plan만 변경할 수 있습니다.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if goal["status"]!="queued" {
        return error(StatusCode::CONFLICT,"실행 시작 후에는 계획이 자동 검증·재계획으로 관리됩니다.");
    }
    match state.store.patch_if("goal",&id,"status",&json!("queued"),
        &json!({"plan":plan,"planHistory":[],"planEditedAt":chrono::Utc::now().to_rfc3339()})) {
        Some(saved)=>{state.bus.record("goal",&saved);Json(saved).into_response()},
        None=>error(StatusCode::CONFLICT,"목표 실행 상태가 변경됐습니다.")
    }
}

/// Plan steering is versioned and takes effect only at the next turn
/// boundary. The initial success contract, tool rights and reviewer remain fixed.
pub async fn steer_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    if body.as_object().is_none_or(|fields|fields.len()!=2) {
        return error(StatusCode::BAD_REQUEST,"plan과 expectedRevision만 제공하세요.");
    }
    let Some(revision)=body["expectedRevision"].as_u64() else{
        return error(StatusCode::BAD_REQUEST,"현재 계획 revision이 필요합니다.");
    };
    let Some(plan)=body["plan"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"새 계획이 필요합니다.");
    };
    use crate::engine::goal_steering::{submit,SteeringError};
    match submit(&state,&id,revision,plan){
        Ok(saved)=>Json(saved).into_response(),
        Err(SteeringError::Missing)=>error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다."),
        Err(SteeringError::Immutable)=>error(StatusCode::CONFLICT,"종료된 목표는 계획을 변경할 수 없습니다."),
        Err(SteeringError::Conflict)=>error(StatusCode::CONFLICT,"다른 계획 변경이 먼저 저장됐습니다. 최신 revision으로 다시 확인하세요."),
        Err(SteeringError::Invalid)=>error(StatusCode::BAD_REQUEST,"계획은 1~6000자이고 NULL 문자를 포함할 수 없습니다."),
    }
}

/// Cancels the durable root and its queued/running job, not merely its UI.
pub async fn cancel_goal(State(state):State<AppState>,Path(id):Path<String>)->Response {
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return error(StatusCode::CONFLICT,"이 목표는 이미 종료됐습니다.");
    }
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"cancelled","phase":"cancelled",
            "finishedAt":chrono::Utc::now().to_rfc3339()})) else {
        return error(StatusCode::CONFLICT,"목표 상태가 변경됐습니다.");
    };
    state.bus.record("goal",&saved);
    if let Some(session)=saved["sessionId"].as_str() {state.cancel_session(session);}
    let job=state.store.all("job").into_iter().find(|job|
        job["kind"]=="goal" && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running")));
    if let Some(job)=job {
        let job_id=job["id"].as_str().unwrap_or("");
        if let Some(cancelled)=state.store.patch_if("job",job_id,"status",&job["status"],
            &json!({"status":"cancelled","cancelRequested":true,
                "finishedAt":chrono::Utc::now().to_rfc3339()})) {
            state.bus.record("job",&cancelled);
        }
    }
    Json(saved).into_response()
}

/// Human-reviewed continuation. Never replays the prior job's uncertain
/// external operations: a new durable job starts with the last checkpointed
/// evidence and must re-inspect external state before any new write.
pub async fn resume_goal(
    State(state):State<AppState>, Path(id):Path<String>, Json(body):Json<Value>,
)->Response {
    if body["acknowledgeUnknownEffects"]!=true
        || body.as_object().is_none_or(|object|object.len()!=1) {
        return error(StatusCode::BAD_REQUEST,
            "이전 외부 작업 상태를 확인한 경우 acknowledgeUnknownEffects=true를 제출하세요.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")) {
        return error(StatusCode::CONFLICT,"검토 대기 또는 실패한 목표만 이어갈 수 있습니다.");
    }
    if state.store.all("job").iter().any(|job|job["kind"]=="goal"
        && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running"))) {
        return error(StatusCode::CONFLICT,"이전 목표 작업이 아직 실행 중입니다.");
    }
    let iterations=goal["iterations"].as_u64().unwrap_or(0);
    let old_max=goal["maxIterations"].as_u64().unwrap_or(16);
    let next_max=if old_max<=iterations {old_max.saturating_add(8).min(64)}else{old_max};
    if next_max<=iterations {
        return error(StatusCode::CONFLICT,"목표 반복 상한(64회)에 도달했습니다.");
    }
    let next=goal["resumeCount"].as_u64().unwrap_or(0)+1;
    if next>12{return error(StatusCode::CONFLICT,"수동 이어가기 한도에 도달했습니다.");}
    let job_id=format!("goal-job-{id}-resume-{next}");
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"jobId":job_id,"previousJobId":goal["jobId"],
        "previousStatus":goal["status"],"startIteration":iterations,
        "acknowledgedBy":"owner","at":chrono::Utc::now().to_rfc3339()}));
    // The resume decision and a runnable job are a single SQLite transaction:
    // a crash or failed insert cannot leave a queued goal without its job.
    // Comparing the previous goal status inside the transaction also rejects
    // concurrent resume requests. No events are emitted on rollback.
    let mut saved=goal.clone();
    saved["status"]=json!("queued");
    saved["phase"]=json!("resume");
    saved["jobId"]=json!(job_id);
    saved["resumeCount"]=json!(next);
    saved["maxIterations"]=json!(next_max);
    saved["resumeHistory"]=json!(history);
    saved["finishedAt"]=Value::Null;
    saved["finalReport"]=Value::Null;
    let job=json!({
        "id":job_id,"kind":"goal","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"goalId":id,"resumeAfterReview":true},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    match state.store.transaction_if_status("goal",&id,
        goal["status"].as_str().unwrap_or(""),&[
            ("goal".to_string(),saved.clone()),("job".to_string(),job.clone())
        ]) {
        Ok(true)=>{},
        Ok(false)=>return error(StatusCode::CONFLICT,"목표 상태가 변경됐습니다."),
        Err(reason)=>{
            state.store.audit("goal.resume_enqueue_failed",json!({"goalId":id,"jobId":job_id,"error":reason}));
            return error(StatusCode::INTERNAL_SERVER_ERROR,"이어가기 목표와 작업을 함께 저장하지 못했습니다.");
        }
    }
    if let Some(session)=saved["sessionId"].as_str(){state.clear_cancel(session);}
    state.bus.record("goal",&saved);
    state.bus.record("job",&job);
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    state.store.audit("goal.owner_reviewed_resume",json!({
        "goalId":id,"jobId":job_id,"previousJobId":goal["jobId"],
        "startIteration":iterations,
    }));
    Json(saved).into_response()
}

pub async fn create_goal(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "목표 내용이 필요합니다.");
    }
    let criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Err(message) = engine::goals::validate_criteria(&criteria) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    if body.get("isolateWorktree").is_some_and(|v|!v.is_boolean()) {
        return error(StatusCode::BAD_REQUEST,"Git 격리 옵션은 불리언이어야 합니다.");
    }
    if body.get("tokenBudget").is_some_and(|value|
        value.as_u64().is_none_or(|n|n>2_000_000)) {
        return error(StatusCode::BAD_REQUEST,"목표 토큰 상한은 0~200만으로 입력하세요.");
    }
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(16));

    for key in ["executor","reviewer","planner"] { if body.get(key).is_some_and(|v|!v.is_object()){return error(StatusCode::BAD_REQUEST,"실행자·검토자·계획자는 객체로 지정하세요.");} }
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    // Goal + job must commit together: never expose a queued ghost goal.
    // Use durable deterministic job ID so the UI, audit and worker agree.
    let goal_id=uuid::Uuid::new_v4().to_string();
    let job_id=format!("goal-job-{goal_id}");
    let record=json!({
        "id":goal_id,"jobId":job_id,
        "taskId":uuid::Uuid::new_v4().to_string(),
        "goal":text,"successCriteria":criteria,"executor":executor,
        "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
        "planner":body.get("planner").cloned().unwrap_or(json!({})),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "tokenBudget":body["tokenBudget"].as_u64().unwrap_or(150_000),
        "isolateWorktree":body["isolateWorktree"].as_bool().unwrap_or(false),
        "replans":0,"planHistory":[],"reviewHistory":[],
        "planRevision":0,"appliedPlanRevision":0,"ownerPlan":null,
        "status":"queued","iterations":0,"maxIterations":max_iterations,
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    let job=json!({
        "id":job_id,"kind":"goal","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"goalId":goal_id},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    if let Err(reason)=state.store.transaction(&[
        ("goal".to_string(),record.clone()),("job".to_string(),job.clone())
    ]){
        state.store.audit("goal.enqueue_failed",json!({"goalId":goal_id,"jobId":job_id,"error":reason}));
        return error(StatusCode::INTERNAL_SERVER_ERROR,"목표와 실행 작업을 함께 저장하지 못했습니다.");
    }
    state.bus.record("goal",&record);
    state.bus.record("job",&job);
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    Json(record).into_response()
}

/// Read the isolated diff and a digest over both tracked and untracked
/// file contents. The owner must submit that exact receipt before integration.
pub async fn changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::engine::goal_git::changes(&state,&id).await {
        Ok(result)=>Json(result).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}

/// Remove an unmerged goal checkout, never reset the user's active branch.
pub async fn discard_changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match crate::engine::goal_git::rollback(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}

/// User-approved fast-forward only: no force, reset or unattended source edit.
pub async fn apply_changes(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>
)->Response{
    let Some(digest)=body["reviewDigest"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"변경 미리보기 검토 영수증이 필요합니다.");
    };
    if body["acknowledgeReview"]!=true || body.as_object().is_none_or(|x|x.len()!=2){
        return error(StatusCode::BAD_REQUEST,"변경 검토 후 명시적으로 승인이 필요합니다.");
    }
    match crate::engine::goal_git::apply(&state,&id,digest).await {
        Ok(result)=>Json(result).into_response(),
        Err(reason)=>error(StatusCode::CONFLICT,&reason),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/goals_api_lifecycle_tests.rs"]
mod lifecycle_tests;
