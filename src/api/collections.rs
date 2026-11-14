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
            "workflow" => input["workflowId"].as_str().is_some_and(|id|
                state.store.get("workflow",id).is_some_and(|flow|
                    flow["steps"].as_array().is_some_and(|steps|!steps.is_empty()))),
            _ => false,
        };
        let interval = body["intervalMinutes"].as_i64().unwrap_or(0);
        if !valid || !(1..=525600).contains(&interval) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "자동화에는 실제 에이전트·회의실·워크플로와 1분~525600분의 반복 간격이 필요합니다."
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
                    && step.get("acceptanceChecks").is_none_or(|checks|
                        crate::engine::verification::validate_criteria(checks).is_ok()
                        && checks.as_array().is_some_and(|rows|rows.len()<=8)
                    )
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
            // The owner edits only the memory text, never its scope, evidence
            // provenance or lifecycle fields. Use an atomic text comparison so
            // an older UI view cannot overwrite a newer memory correction.
            let Some(new_text) = body["text"].as_str().map(str::trim) else {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"수정할 기억 내용이 필요합니다."}))).into_response();
            };
            let Some(expected) = body["expectedText"].as_str() else {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"변경 전 기억 내용이 필요합니다."}))).into_response();
            };
            if new_text.is_empty() || new_text.chars().count() > 1600 {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"기억은 1~1,600자여야 합니다."}))).into_response();
            }
            if let Some(object) = body.as_object() {
                if object.len()!=2 || !object.contains_key("text") || !object.contains_key("expectedText") {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error":"기억 내용만 수정할 수 있습니다."}))).into_response();
                }
            }
            let Some(old)=state.store.get("memory",&id) else {
                return (StatusCode::NOT_FOUND, Json(json!({"error":"기억을 찾을 수 없습니다."}))).into_response();
            };
            // Learned/derived memories carry evidence and attribution that a text
            // edit would falsify. Only explicitly owner-authored active records
            // without model/source-quote provenance are editable in place.
            // A learned memory can instead be superseded by a separately
            // grounded correction through the learning pipeline.
            if old["source"]!="user" || old["status"]!="active"
                || old["verification"]=="source_quote"
                || old["evidenceQuote"].as_str().is_some_and(|q|!q.is_empty())
                || old["sourceMessageIds"].as_array().is_some_and(|ids|!ids.is_empty())
            {
                return (StatusCode::FORBIDDEN,Json(json!({"error":
                    "사용자가 직접 등록한 활성 기억만 수정할 수 있습니다. 학습 근거와 보관 기록은 변경하지 않습니다."}))).into_response();
            }
            if old["text"].as_str()!=Some(expected) {
                return (StatusCode::CONFLICT, Json(json!({"error":"다른 실행에서 기억이 변경됐습니다. 새로고침 후 다시 검토하세요."}))).into_response();
            }
            if expected==new_text{return Json(old).into_response();}
            let now=chrono::Utc::now().to_rfc3339();
            // Corrected text must not impersonate the original source quote.
            // Keep the original source/evidence and mark the independent owner
            // revision; retrieval eligibility and scope remain unchanged.
            let patch=json!({"text":new_text,"verification":"owner_revision",
                "ownerEditedAt":now,
                "revision":format!("owner-edit-{}",uuid::Uuid::new_v4())});
            let expected_fields=json!({"text":expected,"source":old["source"],
                "status":old["status"],"scope":old["scope"],
                "agentId":old["agentId"],"partitionKey":old["partitionKey"],
                "verification":old["verification"],
                "evidenceQuote":old["evidenceQuote"],
                "sourceMessageIds":old["sourceMessageIds"]});
            match state.store.patch_if_fields("memory",&id,&expected_fields,&patch) {
                Some(record)=>{
                    state.bus.record("memory",&record);
                    state.store.audit("memory.owner_edited",json!({"memoryId":id,
                        "scope":old["scope"],"originalVerification":old["verification"]}));
                    Json(record).into_response()
                },
                None=>(StatusCode::CONFLICT,Json(json!({"error":"기억 변경 충돌입니다. 목록을 새로고침하세요."}))).into_response()
            }
        },
        ("automation","review")=>{
            if body["acknowledgeFutureRuns"]!=true{
                return (StatusCode::BAD_REQUEST,Json(json!({
                    "error":"이전 작업 검토와 향후 반복 실행을 명시적으로 승인해야 합니다."
                }))).into_response();
            }
            let job_id=body["lastJobId"].as_str().unwrap_or("");
            let note=body["resolutionNote"].as_str().unwrap_or("").trim();
            match state.store.acknowledge_automation_review(&id,job_id,note){
                Ok(Some((automation,job)))=>{
                    state.bus.record("automation",&automation);
                    state.bus.record("job",&job);
                    state.store.audit("automation.owner_review",json!({
                        "automationId":id,"jobId":job_id,
                        "nextAt":automation["nextAt"],
                        "ownerApprovedFutureRuns":true,
                    }));
                    Json(json!({"ok":true,"automation":automation,"job":job})).into_response()
                }
                Ok(None)=>(StatusCode::CONFLICT,Json(json!({
                    "error":"해당 자동화의 보류 작업이 이미 변경됐습니다. 다시 확인하세요."
                }))).into_response(),
                Err(reason)=>(StatusCode::BAD_REQUEST,Json(json!({"error":reason}))).into_response(),
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
mod owner_memory_edit_tests {
    use super::*;
    #[tokio::test]
    async fn owner_memory_edit_requires_matching_text_and_preserves_provenance() {
        let dir=std::env::temp_dir().join(format!("dominion-memory-edit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{port:0,data_dir:dir.clone(),
            workspace:dir.join("workspace"),web_dir:dir.join("web")};
        let state=AppState::new(config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()));
        state.store.put("memory",&json!({"id":"m1","text":"old content",
            "scope":"agent","agentId":"a1","status":"active",
            "source":"user","verification":"owner_direct"})).unwrap();
        let uri=axum::http::Uri::from_static("/api/memories/m1/edit");
        let bad=action(State(state.clone()),OriginalUri(uri.clone()),
            Json(json!({"text":"replacement","expectedText":"stale"}))).await;
        assert_eq!(bad.status(),StatusCode::CONFLICT);
        let ok=action(State(state.clone()),OriginalUri(uri.clone()),
            Json(json!({"text":"replacement","expectedText":"old content"}))).await;
        assert_eq!(ok.status(),StatusCode::OK);
        let saved=state.store.get("memory","m1").unwrap();
        assert_eq!(saved["text"],"replacement");
        assert_eq!(saved["agentId"],"a1");
        assert_eq!(saved["scope"],"agent");
        assert!(saved["evidenceQuote"].is_null());
        assert!(saved["sourceMessageIds"].is_null());
        assert_eq!(saved["source"],"user");
        assert_eq!(saved["verification"],"owner_revision");
        let stale=action(State(state.clone()),OriginalUri(uri),
            Json(json!({"text":"clobber","expectedText":"old content"}))).await;
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        // Rewriting learned evidence as a user correction must be impossible.
        for (id,source,status) in [("learned","learn","active"),
            ("archived","user","archived")] {
            state.store.put("memory",&json!({"id":id,"text":"source truth",
                "scope":"agent","agentId":"a1","status":status,
                "source":source,"verification":"source_quote",
                "sourceMessageIds":["source-message"],"evidenceQuote":"source truth"})).unwrap();
            let uri=format!("/api/memories/{id}/edit").parse::<axum::http::Uri>().unwrap();
            let response=action(State(state.clone()),OriginalUri(uri),
                Json(json!({"text":"rewritten","expectedText":"source truth"}))).await;
            assert_eq!(response.status(),StatusCode::FORBIDDEN);
            assert_eq!(state.store.get("memory",id).unwrap()["text"],"source truth");
        }
        // An active manually created memory with later attached source
        // provenance is also immutable, regardless of its source label.
        state.store.put("memory",&json!({"id":"protected","text":"evidence",
            "scope":"shared","status":"active","source":"user",
            "evidenceQuote":"source text","sourceMessageIds":["id1"]})).unwrap();
        let protected=action(State(state.clone()),OriginalUri(
            axum::http::Uri::from_static("/api/memories/protected/edit")),
            Json(json!({"text":"changed","expectedText":"evidence"}))).await;
        assert_eq!(protected.status(),StatusCode::FORBIDDEN);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
}
