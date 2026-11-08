//! Owner-centered room API. Room sessions and worker checkpoints are server
//! state; clients can change the roster and policy, never rebind a transcript.
use crate::engine::{memory, rooms};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub(crate) type ApiResult = Result<Value, (StatusCode, String)>;
fn response(result: ApiResult) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err((status, error)) => (status, Json(json!({"error":error}))).into_response(),
    }
}
fn bad(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, message.into())
}
fn failed(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, message.into())
}
fn conflict(message: impl Into<String>) -> (StatusCode, String) {
    (StatusCode::CONFLICT, message.into())
}
fn room(state: &AppState, id: &str) -> Result<Value, (StatusCode, String)> {
    state
        .store
        .get("room", id)
        .ok_or((StatusCode::NOT_FOUND, "회의실을 찾을 수 없습니다.".into()))
}
fn lock(state: &AppState, id: &str) -> Result<crate::state::SessionGuard, (StatusCode, String)> {
    let key = format!("room-submit:{id}");
    if !state.begin_session(&key) {
        return Err(conflict("회의실 상태가 변경 중입니다. 다시 시도하세요."));
    }
    Ok(state.session_guard(&key))
}
fn text_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: &str,
    limit: usize,
) -> Result<String, (StatusCode, String)> {
    let value = body.get(key).or_else(|| previous.get(key));
    let text = match value {
        Some(value) => value
            .as_str()
            .ok_or_else(|| bad(format!("{key}는 문자열이어야 합니다.")))?,
        None => default,
    }
    .trim();
    if text.chars().count() > limit {
        return Err(bad(format!("{key}는 {limit}자 이하여야 합니다.")));
    }
    Ok(text.into())
}
fn bool_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: bool,
) -> Result<bool, (StatusCode, String)> {
    body.get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| bad(format!("{key}는 true/false여야 합니다.")))
        })
        .unwrap_or(Ok(default))
}
fn number_field(
    body: &Value,
    previous: &Value,
    key: &str,
    default: u64,
    max: u64,
) -> Result<u64, (StatusCode, String)> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 양의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(default))?;
    if number == 0 || number > max {
        return Err(bad(format!("{key} 범위는 1~{max}입니다.")));
    }
    Ok(number)
}
fn number_field_allow_zero(
    body: &Value,
    previous: &Value,
    key: &str,
    max: u64,
) -> Result<u64, (StatusCode, String)> {
    let number = body
        .get(key)
        .or_else(|| previous.get(key))
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| bad(format!("{key}는 0 이상의 정수여야 합니다.")))
        })
        .unwrap_or(Ok(0))?;
    if number > max {
        return Err(bad(format!("{key} 범위는 0~{max}입니다.")));
    }
    Ok(number)
}
fn validated_config(state: &AppState, id: &str, previous: &Value, body: &Value) -> ApiResult {
    if !body.is_object() {
        return Err(bad("회의실 객체가 필요합니다."));
    }
    if let Some(binding) = body.get("sessionId") {
        if binding != &previous["sessionId"] {
            return Err(bad("회의실의 대화 세션은 변경할 수 없습니다."));
        }
    }
    for reserved in [
        "roomState",
        "activeAgentId",
        "status",
        "generation",
        "cancelRequested",
        "ownerId",
        "partitionKey",
    ] {
        if body.get(reserved).is_some() {
            return Err(bad(format!("{reserved}는 서버가 관리하는 상태입니다.")));
        }
    }
    let name = text_field(body, previous, "name", "새 회의실", 160)?;
    if name.is_empty() {
        return Err(bad("회의실 이름이 필요합니다."));
    }
    let source = body
        .get("agentIds")
        .or_else(|| previous.get("agentIds"))
        .and_then(Value::as_array)
        .ok_or_else(|| bad("참가 에이전트 목록이 필요합니다."))?;
    if source.is_empty() || source.len() > 32 {
        return Err(bad("참가자는 1~32명이어야 합니다."));
    }
    let mut known = HashSet::new();
    let mut agent_ids = Vec::new();
    for value in source {
        let id = value
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| bad("유효한 에이전트 ID가 필요합니다."))?;
        if state.store.get("agent", id).is_none() {
            return Err(bad(format!("참가 에이전트를 찾을 수 없습니다: {id}")));
        }
        if known.insert(id.to_string()) {
            agent_ids.push(id.to_string());
        }
    }
    let routing = text_field(body, previous, "routingMode", "discussion", 32)?;
    if !matches!(routing.as_str(), "discussion" | "round_robin" | "mentioned") {
        return Err(bad("지원하지 않는 발언 방식입니다."));
    }
    // Owner-picked model pool. FreeLLMAPI exposes dozens of models; the owner
    // checks the ones they trust once and every turn rotates inside that list.
    let models_source = body
        .get("models")
        .or_else(|| previous.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if models_source.len() > 64 {
        return Err(bad("방 모델은 최대 64개까지 선택할 수 있습니다."));
    }
    let mut models = Vec::new();
    let mut seen_models = HashSet::new();
    for entry in &models_source {
        let provider_id = entry
            .get("providerId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad("방 모델에 providerId가 필요합니다."))?;
        let model = entry
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 200)
            .ok_or_else(|| bad("방 모델에 model 이름이 필요합니다."))?;
        let provider = state
            .store
            .get("provider", provider_id)
            .ok_or_else(|| bad(format!("제공자를 찾을 수 없습니다: {provider_id}")))?;
        if crate::engine::gateway::is_local_runtime(&provider) {
            return Err(bad("로컬 런타임 모델은 방 모델로 선택할 수 없습니다."));
        }
        let effort = entry
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !valid_effort(effort) {
            return Err(bad("추론 강도는 none/minimal/low/medium/high 중 하나여야 합니다."));
        }
        if seen_models.insert(format!("{provider_id}|{model}")) {
            let mut picked = json!({"providerId":provider_id,"model":model});
            if !effort.is_empty() {
                picked["reasoningEffort"] = json!(effort);
            }
            models.push(picked);
        }
    }
    let max_turns = body
        .get("maxTurns")
        .and_then(Value::as_i64)
        .or_else(|| previous.get("maxTurns").and_then(Value::as_i64))
        .unwrap_or(0);
    if !(0..=100_000).contains(&max_turns) {
        return Err(bad("총 AI 발언 상한은 0~100000 사이여야 합니다."));
    }
    let reasoning_effort=text_field(body,previous,"reasoningEffort","",16)?;
    if !valid_effort(&reasoning_effort) {
        return Err(bad("추론 강도는 none/minimal/low/medium/high 중 하나여야 합니다."));
    }
    Ok(json!({
        "id":id,"name":name,"agentIds":agent_ids,"routingMode":routing,
        "description":text_field(body,previous,"description","",4000)?,
        "topic":text_field(body,previous,"topic","",16000)?,
        "ownerName":text_field(body,previous,"ownerName","나",100)?,
        "rounds":number_field(body,previous,"rounds",3,100_000)?,
        "maxTurns":max_turns,
        "tokenBudget":number_field_allow_zero(body,previous,"tokenBudget",10_000_000)?,
        "contextBudgetTokens":number_field(body,previous,"contextBudgetTokens",2_000,1_000_000)?,
        "toolsEnabled":bool_field(body,previous,"toolsEnabled",false)?,
        "autoLearning":bool_field(body,previous,"autoLearning",true)?,
        "autoModels":bool_field(body,previous,"autoModels",true)?,
        "reasoningEffort":reasoning_effort,
        "models":models,
    }))
}
fn valid_effort(value: &str) -> bool {
    matches!(value, "" | "none" | "minimal" | "low" | "medium" | "high")
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
}
fn save_room(state: &AppState, id_override: Option<&str>, body: &Value) -> ApiResult {
    let chosen = id_override.or_else(|| body["id"].as_str());
    let id = chosen
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if !valid_id(&id) {
        return Err(bad("유효하지 않은 회의실 ID입니다."));
    }
    if let Some(body_id) = body["id"].as_str() {
        if body_id != id {
            return Err(bad("경로와 본문의 회의실 ID가 다릅니다."));
        }
    }
    let _guard = lock(state, &id)?;
    let previous = state.store.get("room", &id).unwrap_or(Value::Null);
    if id_override.is_some() && previous.is_null() {
        return Err((StatusCode::NOT_FOUND, "회의실을 찾을 수 없습니다.".into()));
    }
    let config = validated_config(state, &id, &previous, body)?;
    let saved = if previous.is_null() {
        state.store.put("room", &config)
    } else {
        state.store.patch_if("room", &id, "id", &json!(id), &config)
    }
    .ok_or_else(|| failed("회의실을 저장할 수 없습니다."))?;
    state.bus.record("room", &saved);
    rooms::ensure_room_session(state, &id).map_err(bad)?;
    room(state, &id)
}

pub async fn list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("room")))
}
pub async fn save(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    response(save_room(&state, None, &body))
}
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(save_room(&state, Some(&id), &body))
}
pub async fn one(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(room(&state, &id))
}

pub async fn messages(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        Ok(json!(state.store.messages(session_id)))
    })())
}

pub async fn system_prompt(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    response((|| {
        room(&state, &id)?;
        crate::engine::chat::system_prompt_preview(&state, &id, query.get("agentId").map(String::as_str))
            .map_err(bad)
    })())
}

pub async fn knowledge(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        Ok(json!(crate::engine::room_knowledge::list(&state, &id)))
    })())
}

pub async fn learn(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let through = state
            .store
            .messages(session_id)
            .last()
            .and_then(|message| message["id"].as_str().map(str::to_string))
            .ok_or_else(|| bad("학습할 대화가 없습니다."))?;
        crate::engine::room_knowledge::enqueue(&state, &id, session_id, &through)
            .ok_or_else(|| failed("방 학습 작업을 등록할 수 없습니다."))
    })())
}

fn submit(state: &AppState, id: &str, body: &Value) -> ApiResult {
    submit_internal(state, id, body, None, true)
}
pub(crate) fn submit_internal(
    state: &AppState,
    id: &str,
    body: &Value,
    origin_metadata: Option<Value>,
    tools_allowed: bool,
) -> ApiResult {
    if !body.is_object() {
        return Err(bad("메시지 객체가 필요합니다."));
    }
    let _guard = lock(state, id)?;
    let mut config = room(state, id)?;
    if !tools_allowed {
        config["toolsEnabled"] = json!(false);
    }
    if origin_metadata
        .as_ref()
        .is_some_and(|origin| origin["authorIsBot"] == true)
    {
        return Err(bad("봇 발언은 방 주인의 메시지로 제출할 수 없습니다."));
    }
    let text = body
        .get("input")
        .or_else(|| body.get("goal"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let attachment_ids = crate::engine::attachments::ids(&body["attachmentIds"]).map_err(bad)?;
    if text.is_empty() && attachment_ids.is_empty() || text.chars().count() > 32000 {
        return Err(bad("메시지는 1~32000자여야 합니다."));
    }
    if body.get("sessionId").is_some()
        || body.get("metadata").is_some()
        || body.get("role").is_some()
        || body.get("agentId").is_some()
    {
        return Err(bad("화자와 세션 정보는 서버가 정합니다."));
    }
    let session = rooms::ensure_room_session(state, id).map_err(bad)?;
    let session_id = session["id"]
        .as_str()
        .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
    let attachments = crate::engine::attachments::validate_ids(state, session_id, &attachment_ids)
        .map_err(bad)?;
    let settings=crate::engine::chat::TurnRequest::from_value(&json!({"input":if text.is_empty(){"첨부 파일"}else{text},"thinking":body["thinking"],"reasoningEffort":body["reasoningEffort"],"providerId":body["providerId"],"model":body["model"]})).map_err(bad)?;
    if let Some(provider) = settings.provider_id.as_deref() {
        if !state
            .store
            .get("provider", provider)
            .is_some_and(|provider| provider["enabled"].as_bool() != Some(false))
        {
            return Err(bad("활성 제공자를 찾을 수 없습니다."));
        }
    }
    if settings
        .model
        .as_ref()
        .is_some_and(|model| model.len() > 256)
    {
        return Err(bad("모델 이름이 너무 깁니다."));
    }
    if !body["reasoningLanguage"].is_null()
        && !matches!(body["reasoningLanguage"].as_str(), Some("ko" | "default"))
    {
        return Err(bad("추론 언어는 ko 또는 default여야 합니다."));
    }
    let nonce = if let Some(value) = body.get("clientMessageId") {
        let nonce = value
            .as_str()
            .filter(|value| !value.is_empty() && value.len() <= 160)
            .ok_or_else(|| bad("clientMessageId는 1~160자의 문자열이어야 합니다."))?;
        format!("room-message-{}", memory::stable_id(&[id, nonce]))
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    let job_id = format!("room-job-{}", memory::stable_id(&[id, &nonce]));
    if let Some(existing) = state.store.get("job", &job_id) {
        if existing["input"]["input"] != text
            || existing["input"]["requestedAgentIds"] != body["agentIds"]
            || existing["input"]["attachmentIds"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                != json!(attachment_ids)
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            || ["thinking", "reasoningEffort", "reasoningLanguage", "providerId", "model"]
                .iter()
                .any(|key| existing["input"][*key] != body[*key])
        {
            return Err(conflict(
                "같은 clientMessageId로 다른 메시지를 보낼 수 없습니다.",
            ));
        }
        let message = state
            .store
            .message(&nonce)
            .ok_or_else(|| failed("저장된 메시지를 찾을 수 없습니다."))?;
        return Ok(
            json!({"id":job_id,"jobId":job_id,"status":existing["status"],"room":config,"sessionId":session_id,"message":message,"job":existing}),
        );
    }
    let speakers = rooms::plan_speakers(state, &config, text, body.get("agentIds")).map_err(bad)?;
    let generation = state.cancellation_generation(session_id);
    let acceptance=crate::engine::chat::prepare_owner_action(state,text).map(|(request,checks)|json!({"request":request,"checks":checks}));
    let message = json!({"id":nonce,"session_id":session_id,"role":"user","agent_id":null,"content":text,
        "metadata":{"acceptance":acceptance,"authorKind":"owner","roomId":id,"ownerName":config["ownerName"],"clientMessageId":body["clientMessageId"],"attachments":attachments,"origin":origin_metadata}});
    let job = json!({"id":job_id,"kind":"room","status":"queued","attempts":0,"stepIndex":0,"stepTotal":0,"events":[],
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "sessionId":session_id,"input":{"roomId":id,"sessionId":session_id,"messageId":nonce,"sourceMessageId":nonce,"input":text,
            "agentIds":speakers,"requestedAgentIds":body["agentIds"],"explicitAgentIds":body.get("agentIds").is_some(),"routingMode":config["routingMode"],"generation":generation,"roomSnapshot":config,
            "attachmentIds":attachment_ids,"reasoningLanguage":body["reasoningLanguage"],"thinking":body["thinking"],"reasoningEffort":body["reasoningEffort"],"providerId":body["providerId"],"model":body["model"]}});
    let (saved_message, saved_job) = state
        .store
        .write_message_with_record(&message, "job", &job)
        .ok_or_else(|| failed("메시지와 응답 작업을 저장할 수 없습니다."))?;
    state.bus.message(&saved_message);
    state.bus.record("job", &saved_job);
    state.store.audit("room.owner_message",json!({"roomId":id,"sessionId":session_id,"messageId":saved_message["id"],"jobId":saved_job["id"]}));
    if let Some(saved)=state.store.patch_if("room",id,"id",&json!(id),&json!({"lastOwnerMessageId":saved_message["id"],"lastSubmittedAt":chrono::Utc::now().to_rfc3339(),"stoppedAt":null})){
        state.bus.record("room",&saved);
    }
    Ok(
        json!({"id":job_id,"jobId":job_id,"status":"queued","room":room(state,id)?,"sessionId":session_id,"message":saved_message,"job":saved_job}),
    )
}
pub async fn send_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}
/// The historical /run endpoint uses exactly the same persisted owner flow.
pub async fn run(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    response(submit(&state, &id, &body))
}

fn stop_room(state: &AppState, id: &str) -> ApiResult {
    let _guard = lock(state, id)?;
    stop_room_locked(state, id)
}
fn stop_room_locked(state: &AppState, id: &str) -> ApiResult {
    let config = room(state, id)?;
    let session_id = config["sessionId"].as_str();
    let pending = state
        .store
        .all("job")
        .into_iter()
        .filter(|job| {
            job["kind"] == "room"
                && job["input"]["roomId"] == id
                && matches!(job["status"].as_str(), Some("queued" | "running"))
        })
        .collect::<Vec<_>>();
    if let Some(session_id) = session_id {
        state.cancel_session(session_id);
    }
    // Old imported jobs may lack a session binding until their first turn.
    for job in &pending {
        if let Some(job_id) = job["id"].as_str() {
            if let Some(saved)=state.store.patch_if("job",job_id,"cancelRequested",job.get("cancelRequested").unwrap_or(&Value::Null),
                &json!({"cancelRequested":true,"status":"cancelled","finishedAt":chrono::Utc::now().to_rfc3339()})){state.bus.record("job",&saved);}
        }
    }
    if let Some(saved) = state.store.patch_if(
        "room",
        id,
        "id",
        &json!(id),
        &json!({"stoppedAt":chrono::Utc::now().to_rfc3339()}),
    ) {
        state.bus.record("room", &saved);
    }
    Ok(
        json!({"ok":true,"roomId":id,"sessionId":session_id,"status":"stopped","cancelledJobs":pending.len()}),
    )
}
pub async fn stop(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response(stop_room(&state, &id))
}
pub async fn remove(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        if !state.store.remove("room", &id) {
            return Err(failed("회의실을 삭제할 수 없습니다."));
        }
        state
            .bus
            .emit(json!({"type":"record","kind":"room","removed":id}));
        // Keep the transcript as a durable archive; never erase owner messages.
        Ok(json!({"ok":true}))
    })())
}

/// Archives the current room transcript and binds the room to a fresh empty
/// session. The room, members, settings and knowledge stay intact; the old
/// session and all of its messages remain available as a durable archive.
pub async fn reset(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let _guard = lock(&state, &id)?;
        stop_room_locked(&state, &id)?;
        let current = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let previous_session_id = current["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?
            .to_string();
        crate::engine::chat::close_session_approvals(&state, &previous_session_id);

        let current_room = room(&state, &id)?;
        let title = current_room
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("대화방");
        let session_id = format!("room:{id}:{}", uuid::Uuid::new_v4().simple());
        let record = json!({
            "id": session_id,
            "title": title,
            "roomId": id,
            "kind": "room",
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        if state.store.put_if_absent("session", &record).is_none() {
            return Err(failed("새 대화방 세션을 만들 수 없습니다."));
        }
        let binding = current_room.get("sessionId").unwrap_or(&Value::Null);
        let Some(saved) = state.store.patch_if(
            "room",
            &id,
            "sessionId",
            binding,
            &json!({
                "sessionId": session_id,
                "status": "idle",
                "pauseReason": Value::Null,
                "activeAgentId": Value::Null,
                "resetAt": chrono::Utc::now().to_rfc3339(),
            }),
        ) else {
            return Err(conflict("대화방 세션이 동시에 변경되었습니다. 다시 시도하세요."));
        };
        state.bus.record("room", &saved);
        Ok(json!({
            "ok": true,
            "roomId": id,
            "previousSessionId": previous_session_id,
            "sessionId": session_id,
        }))
    })())
}

pub async fn state(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    response((|| {
        let config = room(&state, &id)?;
        let session = rooms::ensure_room_session(&state, &id).map_err(bad)?;
        let session_id = session["id"]
            .as_str()
            .ok_or_else(|| failed("회의실 세션 ID가 없습니다."))?;
        let mut jobs = state
            .store
            .all("job")
            .into_iter()
            .filter(|job| job["kind"] == "room" && job["input"]["roomId"] == id)
            .collect::<Vec<_>>();
        jobs.sort_by(|a, b| {
            b["queuedAt"]
                .as_str()
                .unwrap_or("")
                .cmp(a["queuedAt"].as_str().unwrap_or(""))
        });
        let queue_depth = jobs.iter().filter(|job| job["status"] == "queued").count();
        let running = jobs.iter().find(|job| job["status"] == "running");
        let active_agent = running
            .and_then(|job| job["roomState"]["activeAgentId"].as_str())
            .or_else(|| {
                if running.is_some() {
                    config["activeAgentId"].as_str()
                } else {
                    None
                }
            });
        let status = if running.is_some() {
            "running"
        } else if queue_depth > 0 {
            "queued"
        } else if !config["stoppedAt"].is_null() {
            "stopped"
        } else if matches!(config["status"].as_str(), Some("paused" | "needs_review")) {
            config["status"].as_str().unwrap_or("idle")
        } else {
            jobs.first()
                .and_then(|job| job["status"].as_str())
                .filter(|status| matches!(*status, "failed" | "needs_review"))
                .unwrap_or("idle")
        };
        let executions = state
            .store
            .all("execution")
            .into_iter()
            .filter(|execution| execution["sessionId"] == session_id)
            .take(30)
            .collect::<Vec<_>>();
        let learning = state
            .store
            .all("job")
            .into_iter()
            .filter(|job| job["kind"] == "learn" && job["input"]["sessionId"] == session_id)
            .take(20)
            .collect::<Vec<_>>();
        let run_ids = executions
            .iter()
            .filter_map(|execution| execution["id"].as_str())
            .collect::<HashSet<_>>();
        let operations=state.store.all("execution_operation").into_iter()
            .filter(|operation|operation["sessionId"]==session_id||operation["runId"].as_str().is_some_and(|run|run_ids.contains(run)))
            .take(60).map(|operation|json!({"id":operation["id"],"runId":operation["runId"],"tool":operation["tool"],"status":operation["status"],
                "startedAt":operation["startedAt"],"finishedAt":operation["finishedAt"],
                "arguments":bounded_json(&operation["arguments"],2000),"result":bounded_json(&operation["result"],6000)})).collect::<Vec<_>>();
        let approvals = state
            .store
            .all("approval")
            .into_iter()
            .filter(|approval| approval["sessionId"] == session_id)
            .take(20)
            .collect::<Vec<_>>();
        Ok(
            json!({"room":room(&state,&id)?,"sessionId":session_id,"status":status,"activeAgentId":active_agent,"queueDepth":queue_depth,
            "jobs":jobs.iter().take(30).collect::<Vec<_>>(),"executions":executions,"learningJobs":learning,"operations":operations,"approvals":approvals,"usage":state.store.session_usage(session_id)}),
        )
    })())
}

fn bounded_json(value: &Value, limit: usize) -> Value {
    let text = value.to_string();
    if text.chars().count() <= limit {
        value.clone()
    } else {
        json!({"preview":text.chars().take(limit).collect::<String>(),"truncated":true})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-room-api-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        for index in 0..9 {
            state.store.put(
                "agent",
                &json!({"id":format!("a{index}"),"name":format!("참가자{index}")}),
            );
        }
        (state, dir)
    }
    fn config() -> Value {
        json!({"id":"room-one","name":"우리 방","agentIds":(0..9).map(|i|format!("a{i}")).collect::<Vec<_>>(),"routingMode":"discussion"})
    }
    #[test]
    fn nine_agent_roster_has_stable_private_session_and_safe_defaults() {
        let (state, dir) = test_state();
        let saved = save_room(&state, None, &config()).unwrap();
        assert_eq!(saved["agentIds"].as_array().unwrap().len(), 9);
        assert_eq!(saved["toolsEnabled"], false);
        assert_eq!(saved["autoLearning"], true);
        let session_id = saved["sessionId"].as_str().unwrap();
        let session = state.store.get("session", session_id).unwrap();
        assert_eq!(session["roomId"], "room-one");
        let update = save_room(
            &state,
            Some("room-one"),
            &json!({"name":"오래 가는 방","rounds":2}),
        )
        .unwrap();
        assert_eq!(update["sessionId"], session_id);
        assert!(save_room(
            &state,
            None,
            &json!({"id":"rogue","name":"탈취","agentIds":["a0"],"sessionId":session_id})
        )
        .is_err());
        assert!(save_room(
            &state,
            Some("room-one"),
            &json!({"sessionId":"private-session"})
        )
        .is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"agentIds":["missing"]})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"activeAgentId":"a8"})).is_err());
        assert!(save_room(&state, Some("room-one"), &json!({"maxTurns":100001})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn room_attachments_and_response_overrides_are_bound_and_immutable() {
        use base64::Engine;
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        state
            .store
            .put("provider", &json!({"id":"chosen","enabled":true}));
        let file=crate::engine::attachments::upload(&state,&json!({"roomId":"room-one","name":"notes.txt","data":base64::engine::general_purpose::STANDARD.encode("실제 사용자 원문") })).unwrap();
        let body = json!({"input":"첨부 확인","attachmentIds":[file["id"]],"thinking":false,"reasoningEffort":"minimal","providerId":"chosen","model":"chosen-model","clientMessageId":"file-turn"});
        let submitted = submit(&state, "room-one", &body).unwrap();
        assert_eq!(
            submitted["message"]["metadata"]["attachments"][0]["id"],
            file["id"]
        );
        assert_eq!(submitted["job"]["input"]["reasoningEffort"], "minimal");
        assert_eq!(submitted["job"]["input"]["providerId"], "chosen");
        let mut changed = body.clone();
        changed["thinking"] = json!(true);
        assert!(submit(&state, "room-one", &changed).is_err());
        let internal = submit_internal(
            &state,
            "room-one",
            &json!({"input":"외부 명령","clientMessageId":"external"}),
            Some(json!({"transport":"telegram","messageId":"inbound-one"})),
            false,
        )
        .unwrap();
        assert_eq!(
            internal["job"]["input"]["roomSnapshot"]["toolsEnabled"],
            false
        );
        assert_eq!(
            internal["message"]["metadata"]["origin"]["transport"],
            "telegram"
        );
        assert!(submit_internal(
            &state,
            "room-one",
            &json!({"input":"봇을 주인으로 속임"}),
            Some(json!({"authorIsBot":true})),
            false
        )
        .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn owner_message_and_job_are_atomic_idempotent_and_stop_preserves_transcript() {
        let (state, dir) = test_state();
        save_room(&state, None, &config()).unwrap();
        let body =
            json!({"input":"아홉 명 다 같이 계획을 논의해","clientMessageId":"owner-message-one"});
        let sent = submit(&state, "room-one", &body).unwrap();
        let session_id = sent["sessionId"].as_str().unwrap();
        let messages = state.store.messages(session_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert!(messages[0]["agent_id"].is_null());
        assert_eq!(messages[0]["metadata"]["authorKind"], "owner");
        assert_eq!(
            sent["job"]["input"]["agentIds"].as_array().unwrap().len(),
            9
        );
        assert_eq!(sent["job"]["input"]["sourceMessageId"], messages[0]["id"]);
        assert_eq!(sent["job"]["input"]["roomSnapshot"]["toolsEnabled"], false);
        let again = submit(&state, "room-one", &body).unwrap();
        assert_eq!(sent["jobId"], again["jobId"]);
        assert_eq!(state.store.messages(session_id).len(), 1);
        assert_eq!(state.store.count("job"), 1);
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 문장","clientMessageId":"owner-message-one"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"화자 탈취","role":"assistant"})
        )
        .is_err());
        assert!(submit(
            &state,
            "room-one",
            &json!({"input":"다른 방 참가자","agentIds":["outsider"]})
        )
        .is_err());
        let original_generation = sent["job"]["input"]["generation"].as_u64().unwrap();
        let stopped = stop_room(&state, "room-one").unwrap();
        assert_eq!(stopped["cancelledJobs"], 1);
        let cancelled = state
            .store
            .get("job", sent["jobId"].as_str().unwrap())
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["cancelRequested"], true);
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 1);
        let next = submit(
            &state,
            "room-one",
            &json!({"input":"이제 새 주제로 다시 시작해","clientMessageId":"second"}),
        )
        .unwrap();
        assert!(next["job"]["input"]["generation"].as_u64().unwrap() > original_generation);
        // Submitting fresh work never clears the stop of an older running turn.
        assert!(state.is_cancelled(session_id));
        assert_eq!(state.store.messages(session_id).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
