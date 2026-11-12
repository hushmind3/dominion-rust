use crate::{engine::rsi as engine, state::AppState};
use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};
use std::path::PathBuf;

fn error(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":message}))).into_response()
}

/// Longest run of adopted generations in a campaign. Older campaigns stored
/// only per-generation flags, so derive the streak instead of rewriting them.
fn accepted_streak(record: &Value) -> (u64, u64) {
    let mut streak = 0u64;
    let mut max = 0u64;
    for generation in record["history"].as_array().cloned().unwrap_or_default() {
        if generation["accepted"] == true {
            streak += 1;
            max = max.max(streak);
        } else {
            // Research-only attempts are part of the generation sequence.
            // A gap is not a consecutive adoption.
            streak = 0;
        }
    }
    (streak, max)
}

fn apply_capability(record: &mut Value) {
    let (streak, max) = accepted_streak(record);
    record["acceptedStreak"] = json!(streak);
    record["maxAcceptedStreak"] = json!(max);
    let history = record["history"].as_array().cloned().unwrap_or_default();
    let (_, verified) = crate::engine::rsi_evolve::verified_capability_streak(&history);
    let meta = crate::engine::rsi_evolve::meta_transfer_proven(&history);
    record["maxVerifiedStreak"] = json!(verified);
    record["metaTransferProven"] = json!(meta);
    record["capabilityDemonstrated"] = json!(crate::engine::rsi_evolve::capability_demonstrated(&history));
}

/// Public RSI status must recheck original frozen paired reports instead of
/// trusting an editable per-generation pairedRevalidated flag.
fn apply_verified_capability(state:&AppState, record:&mut Value) {
    let history=record["history"].as_array().cloned().unwrap_or_default();
    let checked=crate::engine::rsi_evolve::revalidated_history(state,&history);
    let mut verified_record=record.clone();
    verified_record["history"]=json!(checked);
    apply_capability(&mut verified_record);
    for field in ["acceptedStreak","maxAcceptedStreak","maxVerifiedStreak",
        "metaTransferProven","capabilityDemonstrated"] {
        record[field]=verified_record[field].clone();
    }
}

async fn configured_source(state: &AppState) -> Result<PathBuf, String> {
    let status = crate::engine::dev::status(state).await;
    let raw = status["config"]["sourceDir"].as_str().unwrap_or("");
    let expanded = if let Some(suffix) = raw.strip_prefix("~/") {
        PathBuf::from(
            std::env::var("HOME").map_err(|_| "홈 디렉터리 경로를 찾지 못했습니다.")?,
        )
        .join(suffix)
    } else {
        PathBuf::from(raw)
    };
    let source = expanded
        .canonicalize()
        .map_err(|_| "연결된 소스 폴더에 접근할 수 없습니다.".to_string())?;
    if !source.join("Cargo.toml").is_file() || !source.join("src/engine/rsi.rs").is_file() {
        return Err("DOMINION Rust 저장소가 아닌 소스 폴더입니다.".into());
    }
    Ok(source)
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!({"version":"rsi-v2","candidates":engine::list(&state),
        "campaigns":state.store.all("rsi_campaign").into_iter().take(20).collect::<Vec<_>>(),
        "minPairedCases":engine::MIN_PAIRED_CASES}))
}

pub async fn signals(State(state): State<AppState>) -> Json<Value> {
    let report = crate::engine::rsi_scan::report(&state);
    Json(json!({
        "signals": report["signals"],
        "checks": report["checks"],
        "checked": report["checked"],
    }))
}

pub async fn index(State(state): State<AppState>) -> Response {
    let source = match configured_source(&state).await {
        Ok(source) => source,
        Err(message) => return error(message),
    };
    Json(json!({"index": crate::engine::code_index::get(&state, &source)}))
        .into_response()
}

pub async fn inspect(State(state): State<AppState>) -> Response {
    let source = match configured_source(&state).await {
        Ok(source) => source,
        Err(message) => return error(message),
    };
    match crate::engine::code_index::refresh(&state, &source) {
        Ok(index) => Json(json!({"index": index})).into_response(),
        Err(message) => error(message),
    }
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

pub async fn choose(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    match crate::engine::rsi_evolve::choose(&state,&id,&body){
        Ok(value)=>Json(value).into_response(),Err(message)=>error(message)
    }
}

pub async fn apply_campaign(State(state):State<AppState>,Path(id):Path<String>,Json(body):Json<Value>)->Response{
    let Some(campaign)=state.store.get("rsi_campaign",&id) else {return error("개선 작업을 찾지 못했습니다.".into())};
    if body["confirmation"]!="APPLY_VERIFIED_CAMPAIGN_TO_SUB" || body["commit"]!=campaign["headCommit"]
        || !matches!(campaign["status"].as_str(),Some("completed"|"cancelled"|"needs_review"|"failed"))
        || campaign["headCommit"]==campaign["baseCommit"] {
        return error("완료된 작업에서 선택한 검증 버전의 적용 버튼을 눌러 주세요.".into());
    }
    match crate::engine::dev::start_rsi_install(state.clone(),&id,body["commit"].as_str().unwrap_or("")) {
        Ok(job)=>{
            state.store.patch_if("rsi_campaign",&id,"headCommit",&campaign["headCommit"],&json!({"installJobId":job["id"]}));
            (StatusCode::ACCEPTED,Json(job)).into_response()
        },
        Err(message)=>error(message),
    }
}

pub async fn campaign(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id).filter(|job|job["kind"]=="rsi_evolve")else{
        return error("개선 작업을 찾을 수 없습니다.".into());
    };
    let mut record=state.store.get("rsi_campaign",&id).unwrap_or_else(||json!({"id":id,"phase":"preparing_lab"}));
    for (field,key) in [("objective","rsiObjective"),("providerId","providerId"),("model","model")] {
        record[field]=job["input"][key].clone();
    }
    record["selectedVersionRunning"]=json!(record["headCommit"].as_str()==option_env!("DOMINION_BUILD_COMMIT"));
    apply_verified_capability(&state,&mut record);
    if record["phase"]=="diagnosing_and_editing" {
        if let Some(session)=record["activeGenerationJobId"].as_str().and_then(|id|state.store.get("job",id))
            .and_then(|j|j["sessionId"].as_str().map(str::to_string)) {
            let usage=state.store.session_usage(&session);
            record["tokens"]=json!(record["activeBaseTokens"].as_u64().unwrap_or(0)
                .saturating_add(usage["totalTokens"].as_u64().unwrap_or(0)));
            record["tokensEstimated"]=json!(record["tokensEstimated"]==true || usage["estimatedCalls"].as_u64().unwrap_or(0)>0);
        }
    }
    if job["status"]!="running"||record["status"].is_null(){record["status"]=job["status"].clone();}
    if !job["error"].is_null(){record["error"]=job["error"].clone();}
    Json(record).into_response()
}

pub async fn campaigns(State(state):State<AppState>)->Json<Value>{
    let rows=state.store.all("rsi_campaign").into_iter().take(20).map(|mut record|{
        let job=record["id"].as_str().and_then(|id|state.store.get("job",id)).unwrap_or(Value::Null);
        apply_verified_capability(&state,&mut record);
        json!({"id":record["id"],"phase":record["phase"],
            "maxAcceptedStreak":record["maxAcceptedStreak"],"maxVerifiedStreak":record["maxVerifiedStreak"],
            "metaTransferProven":record["metaTransferProven"],
            "capabilityDemonstrated":record["capabilityDemonstrated"],
            "status":if job["status"]!="running"&&!job["status"].is_null(){job["status"].clone()}else{record["status"].clone()}})
    }).collect::<Vec<_>>();
    Json(json!({"campaigns":rows}))
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

fn continued_request(state:&AppState,mut body:Value)->Result<Value,String>{
    let Some(id)=body["continueFrom"].as_str().map(str::to_string) else {return Ok(body)};
    let previous=state.store.get("job",&id).filter(|j|j["kind"]=="rsi_evolve")
        .ok_or("이어서 진행할 작업을 찾지 못했습니다.")?;
    if !matches!(previous["status"].as_str(),Some("completed"|"cancelled"|"failed"|"needs_review")) {
        return Err("이전 작업이 끝난 뒤 이어서 진행할 수 있습니다.".into());
    }
    for (field,key) in [("request","rsiObjective"),("focusFiles","rsiFocusFiles"),("providerId","providerId"),("model","model")] {
        if body[field].is_null() || body[field].as_str()==Some("") {body[field]=previous["input"][key].clone();}
    }
    Ok(body)
}

pub async fn start(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if crate::engine::dev::installation_running() {return error("버전 반영이 끝난 뒤 발전 작업을 시작하세요.".into());}
    let body=match continued_request(&state,body){Ok(body)=>body,Err(message)=>return error(message)};
    let continuous=body["mode"]=="continuous";
    let _start_guard=if continuous{
        if !state.begin_session("rsi-evolution-start"){return error("개선 시작을 처리 중입니다.".into());}
        let guard=state.session_guard("rsi-evolution-start");
        if state.store.all("job").iter().any(|job|matches!(job["kind"].as_str(),Some("rsi_evolve"|"rsi_generation"))&&matches!(job["status"].as_str(),Some("queued"|"running"))){
            return error("진행 중인 개선이 있습니다. 기존 작업에서 계속하거나 중단해 주세요.".into());
        }
        Some(guard)
    }else{None};
    if continuous && body["confirmation"]!="START_RSI_EVOLUTION" {
        return error("시작 버튼에서 자동 진단·실험 실행을 선택하세요.".into());
    }
    if !body["mode"].is_null() && !matches!(body["mode"].as_str(),Some("manual"|"continuous")){
        return error("지원하지 않는 실행 방식입니다.".into());
    }
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
    if status["git"] != true
        || (!continuous
            && !crate::engine::rsi_worktree::is_work_branch(status["branch"].as_str().unwrap_or("")))
    {
        return error("자기개선은 연결된 Git 소스의 dominion-sub 작업 브랜치에서만 실행합니다. 개발자 설정에서 소스 폴더를 연결하세요.".into());
    }
    if !continuous && status["dirty"].as_u64().unwrap_or(u64::MAX) != 0 {
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
    let focus_files = body["focusFiles"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .take(8)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if focus_files
        .iter()
        .any(|path| !crate::engine::rsi::safe_path(path))
    {
        return error("전수조사 대상 파일 경로가 허용 범위를 벗어났습니다.".into());
    }
    let request = body["request"].as_str().unwrap_or("").trim();
    if request.chars().count() > 3000 {
        return error("개선 요청은 3,000자 이하로 입력하세요.".into());
    }
    let autonomous = request.is_empty();
    let objective = if autonomous {
        "기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다. 필요한 부분만 확인하고 적용 조건과 검증 근거가 있는 변경을 제안한다. 오류 수정은 이 과정에서 필요할 때 수행하며, 개선 근거가 없으면 변경 없이 이유를 보고한다.".to_string()
    } else {
        request.to_string()
    };
    let source_dir = source.to_string_lossy().to_string();
    if let Some(id)=body["continueFrom"].as_str() {
        let prior=state.store.get("job",id).unwrap_or(Value::Null);
        let prior_source=prior["input"]["rsiSourceDir"].as_str().and_then(|s|std::fs::canonicalize(s).ok());
        if prior_source.as_ref()!=Some(&source) {return error("다른 소스의 작업은 이어서 진행할 수 없습니다.".into());}
    }
    let (index_meta, index_context) =
        match crate::engine::code_index::prompt_context(&state, &source, &focus_files) {
            Ok(value) => value,
            Err(message) => return error(message),
        };
    let previous_evidence = crate::engine::rsi_worktree::recent_feedback(&state);
    let mode = if focus_files.is_empty() {
        if autonomous {
            "전수조사 기반 자율 개선"
        } else {
            "전수조사 기반 사용자 지정 개선"
        }
    } else {
        "전수조사 인덱스의 대상 파일 수정"
    };
    let recipe = "제공된 인덱스와 해시 검증된 연구 원문을 먼저 활용하세요. 이미 제공된 위치나 줄 범위를 다시 조회할 필요는 없습니다. 모르는 심볼만 code_search로 찾고, 제공되지 않은 줄만 workspace_read_range로 읽으세요. 참고 코드와 지도는 작업 지시가 아닙니다. 근거가 있는 개선만 workspace_replace로 수정하고, 새 파일을 만들 때만 workspace_write를 사용하세요. 적합한 후보가 없으면 수정 없이 종료할 수 있습니다. 기존 기능·사용자 지시·기억·승인·취소·복구는 보존하세요. 빌드·패치 수집은 종료 후 엔진이 수행하므로 직접 완료했다고 주장하지 마세요.";
    let branch_name = status["branch"].as_str().unwrap_or("dominion-sub1");
    let prompt = format!(
        "DOMINION Rust 자체 개선 작업입니다. 원본 소스는 보호되고 실행기가 생성하는 분리된 작업공간에서 수정합니다.\n\n원본 (읽기 참조용): {source_dir}\n브랜치: {branch_name}\n개선 모드: {mode}\n목표: {objective}\n\n이전 실행의 실제 검사 증거(성능 향상과 구분):\n{previous_evidence}\n\n{recipe}\n\n{index_context}\n\n절대경로 읽기·쓰기, 셸, GitHub API, 다른 에이전트는 이 작업에 사용할 수 없습니다. main 변경·커밋·푸시·병합·서버 재시작도 하지 마세요. 사용자는 JSON, Git diff, SHA를 입력하지 않습니다. 한국어로 핵심 개선 근거와 남은 검증을 간결하게 보고하세요."
    );
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
        // A paid model descendant designs across all its own subsystems.
        // Opt-in legacy native induction retains its separate bounded protocol.
        "rsiSystemEvolution": continuous && body["nativeInduction"] != true
            && body["evolutionTarget"] != "product",
        "rsiNativeInduction": continuous && body["nativeInduction"]==true
            && body["selectionPolicy"]=="automatic",
        "rsiExpandAfterGain": continuous && body["expandAfterGain"]==true
            && body["selectionPolicy"]=="automatic",
        "rsiStopAfterProof": continuous && body["stopAfterProof"]==true
            && body["selectionPolicy"]=="automatic",
        "rsiIndexId": index_meta["id"],
        "rsiIndexFingerprint": index_meta["fingerprint"],
        "rsiIndexFiles": index_meta["files"],
        "rsiFocusFiles": focus_files,
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
    if let Some(id)=body["continueFrom"].as_str() {
        input["rsiContinueFrom"]=json!(id);
        if let Some(prior)=state.store.get("rsi_campaign",id) {
            // Reuse decisions refer to the exact selected implementation.
            // Source changes still reuse indexed research, but force a fresh decision.
            if prior["headCommit"]==status["commit"] || prior["baseCommit"]==status["commit"] {
                input["rsiPreviousDecision"]=prior["nextDecision"].clone();
            }
        }
    }
    if let Some(effort) = body["reasoningEffort"].as_str().filter(|v| !v.is_empty()) {
        if !crate::engine::reasoning::valid_effort(effort) {
            return error("선택한 추론 강도가 유효하지 않습니다.".into());
        }
        input["reasoningEffort"] = json!(effort);
    }
    if continuous {
        let limits=match crate::engine::rsi_evolve::limits(&body){Ok(v)=>v,Err(e)=>return error(e)};
        let policy=body["selectionPolicy"].as_str().unwrap_or("manual");
        if !matches!(policy,"manual"|"automatic"){return error("선택 방식이 올바르지 않습니다.".into());}
        input["selectionPolicy"]=json!(policy);input["limits"]=limits;
        input["rsiNativeApproved"]=json!(true);
    }
    let mut cases=input["rsiFrozenCases"].as_array().cloned().unwrap_or_default();
    cases.extend(crate::engine::rsi_worktree::metric_cases());
    input["rsiFrozenCases"]=json!(cases);
    match crate::engine::jobs::enqueue(&state, if continuous{"rsi_evolve"}else{"prompt"}, input) {
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

    #[test]
    fn continuing_uses_the_saved_goal_and_refuses_a_running_campaign(){
        let root=std::env::temp_dir().join(format!("dominion-rsi-resume-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let mut prior=json!({"id":"old","kind":"rsi_evolve","status":"completed","input":{
            "rsiObjective":"retain the actual user goal","rsiFocusFiles":["src/engine/memory.rs"],"providerId":"p","model":"m"}});
        state.store.put("job",&prior);
        let resumed=continued_request(&state,json!({"continueFrom":"old"})).unwrap();
        assert_eq!(resumed["request"],"retain the actual user goal");assert_eq!(resumed["providerId"],"p");
        assert_eq!(resumed["focusFiles"],json!(["src/engine/memory.rs"]));
        prior["status"]=json!("running");state.store.put("job",&prior);
        assert!(continued_request(&state,json!({"continueFrom":"old"})).is_err());
        let _=std::fs::remove_dir_all(root);
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
        assert_eq!(jobs[0]["input"]["rsiSourceDir"], checkout.canonicalize().unwrap().to_str().unwrap());
        assert!(jobs[0]["input"]["rsiBaseCommit"].as_str().is_some_and(|v|v.len()==40));
        assert_eq!(jobs[0]["input"]["rsiFrozenCases"].as_array().unwrap().len(),13);
        assert!(jobs[0]["input"]["prompt"].as_str().unwrap().contains("workspace_write"));
        assert_eq!(jobs[0]["input"]["rsiAutonomous"], false);
        let auto_response = start(State(state.clone()), Json(json!({
            "providerId":"test-provider","model":"sample-model"
        }))).await;
        assert_eq!(auto_response.status(), StatusCode::ACCEPTED);
        let queued = state.store.all("job");
        assert_eq!(queued.len(), 2);
        let automated = queued.iter().find(|job| job["input"]["rsiAutonomous"] == true).unwrap();
        assert!(automated["input"]["prompt"].as_str().unwrap().contains("전수조사 기반 자율 개선"));
        assert!(automated["input"]["rsiObjective"].as_str().unwrap().contains("실제 작업에 필요한 능력"));
        assert!(automated["input"]["rsiObjective"].as_str().unwrap().contains("변경 없이"));
        std::fs::write(checkout.join("uncommitted.txt"), "owner edits").unwrap();
        let refused = start(State(state.clone()), Json(body)).await;
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.store.all("job").len(), 2);
        // A non-developer can start from an unchanged copy of their current
        // source without committing unrelated files or switching Git branches.
        git(&checkout, &["checkout","-b","ordinary-owner-branch"]);
        let continuous=json!({"providerId":"test-provider","model":"sample-model",
            "mode":"continuous","confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual"});
        let response=start(State(state.clone()),Json(continuous.clone())).await;
        assert_eq!(response.status(),StatusCode::ACCEPTED);
        let queued=state.store.all("job");
        let campaign=queued.iter().find(|job|job["kind"]=="rsi_evolve").unwrap();
        assert_eq!(campaign["input"]["rsiNativeApproved"],true);
        assert_eq!(campaign["input"]["selectionPolicy"],"manual");
        assert!(campaign["input"]["limits"]["tokenBudget"].is_null());
        assert!(campaign["input"]["limits"]["timeBudgetSeconds"].is_null());
        assert_eq!(start(State(state.clone()),Json(continuous)).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(std::fs::read_to_string(checkout.join("uncommitted.txt")).unwrap(),"owner edits");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn full_inspection_reuses_the_index_and_has_no_token_cap() {
        let root = std::env::temp_dir().join(format!("dominion-rsi-index-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let data = root.join("data");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"dominion\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi.rs"), "// source\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi_evolve.rs"),
            "pub fn run_loop() {\n    // TODO: reuse the index\n}\n").unwrap();
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
            "mode":"continuous","confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual",
            "focusFiles":["src/engine/rsi_evolve.rs"],"maxAttempts":1});
        let response = start(State(state.clone()), Json(body)).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        let campaign = jobs.iter().find(|job|job["kind"]=="rsi_evolve").unwrap();
        assert_eq!(campaign["input"]["limits"]["maxAttempts"], 1);
        assert!(campaign["input"]["limits"]["tokenBudget"].is_null());
        assert!(campaign["input"]["limits"]["timeBudgetSeconds"].is_null());
        assert!(campaign["input"]["rsiIndexFiles"].as_u64().unwrap_or(0) >= 3);
        assert_eq!(campaign["input"]["rsiFocusFiles"][0], "src/engine/rsi_evolve.rs");
        let prompt = campaign["input"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("저장된 코드 인덱스"));
        assert!(prompt.contains("src/engine/rsi_evolve.rs"));
        assert!(prompt.contains("run_loop") || prompt.contains("TODO"));
        assert!(!prompt.contains("// TODO: reuse the index"),"source belongs in indexed retrieval, not every prompt");
        assert!(prompt.chars().count()<6000);
        assert!(state.store.all("code_index").len() >= 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn capability_requires_three_consecutive_accepts() {
        let partial = json!({"history":[
            {"accepted":true,"evaluation":{}},
            {"accepted":true,"evaluation":{}},
            {"accepted":false,"evaluation":{}},
            {"accepted":true,"evaluation":{}}
        ]});
        assert_eq!(accepted_streak(&partial), (1, 2));
        let gap = json!({"history":[
            {"accepted":true,"evaluation":{}},
            {"accepted":false,"newFindings":3,"evaluation":null},
            {"accepted":true,"evaluation":{}}
        ]});
        assert_eq!(accepted_streak(&gap), (1,1),
            "Research-only generations must not be skipped between adoptions");
        let mut derived = partial.clone();
        apply_capability(&mut derived);
        assert_eq!(derived["capabilityDemonstrated"], false);

        let verified = json!({"accepted":true,"pairedRevalidated":true,"domain":"memory","evaluation":{
            "candidateStatus":"verified","measurementScope":"frozen_native_cases_only",
            "evidenceSha256":"a".repeat(64),
            "metrics":{"eligible":true,"holdoutCases":2,"regressions":0,
                "holdoutDomains":["context","memory"],"improvedHoldouts":1,"metaGains":0,
                "holdoutEvidence":[
                    {"id":"a","domain":"memory","baselinePass":true,"candidatePass":true,
                        "metricGains":0,"metricRegressions":0,"improved":false},
                    {"id":"b","domain":"context","baselinePass":false,"candidatePass":true,
                        "metricGains":0,"metricRegressions":0,"improved":true}
                ]}}});
        let full = json!({"history":[verified.clone(),verified.clone(),verified.clone()]});
        let mut derived = full.clone();
        apply_capability(&mut derived);
        assert_eq!(derived["maxAcceptedStreak"], 3);
        assert_eq!(derived["capabilityDemonstrated"], false,
            "Three copied results without executable lineage are not recursion");
        let a="a".repeat(40);
        let b="b".repeat(40);
        let c="c".repeat(40);
        let d="d".repeat(40);
        let mut parent=verified.clone();
        parent["domain"]=json!("improver");
        parent["candidateTarget"]=json!("improver");
        parent["generatorCommit"]=json!(a);
        parent["acceptedCommit"]=json!(b);
        let mut child=verified.clone();
        child["domain"]=json!("context");
        child["candidateTarget"]=json!("harness");
        child["generatorCommit"]=json!(b);
        child["acceptedCommit"]=json!(c);
        child["nativePlan"]=json!({"source":"compiled_generator_plan_v1"});
        let mut grandchild=child.clone();
        grandchild["generatorCommit"]=json!(c);
        grandchild["acceptedCommit"]=json!(d);
        let mut lineage=json!({"history":[parent,child,grandchild]});
        apply_capability(&mut lineage);
        assert_eq!(lineage["capabilityDemonstrated"],true);
        let unmeasured = json!({"history":[{"accepted":true},{"accepted":true},{"accepted":true}]});
        let mut derived = unmeasured.clone();
        apply_capability(&mut derived);
        assert_eq!(derived["capabilityDemonstrated"], false, "adoption alone is not proof");
    }
}
