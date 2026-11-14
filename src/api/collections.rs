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
    if kind == "memory" {
        // User creation is insert-only. Every existing memory, including a
        // learned one, is immutable through this generic collection endpoint.
        // Owner edits must use the constrained /edit action below.
        if id_from_uri(&uri).is_some() || body.get("id").is_some() {
            return (StatusCode::CONFLICT, Json(json!({
                "error":"기존 기억은 수정 전용 API를 사용하세요. 새 기억 ID는 서버가 발급합니다."
            }))).into_response();
        }
        if body.get("source").is_some_and(|value| value != "user")
            || body.get("status").is_some_and(|value| value != "active") {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"수동 등록은 사용자 출처의 활성 기억만 생성할 수 있습니다."
            }))).into_response();
        }
        let Some(text) = body["text"].as_str().map(str::trim)
            .filter(|text| !text.is_empty() && text.chars().count() <= 4096) else {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"기억은 1~4096자여야 합니다."
            }))).into_response();
        };
        let scope = body["scope"].as_str().unwrap_or("shared");
        if !matches!(scope, "shared" | "agent") {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"수동 기억의 범위는 공유 또는 에이전트만 지정할 수 있습니다."
            }))).into_response();
        }
        let agent_id = body["agentId"].as_str().unwrap_or("");
        if scope == "agent" && (agent_id.is_empty()
            || state.store.get("agent", agent_id).is_none()) {
            return (StatusCode::BAD_REQUEST, Json(json!({
                "error":"지정한 기억 담당 에이전트를 찾을 수 없습니다."
            }))).into_response();
        }
        let record = json!({
            "id":uuid::Uuid::new_v4().to_string(),
            "text":text,"source":"user","status":"active",
            "scope":scope,
            "agentId":if scope=="agent" {json!(agent_id)}else{Value::Null},
            "createdAt":chrono::Utc::now().to_rfc3339(),
            "revision":0
        });
        return match state.store.put_if_absent("memory",&record) {
            Some(saved) => {state.bus.record("memory",&saved);Json(saved).into_response()},
            None => (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({
                "error":"기억 저장에 실패했습니다."
            }))).into_response()
        };
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
        ("memory", "edit") => {
            if body.as_object().is_none_or(|fields|fields.len()!=2
                || !fields.contains_key("text") || !fields.contains_key("expectedText")) {
                return (StatusCode::BAD_REQUEST,Json(json!({
                    "error":"text와 expectedText만 보내세요."
                }))).into_response();
            }
            let Some(expected) = body["expectedText"].as_str() else {
                return (StatusCode::BAD_REQUEST,Json(json!({
                    "error":"수정 전에 읽은 기억 내용이 필요합니다."
                }))).into_response();
            };
            let Some(text) = body["text"].as_str().map(str::trim)
                .filter(|text|!text.is_empty()&&text.chars().count()<=4096) else {
                return (StatusCode::BAD_REQUEST,Json(json!({
                    "error":"기억 내용은 1~4096자여야 합니다."
                }))).into_response();
            };
            if state.store.get("memory",&id).is_none() {
                return (StatusCode::NOT_FOUND,Json(json!({
                    "error":"기억을 찾을 수 없습니다."
                }))).into_response();
            }
            match state.store.edit_manual_memory(&id,expected,text) {
                Some(updated) => {
                    state.store.audit("memory.owner_edited",json!({
                        "id":id,"scope":updated["scope"],"revision":updated["revision"]
                    }));
                    state.bus.record("memory",&updated);
                    Json(updated).into_response()
                },
                None => (StatusCode::CONFLICT,Json(json!({
                    "error":"기억이 변경되었거나 사용자 작성 활성 기억이 아닙니다. 새로고침 후 다시 확인하세요."
                }))).into_response(),
            }
        },
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
