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
/// Explicit operator-triggered paired code check. This runs model-written
/// Rust tests on the local host, so the UI must require an explicit action.
pub async fn compare(State(state):State<AppState>,Path(id):Path<String>,
    Json(body):Json<Value>)->Response{
    if body["confirmation"]!="RUN_PAIRED_NATIVE_TESTS" {
        return error("수정된 Rust 코드의 실제 로컬 테스트 실행을 명시적으로 승인하세요.".into());
    }
    let Some(candidate)=engine::one(&state,&id)else{
        return error("자기개선 후보가 없습니다.".into());
    };
    if !matches!(candidate["status"].as_str(),Some("proposed"|"inconclusive"))
        || candidate["cases"].as_array().is_none_or(|c|c.len()<engine::MIN_PAIRED_CASES){
        return error("평가 가능한 8개 이상 동결 사례와 대기 중인 후보가 필요합니다.".into());
    }
    let run_id=format!("rsi-run-{}",candidate["rsiJobId"].as_str().unwrap_or(""));
    let Some(evidence)=state.store.get("rsi_run",&run_id)else{
        return error("코드 수정 증거가 없습니다.".into());
    };
    if evidence["candidateId"]!=candidate["id"]||evidence["checksPassed"]!=true {
        return error("기존 정적 검사를 통과한 코드 후보만 비교할 수 있습니다.".into());
    }
    match crate::engine::jobs::enqueue(&state,"rsi_compare",json!({"candidateId":id})){
        Some(job)=>(StatusCode::ACCEPTED,Json(job)).into_response(),
        None=>error("A/B 실험을 대기열에 넣지 못했습니다.".into())
    }
}

pub async fn apply(
    State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<Value>
) -> Response {
    match crate::engine::rsi_worktree::apply_to_sub(&state,&id,&body).await {
        Ok(value)=>Json(value).into_response(),
        Err(message)=>error(message),
    }
}

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
    let autonomous = objective.is_empty();
    let objective = if autonomous {
        "DOMINION 소스와 기존 테스트·실험 기록을 스스로 조사하고 개선 가치가 큰 문제를 직접 선정한다. 기존 기능을 유지하면서 실제로 더 정확하고 효율적이거나 견고해지는 알고리즘·하네스·개선기 구조를 탐색하고, 근거가 있는 개선안 하나를 코드로 구현한다. 이미 구현된 기능을 중복 작성하거나 근거 없이 리팩터링하지 않는다."
    } else { objective };
    let source_dir = source.to_string_lossy().to_string();
    let previous_evidence=crate::engine::rsi_worktree::recent_feedback(&state);
    let prompt = format!(
        "DOMINION Rust 자체 개선 작업입니다. 원본 소스는 보호되고 실행기가 생성하는 분리된 작업공간에서 수정합니다.\\n\\n원본 (읽기 참조용): {source_dir}\\n브랜치: dominion-sub\\n개선 모드: {}\\n목표: {objective}\\n\\n이전 실행의 실제 검사 증거(성능 향상과 구분):\\n{previous_evidence}\\n\\n1. workspace_list로 저장소 구조를 먼저 살펴보고 실제 Rust 실행 엔진, 문맥·기억, 도구, 학습·자기개선 관련 소스와 기존 테스트의 연결 관계를 조사하세요. workspace_read로 근거가 되는 코드를 읽으세요.\\n2. 실패·성능 저하·중복 계산·알고리즘 정확도·유지보수성 가운데 가장 효과가 클 수 있는 개선 후보를 우선순위로 비교하세요. 이미 있는 기능을 새로 만들지 마세요.\\n3. 가장 근거가 탄탄한 변경 하나를 고르고 원인·변경 전 행동·기대되는 효과·검증 방법을 정하세요. 실질적 개선을 확인할 방법이 없으면 무리하게 코드를 바꾸지 마세요.\\n4. 필요한 경우 workspace_write로 분리된 작업공간의 허용된 코드만 수정하세요. 기존 사용자 지시·페르소나·장기기억·승인·취소·복구 능력은 보존하세요.\\n5. 실제 수정 여부와 검증 여부를 구분해 보고하세요. 컴파일·타입 검사와 패치 수집은 모델 종료 후 Rust 엔진이 독립 수행하므로, 이를 모델이 직접 완료했다고 주장하지 마세요.\\n절대경로 읽기·쓰기, 셸, GitHub API, 다른 에이전트는 이 작업에 사용할 수 없습니다. main 변경·커밋·푸시·병합·서버 재시작도 하지 마세요. 사용자는 JSON, Git diff, SHA를 입력하지 않습니다. 한국어로 핵심 개선 근거와 남은 검증을 간결하게 보고하세요.", if autonomous { "자율 진단 및 개선" } else { "사용자 지정 개선" }
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
        "rsiAutonomous": autonomous,
        "rsiFrozenCases": [
            {"id":"memory-explicit","domain":"memory","holdout":true,
                "filter":"engine::room_state::tests::explicit_owner_state_survives_and_a_correction_supersedes_the_old_code"},
            {"id":"memory-correction","domain":"memory","holdout":true,
                "filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
            {"id":"memory-long","domain":"memory","holdout":true,
                "filter":"engine::room_state::tests::a_hundred_unrelated_owner_decisions_do_not_evict_a_relevant_early_fact"},
            {"id":"memory-distinct","domain":"memory","holdout":false,
                "filter":"engine::room_state::tests::corrections_keep_unrelated_domains_and_do_not_promote_questions"},
            {"id":"kernel-generation","domain":"algorithm","holdout":true,
                "filter":"engine::algorithms::tests::generation_uses_evidence_and_returns_executable_kernels"},
            {"id":"kernel-promotion","domain":"algorithm","holdout":false,
                "filter":"engine::algorithms::tests::unverified_generated_algorithms_never_enter_live_ucb_until_promoted"},
            {"id":"rsi-oracle","domain":"improver","holdout":true,
                "filter":"engine::rsi::tests::evaluation_cannot_switch_its_frozen_oracle_or_test_names"},
            {"id":"rsi-lineage","domain":"improver","holdout":true,
                "filter":"engine::rsi::tests::only_a_deployed_improver_can_seed_the_next_generation"}
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
        assert_eq!(jobs[0]["input"]["rsiFrozenCases"].as_array().unwrap().len(),8);
        assert!(jobs[0]["input"]["prompt"].as_str().unwrap().contains("workspace_write"));
        assert_eq!(jobs[0]["input"]["rsiAutonomous"], false);
        let auto_response = start(State(state.clone()), Json(json!({
            "providerId":"test-provider","model":"sample-model"
        }))).await;
        assert_eq!(auto_response.status(), StatusCode::ACCEPTED);
        let queued = state.store.all("job");
        assert_eq!(queued.len(), 2);
        let automated = queued.iter().find(|job| job["input"]["rsiAutonomous"] == true).unwrap();
        assert!(automated["input"]["prompt"].as_str().unwrap().contains("개선 모드: 자율 진단 및 개선"));
        assert!(automated["input"]["rsiObjective"].as_str().unwrap().contains("스스로 조사"));
        std::fs::write(checkout.join("uncommitted.txt"), "owner edits").unwrap();
        let refused = start(State(state.clone()), Json(body)).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.store.all("job").len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
