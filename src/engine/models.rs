use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::process::{Child, Command};

const RUNTIME_PORT: u16 = 4319;
const PROVIDER_ID: &str = "dominion-native";
const MODEL_KIND: &str = "model_job";

struct Runtime {
    child: Option<Child>,
    status: String,
    id: String,
    name: String,
    engine: String,
    backend: String,
    model_key: String,
    error: String,
    logs: Vec<String>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Mutex::new(Runtime {
            child: None,
            status: "idle".into(),
            id: String::new(),
            name: String::new(),
            engine: String::new(),
            backend: String::new(),
            model_key: String::new(),
            error: String::new(),
            logs: Vec::new(),
        })
    })
}

pub fn models_dir(state: &AppState) -> PathBuf {
    state.config.data_dir.join("models")
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

/// Installed local models: GGUF files under the data dir (recursive).
pub fn installed(state: &AppState) -> Vec<Value> {
    let dir = models_dir(state);
    let mut found = Vec::new();
    walk(&dir, 0, &mut found);
    found
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false)
        })
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            let relative = path
                .strip_prefix(&dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let file = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| relative.clone());
            Some(json!({
                "id": relative,
                "repo": relative.trim_end_matches(".gguf"),
                "optionId": file,
                "source": "local",
                "format": "gguf",
                "label": file,
                "bytes": metadata.len(),
                "sizeLabel": format_bytes(metadata.len()),
                "installedAt": metadata.modified().ok()
                    .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
            }))
        })
        .chain(lmstudio_models())
        .collect()
}

/// Models already managed by LM Studio (including its native MLX runtime).
fn lmstudio_models() -> Vec<Value> {
    let Some(lms) = lms_bin() else {
        return Vec::new();
    };
    let Ok(output) = std::process::Command::new(lms)
        .args(["ls", "--json"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(items) = serde_json::from_slice::<Vec<Value>>(&output.stdout) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter(|item| item["type"] == "llm")
        .filter_map(|item| {
            let key = item["modelKey"].as_str()?.to_string();
            let raw_format = item["format"].as_str().unwrap_or("gguf");
            let format = if raw_format == "safetensors" || raw_format == "mlx" {
                "mlx"
            } else {
                "gguf"
            };
            let bytes = item["sizeBytes"].as_u64().unwrap_or(0);
            Some(json!({
                "id": format!("lms:{key}"),
                "repo": item["publisher"].as_str().unwrap_or(&key),
                "optionId": key,
                "source": "lmstudio",
                "format": format,
                "label": item["displayName"].as_str().unwrap_or(&key),
                "bytes": bytes,
                "sizeLabel": format_bytes(bytes),
                "engine": if format == "mlx" { "MLX (LM Studio)" } else { "LM Studio (GGUF)" },
                "installedAt": Value::Null,
            }))
        })
        .collect()
}

fn lms_bin() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DOMINION_LMS_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = PathBuf::from(home).join(".lmstudio/bin/lms");
    path.is_file().then_some(path)
}

fn llama_server_bin() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }))
        .chain(
            ["/opt/homebrew/bin/llama-server", "/usr/local/bin/llama-server"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|path| path.is_file())
}

fn llama_available() -> bool {
    llama_server_bin().is_some()
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk(&path, depth + 1, found);
        } else {
            found.push(path);
        }
    }
}

pub fn import_local(state: &AppState, file_path: &str) -> Result<Value, String> {
    let source = PathBuf::from(file_path.trim());
    if !source.is_file() {
        return Err("파일을 찾을 수 없습니다.".into());
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension != "gguf" {
        return Err("지금은 GGUF 파일만 가져올 수 있습니다.".into());
    }
    let dir = models_dir(state);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let name = source
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or("파일 이름이 없습니다.")?;
    let target = dir.join(&name);
    if source != target {
        std::fs::copy(&source, &target).map_err(|error| error.to_string())?;
    }
    Ok(installed(state)
        .into_iter()
        .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(name.as_str()))
        .unwrap_or_else(|| json!({ "id": name })))
}

fn total_memory() -> u64 {
    // Cheapest portable-enough source; unknown stays 0 rather than guessing.
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            if let Ok(text) = String::from_utf8(output.stdout) {
                if let Ok(value) = text.trim().parse::<u64>() {
                    return value;
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb) = rest
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<u64>().ok())
                    {
                        return kb * 1024;
                    }
                }
            }
        }
    }
    0
}

pub fn runtime_snapshot(_state: &AppState) -> Value {
    let mut guard = runtime().lock().unwrap();
    if let Some(child) = guard.child.as_mut() {
        match child.try_wait() {
            Ok(Some(status)) => {
                guard.status = "error".into();
                guard.error = format!("{} 종료 (exit {status})", guard.engine);
                guard.child = None;
            }
            _ => {}
        }
    }
    let logs = guard
        .logs
        .iter()
        .rev()
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    json!({
        "status": guard.status,
        "id": guard.id,
        "name": guard.name,
        "engine": if guard.engine.is_empty() { "none" } else { guard.engine.as_str() },
        "backend": guard.backend,
        "port": RUNTIME_PORT,
        "error": guard.error,
        "logs": logs.into_iter().rev().collect::<Vec<_>>(),
        "capabilities": {
            "gguf": { "available": lms_bin().is_some() || llama_available(), "backend": "llama.cpp", "installable": false },
            "mlx": { "available": lms_bin().is_some(), "backend": "LM Studio MLX" },
            "memory": {
                "total": total_memory(),
                "free": 0,
                "platform": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
            },
            "gpu": null,
        }
    })
}

pub async fn load(state: &AppState, id: &str, context: u64) -> Result<Value, String> {
    if let Some(key) = id.strip_prefix("lms:") {
        return load_lmstudio(state, id, key, context).await;
    }
    let dir = models_dir(state);
    let candidate = dir.join(id);
    let resolved = if candidate.is_file() {
        candidate
    } else {
        installed(state)
            .into_iter()
            .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(id))
            .map(|_| dir.join(id))
            .filter(|path| path.is_file())
            .ok_or_else(|| format!("설치된 모델을 찾을 수 없습니다: {id}"))?
    };

    unload(state);
    {
        let mut guard = runtime().lock().unwrap();
        guard.status = "loading".into();
        guard.id = id.to_string();
        guard.engine = "llama.cpp".into();
        guard.backend = "llama".into();
        guard.model_key = id.to_string();
        guard.name = resolved
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| id.to_string());
        guard.error.clear();
        guard.logs.clear();
    }

    let llama_server = llama_server_bin()
        .ok_or("llama-server를 찾을 수 없습니다. llama.cpp를 설치하거나 PATH를 확인하세요.")?;
    let mut command = Command::new(llama_server);
    command.args([
        "-m",
        &resolved.to_string_lossy(),
        "--host",
        "127.0.0.1",
        "--port",
        &RUNTIME_PORT.to_string(),
        "-c",
        &context.clamp(512, 32768).to_string(),
        "--alias",
        id,
        "-ngl",
        "999",
    ]);
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("llama-server 실행 실패: {error}"))?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                push_log(line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                push_log(line);
            }
        });
    }
    runtime().lock().unwrap().child = Some(child);

    // Wait for llama-server to become healthy, then expose it as a provider so
    // the existing chat engine can talk to it unchanged.
    let client = reqwest::Client::new();
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{RUNTIME_PORT}/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if response.status().is_success() {
                {
                    let mut guard = runtime().lock().unwrap();
                    guard.status = "ready".into();
                }
                state.store.put(
                    "provider",
                    &json!({
                        "id": PROVIDER_ID,
                        "name": "DOMINION Native",
                        "baseUrl": format!("http://127.0.0.1:{RUNTIME_PORT}/v1"),
                        "kind": "openai",
                        "local": true,
                        "enabled": true,
                        "model": id,
                        "description": "llama.cpp runtime managed by the Rust engine",
                    }),
                );
                let model = id.to_string();
                return Ok(json!({ "status": "ready", "id": model, "name": model }));
            }
        }
        let exited = runtime()
            .lock()
            .unwrap()
            .child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten();
        if exited.is_some() {
            let logs = runtime().lock().unwrap().logs.clone();
            let mut guard = runtime().lock().unwrap();
            guard.status = "error".into();
            guard.error = format!(
                "llama-server 가 시작 중 종료되었습니다: {}",
                logs.last().cloned().unwrap_or_default()
            );
            return Err(guard.error.clone());
        }
    }

    let mut guard = runtime().lock().unwrap();
    guard.status = "error".into();
    guard.error = "모델 로딩 시간이 초과되었습니다.".into();
    Err(guard.error.clone())
}

async fn load_lmstudio(
    state: &AppState,
    id: &str,
    key: &str,
    context: u64,
) -> Result<Value, String> {
    let lms = lms_bin().ok_or("LM Studio CLI(lms)를 찾을 수 없습니다. LM Studio를 설치하세요.")?;
    unload(state);
    {
        let mut guard = runtime().lock().unwrap();
        guard.status = "loading".into();
        guard.id = id.to_string();
        guard.name = key.to_string();
        guard.engine = "MLX (LM Studio)".into();
        guard.backend = "lmstudio".into();
        guard.model_key = key.to_string();
        guard.error.clear();
        guard.logs.clear();
    }

    let running = std::process::Command::new(&lms)
        .args(["server", "status", "--json"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
        .and_then(|value| value["running"].as_bool())
        .unwrap_or(false);
    if !running {
        let output = std::process::Command::new(&lms)
            .args(["server", "start", "--port", "1234"])
            .output()
            .map_err(|error| format!("LM Studio 서버 시작 실패: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "LM Studio 서버 시작 실패: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }

    let already_loaded = std::process::Command::new(&lms)
        .args(["ps", "--json"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Vec<Value>>(&output.stdout).ok())
        .is_some_and(|items| {
            items.iter().any(|item| {
                item["modelKey"].as_str() == Some(key)
                    || item["identifier"].as_str() == Some(key)
            })
        });
    if !already_loaded {
        let output = std::process::Command::new(&lms)
            .args([
                "load",
                key,
                "--gpu",
                "max",
                "--context-length",
                &context.clamp(512, 32768).to_string(),
            ])
            .output()
            .map_err(|error| format!("LM Studio 모델 로드 실패: {error}"))?;
        if !output.status.success() {
            let mut guard = runtime().lock().unwrap();
            guard.status = "error".into();
            guard.error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(guard.error.clone());
        }
    }

    let client = reqwest::Client::new();
    for _ in 0..240 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Ok(response) = client
            .get("http://127.0.0.1:1234/v1/models")
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if response.status().is_success() {
                let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
                let served = value["data"]
                    .as_array()
                    .and_then(|items| {
                        items.iter().find(|item| item["id"] == key).or_else(|| items.first())
                    })
                    .and_then(|item| item["id"].as_str())
                    .unwrap_or(key);
                {
                    let mut guard = runtime().lock().unwrap();
                    guard.status = "ready".into();
                }
                state.store.put(
                    "provider",
                    &json!({
                        "id": PROVIDER_ID,
                        "name": "DOMINION Native",
                        "baseUrl": "http://127.0.0.1:1234/v1",
                        "kind": "openai",
                        "local": true,
                        "enabled": true,
                        "model": served,
                        "description": "LM Studio MLX runtime managed by the Rust engine",
                    }),
                );
                return Ok(json!({
                    "status": "ready",
                    "id": id,
                    "name": served,
                    "engine": "MLX (LM Studio)",
                }));
            }
        }
    }
    let mut guard = runtime().lock().unwrap();
    guard.status = "error".into();
    guard.error = "MLX 모델 로딩 시간이 초과되었습니다.".into();
    Err(guard.error.clone())
}

fn push_log(line: String) {
    let mut guard = runtime().lock().unwrap();
    guard.logs.push(line);
    if guard.logs.len() > 200 {
        let excess = guard.logs.len() - 200;
        guard.logs.drain(0..excess);
    }
}

pub fn unload(state: &AppState) -> Value {
    state.store.remove("provider", PROVIDER_ID);
    state
        .bus
        .emit(json!({ "type": "record", "kind": "provider", "removed": PROVIDER_ID }));
    let mut guard = runtime().lock().unwrap();
    if guard.backend == "lmstudio" && !guard.model_key.is_empty() {
        if let Some(lms) = lms_bin() {
            let _ = std::process::Command::new(lms)
                .args(["unload", &guard.model_key])
                .output();
        }
    }
    if let Some(child) = guard.child.as_mut() {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &format!("-{pid}")])
                .status();
        }
        let _ = child.start_kill();
    }
    guard.child = None;
    guard.status = "idle".into();
    guard.id.clear();
    guard.name.clear();
    guard.engine.clear();
    guard.backend.clear();
    guard.model_key.clear();
    json!({ "status": "idle" })
}

/// Hugging Face search, mapped to the shape the model explorer expects.
pub async fn catalog(query: &str, limit: usize) -> Result<Vec<Value>, String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if !query.trim().is_empty() {
            pairs.append_pair("search", query.trim());
        }
        pairs
            .append_pair("filter", "gguf")
            .append_pair("sort", "downloads")
            .append_pair("direction", "-1")
            .append_pair("limit", &limit.clamp(1, 100).to_string());
    }
    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Hugging Face HTTP {}", response.status()));
    }
    let items: Vec<Value> = response.json().await.map_err(|error| error.to_string())?;
    Ok(items
        .into_iter()
        .map(|item| {
            let id = item.get("id").and_then(|value| value.as_str()).unwrap_or("");
            json!({
                "id": id,
                "author": id.split('/').next().unwrap_or(""),
                "name": id.split('/').nth(1).unwrap_or(id),
                "library": item.get("library_name").and_then(|value| value.as_str()).unwrap_or("gguf"),
                "downloads": item.get("downloads").and_then(|value| value.as_u64()).unwrap_or(0),
                "likes": item.get("likes").and_then(|value| value.as_u64()).unwrap_or(0),
                "gated": item.get("gated").and_then(|value| value.as_bool()).unwrap_or(false),
                "updatedAt": item.get("lastModified"),
                "description": "",
                "task": item.get("pipeline_tag").and_then(|value| value.as_str()).unwrap_or(""),
                "tags": item.get("tags").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect())
}

pub async fn details(repo: &str) -> Result<Value, String> {
    if repo.trim().is_empty() {
        return Err("모델 id 가 필요합니다.".into());
    }
    let url = format!("https://huggingface.co/api/models/{repo}?blobs=true");
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Hugging Face HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    let mut options = Vec::new();
    if let Some(siblings) = value.get("siblings").and_then(|value| value.as_array()) {
        for sibling in siblings {
            let Some(file) = sibling.get("rfilename").and_then(|value| value.as_str()) else {
                continue;
            };
            if !file.to_ascii_lowercase().ends_with(".gguf") {
                continue;
            }
            let bytes = sibling
                .get("size")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let quantization = file.rsplit('.').nth(1).unwrap_or("").to_uppercase();
            options.push(json!({
                "id": file,
                "format": "gguf",
                "label": file.rsplit('/').next().unwrap_or(file),
                "quantization": quantization,
                "bytes": bytes,
                "sizeLabel": format_bytes(bytes),
                "files": [file],
                "shards": 1,
            }));
        }
    }
    options.sort_by(|a, b| {
        a.get("bytes")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .cmp(&b.get("bytes").and_then(|value| value.as_u64()).unwrap_or(0))
    });
    let author = repo.split('/').next().unwrap_or("");
    Ok(json!({
        "id": repo,
        "author": author,
        "name": repo.split('/').nth(1).unwrap_or(repo),
        "library": value.get("library_name").and_then(|v| v.as_str()).unwrap_or("gguf"),
        "downloads": value.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        "likes": value.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
        "gated": value.get("gated").and_then(|v| v.as_bool()).unwrap_or(false),
        "updatedAt": value.get("lastModified"),
        "description": "",
        "task": value.get("pipeline_tag").and_then(|v| v.as_str()).unwrap_or(""),
        "tags": value.get("tags").cloned().unwrap_or_else(|| json!([])),
        "revision": value.get("sha").and_then(|v| v.as_str()).unwrap_or("main"),
        "license": "",
        "architecture": "",
        "options": options,
        "readme": "",
    }))
}

pub fn jobs(state: &AppState) -> Vec<Value> {
    state.store.all(MODEL_KIND)
}

/// Streams a GGUF file from Hugging Face and records progress as a job.
pub fn start_download(state: AppState, repo: String, option: String) -> Result<Value, String> {
    if repo.trim().is_empty() || option.trim().is_empty() {
        return Err("repo 와 optionId 가 필요합니다.".into());
    }
    let file = option.rsplit('/').next().unwrap_or(&option).to_string();
    let job = state.store.put(
        MODEL_KIND,
        &json!({
            "repo": repo,
            "optionId": option,
            "status": "downloading",
            "downloaded": 0,
            "total": 0,
            "current": file,
            "error": "",
            "startedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    let Some(job) = job else {
        return Err("다운로드 작업을 만들 수 없습니다.".into());
    };
    let job_id = job
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    tokio::spawn(async move {
        download_worker(state, job_id, repo, option).await;
    });
    Ok(job)
}

async fn download_worker(state: AppState, job_id: String, repo: String, option: String) {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let dir = models_dir(&state);
    let _ = std::fs::create_dir_all(&dir);
    let file = option.rsplit('/').next().unwrap_or(&option).to_string();
    let target = dir.join(&file);
    let partial = dir.join(format!("{file}.part"));
    let url = format!("https://huggingface.co/{repo}/resolve/main/{option}?download=true");

    let patch = |state: &AppState, patch: Value| {
        if let Some(mut job) = state.store.get(MODEL_KIND, &job_id) {
            if let (Value::Object(target), Value::Object(source)) = (&mut job, patch) {
                for (key, value) in source {
                    target.insert(key, value);
                }
            }
            if let Some(saved) = state.store.put(MODEL_KIND, &job) {
                state.bus.record("model_job", &saved);
            }
        }
    };

    let response = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(3600))
        .send()
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            patch(
                &state,
                json!({ "status": "failed", "error": format!("다운로드 HTTP {}", response.status()) }),
            );
            return;
        }
        Err(error) => {
            patch(
                &state,
                json!({ "status": "failed", "error": error.to_string() }),
            );
            return;
        }
    };
    let total = response.content_length().unwrap_or(0);
    patch(&state, json!({ "total": total }));

    let mut file_handle = match tokio::fs::File::create(&partial).await {
        Ok(file) => file,
        Err(error) => {
            patch(
                &state,
                json!({ "status": "failed", "error": error.to_string() }),
            );
            return;
        }
    };
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let cancelled = state
            .store
            .get(MODEL_KIND, &job_id)
            .and_then(|job| job.get("status").cloned())
            .and_then(|value| value.as_str().map(str::to_string))
            .map(|status| status == "cancelled")
            .unwrap_or(false);
        if cancelled {
            let _ = tokio::fs::remove_file(&partial).await;
            return;
        }
        let Ok(chunk) = chunk else {
            patch(
                &state,
                json!({ "status": "failed", "error": "스트림 오류" }),
            );
            let _ = tokio::fs::remove_file(&partial).await;
            return;
        };
        if let Err(error) = file_handle.write_all(&chunk).await {
            patch(
                &state,
                json!({ "status": "failed", "error": error.to_string() }),
            );
            return;
        }
        downloaded += chunk.len() as u64;
        if downloaded % (2 * 1024 * 1024) < chunk.len() as u64 {
            patch(&state, json!({ "downloaded": downloaded }));
        }
    }
    let _ = file_handle.flush().await;
    if let Err(error) = tokio::fs::rename(&partial, &target).await {
        patch(
            &state,
            json!({ "status": "failed", "error": error.to_string() }),
        );
        return;
    }
    patch(
        &state,
        json!({
            "status": "completed",
            "downloaded": downloaded,
            "total": if total == 0 { downloaded } else { total },
            "finishedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state.store.audit(
        "model.downloaded",
        json!({ "repo": repo, "file": file, "bytes": downloaded }),
    );
}

pub fn cancel_job(state: &AppState, id: &str) -> bool {
    let Some(mut job) = state.store.get(MODEL_KIND, id) else {
        return false;
    };
    job["status"] = json!("cancelled");
    job["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    state.store.put(MODEL_KIND, &job).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-models-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn lists_only_gguf_files() {
        let (state, dir) = temp_state();
        let models = models_dir(&state);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("tiny.gguf"), b"x").unwrap();
        std::fs::write(models.join("notes.txt"), b"x").unwrap();
        let items = installed(&state)
            .into_iter()
            .filter(|item| item["source"] != "lmstudio")
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "tiny.gguf");
        assert_eq!(items[0]["format"], "gguf");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_a_local_gguf() {
        let (state, dir) = temp_state();
        let source = dir.join("source.gguf");
        std::fs::write(&source, b"model").unwrap();
        let item = import_local(&state, &source.to_string_lossy()).expect("import");
        assert_eq!(item["id"], "source.gguf");
        assert!(models_dir(&state).join("source.gguf").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_non_gguf_import() {
        let (state, dir) = temp_state();
        let source = dir.join("model.bin");
        std::fs::write(&source, b"x").unwrap();
        assert!(import_local(&state, &source.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
