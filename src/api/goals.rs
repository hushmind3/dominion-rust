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
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");};
    if matches!(goal["status"].as_str(),Some("queued"|"running")){
        return error(StatusCode::CONFLICT,"실행 중인 목표는 먼저 중단하세요.");
    }
    if goal["isolateWorktree"]==true && goal["gitWorktree"].is_object()
        && goal["gitWorktree"]["discarded"]!=true{
        return error(StatusCode::CONFLICT,"격리 작업공간의 변경사항을 확인하고 별도로 폐기한 뒤 기록을 삭제하세요.");
    }
    state.store.remove("goal", &id);
    state.bus.emit(json!({"type":"record","kind":"goal","removed":id}));
    Json(json!({"ok":true})).into_response()
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
    if body.get("isolateWorktree").is_some_and(|value|!value.is_boolean()){
        return error(StatusCode::BAD_REQUEST,"Git 작업공간 격리 옵션은 true 또는 false여야 합니다.");
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
    for (key,min,max) in [("maxIterations",1,64),("maxReplans",0,16),("replanAfter",1,8),("maxExecutorTokens",5000,2000000)] {if body.get(key).is_some_and(|v|v.as_u64().is_none_or(|n|n<min||n>max)){return error(StatusCode::BAD_REQUEST,"반복·재계획 한도가 유효하지 않습니다.");}}
    // Persist the goal and its runnable job together. Inserting the goal
    // first and enqueuing later could leave a permanently queued ghost goal
    // when SQLite rejects the job write (disk full / corrupted database).
    // Dispatch events only after the transaction commits successfully.
    let goal_id=uuid::Uuid::new_v4().to_string();
    let job_id=uuid::Uuid::new_v4().to_string();
    let queued_at=chrono::Utc::now().to_rfc3339_opts(
        chrono::SecondsFormat::Micros,true);
    let goal=json!({
        "id":goal_id,"jobId":job_id,
        "taskId":uuid::Uuid::new_v4().to_string(),
        "goal":text,"successCriteria":criteria,"executor":executor,
        "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
        "planner":body.get("planner").cloned().unwrap_or(json!({})),
        "maxReplans":body["maxReplans"].as_u64().unwrap_or(6),
        "replanAfter":body["replanAfter"].as_u64().unwrap_or(2),
        "replans":0,"planHistory":[],"reviewHistory":[],
        "status":"queued","iterations":0,
        "maxIterations":max_iterations,
        "maxExecutorTokens":body["maxExecutorTokens"].as_u64().unwrap_or(80000),
        "isolateWorktree":body["isolateWorktree"]==true,
        "createdAt":chrono::Utc::now().to_rfc3339()
    });
    let job=json!({
        "id":job_id,"kind":"goal","status":"queued",
        "queuedAt":queued_at,"input":{"goalId":goal_id},
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]
    });
    if let Err(cause)=state.store.transaction(&[
        ("goal".to_string(),goal.clone()),
        ("job".to_string(),job.clone())
    ]){
        state.store.audit("goal.enqueue_failed",json!({
            "error":cause,"goalId":goal_id,"jobId":job_id
        }));
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표와 실행 작업을 함께 저장하지 못했습니다. 재시도할 수 있습니다.");
    }
    let record=state.store.get("goal",&goal_id).unwrap_or(goal);
    let job_record=state.store.get("job",&job_id).unwrap_or(job);
    state.bus.record("goal",&record);
    state.bus.record("job",&job_record);
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    Json(record).into_response()
}

/// Resume a completed checkpoint only after owner review of unknown effects.
/// A new job means the prior execution remains immutable and auditable.
pub async fn resume_goal(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if !body.is_object(){return error(StatusCode::BAD_REQUEST,"복구 설정 객체가 필요합니다.");}
    let lock_id=format!("goal-resume:{id}");
    if !state.begin_session(&lock_id){return error(StatusCode::CONFLICT,"목표 복구가 진행 중입니다.");}
    let _guard=state.session_guard(&lock_id);
    let Some(goal)=state.store.get("goal",&id) else{return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");};
    if goal["isolateWorktree"]==true
        && engine::goal_workspace::workspace_for_goal(&state,&goal).is_err(){
        return error(StatusCode::CONFLICT,"이 목표의 Git 작업공간이 제거되거나 변경되어 자동 재개할 수 없습니다.");
    }
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed"|"cancelled")){
        return error(StatusCode::CONFLICT,"검토·실패·중단된 목표만 이어서 실행할 수 있습니다.");
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
    let persisted_unknown=if session.is_empty(){false}else{
        match state.store.session_has_uncertain_effects(session){
            Ok(value)=>value,
            Err(cause)=>{
                state.store.audit("goal.resume_inspection_failed",
                    json!({"goalId":id,"reason":cause}));
                return error(StatusCode::INTERNAL_SERVER_ERROR,
                    "도구 실행 상태를 조회할 수 없습니다. 복구 작업을 중단했습니다.");
            }
        }
    };
    let unknown=old["inFlight"]==true||old["reviewPending"]==true
        ||persisted_unknown;
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
    let previous_cap=goal["maxExecutorTokens"].as_u64().unwrap_or(80000);
    let new_cap=body["maxExecutorTokens"].as_u64().unwrap_or(previous_cap);
    if new_cap<previous_cap || new_cap>2000000 || new_cap<5000{
        return error(StatusCode::BAD_REQUEST,"재개 시 실행 모델 토큰 한도는 기존 한도 이상 2,000,000 이하로 지정하세요.");
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
    resume["evidence"]=if goal["evidence"].is_array(){goal["evidence"].clone()}
        else if old["evidence"].is_array(){old["evidence"].clone()}
        else{json!([])};
    resume["stalled"]=json!(0);
    resume["replans"]=goal["replans"].clone();
    resume["plan"]=goal["plan"].clone();
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"previousJobId":previous["id"],"previousStatus":previous["status"],
        "unknownEffectsAcknowledged":unknown,"resolutionNote":note,"nextIteration":next_step,
        "at":chrono::Utc::now().to_rfc3339()}));
    let job_id=uuid::Uuid::new_v4().to_string();
    let job=json!({"id":job_id,"kind":"goal","status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "input":{"goalId":id},"attempts":0,"stepIndex":0,
        "stepTotal":0,"events":[]});
    let patch=json!({
        "status":"queued","phase":"queued","resumeState":resume,"resumeHistory":history,
        "maxIterations":max,"maxExecutorTokens":new_cap,"error":Value::Null,"gaps":gaps,
        "resumedAt":chrono::Utc::now().to_rfc3339()
    });
    let reconciliation=if persisted_unknown && unknown{
        Some((session,note))
    }else{None};
    let saved=match state.store.resume_goal_atomic(
        &id,goal["status"].as_str().unwrap_or(""),
        goal["jobId"].as_str().unwrap_or(""),&patch,&job,reconciliation
    ){
        Ok(Some(saved))=>saved,
        Ok(None)=>return error(StatusCode::CONFLICT,"목표 상태 또는 작업 버전이 변경되었습니다."),
        Err(cause)=>{
            state.store.audit("goal.resume_transaction_failed",
                json!({"goalId":id,"reason":cause}));
            return error(StatusCode::INTERNAL_SERVER_ERROR,
                "목표 재개와 작업 등록을 함께 저장하지 못했습니다. 다시 시도할 수 있습니다.");
        }
    };
    state.bus.record("goal",&saved);
    let saved_job=state.store.get("job",&job_id).unwrap_or(job);
    state.bus.record("job",&saved_job);
    if !session.is_empty(){state.clear_cancel(session);}
    state.store.audit("job.queued",json!({"kind":"goal","jobId":job_id}));
    state.store.audit("goal.resumed",json!({"goalId":id,"jobId":job_id,
        "previousJobId":previous["id"],"explicitReview":unknown}));
    (StatusCode::ACCEPTED,Json(json!({"goalId":id,"jobId":job_id,
        "status":"queued","nextIteration":next_step,"reviewAcknowledged":unknown}))).into_response()
}

pub async fn goal_diff(State(state):State<AppState>,Path(id):Path<String>)->Response{
    if state.store.get("goal",&id).is_none(){
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    }
    match engine::goals::diff_for_goal(&state,&id).await{
        Ok(result)=>Json(result).into_response(),
        Err(message)=>error(StatusCode::BAD_REQUEST,&message),
    }
}

/// Owner-reviewed promotion requires the SHA-256 of exactly the patch shown
/// by /diff. No implicit Git commit or merge is performed.
pub async fn apply_workspace(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if body.as_object().is_none_or(|object|
        object.len()!=1 || !object.contains_key("expectedPatchSha256")){
        return error(StatusCode::BAD_REQUEST,"검토한 패치 SHA-256만 제출하세요.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"패치 SHA-256이 문자열이 아닙니다.");
    };
    match engine::goal_workspace::apply_reviewed(&state,&id,digest).await{
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message)
    }
}

/// Destructive operation is explicitly opt-in, and never touches the source
/// checkout. The owner must inspect the diff before discarding local edits.
pub async fn discard_workspace(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if body["confirmation"]!="DISCARD_GOAL_WORKTREE"{
        return error(StatusCode::BAD_REQUEST,"격리 작업공간 폐기 확인 문자열이 필요합니다.");
    }
    match engine::goal_workspace::discard(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message)
    }
}

/// Running goal owner steering is queued until the next safe executor turn.
/// This does not alter the frozen acceptance checks or currently running tools.
pub async fn steer_goal(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let Some(expected)=body["expectedRevision"].as_u64() else{
        return error(StatusCode::BAD_REQUEST,"현재 계획 버전이 필요합니다.");
    };
    let Some(plan)=body["plan"].as_str() else{
        return error(StatusCode::BAD_REQUEST,"수정 계획이 필요합니다.");
    };
    use crate::engine::goal_steering::SteeringError;
    match crate::engine::goal_steering::submit(&state,&id,expected,plan){
        Ok(value)=>(StatusCode::ACCEPTED,Json(value)).into_response(),
        Err(SteeringError::Missing)=>error(StatusCode::NOT_FOUND,"목표가 없습니다."),
        Err(SteeringError::Invalid)=>error(StatusCode::BAD_REQUEST,"계획은 1~6,000자여야 합니다."),
        Err(SteeringError::Immutable)=>error(StatusCode::CONFLICT,"실행 중인 목표만 조향할 수 있습니다."),
        Err(SteeringError::Conflict)=>error(StatusCode::CONFLICT,"목표 계획이 변경되었습니다. 새로고침한 뒤 다시 수정하세요.")
    }
}

/// Owner edits only a paused/queued plan; a running model must never silently
/// ignore edits made to its captured in-memory plan.
pub async fn edit_plan(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    let lock_id=format!("goal-resume:{id}");
    if !state.begin_session(&lock_id){return error(StatusCode::CONFLICT,"목표 상태를 변경 중입니다.");}
    let _guard=state.session_guard(&lock_id);
    let Some(goal)=state.store.get("goal",&id)else{return error(StatusCode::NOT_FOUND,"목표가 없습니다.");};
    if !matches!(goal["status"].as_str(),Some("queued"|"needs_review"|"failed"|"cancelled")){
        return error(StatusCode::CONFLICT,"실행 중이거나 완료된 목표의 계획은 직접 변경할 수 없습니다.");
    }
    let Some(plan)=body["plan"].as_str().map(str::trim).filter(|s|!s.is_empty()&&s.chars().count()<=8000)
        else{return error(StatusCode::BAD_REQUEST,"계획은 1~8,000자여야 합니다.");};
    let mut history=goal["planHistory"].as_array().cloned().unwrap_or_default();
    let revision=history.iter().filter_map(|v|v["version"].as_u64()).max().unwrap_or(0)+1;
    history.push(json!({"version":revision,"plan":plan,"reason":"owner_revision",
        "at":chrono::Utc::now().to_rfc3339()}));
    if history.len()>40{history.drain(..history.len()-40);}
    let text=goal["goal"].as_str().unwrap_or("");
    let updated=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"plan":plan,"planHistory":history,
        "planScore":crate::engine::plan_quality::score(plan,text)}));
    let Some(updated)=updated else{return error(StatusCode::CONFLICT,"목표 상태가 변경되었습니다.");};
    state.bus.record("goal",&updated);
    state.store.audit("goal.plan_owner_revision",json!({"goalId":id,"revision":revision}));
    Json(updated).into_response()
}

#[cfg(test)]
#[path = "goals/tests.rs"]
mod tests;

/// Pending approval items for the owner goal and its bounded descendant
/// sessions. Never reveal other chat/room sessions through the goal view.
fn belongs_to_goal(state:&AppState,child:&str,root:&str)->bool{
    let mut current=child.to_string();
    let mut visited=std::collections::HashSet::new();
    for _ in 0..6{
        if current==root{return true;}
        if !visited.insert(current.clone()){return false;}
        let Some(session)=state.store.get("session",&current)else{return false;};
        let Some(parent)=session["parentSessionId"].as_str() else{return false;};
        current=parent.to_string();
    }
    false
}
pub async fn goal_approvals(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    let Some(root)=goal["sessionId"].as_str().filter(|s|!s.is_empty())else{
        return Json(json!([])).into_response();
    };
    let pending=state.store.all("approval").into_iter()
        .filter(|record|matches!(record["status"].as_str(),Some("pending"|"interrupted")))
        .filter(|record|record["sessionId"].as_str().is_some_and(|id|
            belongs_to_goal(&state,id,root)))
        .map(|approval|json!({
            "id":approval["id"],"tool":approval["tool"],"args":approval["args"],
            "status":approval["status"],"sessionId":approval["sessionId"]
        }))
        .take(32).collect::<Vec<_>>();
    Json(json!(pending)).into_response()
}

/// Real delegated jobs initiated through the existing delegate_task tool.
/// This is a read-only team monitor; the parent remains the only authority
/// for deciding whether a subtask result satisfies the actual goal contract.
pub async fn goal_subtasks(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(goal)=state.store.get("goal",&id) else{
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    let Some(root)=goal["sessionId"].as_str().filter(|s|!s.is_empty())else{
        return Json(json!([])).into_response();
    };
    let mut jobs=state.store.all("job").into_iter()
        .filter(|job|job["kind"]=="prompt" && job["input"]["delegation"]==true)
        .filter(|job|job["input"]["parentSessionId"].as_str().is_some_and(|parent|
            belongs_to_goal(&state,parent,root)))
        .collect::<Vec<_>>();
    jobs.sort_by(|a,b|b["queuedAt"].as_str().cmp(&a["queuedAt"].as_str()));
    let report=jobs.into_iter().take(32).map(|job|json!({
        "id":job["id"],"status":job["status"],"agentId":job["input"]["agentId"],
        "sessionId":job["input"]["sessionId"],"parentSessionId":job["input"]["parentSessionId"],
        "startedAt":job["startedAt"],"queuedAt":job["queuedAt"],
        "finishedAt":job["finishedAt"],"error":job["error"],
        "hasResult":job["result"].is_object(),
        "isVerifiedGoalCompletion":false
    })).collect::<Vec<_>>();
    Json(json!(report)).into_response()
}
