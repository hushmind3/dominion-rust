//! Atomic goal+job creation and input validation.
use super::*;

pub async fn prepare_goal(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    match engine::goals::admission::prepare(&state,&body){
        Ok(advice)=>Json(advice).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,&message),
    }
}

pub async fn create_goal(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() || text.chars().count()>12_000 {
        return error(StatusCode::BAD_REQUEST, "목표를 1~12000자로 입력하세요.");
    }
    if body.get("autoRoute").is_some_and(|value|!value.is_boolean()){
        return error(StatusCode::BAD_REQUEST,"자동 모델 선택은 true/false여야 합니다.");
    }
    let criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if let Err(message) = engine::goals::validate_criteria(&criteria) {
        return error(StatusCode::BAD_REQUEST, &message);
    }
    let mut executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    // An explicitly selected executor always wins over automatic selection.
    // Otherwise resolve a provider/agent before recording any queued job.
    let route=if body["autoRoute"]==true &&
        executor.as_object().is_some_and(|map|map.is_empty()){
        match engine::goals::admission::prepare(&state,&body){
            Ok(advice) if advice["ready"]==true => {
                executor=advice["executor"].clone();
                advice["selection"].clone()
            },
            Ok(advice)=>return error(StatusCode::BAD_REQUEST,
                advice["message"].as_str().unwrap_or("사용 가능한 모델이 없습니다.")),
            Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
        }
    }else{Value::Null};
    let review_policy=match crate::engine::goals::validate_review_policy(body.get("reviewPolicy")) {
        Ok(policy)=>policy,
        Err(message)=>return error(StatusCode::BAD_REQUEST,&message),
    };
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
        "routeSelection":route,
        "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
        "planner":body.get("planner").cloned().unwrap_or(json!({})),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "reviewPolicy":review_policy,
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
