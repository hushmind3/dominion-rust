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
            // Research-only and failed attempts interrupt a consecutive run.
            streak = 0;
        }
    }
    (streak, max)
}

fn apply_capability(state: &AppState, record: &mut Value) {
    let (streak, measured_max) = accepted_streak(record);
    let max = measured_max.max(record["maxAcceptedStreak"].as_u64().unwrap_or(0));
    record["acceptedStreak"] = json!(streak);
    record["maxAcceptedStreak"] = json!(max);
    let proof=engine::recursive_proof_report(state,record);
    engine::attach_recursive_proof(record,&proof);
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

pub async fn report_issue(State(state):State<AppState>,Json(body):Json<Value>)->Response {
    let issue=if let Some(id)=body["signalId"].as_str() {
        let report=crate::engine::rsi_scan::report(&state);
        let Some(signal)=report["signals"].as_array().and_then(|rows|rows.iter().find(|row|row["id"]==id)) else {
            return error("문제 기록을 다시 불러온 뒤 시도하세요.".into());
        };
        let mut issue=signal.clone();
        issue["id"]=json!(format!("issue-{}",signal["evidenceFingerprint"].as_str().unwrap_or(id)));
        issue
    }else{
        let Some(description)=body["description"].as_str().map(str::trim).filter(|s|(5..=2000).contains(&s.len())) else {
            return error("문제 설명은 5~2000자로 입력하세요.".into());
        };
        let Some(session)=body["sessionId"].as_str().and_then(|id|state.store.get("session",id)) else {
            return error("문제가 발생한 대화 기록이 없습니다.".into());
        };
        json!({"kind":"user_report","title":"사용 중 발견한 문제","detail":description,
            "suggestedRequest":"이 대화에서 발생한 문제를 실제 기록과 현재 구현으로 진단한다. 설정·외부 서비스 원인과 코드 결함을 구분하고, 코드 결함이면 수정 후 독립 검증한다.",
            "evidence":[{"sessionId":session["id"],"reportedProblem":description}]})
    };
    let mut issue=issue;issue["status"]=json!("open");
    match state.store.put("improvement_issue",&issue) {
        Some(saved)=>Json(saved).into_response(),None=>error("문제 기록 저장 실패".into())
    }
}

pub async fn issue(State(state):State<AppState>,Path(id):Path<String>)->Response {
    match state.store.get("improvement_issue",&id) {
        Some(issue)=>Json(issue).into_response(),None=>error("저장된 문제를 찾지 못했습니다.".into())
    }
}

pub async fn evaluation_profiles(State(state):State<AppState>)->Response {
    match configured_source(&state).await {
        Ok(root)=>Json(crate::engine::rsi_profile::catalogue(&root)).into_response(),
        Err(message)=>error(message),
    }
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    let campaigns=state.store.all("rsi_campaign").into_iter().take(20)
        .map(|mut campaign| {
            if let Some(job)=campaign["id"].as_str().and_then(|id|state.store.get("job",id)) {
                campaign["systemEvolution"]=job["input"]["rsiSystemEvolution"].clone();
            }
            apply_capability(&state,&mut campaign);
            campaign
        }).collect::<Vec<_>>();
    Json(json!({"version":"rsi-v2","candidates":engine::list(&state),
        "campaigns":campaigns,"minPairedCases":engine::MIN_PAIRED_CASES}))
}

pub async fn signals(State(state): State<AppState>) -> Json<Value> {
    let report = crate::engine::rsi_scan::report(&state);
    let idle=!state.store.all("job").iter().any(|j|matches!(j["status"].as_str(),Some("running"|"queued")))
        && !state.store.all("execution").iter().any(|e|e["status"]=="running");
    Json(json!({
        "recommendationReady":idle,
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

fn is_rsi_evaluation_episode(job:&Value)->bool {
    job["rsiEvaluationEpisode"]==true || job["input"]["rsiMetaEpisode"]==true
}

#[cfg(test)]
include!("../../tests/support/api/rsi.tests.rs");

fn campaign_view(state: &AppState, job: &Value) -> Value {
    let id = job["id"].as_str().unwrap_or("");
    let mut record=state.store.get("rsi_campaign",id)
        .unwrap_or_else(||json!({"id":id,"phase":"preparing_lab"}));
    for (field,key) in [("objective","rsiObjective"),("providerId","providerId"),("model","model"),
        ("applyWhenFinished","rsiApplyWhenFinished")] {
        record[field]=job["input"][key].clone();
    }
    record["selectedVersionRunning"]=json!(record["headCommit"].as_str()==option_env!("DOMINION_BUILD_COMMIT"));
    record["evaluationProfile"]=crate::engine::rsi_profile::summary(&crate::engine::rsi_profile::from_input(&job["input"]).unwrap_or(Value::Null));
    record["systemEvolution"]=job["input"]["rsiSystemEvolution"].clone();
    apply_capability(state, &mut record);
    let children=state.store.all("job").into_iter().filter(|j|
        j["kind"]=="rsi_generation" && j["input"]["rsiCampaignId"].as_str()==Some(id)).collect::<Vec<_>>();
    let sessions=children.iter().filter_map(|j|j["sessionId"].as_str()).collect::<std::collections::HashSet<_>>();
    let mut calls=0u64;let mut tokens=0u64;let mut estimated=false;
    for session in &sessions {
        let usage=state.store.session_usage(session);
        calls+=usage["calls"].as_u64().unwrap_or(0);
        tokens+=usage["totalTokens"].as_u64().unwrap_or(0);
        estimated|=usage["estimatedCalls"].as_u64().unwrap_or(0)>0;
    }
    let writes=state.store.all("execution_operation").iter().filter(|op|
        op["sessionId"].as_str().is_some_and(|id|sessions.contains(id))
        && op["origin"]=="model" && op["status"]=="completed"
        && op["result"]["written"]==true && op["toolReceipt"]["verified"]==true
        && matches!(op["tool"].as_str(),Some("workspace_replace"|"workspace_replace_range"|"workspace_write"))).count();
    let proposal_sessions=children.iter().filter(|j|!is_rsi_evaluation_episode(j))
        .filter_map(|j|j["sessionId"].as_str()).collect::<std::collections::HashSet<_>>();
    let system_writes=state.store.all("execution_operation").iter().filter(|op|
        op["sessionId"].as_str().is_some_and(|id|proposal_sessions.contains(id))
        && op["origin"]=="model" && op["status"]=="completed"
        && op["result"]["written"]==true && op["toolReceipt"]["verified"]==true).count();
    record["systemSourceWrites"]=json!(system_writes);
    record["evaluationEpisodes"]=json!(children.iter().filter(|j|is_rsi_evaluation_episode(j)).count());
    let candidates=state.store.all("rsi_candidate").iter().filter(|c|
        c["experimentCampaignId"].as_str()==Some(id) && c["rsiJobId"].as_str()
            .and_then(|id|state.store.get("job",id)).is_some_and(|j|!is_rsi_evaluation_episode(&j))).count();
    let mut reused=0usize;
    for session in &sessions {
        reused+=state.store.get("session",session).and_then(|s|s["rsiKnowledgeReuse"].as_array().cloned())
            .unwrap_or_default().iter().filter(|r|r["status"]=="provided").count();
    }
    record["reusedArtifacts"]=json!(reused);
    record["preservedFailureAttempts"]=json!(record["history"].as_array().into_iter().flatten()
        .filter(|row|row["accepted"]!=true && row["error"].as_str().is_some_and(|s|!s.is_empty())).count());
    record["modelCalls"]=json!(calls);record["verifiedSourceWrites"]=json!(writes);
    record["candidateCount"]=json!(candidates);
    record["tokens"]=json!(tokens.max(record["tokens"].as_u64().unwrap_or(0)));
    record["tokensEstimated"]=json!(estimated || record["tokensEstimated"]==true);
    record["systemEvolution"]=job["input"]["rsiSystemEvolution"].clone();
    record["scope"]=json!({"mode":"whole_source","includes":["algorithms","improver","harness","memory","learning","tools","backend","frontend"],"indexedFiles":job["input"]["rsiIndexFiles"]});
    if record["phase"]=="measuring" && job["rsiPhase"]=="meta_evaluating" {
        record["phase"]=json!("meta_evaluating");
        record["metaEvaluationProgress"]=json!({"arm":job["rsiMetaArm"],"task":job["rsiMetaTask"],"jobId":job["rsiMetaActiveJobId"]});
    }
    if let Some(active)=children.iter().find(|j|Some(&j["id"])==record.get("activeGenerationJobId")) {
        record["activeWorker"]=json!({"id":active["id"],"status":active["status"],
            "phase":active["rsiPhase"],"generatorCommit":active["input"]["rsiGeneratorCommit"],
            "focusFiles":active["input"]["rsiFocusFiles"]});
        if record["phase"]=="diagnosing_and_editing" && active["status"]=="running"
            && active["rsiPhase"].is_string() {record["phase"]=active["rsiPhase"].clone();}
    }
    if job["status"]!="running"||record["status"].is_null(){record["status"]=job["status"].clone();}
    if !job["error"].is_null(){record["error"]=job["error"].clone();}
    record
}

pub async fn campaign(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(job)=state.store.get("job",&id).filter(|job|job["kind"]=="rsi_evolve")else{
        return error("개선 작업을 찾을 수 없습니다.".into());
    };
    Json(campaign_view(&state,&job)).into_response()
}

pub async fn verify(State(state):State<AppState>,Path(id):Path<String>)->Response{
    let Some(campaign)=state.store.get("rsi_campaign",&id) else {
        return error("캠페인 기록을 찾을 수 없습니다.".into());
    };
    let mut authoritative=campaign;
    if let Some(job)=state.store.get("job",&id) {
        authoritative["systemEvolution"]=job["input"]["rsiSystemEvolution"].clone();
    }
    Json(engine::capability_audit(&state,&authoritative)).into_response()
}

pub async fn campaigns(State(state):State<AppState>)->Json<Value>{
    let rows=state.store.all("job").into_iter().filter(|job|job["kind"]=="rsi_evolve")
        .take(20).map(|job| {
            let record=state.store.get("rsi_campaign",job["id"].as_str().unwrap_or("")).unwrap_or(Value::Null);
            let mut view=record;
            view["systemEvolution"]=job["input"]["rsiSystemEvolution"].clone();
            apply_capability(&state,&mut view);
            json!({"id":job["id"],"status":job["status"],"phase":view["phase"],
                "model":job["input"]["model"],"providerId":job["input"]["providerId"],
                "createdAt":job["queuedAt"],"acceptedGenerations":view["acceptedGenerations"],
                "capabilityDemonstrated":view["capabilityDemonstrated"],
                "generationCapabilityProven":view["generationCapabilityProven"]})
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
    for (field,key) in [("request","rsiObjective"),("focusFiles","rsiFocusFiles"),("providerId","providerId"),("model","model"),("issueId","rsiIssueId")] {
        if body[field].is_null() || body[field].as_str()==Some("") {body[field]=previous["input"][key].clone();}
    }
    Ok(body)
}

pub async fn start(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if crate::engine::dev::installation_running() {return error("버전 반영이 끝난 뒤 발전 작업을 시작하세요.".into());}
    let body=match continued_request(&state,body){Ok(body)=>body,Err(message)=>return error(message)};
    let observed_issue=if let Some(id)=body["issueId"].as_str() {
        match state.store.get("improvement_issue",id) {Some(issue)=>issue,None=>return error("문제 기록을 찾지 못했습니다.".into())}
    }else{Value::Null};
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
    if body["localOnly"]==true && provider["local"]!=true && provider["kind"]!="ollama" {
        return error("로컬 전용 모드에서는 로컬로 등록된 제공자를 선택하세요.".into());
    }
    if provider["enabled"] == false {
        return error("비활성화된 모델 제공자입니다.".into());
    }
    let status = crate::engine::dev::status(&state).await;
    if status["git"] != true
        || !status["branch"].as_str().is_some_and(crate::engine::rsi_worktree::rsi_work_branch) {
        return error("서브3 자기개선은 dominion-sub3 원본 브랜치에서만 시작합니다. 개발자 설정에서 해당 소스 폴더를 연결하세요.".into());
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
    let objective = if autonomous && continuous && body["evolutionTarget"] != "product" && body["nativeInduction"] != true {
        "DOMINION 전체 소스의 알고리즘·하네스·학습·기억·도구를 탐색하여 자기개선기 자체를 수정하고, 선택한 생성 모델과 동일 예산에서 이전 개선기보다 나은 후속 코딩 성과를 독립 평가한다.".to_string()
    } else if autonomous {
        "기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다. 필요한 부분만 확인하고 적용 조건과 검증 근거가 있는 변경을 제안한다. 오류 수정은 이 과정에서 필요할 때 수행하며, 개선 근거가 없으면 변경 없이 이유를 보고한다.".to_string()
    } else {
        request.to_string()
    };
    let source_dir = source.to_string_lossy().to_string();
    let source_branch = status["branch"].as_str().unwrap_or("");
    if let Some(id)=body["continueFrom"].as_str() {
        let prior=state.store.get("job",id).unwrap_or(Value::Null);
        let prior_source=prior["input"]["rsiSourceDir"].as_str().and_then(|s|std::fs::canonicalize(s).ok());
        if prior_source.as_ref()!=Some(&source) {return error("다른 소스의 작업은 이어서 진행할 수 없습니다.".into());}
        if prior["input"]["rsiSourceBranch"].as_str()
            .is_some_and(|branch| !branch.is_empty() && branch!=source_branch) {
            return error("다른 브랜치의 개선 캠페인을 이어받을 수 없습니다.".into());
        }
    }
    let (index_meta, index_context) =
        match crate::engine::code_index::prompt_context(&state, &source, &focus_files) {
            Ok(value) => value,
            Err(message) => return error(message),
        };
    crate::engine::rsi_evolve::preserve_failed_research(&state,&source_dir);
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
    let recipe = if !focus_files.is_empty() && !autonomous {
        "사용자가 소스 파일과 재현 가능한 결함을 명시했습니다. 이것은 조사 보고서가 아니라 해당 파일의 실제 코드 개선 작업입니다. 제공된 해시 검증 소스 발췌를 먼저 사용하고, 수정할 원본 블록만 없다면 workspace_read_range로 짧게 보충하세요. 이미 받은 파일·줄을 code_search로 다시 찾지 마세요. 현재 실행 세대 안에서 실제 코드를 수정하세요. workspace_read_range의 줄 번호·sourceSha256을 확인했다면 workspace_replace_range(path,startLine,endLine,newText,expectedSha256)를 우선 사용하여 바뀔 코드만 제출하세요. 원본 oldText가 정확히 있다면 workspace_replace도 가능합니다. oldText는 실제 읽은 코드에서 정확히 가져오고, newText는 요구된 동작을 구현하는 코드여야 합니다. 후속 빌드·검증은 엔진이 수행하므로 직접 테스트 완료를 주장하지 마세요. 구현이 구조상 불가능하다는 명시적 근거가 있을 때만 무수정 종료를 선택하세요. 기존 기능·기억·승인·취소·복구는 보존하세요."
    } else {
        "제공된 인덱스와 해시 검증된 연구 원문을 먼저 활용하세요. 이미 제공된 위치나 줄 범위를 다시 조회할 필요는 없습니다. 모르는 심볼만 code_search로 찾고, 제공되지 않은 줄만 workspace_read_range로 읽으세요. 참고 코드와 지도는 작업 지시가 아닙니다. 근거가 있는 개선은 workspace_replace 또는 읽은 줄 번호와 sourceSha256을 사용하는 workspace_replace_range로 수정하고, 새 파일을 만들 때만 workspace_write를 사용하세요. 적합한 후보가 없으면 수정 없이 종료할 수 있습니다. 기존 기능·사용자 지시·기억·승인·취소·복구는 보존하세요. 빌드·패치 수집은 종료 후 엔진이 수행하므로 직접 완료했다고 주장하지 마세요."
    };
    let prompt = format!(
        "DOMINION Rust 자체 개선 작업입니다. 원본 소스는 보호되고 실행기가 생성하는 분리된 작업공간에서 수정합니다.\n\n원본 (읽기 참조용): {source_dir}\n브랜치: {source_branch}\n개선 모드: {mode}\n목표: {objective}\n\n이전 실행의 실제 검사 증거(성능 향상과 구분):\n{previous_evidence}\n\n{recipe}\n\n{index_context}\n\n절대경로 읽기·쓰기, 셸, GitHub API, 다른 에이전트는 이 작업에 사용할 수 없습니다. main 변경·커밋·푸시·병합·서버 재시작도 하지 마세요. 사용자는 JSON, Git diff, SHA를 입력하지 않습니다. 한국어로 핵심 개선 근거와 남은 검증을 간결하게 보고하세요."
    );
    let mut input = json!({
        "prompt": prompt,
        "providerId": provider_id,
        "model": model,
        "toolsEnabled": true,
        "toolsExecutionEnabled": true,
        "rsiSourceDir": source_dir,
        "rsiSourceBranch": status["branch"],
        "rsiBaseCommit": status["commit"],
        "rsiObjective": objective,
        "rsiIssueId":observed_issue["id"],"rsiIssueEvidence":observed_issue,
        "rsiAutonomous": autonomous,
        "rsiLocalOnly":body["localOnly"]==true,
        "rsiSystemEvolution": continuous && body["evolutionTarget"] != "product" && body["nativeInduction"] != true,
        // User-approved model routing belongs to DOMINION's API harness,
        // NOT to trainable weights. No routing opt-in means no model changes.
        "rsiAdaptiveApiRouting":continuous && autonomous
            && body["allowAdaptiveApiRouting"]==true,
        "rsiOriginalApiRoute":{"providerId":provider_id,"model":model},
        "rsiApplyWhenFinished": continuous && body["applyWhenFinished"] == true,
        // Only explicitly opted-in automatic campaigns may broaden the
        // source search after independent adoption of the original goal.
        "rsiExpandAfterGain": continuous && body["expandAfterGain"] == true
            && body["selectionPolicy"] == "automatic",
        "rsiNativeInduction": continuous && body["nativeInduction"] == true
            && body["selectionPolicy"] == "automatic",
        "rsiStopAfterProof": continuous && body["stopAfterProof"] == true
            && body["selectionPolicy"] == "automatic",
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
    if input["rsiAdaptiveApiRouting"]==true {
        let original=input["rsiOriginalApiRoute"].clone();
        input["rsiApprovedApiRoutes"]=
            match crate::engine::rsi_api_route_learning::authorized(
                &state,&original,&body["approvedApiRoutes"],
                body["localOnly"]==true) {
                Ok(routes)=>routes,
                Err(message)=>return error(message)
            };
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
    if input["rsiSystemEvolution"]==true {
        let requested=body["evaluationProfileId"].as_str().unwrap_or("default");
        let previous=body["continueFrom"].as_str().and_then(|id|state.store.get("job",id))
            .map(|job|job["input"]["rsiEvaluationProfile"].clone()).filter(|v|!v.is_null());
        let profile=if let Some(previous)=previous {
            if previous["id"]!=requested {return error("평가 프로필을 바꾸려면 새 작업으로 시작하세요.".into());}
            match crate::engine::rsi_profile::from_input(&json!({"rsiEvaluationProfile":previous})) {
                Ok(profile)=>profile,Err(message)=>return error(message)
            }
        }else{
            match crate::engine::rsi_profile::load(std::path::Path::new(&source_dir),requested) {
                Ok(profile)=>profile,Err(message)=>return error(message)
            }
        };
        cases.extend(crate::engine::rsi_meta::cases_for(&profile));
        input["rsiEvaluationProfile"]=profile;
    }
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
    async fn problem_handoff_preserves_context_without_starting_a_model() {
        let root=std::env::temp_dir().join(format!("rsi-issue-{}",uuid::Uuid::new_v4()));
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"affected-chat","title":"Actual user task"}));
        let response=report_issue(State(state.clone()),Json(json!({"sessionId":"affected-chat","description":"The tool did not perform the requested action"}))).await;
        assert_eq!(response.status(),StatusCode::OK);
        let issues=state.store.all("improvement_issue");assert_eq!(issues.len(),1);
        assert_eq!(issues[0]["evidence"][0]["sessionId"],"affected-chat");
        assert_eq!(state.store.count("job"),0,"opening a repair request must never spend model tokens");
        state.store.put("job",&json!({"id":"prior-issue-run","kind":"rsi_evolve","status":"failed","input":{
            "rsiObjective":"redesign the action planner","rsiIssueId":issues[0]["id"],"providerId":"old","model":"small"}}));
        let resumed=continued_request(&state,json!({"continueFrom":"prior-issue-run","providerId":"new","model":"larger"})).unwrap();
        assert_eq!(resumed["issueId"],issues[0]["id"]);
        assert_eq!(resumed["providerId"],"new");assert_eq!(resumed["model"],"larger");
        assert_eq!(resumed["request"],"redesign the action planner");
        let rejected=report_issue(State(state),Json(json!({"sessionId":"missing","description":"Unbound failure report"}))).await;
        assert_eq!(rejected.status(),StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn natural_language_start_queues_model_only_on_owned_sub3_branch() {
        let root = std::env::temp_dir().join(format!("dominion-rsi-start-{}", uuid::Uuid::new_v4()));
        let checkout = root.join("checkout");
        let data = root.join("data");
        std::fs::create_dir_all(checkout.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(checkout.join("Cargo.toml"), "[package]\nname = \"dominion\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(checkout.join("src/engine/rsi.rs"), "// source\n").unwrap();
        git(&checkout, &["init", "-q"]);
        git(&checkout, &["checkout", "-B", "main"]);
        git(&checkout, &["add", "."]);
        git(&checkout, &["-c", "user.email=ci@example.com", "-c", "user.name=CI", "commit", "-qm", "fixture"]);
        let cfg = Config { port:0, data_dir:data.clone(), web_dir:data.join("web"), workspace:data.clone() };
        let state = AppState::new(cfg.clone(), Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state, &json!({"sourceDir":checkout.to_str().unwrap()}));
        state.store.put("provider", &json!({"id":"test-provider","name":"Local","enabled":true,
            "kind":"openai","model":"sample-model","baseUrl":"http://127.0.0.1:1/v1"}));
        let body = json!({"providerId":"test-provider","model":"sample-model",
            "request":"100턴에서 장기기억이 사라지는 문제를 찾아 고쳐 주세요."});
        let denied_manual = start(State(state.clone()), Json(body.clone())).await;
        assert_eq!(denied_manual.status(), StatusCode::BAD_REQUEST,
            "main cannot start sub3 manual RSI");
        let denied_continuous = start(State(state.clone()), Json(json!({
            "providerId":"test-provider","model":"sample-model","mode":"continuous",
            "confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual"
        }))).await;
        assert_eq!(denied_continuous.status(), StatusCode::BAD_REQUEST,
            "main cannot start sub3 continuous RSI");
        assert!(state.store.all("job").is_empty(),"main must not queue any RSI job");
        git(&checkout,&["checkout","-b","dominion-sub3"]);
        let response = start(State(state.clone()), Json(body.clone())).await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let jobs = state.store.all("job");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["input"]["providerId"], "test-provider");
        assert_eq!(jobs[0]["input"]["model"], "sample-model");
        assert_eq!(jobs[0]["input"]["rsiSourceDir"], checkout.canonicalize().unwrap().to_str().unwrap());
        assert_eq!(jobs[0]["input"]["rsiSourceBranch"], "dominion-sub3");
        assert!(jobs[0]["input"]["rsiBaseCommit"].as_str().is_some_and(|v|v.len()==40));
        let cases=jobs[0]["input"]["rsiFrozenCases"].as_array().unwrap();
        assert_eq!(cases.len(),14);
        assert!(cases.iter().any(|case|case["id"]=="planning-development" && case["holdout"]==false));
        assert!(cases.iter().any(|case|case["id"]=="planning-quality" && case["holdout"]==true));
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
        // Even the continuous mode must honor branch ownership and a clean
        // source, not bypass the manual RSI branch gate.
        git(&checkout, &["checkout","-b","ordinary-owner-branch"]);
        let continuous=json!({"providerId":"test-provider","model":"sample-model",
            "mode":"continuous","confirmation":"START_RSI_EVOLUTION","selectionPolicy":"manual"});
        assert_eq!(start(State(state.clone()),Json(continuous.clone())).await.status(),
            StatusCode::BAD_REQUEST,"foreign branch must not queue a campaign");
        git(&checkout,&["checkout","dominion-sub3"]);
        assert_eq!(start(State(state.clone()),Json(continuous.clone())).await.status(),
            StatusCode::BAD_REQUEST,"uncommitted changes must not start continuous evolution");
        std::fs::remove_file(checkout.join("uncommitted.txt")).unwrap();
        let response=start(State(state.clone()),Json(continuous.clone())).await;
        assert_eq!(response.status(),StatusCode::ACCEPTED);
        let queued=state.store.all("job");
        let campaign=queued.iter().find(|job|job["kind"]=="rsi_evolve").unwrap();
        assert_eq!(campaign["input"]["rsiNativeApproved"],true);
        assert_eq!(campaign["input"]["selectionPolicy"],"manual");
        assert!(campaign["input"]["limits"]["tokenBudget"].is_null());
        assert!(campaign["input"]["limits"]["timeBudgetSeconds"].is_null());
        assert_eq!(start(State(state.clone()),Json(continuous)).await.status(),StatusCode::BAD_REQUEST);
        assert!(!checkout.join("uncommitted.txt").exists(),
            "the final owned-source checkout must be clean");
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
        git(&checkout, &["checkout", "-b", "dominion-sub3"]);
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


