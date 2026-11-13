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

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("goal", &id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "goal", "removed": id }));
    Json(json!({ "ok": true }))
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
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(16));

    for key in ["executor","reviewer","planner"] { if body.get(key).is_some_and(|v|!v.is_object()){return error(StatusCode::BAD_REQUEST,"실행자·검토자·계획자는 객체로 지정하세요.");} }
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    let Some(record) = state.store.put(
        "goal",
        &json!({
            "taskId": uuid::Uuid::new_v4().to_string(),
            "goal": text,
            "successCriteria": criteria,
            "executor": executor,
            "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
            "planner":body.get("planner").cloned().unwrap_or(json!({})),
            "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
            "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
            "replans":0,"planHistory":[],"reviewHistory":[],
            "status": "queued",
            "iterations": 0,
            "maxIterations": max_iterations,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    ) else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "목표를 저장할 수 없습니다.",
        );
    };
    state.bus.record("goal", &record);
    let goal_id = record
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let job = crate::engine::jobs::enqueue(&state, "goal", json!({ "goalId": goal_id }));
    let job_id = job
        .as_ref()
        .and_then(|job| job.get("id"))
        .cloned()
        .unwrap_or(Value::Null);

    if let Some(id)=job_id.as_str(){
        if let Some(saved)=state.store.patch_if("goal",&goal_id,"id",&json!(goal_id),&json!({"jobId":id})){
            state.bus.record("goal",&saved);
        }
    }
    let mut payload = record.clone();
    payload["jobId"] = job_id;
    Json(payload).into_response()
}

/// Resume a completed checkpoint only after owner review of unknown effects.
/// A new job means the prior execution remains immutable and auditable.
pub async fn resume_goal(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if !body.is_object(){return error(StatusCode::BAD_REQUEST,"복구 설정 객체가 필요합니다.");}
    let lock_id=format!("goal-resume:{id}");
    if !state.begin_session(&lock_id){return error(StatusCode::CONFLICT,"목표 복구가 진행 중입니다.");}
    let _guard=state.session_guard(&lock_id);
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");};
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")){
        return error(StatusCode::CONFLICT,"검토 또는 실패한 목표만 이어서 실행할 수 있습니다.");
    }
    let previous=goal["jobId"].as_str().and_then(|key|state.store.get("job",key))
        .or_else(||{
            let mut jobs=state.store.all("job").into_iter().filter(|job|
                job["kind"]=="goal" && job["input"]["goalId"]==id
            ).collect::<Vec<_>>();
            jobs.sort_by(|a,b|b["queuedAt"].as_str().cmp(&a["queuedAt"].as_str()));
            jobs.into_iter().next()
        });
    let Some(previous)=previous else{return error(StatusCode::CONFLICT,"이전 실행의 체크포인트가 없습니다.");};
    if matches!(previous["status"].as_str(),Some("running"|"queued")){
        return error(StatusCode::CONFLICT,"이전 목표 작업이 아직 실행 중입니다.");
    }
    let old=&previous["goalState"];
    let session=goal["sessionId"].as_str().unwrap_or("");
    let unknown=old["inFlight"]==true || old["reviewPending"]==true
        || (session.len()>0 && state.store.all("execution_operation").iter().any(|op|
            op["outcomeUnknown"]==true && op["runId"].as_str()
                .and_then(|key|state.store.get("execution",key))
                .is_some_and(|execution|execution["sessionId"]==session)));
    let note=body["resolutionNote"].as_str().unwrap_or("").trim();
    if unknown && (body["acknowledgeExternalEffects"]!=true || note.chars().count()<10){
        return error(StatusCode::BAD_REQUEST,"외부 작업 결과가 불확실합니다. 실행 기록을 직접 확인한 뒤 승인과 10자 이상의 처리 기록을 제출하세요.");
    }
    if note.chars().count()>2000{return error(StatusCode::BAD_REQUEST,"복구 기록은 2,000자 이하입니다.");}
    // A side-effect-uncertain execution must not replay the same iteration.
    // The owner reviews the environment, then the following iteration
    // reconciles actual state against the original acceptance checks.
    let prior_step=old["nextIteration"].as_u64().unwrap_or_else(||goal["iterations"].as_u64().unwrap_or(0));
    let next_step=prior_step.saturating_add(u64::from(old["inFlight"]==true));
    let configured=goal["maxIterations"].as_u64().unwrap_or(16);
    let max=body["maxIterations"].as_u64().unwrap_or(configured);
    if max==0 || max>64 || max<=next_step || max<configured{
        return error(StatusCode::BAD_REQUEST,"총 반복 한도를 남은 실행 단계보다 크게 지정하세요(최대 64).");
    }
    let mut transcript=old["transcript"].as_str().unwrap_or("").to_string();
    if !note.is_empty(){
        transcript.push_str("\n[사용자 복구 확인] ");
        transcript.extend(note.chars().take(2000));
    }
    let mut gaps=goal["gaps"].as_array().cloned().unwrap_or_default();
    if unknown{gaps.push(json!("이전 도구의 외부 결과는 사용자가 확인했습니다. 이전 단계를 자동 재실행하지 않고 현 상태를 다시 검사하세요."));}
    let mut resume=old.clone();
    if !resume.is_object(){resume=json!({});}
    resume["sessionId"]=goal["sessionId"].clone();
    resume["nextIteration"]=json!(next_step);
    resume["inFlight"]=json!(false);
    resume["reviewPending"]=json!(false);
    resume["transcript"]=json!(transcript);
    resume["gaps"]=json!(gaps);
    resume["evidence"]=goal["evidence"].clone();
    resume["stalled"]=json!(0);
    resume["replans"]=goal["replans"].clone();
    resume["plan"]=goal["plan"].clone();
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"previousJobId":previous["id"],"previousStatus":previous["status"],
        "unknownEffectsAcknowledged":unknown,"resolutionNote":note,"nextIteration":next_step,
        "at":chrono::Utc::now().to_rfc3339()}));
    let saved=state.store.patch_if("goal",&id,"status",&goal["status"],&json!({
        "status":"queued","phase":"queued","resumeState":resume,"resumeHistory":history,
        "maxIterations":max,"error":Value::Null,"gaps":gaps,
        "resumedAt":chrono::Utc::now().to_rfc3339()
    }));
    let Some(saved)=saved else{return error(StatusCode::CONFLICT,"목표 상태가 변경되었습니다.");};
    state.bus.record("goal",&saved);
    let Some(job)=crate::engine::jobs::enqueue(&state,"goal",json!({"goalId":id})) else{
        let _=state.store.patch_if("goal",&id,"status",&json!("queued"),
            &json!({"status":"needs_review","phase":"needs_review"}));
        return error(StatusCode::INTERNAL_SERVER_ERROR,"재개 작업을 등록하지 못했습니다.");
    };
    let _=state.store.patch_if("goal",&id,"status",&json!("queued"),
        &json!({"jobId":job["id"]}));
    state.store.audit("goal.resumed",json!({"goalId":id,"jobId":job["id"],
        "previousJobId":previous["id"],"explicitReview":unknown}));
    (StatusCode::ACCEPTED,Json(json!({"goalId":id,"jobId":job["id"],
        "status":"queued","nextIteration":next_step,"reviewAcknowledged":unknown}))).into_response()
}
