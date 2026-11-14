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
    if kind == "memory" {
        // Never let the generic collection upsert overwrite an existing
        // memory or forge learned evidence. Owner edits use the checked,
        // audited /api/memories/:id/edit endpoint instead.
        let invalid = || (StatusCode::BAD_REQUEST,
            Json(json!({"error":"직접 작성한 기억의 내용과 범위만 등록할 수 있습니다."})))
            .into_response();
        let Some(text) = body["text"].as_str().map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count()<=4096)
        else { return invalid(); };
        if body.get("id").is_some()
            || id_from_uri(&uri).is_some()
            || body.get("source").is_some_and(|v|v!="user")
            || body.get("status").is_some_and(|v|v!="active")
            || body.as_object().is_some_and(|obj| obj.keys().any(|key|
                !["text","source","status","scope","agentId"].contains(&key.as_str())))
        {
            return invalid();
        }
        let scope=body["scope"].as_str().unwrap_or("shared");
        let agent_id=body["agentId"].as_str();
        if !matches!(scope,"shared"|"global"|"agent")
            || (scope=="agent" && agent_id.is_none_or(|id|
                state.store.get("agent",id).is_none()))
            || (scope!="agent" && agent_id.is_some()) {
            return invalid();
        }
        body=if scope=="agent" {
            json!({"text":text,"source":"user","status":"active","scope":"agent",
                "agentId":agent_id.unwrap_or_default()})
        }else{
            json!({"text":text,"source":"user","status":"active","scope":scope})
        };
    }
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
            "workflow" => input["workflowId"].as_str()
                .is_some_and(|id|state.store.get("workflow",id).is_some())
                && input["input"].as_str().is_some_and(|text|text.len()<=16*1024)
                && input["tokenBudget"].as_u64().unwrap_or(0)<=2_000_000
                && crate::engine::workflows::resolve_run_input(&state,input).is_ok(),
            _ => false,
        };
        let interval = body["intervalMinutes"].as_i64().unwrap_or(0);
        if !valid || interval < 1 {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "자동화는 등록된 에이전트·회의실·워크플로와 유효한 작업 입력, 1분 이상 간격이 필요합니다."
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
            let bad=|message:&str| (StatusCode::BAD_REQUEST,
                Json(json!({"error":message}))).into_response();
            let Some(fields)=body.as_object() else {
                return bad("수정 내용을 객체로 제출하세요.");
            };
            if fields.len()!=2 || !fields.contains_key("expectedText")
                || !fields.contains_key("text") {
                return bad("text 및 expectedText 항목만 제출하세요.");
            }
            let Some(previous)=body["expectedText"].as_str() else {
                return bad("수정 전 기억 내용이 필요합니다.");
            };
            let Some(new_text)=body["text"].as_str().map(str::trim)
                .filter(|value| !value.is_empty() && value.chars().count()<=4096)
            else { return bad("기억 내용은 1~4096자여야 합니다."); };
            match state.store.edit_owner_memory(&id,previous,new_text) {
                Ok(Some(updated)) => {
                    state.bus.record("memory",&updated);
                    Json(updated).into_response()
                }
                Ok(None) if state.store.get("memory",&id).is_none() => {
                    (StatusCode::NOT_FOUND,
                        Json(json!({"error":"기억을 찾을 수 없습니다."}))).into_response()
                }
                Ok(None) => {
                    (StatusCode::CONFLICT,
                        Json(json!({"error":"기억이 변경됐거나 직접 등록한 활성 기억이 아닙니다. 목록을 새로고침하세요."}))).into_response()
                }
                Err(_message) => (StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"기억 변경을 저장하지 못했습니다."}))).into_response(),
            }
        },
        ("workflow", "run") => {
            let Some(definition)=state.store.get("workflow",&id) else {
                return (StatusCode::NOT_FOUND,Json(json!({
                    "error":"실행할 워크플로를 찾지 못했습니다."
                }))).into_response();
            };
            let (input,source)=match crate::engine::workflows::resolve_run_input(&state,&body){
                Ok(data)=>data,
                Err(message)=>return (StatusCode::BAD_REQUEST,
                    Json(json!({"error":message}))).into_response(),
            };
            let budget=body["tokenBudget"].as_u64().unwrap_or(0).min(2_000_000);
            enqueue_job(&state,"workflow",json!({
                "workflowId":id,"definition":definition,
                "input":input,"sourceArtifact":source,"tokenBudget":budget
            }))
        },
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
    use crate::{config::Config,db::Store,vault::Vault};

    fn state() -> AppState {
        let dir=std::env::temp_dir().join(format!("dominion-sub3-memory-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),workspace:dir.clone(),web_dir:dir.join("web")};
        let store=Store::open(&config.db_path()).unwrap();
        let vault=Vault::open(&config.key_path());
        AppState::new(config,store,vault)
    }
    async fn edit(state:&AppState,id:&str,body:Value)->Response {
        let uri=format!("/api/memories/{id}/edit").parse().unwrap();
        action(State(state.clone()),OriginalUri(uri),Json(body)).await
    }
    async fn create(state:&AppState,body:Value)->Response {
        let uri="/api/memories".parse().unwrap();
        save(State(state.clone()),OriginalUri(uri),Json(body)).await
    }

    #[tokio::test]
    async fn edits_only_active_owner_memory_preserving_provenance_and_auditing_once(){
        let state=state();
        state.store.put("memory",&json!({"id":"owner-1","text":"before",
            "source":"user","scope":"agent","agentId":"agent-7","status":"active",
            "verification":"source_quote","evidenceQuote":"owner testimony",
            "sourceMessageIds":["message-1"],"importance":0.8})).unwrap();
        let ok=edit(&state,"owner-1",json!({"expectedText":"before","text":"after"})).await;
        assert_eq!(ok.status(),StatusCode::OK);
        let updated=state.store.get("memory","owner-1").unwrap();
        assert_eq!(updated["text"],"after");
        assert_eq!(updated["scope"],"agent");
        assert_eq!(updated["agentId"],"agent-7");
        assert_eq!(updated["source"],"user");
        assert_eq!(updated["verification"],"source_quote");
        assert_eq!(updated["evidenceQuote"],"owner testimony");
        assert_eq!(updated["sourceMessageIds"],json!(["message-1"]));
        assert_eq!(updated["importance"],0.8);
        assert_eq!(updated["ownerEditVersion"],1);
        let stale=edit(&state,"owner-1",json!({"expectedText":"before","text":"lost"})).await;
        assert_eq!(stale.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("memory","owner-1").unwrap()["text"],"after");
        let audit=state.store.recent_audit(10);
        let edits:Vec<_>=audit.iter().filter(|row|row["event"]=="memory.owner_edited").collect();
        assert_eq!(edits.len(),1);
        assert_eq!(edits[0]["detail"]["memoryId"],"owner-1");
        assert_eq!(edits[0]["detail"]["version"],1);
        assert!(!edits[0].to_string().contains("owner testimony"));
    }

    #[tokio::test]
    async fn learned_and_inactive_memory_are_never_editable(){
        let state=state();
        for (id,source,status) in [
            ("learned","learn","active"),
            ("reflected","reflect","active"),
            ("archived","user","archived"),
            ("draft","user","draft")
        ]{
            state.store.put("memory",&json!({"id":id,"text":"immutable",
                "source":source,"status":status})).unwrap();
            let result=edit(&state,id,json!({"text":"tamper",
                "expectedText":"immutable"})).await;
            assert_eq!(result.status(),StatusCode::CONFLICT,"memory id: {}",id);
            assert_eq!(state.store.get("memory",id).unwrap()["text"],"immutable");
        }
        let missing=edit(&state,"missing",json!({"text":"x","expectedText":"old"})).await;
        assert_eq!(missing.status(),StatusCode::NOT_FOUND);
        assert!(state.store.recent_audit(10).iter().all(|row|
            row["event"]!="memory.owner_edited"));
    }

    #[tokio::test]
    async fn generic_upsert_cannot_bypass_provenance_or_cas(){
        let state=state();
        state.store.put("memory",&json!({"id":"learned","text":"evidence",
            "source":"learn","status":"active"})).unwrap();
        let bypass=create(&state,json!({"id":"learned","text":"hijack",
            "source":"user","status":"active"})).await;
        assert_eq!(bypass.status(),StatusCode::BAD_REQUEST);
        let direct_uri="/api/memories/learned".parse().unwrap();
        let direct=save(State(state.clone()),OriginalUri(direct_uri),
            Json(json!({"text":"override"}))).await;
        assert_eq!(direct.status(),StatusCode::BAD_REQUEST);
        let spoof=create(&state,json!({"text":"fabricated provenance",
            "source":"learn"})).await;
        assert_eq!(spoof.status(),StatusCode::BAD_REQUEST);
        let extra=create(&state,json!({"text":"user","source":"user",
            "evidenceQuote":"fabricated"})).await;
        assert_eq!(extra.status(),StatusCode::BAD_REQUEST);
        assert_eq!(state.store.get("memory","learned").unwrap()["text"],"evidence");
        let authorized=create(&state,json!({"text":"owner fact","scope":"shared"})).await;
        assert_eq!(authorized.status(),StatusCode::OK);
    }

    #[test]
    fn concurrent_edits_cannot_both_win_or_create_two_audit_rows(){
        let state=state();
        state.store.put("memory",&json!({"id":"race","source":"user",
            "status":"active","text":"baseline"})).unwrap();
        let left=state.store.clone();
        let right=state.store.clone();
        let a=std::thread::spawn(move||
            left.edit_owner_memory("race","baseline","revision-a"));
        let b=std::thread::spawn(move||
            right.edit_owner_memory("race","baseline","revision-b"));
        let result_a=a.join().unwrap().unwrap();
        let result_b=b.join().unwrap().unwrap();
        assert_eq!(u8::from(result_a.is_some())+u8::from(result_b.is_some()),1);
        assert_eq!(state.store.get("memory","race").unwrap()["ownerEditVersion"],1);
        assert_eq!(state.store.recent_audit(10).iter().filter(|row|
            row["event"]=="memory.owner_edited").count(),1);
    }
}
