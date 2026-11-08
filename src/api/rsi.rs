use crate::{engine::rsi as engine, state::AppState};
use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};

fn error(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"version":"manual-rsi-v1","candidates":engine::list(&state),
        "minPairedCases":engine::MIN_PAIRED_CASES}))
}

pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match engine::one(&state, &id) {
        Some(candidate) => Json(candidate).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error":"후보를 찾을 수 없습니다."}))).into_response()
    }
}

pub async fn propose(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    match engine::propose(&state,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}

pub async fn evaluate(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match engine::evaluate(&state,&id,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}

pub async fn decide(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match engine::decide(&state,&id,&body) { Ok(value)=>Json(value).into_response(), Err(err)=>error(err) }
}

pub async fn deployment(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match engine::confirm_deployment(&state,&id,&body) {
        Ok(value)=>Json(value).into_response(), Err(err)=>error(err)
    }
}

 
/// A non-developer chooses a configured model and describes the issue.
 /// The existing Rust tool harness performs inspection, edits and verification;
 /// patch/commit/test manifests remain internal, never a form prerequisite.
pub async fn start(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let provider_id = body["providerId"].as_str().unwrap_or("").trim();
    let model = body["model"].as_str().unwrap_or("").trim();
    if provider_id.is_empty() || model.is_empty() || model.len() > 200 {
        return error("연결된 제공자와 실행할 모델을 선택하세요.".into());
    }
    let Some(provider) = state.store.get("provider", provider_id) else {
        return error("선택한 모델 제공자를 찾을 수 없습니다.".into());
    };
    if provider["enabled"] == false {
        return error("비활성화된 모델 제공자입니다.".into());
    }
    let status = crate::engine::dev::status(&state).await;
    if status["git"] != true || status["branch"] != "dominion-sub" {
        return error("자기개선은 연결된 Git 소스의 dominion-sub 브랜치에서만 실행합니다. 개발자 설정에서 소스 폴더를 연결하세요.".into());
    }
    if status["dirty"].as_u64().unwrap_or(u64::MAX) != 0 {
        return error("소스 폴더에 미저장 변경사항이 있습니다. 기존 작업이 덮어써지지 않도록 먼저 정리하세요.".into());
    }
    let source_raw = status["config"]["sourceDir"].as_str().unwrap_or("");
    let expanded = if let Some(suffix) = source_raw.strip_prefix("~/") {
        match std::env::var("HOME") {
            Ok(home) => std::path::PathBuf::from(home).join(suffix),
            Err(_) => return error("홈 디렉터리 경로를 찾지 못했습니다.".into()),
        }
    } else {
        std::path::PathBuf::from(source_raw)
    };
    let source = match expanded.canonicalize() {
        Ok(path) => path,
        Err(_) => return error("연결된 소스 폴더에 접근할 수 없습니다.".into()),
    };
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file() {
        return error("DOMINION Rust 저장소가 아닌 소스 폴더입니다.".into());
    }
    let remote = status["remote"].as_str().unwrap_or("");
    if !remote.is_empty() && !remote.contains("hushmind3/dominion-rust") {
        return error("현재 연결된 소스가 DOMINION Rust 저장소가 아닙니다.".into());
    }
    let objective = body["request"].as_str().unwrap_or("").trim();
    if objective.chars().count() > 3000 {
        return error("개선 요청은 3,000자 이하로 입력하세요.".into());
    }
    let objective = if objective.is_empty() {
        "기존 실패 기록과 코드를 조사해 DOMINION 하네스의 실제 결함 하나를 수정하고 검증하라. 우선 100턴 회의실 장기기억 회상 실패를 조사하라."
    } else { objective };
    let source_dir = source.to_string_lossy().to_string();
    let prompt = format!(
        "DOMINION Rust 자체 개선 작업입니다. 지정된 저장소만 조사·수정하십시오.\\n\\n저장소: {source_dir}\\n현재 브랜치: dominion-sub\\n개선 목표: {objective}\\n\\n반드시 실제 workspace_list/workspace_read로 현재 소스를 읽고 원인을 분석하세요. 필요한 코드를 workspace_write로 수정하고, shell_exec 또는 terminal_exec로 관련 테스트와 빌드를 실행하세요. 실패하면 허용된 반복 내에서 코드를 재수정하고 테스트하세요. 선택한 모델과 원래의 사용자 페르소나를 변경하지 마세요. main 브랜치 변경·브랜치 생성·커밋·푸시·병합·운영 서버 재시작을 하지 마세요. 완료했다고 주장하기 전에 실제 도구 결과를 확인하세요. 성공 여부, 수정 파일, 테스트 결과, 남은 실패를 한국어로 보고하세요. 사용자는 코딩을 하지 않으므로 JSON, Git diff, SHA 입력을 요청하지 마세요."
    ).replace("\\n", "\n");
    let mut input = json!({
        "prompt": prompt,
        "providerId": provider_id,
        "model": model,
        "toolsEnabled": true,
        "toolsExecutionEnabled": true,
        "rsiSourceDir": source_dir,
        "autoLearning": false
    });
    if let Some(effort) = body["reasoningEffort"].as_str().filter(|v| !v.is_empty()) {
        if !crate::engine::reasoning::valid_effort(effort) {
            return error("선택한 추론 강도가 유효하지 않습니다.".into());
        }
        input["reasoningEffort"] = json!(effort);
    }
    match crate::engine::jobs::enqueue(&state, "prompt", input) {
        Some(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"자기개선 작업을 대기열에 등록하지 못했습니다."}))).into_response()
    }
}
