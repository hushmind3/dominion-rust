//! Status-checked owner review and atomic goal resume API.
use super::*;

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

