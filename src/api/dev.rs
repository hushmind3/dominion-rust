use crate::engine::dev;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Code-assistant goals may run only against the same real Git workspace
/// configured for the Rust harness. A different Developer source folder must
/// not silently send writes to an unrelated repository.
fn aligned_clean_goal_workspace(state:&AppState, status:&Value)->bool{
    let source=status["config"]["sourceDir"].as_str()
        .and_then(|p|std::path::Path::new(p).canonicalize().ok());
    let runtime=state.config.workspace.canonicalize().ok();
    source.is_some()&&source==runtime&&status["git"]==true
        && status["exists"]==true&&status["dirty"].as_u64()==Some(0)
        && status["commit"].as_str().is_some_and(|sha|sha.len()==40)
}
pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let mut record=dev::status(&state).await;
    record["goalWorkspaceReady"]=json!(aligned_clean_goal_workspace(&state,&record));
    record["goalWorkspaceReason"]=json!(if record["goalWorkspaceReady"]==true{
        "설정된 소스와 엔진 작업공간이 동일한 깨끗한 Git 저장소입니다."
    }else{
        "안전한 자율 코딩은 개발자 소스 폴더와 엔진 작업공간이 같고, Git HEAD가 존재하며 변경 파일이 없는 경우에만 시작됩니다."
    });
    Json(record)
}

/// Developer → isolated Goal → real tool receipts → user-approved Git changes.
pub async fn agent_goal(State(state):State<AppState>,Json(body):Json<Value>)->Response{
    let status=dev::status(&state).await;
    if !aligned_clean_goal_workspace(&state,&status){
        return (StatusCode::CONFLICT,Json(json!({"error":
            "개발자 소스 경로와 엔진 Git 작업공간이 다르거나 미커밋 변경이 있습니다. 동일한 깨끗한 Git 저장소에서 시작하세요."}))).into_response();
    }
    let Some(task)=body["task"].as_str().map(str::trim)
        .filter(|s|!s.is_empty()&&s.chars().count()<=6000)else{
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"코딩 작업은 1~6000자로 입력하세요."}))).into_response();
    };
    let Some(agent_id)=body["agentId"].as_str().filter(|id|
        state.store.get("agent",id).is_some())else{
        return (StatusCode::BAD_REQUEST,Json(json!({"error":"실행할 등록된 에이전트를 선택하세요."}))).into_response();
    };
    let budget=body["tokenBudget"].as_u64().unwrap_or(100_000);
    if budget>2_000_000{return (StatusCode::BAD_REQUEST,Json(json!({
        "error":"토큰 예산은 0~200만으로 입력하세요."}))).into_response();}
    let criteria=match body["verificationCommand"].as_str().map(str::trim){
        Some(command) if !command.is_empty()&&command.chars().count()<=300=>
            json!([{"type":"command_succeeded","command":command}]),
        Some(command) if !command.is_empty()=>
            return (StatusCode::BAD_REQUEST,Json(json!({"error":"검증 명령어는 300자 이하여야 합니다."}))).into_response(),
        _=>json!([])
    };
    crate::api::goals::create_goal(State(state),Json(json!({
        "goal":task,"successCriteria":criteria,
        "executor":{"agentId":agent_id},
        "isolateWorktree":true,"tokenBudget":budget,
        "maxIterations":16,"maxReplans":6,"replanAfter":2
    }))).await
}

pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(dev::config(&state))
}

pub async fn save_config(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(dev::save_config(&state, &body))
}

pub async fn connect(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match dev::connect(&state, &body).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn diff(State(state): State<AppState>) -> Response {
    match dev::diff(&state).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn get_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("dev_job", &id) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "개발 작업을 찾을 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn start_job(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !matches!(
        kind.as_str(),
        "setup" | "test" | "build" | "package" | "commit"
    ) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 개발 작업입니다: {kind}") })),
        )
            .into_response();
    }
    match dev::start_job(state, &kind, body) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "개발 작업을 만들 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn preview(State(state): State<AppState>, Path(action): Path<String>) -> Response {
    match action.as_str() {
        "start" => match dev::start_preview(&state).await {
            Ok(result) => Json(result).into_response(),
            Err(error) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
            }
        },
        "stop" => Json(dev::stop_preview()).into_response(),
        other => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 미리보기 작업입니다: {other}") })),
        )
            .into_response(),
    }
}

pub async fn update_status(State(state): State<AppState>) -> Json<Value> {
    Json(dev::update_status(&state).await)
}

pub async fn update(State(state): State<AppState>, Json(_body): Json<Value>) -> Response {
    match dev::start_update_job(state) {
        Ok(job) => Json(job).into_response(),
        Err(message) => (StatusCode::CONFLICT, Json(json!({"error":message}))).into_response(),
    }
}
