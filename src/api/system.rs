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

pub async fn experiment_requests(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("experiment_request")))
}

pub async fn policies(State(state): State<AppState>) -> Json<Value> {
    Json(json!(crate::engine::policy::list(&state)))
}

pub async fn policy_searches(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.store.all("policy_search")))
}

pub async fn policy_patches(State(state): State<AppState>) -> Json<Value> {
    Json(json!(crate::engine::policy_patch::list(&state)))
}

pub async fn scheduler_runs(State(state): State<AppState>) -> Json<Value> {
    let mut runs = state.store.all("scheduler_run");
    runs.sort_by(|a, b| {
        b["startedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["startedAt"].as_str().unwrap_or(""))
    });
    Json(json!(runs))
}

/// Creates a consistent snapshot inside the data directory for a restore drill.
pub async fn create_backup(State(state): State<AppState>) -> axum::response::Response {
    use axum::response::IntoResponse;
    match crate::engine::maintenance::backup_now_with_remote(&state).await {
        Ok((path, remote)) => {
            let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            Json(json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "bytes": bytes,
                "encrypted": true,
                "remote": remote.map(|report| json!({
                    "attempted": true,
                    "uploaded": report.uploaded,
                    "verified": report.verified,
                    "url": report.url,
                    "sha256": report.sha256,
                    "attempts": report.attempts,
                    "error": report.error,
                })),
            }))
            .into_response()
        }
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": error})),
        )
            .into_response(),
    }
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

pub async fn list_backups(State(state): State<AppState>) -> Json<Value> {
    let directory = state.config.data_dir.join("backups");
    let mut items = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("enc" | "sqlite")
                    )
                })
                .filter_map(|path| {
                    let name = path.file_name()?.to_str()?.to_string();
                    let bytes = std::fs::metadata(&path).ok()?.len();
                    Some(json!({
                        "name": name,
                        "path": path.to_string_lossy(),
                        "bytes": bytes,
                        "encrypted": path.extension().and_then(|value| value.to_str()) == Some("enc"),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    Json(json!(items))
}

pub async fn backup_keys(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::backup_keys::status(&state))
}

pub async fn rotate_backup_keys(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"키 회전은 유지보수 모드에서만 가능합니다."})),
        )
            .into_response();
    }
    let keep_old = body["keepOld"].as_bool().unwrap_or(false);
    match crate::engine::backup_keys::rotate(&state, keep_old) {
        Ok(report) => Json(json!({
            "ok": true,
            "activeVersion": report.active_version,
            "previousVersion": report.previous_version,
            "reencrypted": report.reencrypted,
            "retainedVersions": report.retained_versions,
        }))
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
            .into_response(),
    }
}

pub async fn secret_key(State(state): State<AppState>) -> Json<Value> {
    Json(crate::engine::vault_rotation::status(&state))
}

pub async fn rotate_secret_key(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"secret key 회전은 유지보수 모드에서만 가능합니다."})),
        )
            .into_response();
    }
    let keep_old = body["keepOld"].as_bool().unwrap_or(false);
    match crate::engine::vault_rotation::rotate(&state, keep_old) {
        Ok(report) => Json(json!({
            "ok": true,
            "secretsReencrypted": report.secrets_reencrypted,
            "legacyBackupsMigrated": report.legacy_backups_migrated,
            "previousKeys": report.previous_keys,
            "oldKeyRetained": report.old_key_file.is_some(),
        }))
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error, "oldKeyRetained": true})),
        )
            .into_response(),
    }
}

pub async fn maintenance(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let enabled = body["enabled"].as_bool().unwrap_or(false);
    state.set_maintenance(enabled);
    Json(json!({"maintenance": enabled}))
}

/// Restores an encrypted backup while maintenance mode is explicit. The
/// restored data replaces the live database under the store mutex.
pub async fn restore_backup(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !state.maintenance_mode() {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({"error":"유지보수 모드에서만 복구할 수 있습니다."})),
        )
            .into_response();
    }
    if body["confirm"].as_bool() != Some(true) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"confirm=true 가 필요합니다."})),
        )
            .into_response();
    }
    let Some(requested) = body["path"].as_str().filter(|value| !value.is_empty()) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"path 가 필요합니다."})),
        )
            .into_response();
    };
    let directory = state.config.data_dir.join("backups");
    let candidate = {
        let path = std::path::Path::new(requested);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            directory.join(path)
        }
    };
    let canonical_directory = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.clone());
    let Ok(canonical) = candidate.canonicalize() else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(json!({"error":"백업 파일을 찾을 수 없습니다."})),
        )
            .into_response();
    };
    if !canonical.starts_with(&canonical_directory)
        || canonical.extension().and_then(|value| value.to_str()) != Some("enc")
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"data/backups 안의 .enc 백업만 복구할 수 있습니다."})),
        )
            .into_response();
    }
    let sealed = match std::fs::read(&canonical) {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": error.to_string()})),
            )
                .into_response()
        }
    };
    let Some(decoded) = crate::engine::backup_keys::decrypt_backup(&state, &sealed) else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error":"이 키로 백업을 복호화할 수 없습니다."})),
        )
            .into_response();
    };
    let (bytes, embedded_key) = if decoded.len() > 41 && decoded.starts_with(b"DOMVAULT1") {
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded[9..41]);
        (decoded[41..].to_vec(), Some(key))
    } else {
        (decoded, None)
    };
    let target_key = embedded_key.unwrap_or_else(|| state.vault.primary_key());
    // Verify every sealed secret in the snapshot is decryptable with the key
    // we would install. Abort before touching the live database/key otherwise.
    if let Err(error) = crate::engine::vault_rotation::verify_snapshot_secrets(
        &target_key,
        &bytes,
        &state.config.data_dir,
    ) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(json!({
                "error": format!("복구 대상의 secret key를 확인할 수 없습니다: {error}. 현재 상태를 유지합니다.")
            })),
        )
            .into_response();
    }
    if let Some(key) = embedded_key {
        if let Err(error) = state.vault.restore_primary(key) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("secret key 복원 실패: {error}")})),
            )
                .into_response();
        }
    }
    if let Err(error) = state.store.replace_database(&bytes) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error})),
        )
            .into_response();
    }
    if let Err(error) = crate::migrations::run(&state) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("복구 후 마이그레이션 실패: {error}")})),
        )
            .into_response();
    }
    state.store.audit(
        "system.restored",
        json!({"path": canonical.to_string_lossy()}),
    );
    Json(json!({
        "ok": true,
        "executions": state.store.count("execution"),
    }))
    .into_response()
}
