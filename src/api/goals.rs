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

pub async fn changes(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::changes(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}
pub async fn rollback(State(state):State<AppState>,Path(id):Path<String>)->Response{
    match engine::goal_git::rollback(&state,&id).await{
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}

/// Explicit owner action: apply only the exact SHA-256-reviewed isolated patch.
pub async fn apply(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    if body.as_object().is_none_or(|object|object.len()!=1 || !object.contains_key("expectedPatchSha256")){
        return error(StatusCode::BAD_REQUEST,"expectedPatchSha256 항목만 제출하세요.");
    }
    let Some(digest)=body["expectedPatchSha256"].as_str() else {
        return error(StatusCode::BAD_REQUEST,"검토한 Git 패치 SHA-256을 문자열로 제출하세요.");
    };
    match engine::goal_git::apply_reviewed(&state,&id,digest).await {
        Ok(receipt)=>Json(receipt).into_response(),
        Err(message)=>error(StatusCode::CONFLICT,&message),
    }
}
pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("goal", &id);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "goal", "removed": id }));
    Json(json!({ "ok": true }))
}

/// Owner-approved restart of an interrupted goal. Never replay a pending
/// external effect automatically; a fresh durable job starts after review.
pub async fn resume_goal(
    State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>,
)->Response {
    if body.as_object().is_none_or(|fields|
            fields.len()!=1 || fields.get("acknowledgeUnknownEffects")!=Some(&json!(true))) {
        return error(StatusCode::BAD_REQUEST,
            "이전 실행·외부 변경을 확인한 후 acknowledgeUnknownEffects=true로 재개하세요.");
    }
    let Some(goal)=state.store.get("goal",&id) else {
        return error(StatusCode::NOT_FOUND,"목표를 찾을 수 없습니다.");
    };
    if !matches!(goal["status"].as_str(),Some("needs_review"|"failed")) {
        return error(StatusCode::CONFLICT,"검토 대기 또는 실패한 목표만 재개할 수 있습니다.");
    }
    if state.store.all("job").iter().any(|job|job["kind"]=="goal"
        && job["input"]["goalId"]==id
        && matches!(job["status"].as_str(),Some("queued"|"running"))) {
        return error(StatusCode::CONFLICT,"이전 목표 작업이 아직 실행 중입니다.");
    }
    if goal["isolateWorktree"]==true && goal["gitWorktree"].is_object()
        && (goal["gitWorktree"]["rolledBack"]==true
            || goal["gitWorktree"]["appliedAt"].is_string()) {
        return error(StatusCode::CONFLICT,
            "격리 작업공간이 제거되거나 적용됐습니다. 새 목표를 생성하세요.");
    }
    // The previous durable checkpoint, not the new job ID, determines the
    // next safe turn. If a tool turn was interrupted, skip that exact turn:
    // the owner has inspected unknown effects, but Rust must not replay them.
    let previous_job=goal["jobId"].as_str()
        .and_then(|job_id|state.store.get("job",job_id));
    let (resume_state,start_index,unknown_turn)=
        resume_checkpoint(&goal,previous_job.as_ref());
    let old_max=goal["maxIterations"].as_u64().unwrap_or(16);
    let next_max=if old_max<=start_index {
        start_index.saturating_add(8).min(64)
    } else {old_max};
    if next_max<=start_index {
        return error(StatusCode::CONFLICT,"최대 반복 횟수(64회)에 도달했습니다.");
    }
    let next=goal["resumeCount"].as_u64().unwrap_or(0).saturating_add(1);
    if next>12 {return error(StatusCode::CONFLICT,"수동 재개 한도에 도달했습니다.");}
    let job_id=format!("goal-job-{id}-resume-{next}");
    let mut history=goal["resumeHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({"jobId":job_id,"previousJobId":goal["jobId"],
        "previousStatus":goal["status"],"startIteration":start_index,
        "skippedUnknownTurn":unknown_turn,
        "acknowledgedBy":"owner","at":chrono::Utc::now().to_rfc3339()}));
    let Some(saved)=state.store.patch_if("goal",&id,"status",&goal["status"],
        &json!({"status":"queued","phase":"resume","jobId":job_id,
            "resumeCount":next,"maxIterations":next_max,"resumeHistory":history,
            "finishedAt":null,"finalReport":null})) else {
        return error(StatusCode::CONFLICT,"목표 상태가 변경됐습니다.");
    };
    // Put the job and the checkpoint into SQLite in the *same insert*.
    // Patching goalState after enqueue exposes a replay race to job workers.
    if crate::engine::jobs::enqueue_once_with_checkpoint(&state,&job_id,"goal",
        json!({"goalId":id,"resumeAfterReview":true}),Some(resume_state)).is_none() {
        let _=state.store.patch_if("goal",&id,"jobId",&json!(job_id),
            &json!({"status":goal["status"],"phase":"needs_review"}));
        return error(StatusCode::INTERNAL_SERVER_ERROR,"재개 작업을 등록하지 못했습니다.");
    }
    if let Some(session)=saved["sessionId"].as_str() {state.clear_cancel(session);}
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_reviewed_resume",json!({
        "goalId":id,"jobId":job_id,"previousJobId":goal["jobId"],
        "startIteration":start_index,"skippedUnknownTurn":unknown_turn,
    }));
    Json(saved).into_response()
}

/// Preserve completed evidence and the exact cursor on every owner-approved
/// continuation. Only a fully persisted review checkpoint is eligible for
/// read-only review replay; tool turns with uncertain external effects are
/// skipped instead of repeated.
fn resume_checkpoint(goal:&Value, previous_job:Option<&Value>)->(Value,u64,bool) {
    // Bind the fallback for the full function scope; borrowing a temporary
    // serde_json::Value in unwrap_or would be dropped after this statement.
    let empty_state=Value::Null;
    let old_state=previous_job.map(|job|&job["goalState"]).unwrap_or(&empty_state);
    let completed=goal["iterations"].as_u64().unwrap_or(0);
    let prior_cursor=old_state["nextIteration"].as_u64().unwrap_or(0);
    let session=goal["sessionId"].as_str().unwrap_or("");
    let safe_review=previous_job.is_some_and(
        crate::engine::goal_recovery::safe_review_checkpoint);
    if safe_review && old_state["sessionId"].as_str()==Some(session) {
        return (old_state.clone(),prior_cursor.saturating_sub(1),false);
    }
    let unknown=old_state["inFlight"]==true
        || old_state["reviewPending"]==true;
    let start=completed.max(prior_cursor.saturating_add(u64::from(unknown)));
    let evidence=goal["evidence"].as_array().cloned()
        .or_else(||old_state["evidence"].as_array().cloned())
        .unwrap_or_default();
    let gaps=goal["gaps"].as_array().cloned()
        .or_else(||old_state["gaps"].as_array().cloned())
        .unwrap_or_default();
    let transcript=old_state["transcript"].as_str().unwrap_or("");
    let transcript_tail=transcript.chars().rev().take(12_000)
        .collect::<String>().chars().rev().collect::<String>();
    let state=json!({
        "sessionId":session,"nextIteration":start,
        "inFlight":false,"reviewPending":false,
        "evidence":evidence,"gaps":gaps,"transcript":transcript_tail,
        "stalled":old_state["stalled"].as_u64().unwrap_or(0),
        "replans":goal["replans"],"plan":goal["plan"],
        "skippedUnknownTurn":unknown
    });
    (state,start,unknown)
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
    let mut criteria = body
        .get("successCriteria")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let isolate=body["isolateWorktree"].as_bool().unwrap_or(false);
    let publish=body["publishPr"].as_bool().unwrap_or(false);
    if publish && !isolate {return error(StatusCode::BAD_REQUEST,
        "GitHub PR을 자동으로 만들려면 독립 Git 작업공간을 선택해야 합니다.");}
    let commands=body["verifyCommands"].as_array().cloned().unwrap_or_default();
    if commands.len()>4 || commands.iter().any(|v|v.as_str().is_none_or(|cmd|{
        cmd.trim().is_empty() || cmd.len()>180 || cmd.chars().any(char::is_control)
            || cmd.chars().any(|ch|matches!(ch,';'|'|'|'&'|'>'|'<'|'`'))
    })) {
        return error(StatusCode::BAD_REQUEST,"검증 명령은 최대 4개, 개당 180자 이하여야 합니다.");
    }
    let Some(items)=criteria.as_array_mut() else {
        return error(StatusCode::BAD_REQUEST,"검증 조건은 배열이어야 합니다.");
    };
    for command in &commands{
        items.push(json!({"type":"command_succeeded","command":command}));
    }
    if publish {items.push(json!({"type":"github_pr_created"}));}
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
    if body.get("tokenBudget").is_some_and(|v|v.as_u64().is_none_or(|n|n>2_000_000)) {
        return error(StatusCode::BAD_REQUEST,"목표 실행 토큰 상한은 0~2000000 사이여야 합니다.");
    }
    let Some(record) = state.store.put(
        "goal",
        &json!({
            "taskId": uuid::Uuid::new_v4().to_string(),
            "goal": text,
            "successCriteria": criteria,
            "isolateWorktree":isolate,"publishPr":publish,
            "verifyCommands":commands,
            "executor": executor,
            "reviewer":body.get("reviewer").cloned().unwrap_or(json!({})),
            "planner":body.get("planner").cloned().unwrap_or(json!({})),
            "tokenBudget":body["tokenBudget"].as_u64().unwrap_or(0),
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
    let goal_id = record
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Do not leave a permanent "queued" goal without a corresponding durable
    // job. The goal and the job are separate records with a recoverable link.
    let Some(job) = crate::engine::jobs::enqueue(
        &state,"goal",json!({"goalId":goal_id})
    ) else {
        state.store.remove("goal",&goal_id);
        return error(StatusCode::INTERNAL_SERVER_ERROR,
            "목표 실행 작업을 저장할 수 없습니다.");
    };
    let job_id=job["id"].clone();
    let saved=state.store.patch_if("goal",&goal_id,"status",&json!("queued"),
        &json!({"jobId":job_id})).unwrap_or_else(||record.clone());
    state.bus.record("goal",&saved);
    Json(saved).into_response()
}


#[cfg(test)]
mod sub3_goal_resume_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    fn state()->AppState {
        let dir=std::env::temp_dir().join(format!("dominion-sub3-resume-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),workspace:dir.clone(),web_dir:dir.join("web")};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        AppState::new(config,store,vault)
    }
    async fn resume(state:&AppState,id:&str,body:Value)->Response{
        resume_goal(State(state.clone()),Path(id.to_string()),Json(body)).await
    }

    #[tokio::test]
    async fn explicit_review_creates_new_durable_job_without_losing_goal_evidence(){
        let state=state();
        state.store.put("session",&json!({"id":"session-1"})).unwrap();
        state.store.put("goal",&json!({"id":"goal-1","goal":"fix failing tests",
            "status":"needs_review","phase":"review","jobId":"old-job",
            "sessionId":"session-1","iterations":16,"maxIterations":16,
            "evidence":[{"tool":"workspace_read","result":{"ok":true}}],
            "planHistory":[{"version":1,"plan":"inspect"}]})).unwrap();
        let invalid=resume(&state,"goal-1",json!({})).await;
        assert_eq!(invalid.status(),StatusCode::BAD_REQUEST);
        assert_eq!(state.store.get("goal","goal-1").unwrap()["status"],"needs_review");
        let good=resume(&state,"goal-1",json!({"acknowledgeUnknownEffects":true})).await;
        assert_eq!(good.status(),StatusCode::OK);
        let saved=state.store.get("goal","goal-1").unwrap();
        assert_eq!(saved["status"],"queued");
        assert_eq!(saved["maxIterations"],24);
        assert_eq!(saved["resumeCount"],1);
        assert_eq!(saved["sessionId"],"session-1");
        assert_eq!(saved["evidence"][0]["tool"],"workspace_read");
        assert_eq!(saved["planHistory"][0]["plan"],"inspect");
        let job_id=saved["jobId"].as_str().unwrap();
        let job=state.store.get("job",job_id).unwrap();
        assert_eq!(job["kind"],"goal");
        assert_eq!(job["status"],"queued");
        assert_eq!(job["input"]["goalId"],"goal-1");
        assert_eq!(job["input"]["resumeAfterReview"],true);
        assert_eq!(job["goalState"]["nextIteration"],16);
        assert_eq!(job["goalState"]["inFlight"],false);
        assert_eq!(job["goalState"]["evidence"][0]["tool"],"workspace_read");
        assert_eq!(state.store.recent_audit(20).iter().filter(|row|
            row["event"]=="goal.owner_reviewed_resume").count(),1);
        assert_eq!(resume(&state,"goal-1",
            json!({"acknowledgeUnknownEffects":true})).await.status(),
            StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn interrupted_tool_turn_is_skipped_instead_of_replayed() {
        let state=state();
        state.store.put("job",&json!({"id":"old","kind":"goal",
            "status":"needs_review","input":{"goalId":"interrupted"},
            "goalState":{"nextIteration":2,"inFlight":true,
                "evidence":[{"tool":"workspace_write","result":{"ok":true}}],
                "transcript":"prior completed steps"}})).unwrap();
        state.store.put("goal",&json!({"id":"interrupted","goal":"edit source",
            "status":"needs_review","jobId":"old","iterations":2,
            "maxIterations":8,"evidence":[{"tool":"workspace_read","result":{"ok":true}}]})).unwrap();
        assert_eq!(resume(&state,"interrupted",
            json!({"acknowledgeUnknownEffects":true})).await.status(),StatusCode::OK);
        let goal=state.store.get("goal","interrupted").unwrap();
        assert_eq!(goal["resumeHistory"][0]["startIteration"],3);
        assert_eq!(goal["resumeHistory"][0]["skippedUnknownTurn"],true);
        let job=state.store.get("job",goal["jobId"].as_str().unwrap()).unwrap();
        assert_eq!(job["goalState"]["nextIteration"],3);
        assert_eq!(job["goalState"]["reviewPending"],false);
        assert_eq!(job["goalState"]["inFlight"],false);
        assert_eq!(job["goalState"]["evidence"][0]["tool"],"workspace_read");
    }

    #[tokio::test]
    async fn completed_executor_turn_only_replays_read_only_review() {
        let state=state();
        state.store.put("job",&json!({"id":"review-job","kind":"goal",
            "status":"needs_review","input":{"goalId":"review-only"},
            "goalState":{"sessionId":"s","nextIteration":3,
                "inFlight":false,"reviewPending":true,
                "evidence":[{"tool":"workspace_write","result":{"ok":true}}],
                "transcript":"saved executor result","gaps":[],
                "toolErrors":[],"turnError":null}})).unwrap();
        state.store.put("goal",&json!({"id":"review-only","goal":"update code",
            "sessionId":"s","status":"needs_review","jobId":"review-job",
            "iterations":3,"maxIterations":8})).unwrap();
        let res=resume(&state,"review-only",json!({"acknowledgeUnknownEffects":true})).await;
        assert_eq!(res.status(),StatusCode::OK);
        let goal=state.store.get("goal","review-only").unwrap();
        assert_eq!(goal["resumeHistory"][0]["startIteration"],2);
        let job=state.store.get("job",goal["jobId"].as_str().unwrap()).unwrap();
        assert!(crate::engine::goal_recovery::safe_review_checkpoint(&job));
        assert_eq!(job["goalState"]["nextIteration"],3);
        assert_eq!(job["goalState"]["transcript"],"saved executor result");
        assert_eq!(job["goalState"]["evidence"][0]["tool"],"workspace_write");
    }

    #[tokio::test]
    async fn live_job_and_final_result_cannot_be_restarted(){
        let state=state();
        state.store.put("goal",&json!({"id":"done","status":"completed"})).unwrap();
        let done=resume(&state,"done",json!({"acknowledgeUnknownEffects":true})).await;
        assert_eq!(done.status(),StatusCode::CONFLICT);
        state.store.put("goal",&json!({"id":"review","status":"needs_review"})).unwrap();
        state.store.put("job",&json!({"id":"live-job","kind":"goal",
            "input":{"goalId":"review"},"status":"running"})).unwrap();
        let live=resume(&state,"review",json!({"acknowledgeUnknownEffects":true})).await;
        assert_eq!(live.status(),StatusCode::CONFLICT);
        assert!(state.store.get("goal","review").unwrap()["status"]=="needs_review");
    }
}
