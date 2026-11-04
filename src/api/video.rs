use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

fn default_config() -> Value {
    json!({
        "engine": "comfyui",
        "videoApiUrl": "",
        "videoModel": "",
        "hasVideoKey": false,
        "comfyUrl": "",
        "comfyDir": "",
        "workflowDir": "",
        "outputDir": "",
    })
}

fn config(state: &AppState) -> Value {
    let mut merged = default_config();
    if let Some(record) = state.store.get("video_config", "default") {
        if let (Value::Object(target), Value::Object(source)) = (&mut merged, record) {
            for (key, value) in source {
                target.insert(key, value);
            }
        }
    }
    merged
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn scan_workflows(dir: &str) -> Vec<Value> {
    if dir.trim().is_empty() {
        return Vec::new();
    }
    let dir = expand(dir);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_string();
            if !name.ends_with(".json") {
                return None;
            }
            let bytes = entry.metadata().ok()?.len();
            Some(json!({ "id": name, "name": name, "bytes": bytes }))
        })
        .collect()
}

pub fn outputs(state: &AppState) -> Vec<Value> {
    let config = config(state);
    let dir = string_field(&config, "outputDir");
    if dir.trim().is_empty() {
        return Vec::new();
    }
    let dir = expand(&dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut rows = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !["mp4", "webm", "mov", "gif"]
                .iter()
                .any(|ext| lower.ends_with(ext))
            {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            Some(json!({
                "file": path.to_string_lossy(),
                "name": name,
                "bytes": metadata.len(),
                "modified": metadata.modified().ok()
                    .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
            }))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.get("modified").and_then(|v| v.as_str()).unwrap_or("")
            .cmp(a.get("modified").and_then(|v| v.as_str()).unwrap_or(""))
    });
    rows
}

async fn comfy_reachable(url: &str) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let endpoint = format!(
        "{}/system_stats",
        parsed.as_str().trim_end_matches('/')
    );
    reqwest::Client::new()
        .get(endpoint)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

pub async fn status(State(state): State<AppState>) -> Json<Value> {
    let config = config(&state);
    let comfy_url = string_field(&config, "comfyUrl");
    let reachable = comfy_reachable(&comfy_url).await;
    let workflows = scan_workflows(&string_field(&config, "workflowDir"));
    let comfy_dir = string_field(&config, "comfyDir");
    let jobs = state.store.all("video_job");
    Json(json!({
        "config": config,
        "comfy": { "reachable": reachable },
        "installed": !comfy_dir.trim().is_empty() && expand(&comfy_dir).exists(),
        "workflows": workflows,
        "models": [],
        "modelsReady": false,
        "jobs": jobs,
        "runtime": "rust",
        "available": false,
    }))
}

pub async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(config(&state))
}

pub async fn save_config(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let mut record = body.clone();
    record["id"] = json!("default");
    if let Some(key) = body
        .get("videoApiKey")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    {
        state.save_secret("video:api", key);
    }
    if let Value::Object(ref mut map) = record {
        map.remove("videoApiKey");
    }
    record["hasVideoKey"] = json!(state.store.has_secret("video:api"));
    state.store.put("video_config", &record);
    Json(config(&state))
}

pub async fn locations(State(state): State<AppState>) -> Json<Value> {
    let config = config(&state);
    let collect = |key: &str| {
        [string_field(&config, key)]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    };
    Json(json!({
        "comfy": collect("comfyDir"),
        "workflows": collect("workflowDir"),
        "outputs": collect("outputDir"),
    }))
}

pub async fn outputs_handler(State(state): State<AppState>) -> Json<Value> {
    Json(json!(outputs(&state)))
}

pub async fn empty_list() -> Json<Value> {
    Json(json!([]))
}

pub async fn job(Path(_id): Path<String>) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "영상 작업을 찾을 수 없습니다." })),
    )
        .into_response()
}

pub async fn not_available() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "영상 생성 파이프라인은 아직 Rust 백엔드로 이식되지 않았습니다. ComfyUI 설정과 워크플로만 표시됩니다.",
            "ported": false,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-video-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&cfg.db_path()).unwrap();
        let vault = Vault::open(&cfg.key_path());
        (AppState::new(cfg, store, vault), dir)
    }

    #[test]
    fn status_shape_has_every_field_the_ui_reads() {
        let (state, dir) = temp_state();
        let value = config(&state);
        for key in [
            "engine",
            "videoApiUrl",
            "videoModel",
            "comfyUrl",
            "comfyDir",
            "workflowDir",
            "outputDir",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["engine"], "comfyui");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outputs_scan_only_video_files() {
        let (state, dir) = temp_state();
        let out = dir.join("outputs");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("clip.mp4"), b"x").unwrap();
        std::fs::write(out.join("note.txt"), b"x").unwrap();
        state.store.put(
            "video_config",
            &json!({ "id": "default", "outputDir": out.to_string_lossy() }),
        );
        let rows = outputs(&state);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], "clip.mp4");
        let _ = std::fs::remove_dir_all(dir);
    }
}
