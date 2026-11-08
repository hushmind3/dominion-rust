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
        "DOMINION Rust 자체 개선 작업입니다. 원본 소스는 보호되고 실행기가 생성하는 분리된 작업공간에서 수정합니다.\\n\\n원본 (읽기 참조용): {source_dir}\\n브랜치: dominion-sub\\n개선 목표: {objective}\\n\\nworkspace_list/workspace_read로 코드를 읽고 원인을 분석하세요. 필요한 기존 코드 또는 새 소스는 workspace_write로 수정하세요. 절대경로 읽기·쓰기, 셸, GitHub API, 다른 에이전트는 이 작업에 사용할 수 없습니다. 소스 검사와 후보 기록은 종료 후 엔진이 직접 수행합니다. 작업을 하지 않았는데 성공이라고 주장하지 마세요. 코드 변경 내용과 이유를 한국어로 설명하세요. main 변경·커밋·푸시·병합·서버 재시작은 하지 마세요."
    ).replace("\\n", "\n");
    let mut input = json!({
        "prompt": prompt,
        "providerId": provider_id,
        "model": model,
        "toolsEnabled": true,
        "toolsExecutionEnabled": true,
        "rsiSourceDir": source_dir,
        "rsiBaseCommit": status["commit"],
        "rsiObjective": objective,
        "rsiFrozenCases": [
            {"id":"owner-memory","domain":"memory","holdout":true,
                "filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
            {"id":"algorithm-generation","domain":"algorithm","holdout":true,
                "filter":"engine::algorithms::tests::generation_uses_evidence_and_returns_executable_kernels"}
        ],
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

 
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(output.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&output.stderr));
    }

    #[tokio::test]
    async fn natural_language_start_queues_selected_model_only_on_clean_sub_branch() {
        let root = std::env::temp_dir().join(format!("dominion-rsi-start-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let data = root.join("data");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"dominion\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi.rs"), "// source\n").unwrap();
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["checkout", "-b", "dominion-sub"]);
        git(&checkout, &["add", "."]);
        git(&checkout, &["-c", "user.email=ci@example.com", "-c", "user.name=CI", "commit", "-qm", "fixture"]);
        let cfg = Config { port:0, data_dir:data.clone(), web_dir:data.join("web"), workspace:data.clone() };
        let state = AppState::new(cfg.clone(), Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state, &json!({"sourceDir":checkout.to_str().unwrap()}));
        state.store.put("provider", &json!({"id":"test-provider","name":"Local","enabled":true,
            "kind":"openai","model":"sample-model","baseUrl":"http://127.0.0.1:1/v1"}));
        let body = json!({"providerId":"test-provider","model":"sample-model",
            "request":"100턴에서 장기기억이 사라지는 문제를 찾아 고쳐 주세요."});
        let response = start(State(state.clone()), Json(body.clone())).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["input"]["providerId"], "test-provider");
        assert_eq!(jobs[0]["input"]["model"], "sample-model");
        assert_eq!(jobs[0]["input"]["rsiSourceDir"], checkout.to_str().unwrap());
        assert!(jobs[0]["input"]["rsiBaseCommit"].as_str().is_some_and(|v|v.len()==40));
        assert_eq!(jobs[0]["input"]["rsiFrozenCases"].as_array().unwrap().len(),2);
        assert!(jobs[0]["input"]["prompt"].as_str().unwrap().contains("workspace_write"));
        std::fs::write(checkout.join("uncommitted.txt"), "owner edits").unwrap();
        let refused = start(State(state.clone()), Json(body)).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.store.all("job").len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
