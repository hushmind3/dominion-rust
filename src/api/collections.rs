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
            use crate::db::ManualMemoryEditError as E;
            if body.as_object().is_none_or(|map|
                map.len() != 2 || !map.contains_key("text") || !map.contains_key("expectedText")
            ) {
                return (StatusCode::BAD_REQUEST, Json(json!({
                    "error":"수정할 내용과 이전 내용만 지정하세요."
                }))).into_response();
            }
            let Some(text) = body["text"].as_str().map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count() <= 16_000) else {
                return (StatusCode::BAD_REQUEST, Json(json!({
                    "error":"수정할 기억 내용은 1~16000자여야 합니다."
                }))).into_response();
            };
            let Some(expected) = body["expectedText"].as_str() else {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"현재 기억 내용이 필요합니다."}))).into_response();
            };
            match state.store.edit_manual_memory(&id, expected, text) {
                Ok(saved) => {
                    state.bus.record("memory", &saved);
                    Json(saved).into_response()
                }
                Err(reason) => {
                    let (status, message) = match reason {
                        E::NotFound => (StatusCode::NOT_FOUND, "기억을 찾을 수 없습니다."),
                        E::NotManual => (StatusCode::FORBIDDEN, "학습·증거 기반 기억은 원문을 직접 수정할 수 없습니다."),
                        E::NotActive => (StatusCode::CONFLICT, "비활성·만료된 기억은 직접 수정할 수 없습니다."),
                        E::Conflict => (StatusCode::CONFLICT, "기억이 변경됐습니다. 새로고침 후 다시 수정하세요."),
                        E::Database => (StatusCode::INTERNAL_SERVER_ERROR, "기억 수정 트랜잭션이 실패했습니다."),
                    };
                    (status, Json(json!({"error":message}))).into_response()
                }
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

#[cfg(test)]
mod manual_memory_edit_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};

    fn state() -> (AppState,std::path::PathBuf) {
        let root=std::env::temp_dir().join(format!("dominion-memory-edit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        (AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path())),root)
    }

    async fn edit(state:&AppState,id:&str,body:Value) -> Response {
        action(State(state.clone()),
            OriginalUri(format!("/api/memories/{id}/edit").parse().unwrap()),
            Json(body)).await
    }

    #[tokio::test]
    async fn manual_memory_edit_preserves_scope_and_removes_stale_quote() {
        let (state,root)=state();
        state.store.put("memory",&json!({
            "id":"manual","source":"user","scope":"agent","agentId":"a",
            "text":"old","verification":"source_quote","sourceMessageIds":["prior"],
            "evidenceQuote":"old","importance":0.9
        }));
        let response=edit(&state,"manual",json!({"text":"new","expectedText":"old"})).await;
        assert_eq!(response.status(),StatusCode::OK);
        let saved=state.store.get("memory","manual").unwrap();
        assert_eq!(saved["text"],"new");
        assert_eq!(saved["agentId"],"a");
        assert_eq!(saved["scope"],"agent");
        assert_eq!(saved["importance"],0.9);
        assert_eq!(saved["verification"],"manual_edit");
        assert_eq!(saved["sourceMessageIds"],json!([]));
        assert!(saved["evidenceQuote"].is_null());
        assert_eq!(edit(&state,"manual",json!({"text":"stale","expectedText":"old"})).await.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","manual").unwrap()["text"],"new");
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn learned_memories_and_invalid_edits_are_rejected() {
        let (state,root)=state();
        state.store.put("memory",&json!({
            "id":"learned","source":"learn","scope":"session","sessionId":"s",
            "text":"original","sourceMessageIds":["evidence"]
        }));
        assert_eq!(edit(&state,"learned",json!({"text":"tampered","expectedText":"original"})).await.status(),StatusCode::FORBIDDEN);
        assert_eq!(state.store.get("memory","learned").unwrap()["text"],"original");
        state.store.put("memory",&json!({"id":"manual","source":"user","text":"yes"}));
        assert_eq!(edit(&state,"manual",json!({"text":" ","expectedText":"yes"})).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(edit(&state,"missing",json!({"text":"new","expectedText":"old"})).await.status(),StatusCode::NOT_FOUND);
        let _=std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn competing_edits_have_one_winner_and_preserve_provenance() {
        let (state,root)=state();
        state.store.put("memory",&json!({"id":"manual","source":"user",
            "scope":"agent","agentId":"owner","status":"active","text":"start"}));
        let (first,second)=tokio::join!(
            edit(&state,"manual",json!({"expectedText":"start","text":"alpha"})),
            edit(&state,"manual",json!({"expectedText":"start","text":"beta"}))
        );
        let mut statuses=[first.status().as_u16(),second.status().as_u16()];
        statuses.sort();
        assert_eq!(statuses,[200,409]);
        let saved=state.store.get("memory","manual").unwrap();
        assert!(saved["text"]=="alpha"||saved["text"]=="beta");
        assert_eq!(saved["scope"],"agent");
        assert_eq!(saved["agentId"],"owner");
        assert_eq!(saved["verification"],"manual_edit");
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn archived_or_disabled_manual_memory_is_not_reactivated_by_edit() {
        let (state,root)=state();
        state.store.put("memory",&json!({"id":"archived","source":"user",
            "status":"archived","text":"historic"}));
        assert_eq!(edit(&state,"archived",json!({"expectedText":"historic","text":"mutated"}))
            .await.status(),StatusCode::CONFLICT);
        state.store.put("memory",&json!({"id":"disabled","source":"user",
            "status":"active","enabled":false,"text":"keep"}));
        assert_eq!(edit(&state,"disabled",json!({"expectedText":"keep","text":"mutated"}))
            .await.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","archived").unwrap()["text"],"historic");
        assert_eq!(state.store.get("memory","disabled").unwrap()["text"],"keep");
        assert_eq!(edit(&state,"archived",json!({
            "expectedText":"historic","text":"new","scope":"global"}))
            .await.status(),StatusCode::BAD_REQUEST);
        let _=std::fs::remove_dir_all(root);
    }

}
