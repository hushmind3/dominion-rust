use crate::engine;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures_util::Stream;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;
use tokio::sync::mpsc;

pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "dominion",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
    }))
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let jobs = state.store.all("job");
    let running = jobs
        .iter()
        .filter(|job| {
            matches!(
                job.get("status").and_then(|value| value.as_str()),
                Some("queued") | Some("running")
            )
        })
        .count();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
        "providers": state.store.count("provider"),
        "agents": state.store.count("agent"),
        "sessions": state.store.count("session"),
        "rooms": state.store.count("room"),
        "mcp": state.store.count("mcp"),
        "skills": state.store.count("skill"),
        "memories": state.store.count("memory"),
        "jobs": running,
        "executions": state.store.count("execution"),
        "workspace": state.config.workspace.to_string_lossy(),
        "data": state.config.data_dir.to_string_lossy(),
    }))
}

pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.bus.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(value) => yield Ok(Event::default().data(value.to_string())),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn chat(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let task_state = state.clone();
    tokio::spawn(async move {
        engine::chat::run(task_state, body, sender).await;
    });

    let stream = async_stream::stream! {
        while let Some(event) = receiver.recv().await {
            yield Ok::<_, Infallible>(Event::default().data(event.to_string()));
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------- providers

pub async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let items = state
        .store
        .all("provider")
        .into_iter()
        .map(|provider| public_provider(&state, provider))
        .collect::<Vec<_>>();
    Json(json!(items))
}

fn public_provider(state: &AppState, mut provider: Value) -> Value {
    let id = provider
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    if let Value::Object(ref mut map) = provider {
        map.remove("apiKey");
        map.insert(
            "hasKey".into(),
            json!(state.store.has_secret(&format!("provider:{id}"))),
        );
    }
    provider
}

pub async fn save_provider(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let name = body
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let base_url = body
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_string();
    if name.is_empty() || base_url.is_empty() {
        return error(StatusCode::BAD_REQUEST, "제공자 이름과 baseUrl 이 필요합니다.");
    }
    let parsed = match reqwest::Url::parse(&base_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return error(StatusCode::BAD_REQUEST, "HTTP(S) 주소만 허용됩니다."),
    };

    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    if body.get("kind").and_then(|value| value.as_str()) == Some("anthropic") {
        return error(
            StatusCode::BAD_REQUEST,
            "Anthropic 네이티브 형식은 지원하지 않습니다. OpenAI 호환 엔드포인트를 사용하세요.",
        );
    }
    let kind = match body.get("kind").and_then(|value| value.as_str()) {
        Some("ollama") => "ollama",
        _ => "openai",
    };

    let mut record = body.clone();
    record["id"] = json!(id);
    record["name"] = json!(name);
    record["baseUrl"] = json!(parsed.to_string().trim_end_matches('/'));
    record["kind"] = json!(kind);
    record["enabled"] = json!(body
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true));
    record["toolMode"] = json!(match body.get("toolMode").and_then(|value| value.as_str()) {
        Some("native") => "native",
        Some("compat") => "compat",
        Some("auto") => "auto",
        _ => if kind == "ollama" { "compat" } else { "auto" },
    });
    if let Value::Object(ref mut map) = record {
        map.remove("apiKey");
        map.remove("hasKey");
    }

    if let Some(api_key) = body.get("apiKey").and_then(|value| value.as_str()) {
        if api_key.starts_with("••") {
            // Masked placeholder sent back by the UI: keep the stored secret.
        } else if api_key.is_empty() {
            state.store.remove_secret(&format!("provider:{id}"));
        } else {
            state.save_secret(&format!("provider:{id}"), api_key);
        }
    }

    let saved = state
        .store
        .put("provider", &record)
        .unwrap_or_else(|| json!({ "id": id }));
    state.store.audit("provider.saved", json!({ "providerId": saved.get("id") }));
    state.bus.record("provider", &saved);
    Json(public_provider(&state, saved)).into_response()
}

pub async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    state.store.remove("provider", &id);
    state.store.remove_secret(&format!("provider:{id}"));
    state.bus.emit(json!({ "type": "record", "kind": "provider", "removed": id }));
    Json(json!({ "ok": true }))
}

pub async fn provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(provider) = state.store.get("provider", &id) else {
        return error(StatusCode::NOT_FOUND, "제공자를 찾을 수 없습니다.");
    };
    let base = provider
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let kind = provider
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("openai");
    let url = if kind == "ollama" {
        format!("{base}/api/tags")
    } else {
        format!("{base}/models")
    };
    let key = state.read_secret(&format!("provider:{id}")).unwrap_or_default();

    let client = reqwest::Client::new();
    let mut request = client.get(&url).timeout(Duration::from_secs(12));
    if !key.is_empty() {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    match request.send().await {
        Ok(response) if response.status().is_success() => {
            let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
            let models: Vec<Value> = if kind == "ollama" {
                value["models"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("name"))
                            .map(|name| json!({ "id": name, "name": name }))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                value["data"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("id"))
                            .map(|id| json!({ "id": id, "name": id }))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            Json(json!(models)).into_response()
        }
        Ok(response) => error(
            StatusCode::BAD_GATEWAY,
            &format!("모델 목록 HTTP {}", response.status()),
        ),
        Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
    }
}

// ------------------------------------------------------------------- agents

pub async fn list_agents(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("agent")))
}

pub async fn save_agent(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    if body
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return error(StatusCode::BAD_REQUEST, "에이전트 이름이 필요합니다.");
    }
    let saved = match state.store.put("agent", &body) {
        Some(record) => record,
        None => return error(StatusCode::BAD_REQUEST, "에이전트를 저장할 수 없습니다."),
    };
    state.bus.record("agent", &saved);
    Json(saved).into_response()
}

pub async fn delete_agent(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("agent", &id);
    state.bus.emit(json!({ "type": "record", "kind": "agent", "removed": id }));
    Json(json!({ "ok": true }))
}

// ----------------------------------------------------------------- sessions

pub async fn list_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("session")))
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut record = body.clone();
    if record
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        record["title"] = json!("새 대화");
    }
    let saved = state
        .store
        .put("session", &record)
        .unwrap_or_else(|| json!({}));
    state.bus.record("session", &saved);
    Json(saved)
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.store.get("session", &id) {
        Some(record) => Json(record).into_response(),
        None => error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다."),
    }
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    state.store.remove("session", &id);
    state.store.clear_messages(&id);
    state.bus.emit(json!({ "type": "record", "kind": "session", "removed": id }));
    Json(json!({ "ok": true }))
}

pub async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    Json(json!(state.store.messages(&id)))
}

pub async fn rename_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let Some(mut session) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
    };
    let title = body
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if title.is_empty() {
        return error(StatusCode::BAD_REQUEST, "제목이 비어 있습니다.");
    }
    session["title"] = json!(title);
    let saved = state.store.put("session", &session).unwrap_or(session);
    state.bus.record("session", &saved);
    Json(saved).into_response()
}

pub async fn clear_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    state.store.clear_messages(&id);
    state.bus.emit(json!({ "type": "record", "kind": "session", "id": id }));
    Json(json!({ "ok": true }))
}

pub async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(source) = state.store.get("session", &id) else {
        return error(StatusCode::NOT_FOUND, "세션을 찾을 수 없습니다.");
    };
    let mut fork = source.clone();
    if let Value::Object(ref mut map) = fork {
        map.remove("id");
        map.remove("createdAt");
        map.remove("updatedAt");
    }
    let title = source
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("새 대화");
    fork["title"] = json!(format!("{title} (fork)"));
    let Some(fork) = state.store.put("session", &fork) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "fork 실패");
    };
    let fork_id = fork
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    for mut message in state.store.messages(&id) {
        if let Value::Object(ref mut map) = message {
            map.remove("id");
            map.insert("session_id".into(), json!(fork_id));
        }
        state.store.write_message(&message);
    }
    state.bus.record("session", &fork);
    Json(fork).into_response()
}

// ---------------------------------------------------------------- approvals

pub async fn list_approvals(State(state): State<AppState>) -> Json<Value> {
    let pending = state
        .store
        .all("approval")
        .into_iter()
        .filter(|record| {
            matches!(
                record.get("status").and_then(|value| value.as_str()),
                Some("pending") | Some("interrupted")
            )
        })
        .collect::<Vec<_>>();
    Json(json!(pending))
}

pub async fn resolve_approval(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let allow = body
        .get("allow")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if id.is_empty() {
        return error(StatusCode::BAD_REQUEST, "승인 id 가 필요합니다.");
    }
    let waiter = state.approvals.lock().unwrap().remove(&id);
    match waiter {
        Some(sender) => {
            let _ = sender.send(allow);
            Json(json!({ "ok": true, "resolved": true })).into_response()
        }
        None => {
            if let Some(mut record) = state.store.get("approval", &id) {
                if record.get("status").and_then(|value| value.as_str()) == Some("pending") {
                    record["status"] = json!("interrupted");
                    state.store.put("approval", &record);
                }
            }
            Json(json!({
                "ok": false,
                "resolved": false,
                "error": "진행 중인 실행이 없습니다. 작업을 다시 요청하세요."
            }))
            .into_response()
        }
    }
}

// ------------------------------------------------------------------- policy

pub async fn get_policy(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.get("policy", "default").unwrap_or_else(|| {
        json!({
            "autoApproveAll": false,
            "autoApproveMcp": false,
            "autoApprove": []
        })
    }))
}

pub async fn save_policy(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut record = body.clone();
    record["id"] = json!("default");
    let saved = state.store.put("policy", &record).unwrap_or(record);
    state.bus.record("policy", &saved);
    Json(saved)
}

// -------------------------------------------------------------------- tools

pub async fn list_tools(State(state): State<AppState>) -> Json<Value> {
    let mut items = engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            let function = definition.get("function")?;
            let name = function.get("name")?.as_str()?.to_string();
            Some(json!({
                "name": name,
                "description": function.get("description"),
                "parameters": function.get("parameters"),
                "approval": engine::tools::requires_approval(&name),
                "source": "builtin",
            }))
        })
        .collect::<Vec<_>>();

    for tool in engine::mcp::tools(&state).await {
        let qualified = tool.qualified();
        items.push(json!({
            "name": qualified,
            "description": format!("[MCP · {}] {}", tool.server_name, tool.description),
            "parameters": tool.schema,
            "approval": engine::mcp::requires_approval(&state, &qualified),
            "source": "mcp",
            "server": tool.server_name,
        }));
    }
    Json(json!(items))
}

pub async fn mcp_presets() -> Json<Value> {
    // Filesystem, git, memory and shell are native Rust tools. Do not push
    // third-party npm MCP servers for capabilities the engine already owns.
    Json(json!([]))
}

pub async fn mcp_discover(State(state): State<AppState>) -> Json<Value> {
    let items = engine::mcp::tools(&state)
        .await
        .into_iter()
        .map(|tool| {
            let qualified = tool.qualified();
            json!({
                "server": tool.server_name,
                "name": qualified,
                "tool": tool.tool,
                "description": tool.description,
                "parameters": tool.schema,
                "approval": engine::mcp::requires_approval(&state, &qualified),
            })
        })
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn audit_log(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.recent_audit(200)))
}

// ----------------------------------------------------------------- translate

pub async fn translate(Json(body): Json<Value>) -> Response {
    let text = body
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let target = body
        .get("target")
        .and_then(|value| value.as_str())
        .unwrap_or("ko");
    if text.is_empty() {
        return Json(json!({ "text": "" })).into_response();
    }
    if !text.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return Json(json!({ "text": text })).into_response();
    }
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={target}&dt=t&q={}",
        urlencode(&text)
    );
    match reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        Ok(response) => match response.json::<Value>().await {
            Ok(value) => {
                let translated = value
                    .get(0)
                    .and_then(|rows| rows.as_array())
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|row| row.get(0).and_then(|cell| cell.as_str()))
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                if translated.is_empty() {
                    Json(json!({ "text": text })).into_response()
                } else {
                    Json(json!({ "text": translated })).into_response()
                }
            }
            Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
        },
        Err(err) => error(StatusCode::BAD_GATEWAY, &err.to_string()),
    }
}

pub async fn translate_config() -> Json<Value> {
    Json(json!({ "target": "ko", "provider": "google" }))
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// -------------------------------------------------------------------- misc

pub async fn model_route(State(state): State<AppState>) -> Json<Value> {
    let providers = state.store.all("provider");
    let provider = providers.iter().find(|provider| {
        provider
            .get("enabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(true)
    });
    match provider {
        Some(provider) => Json(json!({
            "providerId": provider.get("id"),
            "model": provider.get("model").cloned().unwrap_or(json!("")),
            "kind": provider.get("kind"),
        })),
        None => Json(json!({ "providerId": "", "model": "" })),
    }
}

pub async fn memory_overview(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "total": state.store.count("memory"),
        "scopes": [],
        "updatedAt": chrono::Utc::now().to_rfc3339(),
    }))
}

pub async fn capabilities(State(state): State<AppState>) -> Json<Value> {
    let tools = engine::tools::definitions()
        .into_iter()
        .filter_map(|definition| {
            let function = definition.get("function")?;
            Some(json!({
                "id": function.get("name")?,
                "namespace": "builtin",
                "name": function.get("name")?,
                "description": function.get("description"),
                "operations": ["invoke"],
                "available": true,
            }))
        })
        .chain(state.store.all("capability").into_iter().map(|record| record))
        .collect::<Vec<_>>();
    Json(json!({ "items": tools, "updatedAt": chrono::Utc::now().to_rfc3339() }))
}

pub async fn commands(State(state): State<AppState>) -> Json<Value> {
    let candidates = [
        state.config.web_dir.join("shared/commands.json"),
        state.config.web_dir.join("../shared/commands.json"),
        state.config.workspace.join("shared/commands.json"),
    ];
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                return Json(value);
            }
        }
    }
    Json(json!([]))
}

pub async fn active_executions(State(state): State<AppState>) -> Json<Value> {
    let items = state
        .store
        .all("execution")
        .into_iter()
        .filter(|execution| {
            execution
                .get("status")
                .and_then(|value| value.as_str())
                == Some("running")
        })
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn execution_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let items = state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|record| record.get("runId").and_then(|value| value.as_str()) == Some(id.as_str()))
        .collect::<Vec<_>>();
    Json(json!(items))
}

pub async fn export_skill(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("skill", &id) {
        Some(skill) => Json(skill).into_response(),
        None => error(StatusCode::NOT_FOUND, "스킬을 찾을 수 없습니다."),
    }
}

pub async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let running = state
        .store
        .all("execution")
        .iter()
        .any(|execution| {
            execution.get("sessionId").and_then(|value| value.as_str()) == Some(id.as_str())
                && execution.get("status").and_then(|value| value.as_str()) == Some("running")
        });
    state.cancel_session(&id);
    state
        .store
        .audit("session.cancel", json!({ "sessionId": id, "running": running }));
    Json(json!({ "cancelled": running }))
}

pub async fn list_goals(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("goal")))
}

pub async fn get_goal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.store.get("goal", &id) {
        Some(goal) => Json(goal).into_response(),
        None => error(StatusCode::NOT_FOUND, "목표를 찾을 수 없습니다."),
    }
}

pub async fn delete_goal(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.store.remove("goal", &id);
    state.bus.emit(json!({ "type": "record", "kind": "goal", "removed": id }));
    Json(json!({ "ok": true }))
}

pub async fn create_goal(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let text = body
        .get("goal")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "목표 내용이 필요합니다.");
    }
    let executor = body.get("executor").cloned().unwrap_or_else(|| json!({}));
    let max_iterations = body
        .get("maxIterations")
        .cloned()
        .unwrap_or_else(|| json!(5));

    let Some(record) = state.store.put(
        "goal",
        &json!({
            "goal": text,
            "executor": executor,
            "status": "queued",
            "iterations": 0,
            "maxIterations": max_iterations,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    ) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "목표를 저장할 수 없습니다.");
    };
    state.bus.record("goal", &record);
    let goal_id = record
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let job = crate::engine::jobs::enqueue(&state, "goal", json!({ "goalId": goal_id }));
    let job_id = job
        .as_ref()
        .and_then(|job| job.get("id"))
        .cloned()
        .unwrap_or(Value::Null);

    let mut payload = record.clone();
    payload["jobId"] = job_id;
    Json(payload).into_response()
}

pub async fn compact_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let provider_id = body
        .get("providerId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let model = body.get("model").and_then(|value| value.as_str()).unwrap_or("");
    match engine::compact::run(&state, &id, provider_id, model).await {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn skill_catalog(State(state): State<AppState>) -> Json<Value> {
    Json(engine::skills::catalog(&state))
}

pub async fn skill_catalog_meta(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "roots": engine::skills::roots(&state).into_iter().map(|(source, path)| json!({
            "source": source,
            "path": path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    }))
}

pub async fn import_skill(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let id = body.get("id").and_then(|value| value.as_str()).unwrap_or("");
    match engine::skills::import(&state, id) {
        Ok(record) => Json(record).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn import_skill_repo(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let repo = body.get("repo").and_then(|value| value.as_str()).unwrap_or("");
    match engine::skills::import_repo(&state, repo) {
        Ok(result) => Json(result).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

pub async fn mcp_catalog(Query(query): Query<HashMap<String, String>>) -> Response {
    let mut endpoint = match reqwest::Url::parse(
        "https://registry.modelcontextprotocol.io/v0.1/servers",
    ) {
        Ok(url) => url,
        Err(message) => return error(StatusCode::INTERNAL_SERVER_ERROR, &message.to_string()),
    };
    let search = query.get("q").cloned().unwrap_or_default();
    if search.len() >= 3 {
        endpoint.query_pairs_mut().append_pair("search", &search);
    }
    endpoint.query_pairs_mut().append_pair("version", "latest").append_pair("limit", "60");

    let response = reqwest::Client::new()
        .get(endpoint)
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("MCP registry HTTP {}", response.status()),
            )
        }
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let value: Value = match response.json().await {
        Ok(value) => value,
        Err(message) => return error(StatusCode::BAD_GATEWAY, &message.to_string()),
    };
    let mut seen = std::collections::HashSet::new();
    let items = value
        .get("servers")
        .and_then(|value| value.as_array())
        .map(|servers| {
            servers
                .iter()
                .filter_map(|entry| {
                    let server = entry.get("server").unwrap_or(entry);
                    let name = server.get("name").and_then(|value| value.as_str())?;
                    if !seen.insert(name.to_string()) {
                        return None;
                    }
                    let packages = server
                        .get("packages")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items.iter().find(|package| {
                                package
                                    .get("registryType")
                                    .or_else(|| package.get("registry_type"))
                                    .and_then(|value| value.as_str())
                                    == Some("npm")
                            })
                        })
                        .flatten();
                    let remote = server
                        .get("remotes")
                        .and_then(|value| value.as_array())
                        .and_then(|items| {
                            items.iter().find(|remote| {
                                remote
                                    .get("url")
                                    .and_then(|value| value.as_str())
                                    .map(|url| url.starts_with("https://"))
                                    .unwrap_or(false)
                            })
                        });
                    Some(json!({
                        "name": name,
                        "description": server.get("description"),
                        "packageName": packages.and_then(|package| package.get("identifier")),
                        "url": remote.and_then(|remote| remote.get("url")),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Json(json!(items)).into_response()
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}
