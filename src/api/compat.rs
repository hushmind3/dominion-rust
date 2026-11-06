use crate::state::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn empty_list() -> Json<Value> {
    Json(json!([]))
}

pub async fn youtube_status(State(state): State<AppState>) -> Json<Value> {
    let connection = crate::engine::oauth::status(&state)
        .into_iter()
        .find(|p| p["id"] == "youtube")
        .unwrap_or(json!({}));
    Json(
        json!({"connected":connection["connected"],"uploadAllowed":connection["connected"]==true&&connection["grantedScopes"].as_str().is_some_and(|s|s.split_whitespace().any(|s|s=="https://www.googleapis.com/auth/youtube.upload")),"account":connection["account"],"uploads":state.store.all("job").iter().filter(|j|j["kind"]=="youtube_upload").map(crate::engine::video::public_job).collect::<Vec<_>>(),"channel":null}),
    )
}


/// Opens the platform folder picker. Headless hosts can set DOMINION_FOLDER
/// to bypass the dialog; the result is always a real existing directory.
pub async fn pick_folder() -> Response {
    if let Ok(path) = std::env::var("DOMINION_FOLDER") {
        if std::path::Path::new(&path).is_dir() {
            return Json(json!({"path": path})).into_response();
        }
    }
    pick_path(true).await
}

pub async fn pick_model() -> Response {
    pick_path(false).await
}

async fn pick_path(directory: bool) -> Response {
    let command = if cfg!(target_os = "macos") {
        let script = if directory {
            "try\nreturn POSIX path of (choose folder)\non error number -128\nreturn \"\"\nend try"
        } else {
            "try\nreturn POSIX path of (choose file with prompt \"GGUF 파일 선택\")\non error number -128\nreturn \"\"\nend try"
        };
        vec!["/usr/bin/osascript", "-e", script]
    } else if cfg!(target_os = "windows") {
        let script = if directory {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.FolderBrowserDialog; if($d.ShowDialog() -eq 'OK'){$d.SelectedPath}"
        } else {
            "Add-Type -AssemblyName System.Windows.Forms; $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='GGUF|*.gguf'; if($d.ShowDialog() -eq 'OK'){$d.FileName}"
        };
        vec!["powershell", "-NoProfile", "-STA", "-Command", script]
    } else if directory {
        vec!["zenity", "--file-selection", "--directory"]
    } else {
        vec!["zenity", "--file-selection", "--file-filter=GGUF | *.gguf"]
    };
    let output = tokio::process::Command::new(command[0])
        .args(&command[1..])
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                Json(json!({"cancelled": true})).into_response()
            } else {
                Json(json!({"path": path})).into_response()
            }
        }
        Ok(output) if cfg!(all(unix, not(target_os = "macos"))) && output.status.code() == Some(1) =>
            Json(json!({"cancelled": true})).into_response(),
        Ok(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"파일 선택기를 실행하지 못했습니다."}))).into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("폴더 선택기를 실행할 수 없습니다: {error}")})),
        )
            .into_response(),
    }
}

/// Receives an inbound webhook and turns it into a normal durable prompt job.
/// The engine, tools, approvals and checkpointing are the same as any chat.
pub async fn webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let Some(config) = state.store.get("webhook", &id) else {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"unknown webhook"}))).into_response();
    };
    if config["enabled"].as_bool() == Some(false) {
        return (StatusCode::NOT_FOUND, Json(json!({"error":"webhook disabled"}))).into_response();
    }
    if let Some(secret) = config["secret"].as_str().filter(|value| !value.is_empty()) {
        let header = headers
            .get("x-webhook-token")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let body_token = body["token"].as_str().unwrap_or("");
        if header != secret && body_token != secret {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error":"invalid webhook token"}))).into_response();
        }
    }
    let prompt = body["prompt"]
        .as_str()
        .or_else(|| body["text"].as_str())
        .or_else(|| body["message"].as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error":"prompt/text/message is required"}))).into_response();
    }
    let input = json!({
        "prompt": prompt,
        "sessionId": config["sessionId"],
        "agentId": config["agentId"],
    });
    match crate::engine::jobs::enqueue(&state, "prompt", input) {
        Some(job) => Json(json!({"accepted": true, "job": job})).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"could not enqueue"}))).into_response(),
    }
}

/// Compatibility actions for YouTube live/upload flows. Each maps to the
/// existing OAuth/video implementation instead of a 501 stub.
pub async fn youtube_action(
    State(state): State<AppState>,
    Path(action): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match action.as_str() {
        "connect" | "auth" => match crate::engine::oauth::start(&state, "youtube") {
            Ok(value) => Json(value).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
        },
        "disconnect" => {
            crate::engine::oauth::disconnect(&state, "youtube");
            Json(json!({"disconnected": true, "status": crate::engine::oauth::status(&state)}))
                .into_response()
        }
        "upload" => {
            let id = body["jobId"]
                .as_str()
                .or_else(|| body["id"].as_str())
                .unwrap_or("");
            if id.is_empty() {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":"jobId is required"}))).into_response();
            }
            match crate::engine::video::enqueue_upload(&state, id, &body) {
                Ok(job) => Json(job).into_response(),
                Err(error) => (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response(),
            }
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("unknown youtube action: {other}")})),
        )
            .into_response(),
    }
}

pub async fn upload(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    let raw = query
        .get("name")
        .map(String::as_str)
        .unwrap_or("upload.bin")
        .to_string();
    let name: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let dir = state.config.data_dir.join("uploads");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response();
    }
    let path = dir.join(&name);
    match std::fs::write(&path, &body) {
        Ok(_) => Json(json!({
            "ok": true,
            "name": name,
            "path": path.to_string_lossy(),
            "bytes": body.len(),
        }))
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}
