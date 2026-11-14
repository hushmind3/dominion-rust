use crate::state::AppState;
use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Record kinds that behave as plain JSON collections. The React client
/// already talks to these paths, so the Rust backend just needs to persist
/// them with the same contract.
pub const COLLECTIONS: &[&str] = &[
    "memories",
    "skills",
    "mcp",
    "automations",
    "workflows",
    "knowledge",
];

/// URL segments are plural (`/api/jobs`) while the store groups records by a
/// singular kind (`job`), matching the Node data model.
pub fn store_kind(route_kind: &str) -> &str {
    match route_kind {
        "memories" => "memory",
        "skills" => "skill",
        "automations" => "automation",
        "workflows" => "workflow",
        "rooms" => "room",
        "channels" => "channel",
        "jobs" => "job",
        "executions" => "execution",
        "goals" => "goal",
        other => other,
    }
}

fn kind_from_uri(uri: &axum::http::Uri) -> String {
    store_kind(
        uri.path()
            .trim_start_matches('/')
            .split('/')
            .nth(1)
            .unwrap_or_default(),
    )
    .to_string()
}

fn id_from_uri(uri: &axum::http::Uri) -> Option<String> {
    uri.path()
        .trim_start_matches('/')
        .split('/')
        .nth(2)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

pub async fn list(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    let mut records = state.store.all(&kind);
    if kind == "memory" {
        for record in &mut records {
            let eligible = crate::engine::memory::retrieval_eligible(record);
            if let Some(fields) = record.as_object_mut() {
                fields.insert("retrievalEligible".into(), json!(eligible));
            }
        }
    }
    Json(json!(records))
}

pub async fn save(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    if !body.is_object() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드 객체가 필요합니다." })),
        )
            .into_response();
    }
    let mut body = body;
    if kind == "mcp" {
        let id = body
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        body["id"] = json!(id);
        if let Some(key) = body
            .get("apiKey")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty() && !value.starts_with("••"))
            .map(str::to_string)
        {
            state.save_secret(&format!("mcp:{id}"), &key);
        }
        if let Value::Object(ref mut map) = body {
            map.remove("apiKey");
        }
        crate::engine::mcp::invalidate();
    }
    if kind == "skill" {
        // The execution path ignores `draft` even when `enabled` is true.
        // Make the public toggle and the engine contract agree.
        let enabled = body["enabled"].as_bool() == Some(true);
        if enabled {
            let status = body["status"].as_str().unwrap_or("");
            if status.is_empty() || status == "draft" {
                body["status"] = json!("active");
            }
        }
    }
    if kind == "automation" {
        let automation_kind = body["kind"].as_str().unwrap_or("");
        let input = &body["input"];
        let valid = match automation_kind {
            "prompt" => {
                input["prompt"]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty())
                    && input["agentId"].as_str().is_some_and(|id| {
                        state.store.get("agent", id).is_some()
                    })
            }
            "meeting" => {
                input["goal"]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty())
                    && input["roomId"].as_str().is_some_and(|id| {
                        state.store.get("room", id).is_some()
                    })
            }
            _ => false,
        };
        let interval = body["intervalMinutes"].as_i64().unwrap_or(0);
        if !valid || interval < 1 {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "자동화는 유형에 맞는 작업 내용·에이전트 또는 회의실·1분 이상 간격이 필요합니다."
                })),
            )
                .into_response();
        }
    }
    if kind == "workflow" {
        let steps = body["steps"].as_array().cloned().unwrap_or_default();
        let valid = !steps.is_empty()
            && steps.len() <= 30
            && steps.iter().all(|step| {
                step["prompt"]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty())
                    && step["agentId"].as_str().is_some_and(|id| {
                        state.store.get("agent", id).is_some()
                    })
            });
        if !valid {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "워크플로의 모든 단계에는 실행 지시문과 실제 등록된 에이전트가 필요합니다."
                })),
            )
                .into_response();
        }
    }
    match state.store.put(&kind, &body) {
        Some(record) => {
            state.bus.record(&kind, &record);
            Json(record).into_response()
        }
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "레코드를 저장할 수 없습니다." })),
        )
            .into_response(),
    }
}

pub async fn one(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Response {
    let kind = kind_from_uri(&uri);
    match id_from_uri(&uri) {
        Some(id) => match state.store.get(&kind, &id) {
            Some(record) => Json(record).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("{kind} 레코드를 찾을 수 없습니다.") })),
            )
                .into_response(),
        },
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "id 가 필요합니다." })),
        )
            .into_response(),
    }
}

pub async fn remove(State(state): State<AppState>, OriginalUri(uri): OriginalUri) -> Json<Value> {
    let kind = kind_from_uri(&uri);
    if let Some(id) = id_from_uri(&uri) {
        if kind == "mcp" {
            state.store.remove_secret(&format!("mcp:{id}"));
            crate::engine::mcp::invalidate();
        }
        state.store.remove(&kind, &id);
        state
            .bus
            .emit(json!({ "type": "record", "kind": kind, "removed": id }));
    }
    Json(json!({ "ok": true }))
}

/// `POST /api/{kind}/{id}/{action}` for workflow/room execution and channel
/// diagnostics. Unknown actions return a clear JSON error instead of HTML.
pub async fn action(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let kind = kind_from_uri(&uri);
    let id = id_from_uri(&uri).unwrap_or_default();
    let action = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .nth(3)
        .unwrap_or_default()
        .to_string();

    match (kind.as_str(), action.as_str()) {
        // Editing verified/learned evidence in place would falsify provenance.
        // Only owner-authored memories support a guarded, text-only mutation.
        ("memory", "edit") => {
            let Some(mut record) = state.store.get("memory", &id) else {
                return (StatusCode::NOT_FOUND, Json(json!({"error":"기억을 찾을 수 없습니다."}))).into_response();
            };
            if record["source"] != "user" {
                return (StatusCode::FORBIDDEN, Json(json!({
                    "error":"근거가 있는 학습 기억은 원문을 직접 수정할 수 없습니다."
                }))).into_response();
            }
            let Some(text) = body["text"].as_str().map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count() <= 16_000) else {
                return (StatusCode::BAD_REQUEST, Json(json!({
                    "error":"수정할 기억 내용은 1~16000자여야 합니다."
                }))).into_response();
            };
            if body["expectedText"].as_str() != record["text"].as_str() {
                return (StatusCode::CONFLICT, Json(json!({
                    "error":"기억 내용이 다른 곳에서 변경됐습니다. 목록을 새로고침한 뒤 다시 수정하세요."
                }))).into_response();
            }
            record["text"] = json!(text);
            record["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            // Stale quote evidence must not remain attached to altered text.
            record["verification"] = json!("manual_edit");
            record["sourceMessageIds"] = json!([]);
            record["evidenceQuote"] = Value::Null;
            match state.store.put("memory", &record) {
                Some(saved) => {
                    state.store.audit("memory.manual_edit", json!({"id":id,"source":"user"}));
                    state.bus.record("memory", &saved);
                    Json(saved).into_response()
                }
                None => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                    "error":"기억을 수정하지 못했습니다."
                }))).into_response(),
            }
        }
        ("workflow", "run") => enqueue_job(
            &state,
            "workflow",
            json!({
                "workflowId": id,
                "input": body.get("input").cloned().unwrap_or(json!("")),
            }),
        ),
        ("room", "run") => enqueue_job(
            &state,
            "room",
            json!({
                "roomId": id,
                "goal": body.get("goal").or_else(|| body.get("input")).cloned().unwrap_or(json!("")),
            }),
        ),
        ("skill", "export") => match state.store.get("skill", &id) {
            Some(skill) => Json(skill).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "스킬을 찾을 수 없습니다." })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("지원하지 않는 작업입니다: {kind}/{action}") })),
        )
            .into_response(),
    }
}

fn enqueue_job(state: &AppState, kind: &str, input: Value) -> Response {
    match crate::engine::jobs::enqueue(state, kind, input) {
        Some(job) => Json(job).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "작업을 만들 수 없습니다." })),
        )
            .into_response(),
    }
}
