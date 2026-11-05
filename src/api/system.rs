use crate::engine;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use std::collections::HashMap;
use serde_json::{json,Value};
use std::time::Duration;
use std::convert::Infallible;
use futures_util::Stream;

pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "dominion",
        "version": env!("CARGO_PKG_VERSION"),
        "runtime": "rust",
        "engine": "rust",
        "instanceId":std::env::var("DOMINION_ENGINE_INSTANCE").ok(),
    }))
}

/// Read-only identity for the isolated benchmark runner. It never exposes
/// secrets and is only authoritative when the server was started with
/// DOMINION_BENCHMARK=1.
pub async fn benchmark_identity(State(state): State<AppState>) -> Json<Value> {
    let canonical = |path: &std::path::Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string()
    };
    Json(json!({
        "instanceId": std::env::var("DOMINION_ENGINE_INSTANCE").ok(),
        "benchmark": std::env::var("DOMINION_BENCHMARK").as_deref() == Ok("1"),
        "dataRoot": canonical(&state.config.data_dir),
        "workspace": canonical(&state.config.workspace),
        "build": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
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
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Reconnect contract: `?after=<seq>` or the SSE `Last-Event-ID` header.
    let after = params
        .get("after")
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            headers
                .get("last-event-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(0);
    let bus = state.bus.clone();
    let backlog = bus.replay(after, 500);
    let mut receiver = bus.subscribe();
    let stream = async_stream::stream! {
        let mut last = after;
        for value in backlog {
            let seq = value["seq"].as_u64().unwrap_or(0);
            last = last.max(seq);
            yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
        }
        loop {
            match receiver.recv().await {
                Ok(value) => {
                    let seq = value["seq"].as_u64().unwrap_or(0);
                    if seq <= last {
                        continue;
                    }
                    last = seq;
                    yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // The live buffer is bounded. Replay the gap from the
                    // durable log instead of silently dropping writes.
                    for value in bus.replay(last, 500) {
                        let seq = value["seq"].as_u64().unwrap_or(0);
                        if seq <= last {
                            continue;
                        }
                        last = seq;
                        yield Ok(Event::default().id(seq.to_string()).data(value.to_string()));
                    }
                }
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

pub async fn audit_log(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.recent_audit(200)))
}

pub async fn model_usage(State(state): State<AppState>) -> Json<Value> {
    Json(state.store.usage_summary())
}

pub async fn run_wakes(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("run_wake")))
}

// ----------------------------------------------------------------- translate

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
        .chain(
            state
                .store
                .all("capability")
                .into_iter()
                .map(|record| record),
        )
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
