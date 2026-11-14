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
        ("memory", "edit") => {
            // Manual memories may be edited without overwriting their scope,
            // origin, retrieval evidence, timestamp or other server fields.
            // The exact previous text is an optimistic concurrency check.
            let Some(current) = state.store.get("memory", &id) else {
                return (StatusCode::NOT_FOUND,
                    Json(json!({"error":"기억을 찾을 수 없습니다."}))).into_response();
            };
            if current["source"] != "user" || current["status"].as_str()
                .is_some_and(|status| status!="active") {
                return (StatusCode::CONFLICT,
                    Json(json!({"error":"직접 등록한 활성 기억만 수정할 수 있습니다. 학습 출처 기록은 보존됩니다."}))).into_response();
            }
            let Some(previous) = body["expectedText"].as_str() else {
                return (StatusCode::BAD_REQUEST,
                    Json(json!({"error":"수정 전 기억 내용을 함께 보내야 합니다."}))).into_response();
            };
            let Some(new_text) = body["text"].as_str().map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count()<=4096) else {
                return (StatusCode::BAD_REQUEST,
                    Json(json!({"error":"기억 내용은 1~4096자여야 합니다."}))).into_response();
            };
            if previous==new_text {
                return Json(current).into_response();
            }
            match state.store.patch_if("memory", &id, "text", &json!(previous),
                &json!({"text":new_text,"updatedAt":chrono::Utc::now().to_rfc3339()})) {
                Some(updated) => {
                    state.store.audit("memory.owner_edited",
                        json!({"id":id,"scope":updated["scope"],"source":"user"}));
                    state.bus.record("memory",&updated);
                    Json(updated).into_response()
                }
                None => (StatusCode::CONFLICT,
                    Json(json!({"error":"다른 작업에서 기억이 변경되었습니다. 새로고침 후 다시 수정하세요."}))).into_response(),
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


#[cfg(test)]
mod memory_edit_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn state() -> AppState {
        let dir=std::env::temp_dir().join(format!("dominion-memory-edit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),workspace:dir.clone(),web_dir:dir.join("web")};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        AppState::new(config,store,vault)
    }
    async fn edit(state:&AppState,id:&str,body:Value)->Response{
        let uri=format!("/api/memories/{id}/edit").parse().unwrap();
        action(State(state.clone()),OriginalUri(uri),Json(body)).await
    }
    #[tokio::test]
    async fn manual_edit_preserves_scope_and_rejects_stale_or_learned_provenance(){
        let state=state();
        state.store.put("memory",&json!({"id":"manual","text":"before",
            "scope":"agent","agentId":"agent-7","source":"user",
            "status":"active","verification":"owner"})).unwrap();
        let changed=edit(&state,"manual",json!({"text":"after","expectedText":"before"})).await;
        assert_eq!(changed.status(),StatusCode::OK);
        let stored=state.store.get("memory","manual").unwrap();
        assert_eq!(stored["text"],"after");
        assert_eq!(stored["agentId"],"agent-7");
        assert_eq!(stored["verification"],"owner");
        assert!(stored["updatedAt"].as_str().is_some());
        let stale=edit(&state,"manual",json!({"text":"overwritten","expectedText":"before"})).await;
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","manual").unwrap()["text"],"after");
        state.store.put("memory",&json!({"id":"learned","text":"evidence",
            "source":"learn","status":"active"})).unwrap();
        let forbidden=edit(&state,"learned",json!({"text":"bad","expectedText":"evidence"})).await;
        assert_eq!(forbidden.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","learned").unwrap()["text"],"evidence");
    }
}
