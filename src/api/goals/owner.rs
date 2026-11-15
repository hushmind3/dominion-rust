//! Owner steering, cancellation and human-reviewed goal continuation.
use super::*;

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

/// Owner plan or direct instruction is versioned and applied at the next
/// executor turn. The user can correct an in-flight agent without rewriting a
/// provider transcript or issuing a forged tool/result message.
pub async fn steer_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response{
    if body.as_object().is_none_or(|fields|fields.len()!=2) {
        return error(StatusCode::BAD_REQUEST,"expectedRevision과 plan 또는 instruction을 제공하세요.");
    }
    let Some(revision)=body["expectedRevision"].as_u64() else{
        return error(StatusCode::BAD_REQUEST,"현재 계획 revision이 필요합니다.");
    };
    use crate::engine::goal_steering::{submit,submit_instruction,SteeringError};
    let result=match (body["plan"].as_str(),body["instruction"].as_str()) {
        (Some(plan),None)=>submit(&state,&id,revision,plan),
        (None,Some(instruction))=>submit_instruction(&state,&id,revision,instruction),
        _=>return error(StatusCode::BAD_REQUEST,"plan 또는 instruction 중 하나만 지정하세요."),
    };
    match result {
        Ok(saved)=>Json(saved).into_response(),
        Err(SteeringError::Missing)=>error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다."),
        Err(SteeringError::Immutable)=>error(StatusCode::CONFLICT,"이미 끝난 목표는 새 목표로 시작하거나 재개하세요."),
        Err(SteeringError::Conflict)=>error(StatusCode::CONFLICT,"다른 사용자 변경이 먼저 저장됐습니다. 최신 revision으로 다시 제출하세요."),
        Err(SteeringError::Invalid)=>error(StatusCode::BAD_REQUEST,"지시 또는 계획은 1~6000자이고 NULL 문자를 포함할 수 없습니다."),
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
