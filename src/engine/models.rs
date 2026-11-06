use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::process::{Child, Command};

const RUNTIME_PORT: u16 = 4319;
const PROVIDER_ID: &str = "dominion-native";
const MODEL_KIND: &str = "model_job";
const EXTERNAL_KIND: &str = "external_model";
pub static RUNTIME_CHANGE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static DOWNLOAD_COMMIT: Mutex<()> = Mutex::new(());

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

fn safe_repo(repo: &str) -> Result<String, String> {
    let parts = repo.trim().split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.starts_with('.')
                || part.len() > 128
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        })
    {
        return Err("Hugging Face 저장소는 owner/name 형식이어야 합니다.".into());
    }
    Ok(parts.join("/"))
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn mlx_models_root() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(PathBuf::from(home).join(".lmstudio/models"))
}

fn target_root(state: &AppState, format: &str, repo: &str) -> PathBuf {
    if format == "mlx" {
        mlx_models_root()
            .unwrap_or_else(|| models_dir(state).join("mlx"))
            .join(repo)
    } else {
        models_dir(state).join(repo.replace('/', "--"))
    }
}

fn find_lms_key(repo: &str) -> Option<String> {
    let lms = lms_bin()?;
    let output = std::process::Command::new(lms)
        .args(["ls", "--json"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let items: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
    items.into_iter().find_map(|item| {
        let path = item["path"].as_str().unwrap_or("");
        let key = item["modelKey"].as_str().unwrap_or("");
        (path == repo && !key.is_empty()).then(|| key.to_string())
    })
}

fn mlx_download_files(siblings: &[Value]) -> Option<Value> {
    let mut files = Vec::new();
    let mut has_config = false;
    let mut shards = 0usize;
    let mut bytes = 0u64;
    for sibling in siblings {
        let Some(file) = sibling.get("rfilename").and_then(Value::as_str) else {
            continue;
        };
        let lower = file.to_ascii_lowercase();
        if lower.starts_with("original/") || lower.starts_with("onnx/") {
            continue;
        }
        let include = lower == "config.json"
            || lower.ends_with(".safetensors")
            || lower.ends_with(".safetensors.index.json")
            || matches!(
                lower.rsplit('/').next().unwrap_or(""),
                "tokenizer.json"
                    | "tokenizer_config.json"
                    | "special_tokens_map.json"
                    | "vocab.json"
                    | "merges.txt"
                    | "added_tokens.json"
                    | "generation_config.json"
                    | "preprocessor_config.json"
                    | "chat_template.jinja"
            );
        if !include || !safe_relative(file) {
            continue;
        }
        if lower == "config.json" {
            has_config = true;
        }
        if lower.ends_with(".safetensors") {
            shards += 1;
        }
        let size = sibling.get("size").and_then(Value::as_u64).unwrap_or(0);
        bytes += size;
        files.push(json!({"path": file, "bytes": size}));
    }
    if !has_config || shards == 0 {
        return None;
    }
    let paths = files
        .iter()
        .map(|file| file["path"].clone())
        .collect::<Vec<_>>();
    Some(json!({
        "id": "mlx",
        "format": "mlx",
        "label": format!("MLX · {shards} shard{}", if shards == 1 { "" } else { "s" }),
        "quantization": "",
        "bytes": bytes,
        "sizeLabel": format_bytes(bytes),
        "files": paths,
        "fileSizes": files,
        "shards": shards,
    }))
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
    let lms = lmstudio_models();
    let indexed = lms
        .iter()
        .filter_map(|model| model["repo"].as_str().map(str::to_string))
        .collect::<std::collections::HashSet<_>>();
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
        .chain(state.store.all(EXTERNAL_KIND).into_iter().filter_map(|record| {
            let path = Path::new(record["path"].as_str()?);
            external_model(record["id"].as_str()?, path)
        }))
        .chain(lms)
        .chain(local_mlx_models().into_iter().filter(|model| {
            model["repo"]
                .as_str()
                .is_some_and(|repo| !indexed.contains(repo))
        }))
        .collect()
}

/// MLX repositories downloaded directly from Hugging Face. LM Studio indexes
/// them from its own model directory; until it does, they are still visible in
/// the library so the download is not silently lost.
fn local_mlx_models() -> Vec<Value> {
    let Some(root) = mlx_models_root() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    walk_dirs(&root, 0, &mut found);
    found
        .into_iter()
        .filter_map(|dir| {
            let config = dir.join("config.json");
            if !config.is_file() {
                return None;
            }
            let mut bytes = 0u64;
            let mut shards = 0usize;
            for entry in std::fs::read_dir(&dir).ok()?.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.ends_with(".safetensors") {
                    shards += 1;
                    bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                }
            }
            if shards == 0 {
                return None;
            }
            let repo = dir
                .strip_prefix(&root)
                .ok()?
                .to_string_lossy()
                .to_string();
            Some(json!({
                "id": format!("mlx:{repo}"),
                "repo": repo,
                "optionId": "mlx",
                "source": "mlx-download",
                "format": "mlx",
                "label": repo.rsplit('/').next().unwrap_or(&repo),
                "bytes": bytes,
                "sizeLabel": format_bytes(bytes),
                "engine": "MLX (LM Studio 인식 대기)",
                "installedAt": Value::Null,
            }))
        })
        .collect()
}

fn walk_dirs(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.push(path.clone());
            walk_dirs(&path, depth + 1, found);
        }
    }
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
                "repo": item["path"].as_str().unwrap_or_else(|| item["publisher"].as_str().unwrap_or(&key)),
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

fn external_model(id: &str, path: &Path) -> Option<Value> {
    let label = path.file_name()?.to_str()?;
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || !label.to_ascii_lowercase().ends_with(".gguf") {
        return None;
    }
    Some(json!({
        "id": format!("external:{id}"), "repo": label, "label": label,
        "source": "external", "format": "gguf", "bytes": metadata.len(),
        "externalPath": path, "installedAt": null
    }))
}

pub fn register_local(state: &AppState, file_path: &str) -> Result<Value, String> {
    let path = Path::new(file_path.trim()).canonicalize().map_err(|_| "파일을 찾을 수 없습니다.")?;
    let previous = state.store.all(EXTERNAL_KIND).into_iter()
        .find(|record| record["path"].as_str() == path.to_str());
    let id = previous.as_ref().and_then(|record| record["id"].as_str()).map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model = external_model(&id, &path).ok_or("읽을 수 있는 GGUF 파일을 선택하세요.")?;
    state.store.put(EXTERNAL_KIND, &json!({"id":id,"path":path}))
        .ok_or("모델 경로를 저장할 수 없습니다.")?;
    state.bus.record(EXTERNAL_KIND, &json!({"id":id}));
    Ok(model)
}

fn managed_model_path(state: &AppState, id: &str) -> Option<PathBuf> {
    let root = models_dir(state).canonicalize().ok()?;
    let relative = Path::new(id);
    if relative.components().any(|part| !matches!(part, std::path::Component::Normal(_))) {
        return None;
    }
    let path = root.join(relative);
    let metadata = path.symlink_metadata().ok()?;
    if !metadata.is_file() || !path.canonicalize().ok()?.starts_with(&root)
        || !path.extension()?.to_str()?.eq_ignore_ascii_case("gguf") {
        return None;
    }
    Some(path)
}

pub fn remove_installed(state: &AppState, id: &str) -> Result<(), String> {
    if runtime().lock().unwrap().id == id {
        return Err("실행 중인 모델은 먼저 중지하세요.".into());
    }
    if let Some(record) = id.strip_prefix("external:") {
        if !state.store.remove(EXTERNAL_KIND, record) {
            return Err("등록된 모델을 찾을 수 없습니다.".into());
        }
    } else if let Some(repo) = id.strip_prefix("mlx:") {
        let root = mlx_models_root().ok_or("LM Studio 모델 경로를 찾을 수 없습니다.")?;
        let path = root.join(repo);
        if !path.starts_with(&root) || !path.is_dir() {
            return Err("다운로드한 MLX 모델을 찾을 수 없습니다.".into());
        }
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    } else {
        let path = managed_model_path(state, id).ok_or("DOMINION이 관리하는 모델 파일을 찾을 수 없습니다.")?;
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    state.bus.record(EXTERNAL_KIND, &json!({"id":id}));
    Ok(())
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

pub fn runtime_snapshot(state: &AppState) -> Value {
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
        "model": state.store.get("provider", PROVIDER_ID).and_then(|provider| provider["model"].as_str().map(str::to_owned)),
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
    if let Some(repo) = id.strip_prefix("mlx:") {
        let key = find_lms_key(repo).ok_or(
            "LM Studio가 아직 이 MLX 모델을 인식하지 못했습니다. 모델 관리에서 새로고침한 뒤 다시 시도하세요.",
        )?;
        return load_lmstudio(state, id, &key, context).await;
    }
    let resolved = if let Some(record) = id.strip_prefix("external:") {
        state.store.get(EXTERNAL_KIND, record)
            .and_then(|value| value["path"].as_str().map(PathBuf::from))
            .filter(|path| external_model(record, path).is_some())
    } else {
        managed_model_path(state, id)
    }.ok_or_else(|| format!("설치된 모델을 찾을 수 없습니다: {id}"))?;
    let llama_server = llama_server_bin()
        .ok_or("llama-server를 찾을 수 없습니다. llama.cpp를 설치하거나 PATH를 확인하세요.")?;

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
            .get(format!("http://127.0.0.1:{RUNTIME_PORT}/v1/models"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if response.status().is_success()
                && response.json::<Value>().await.is_ok_and(|data| {
                    data["data"].as_array().is_some_and(|models| models.iter().any(|model| model["id"] == id))
                })
            {
                let saved = state.store.put(
                    "provider",
                    &json!({
                        "id": PROVIDER_ID,
                        "name": "DOMINION Native",
                        "baseUrl": format!("http://127.0.0.1:{RUNTIME_PORT}/v1"),
                        "kind": "openai",
                        "local": true,
                        "enabled": true,
                        "model": id,
                        "contextTokens": context.clamp(512, 32768),
                        "description": "llama.cpp runtime managed by the Rust engine",
                    }),
                );
                let saved = saved.ok_or("모델 제공자를 저장할 수 없습니다.")?;
                runtime().lock().unwrap().status = "ready".into();
                state.bus.record("provider", &saved);
                return Ok(runtime_snapshot(state));
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
        guard.model_key.clear();
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

    let running_models = Command::new(&lms)
        .args(["ps", "--json"]).output().await
        .ok().filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Vec<Value>>(&output.stdout).ok())
        .unwrap_or_default();
    let served = match running_models.iter().find_map(|item| {
        (item["modelKey"].as_str() == Some(key))
            .then(|| item["identifier"].as_str())
            .flatten()
    }) {
        Some(identifier) => identifier.to_owned(),
        None => {
            let identifier = format!("dominion-{}", uuid::Uuid::new_v4());
            runtime().lock().unwrap().model_key = identifier.clone();
            let output = Command::new(&lms)
                .args(["load", key, "--identifier", &identifier, "--gpu", "max",
                       "--context-length", &context.clamp(512, 32768).to_string(), "--yes"])
                .output().await
                .map_err(|error| format!("LM Studio 모델 로드 실패: {error}"))?;
            if !output.status.success() {
                return Err(format!("LM Studio 모델 로드 실패: {}", String::from_utf8_lossy(&output.stderr).trim()));
            }
            identifier
        }
    };

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
                if !value["data"].as_array().is_some_and(|items| {
                    items.iter().any(|item| item["id"].as_str() == Some(served.as_str()))
                }) {
                    continue;
                }
                let saved = state.store.put(
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
                let saved = saved.ok_or("모델 제공자를 저장할 수 없습니다.")?;
                runtime().lock().unwrap().status = "ready".into();
                state.bus.record("provider", &saved);
                return Ok(runtime_snapshot(state));
            }
        }
    }
    Err("요청한 LM Studio 모델이 API 목록에서 확인되지 않았습니다.".into())
}

fn push_log(line: String) {
    let mut guard = runtime().lock().unwrap();
    guard.logs.push(line);
    if guard.logs.len() > 200 {
        let excess = guard.logs.len() - 200;
        guard.logs.drain(0..excess);
    }
}

pub fn mark_load_failure(state: &AppState, message: &str) {
    if runtime().lock().unwrap().status == "loading" {
        unload(state);
        let mut guard = runtime().lock().unwrap();
        guard.status = "error".into();
        guard.error = message.into();
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
pub async fn catalog(query: &str, limit: usize, format: &str) -> Result<Vec<Value>, String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if !query.trim().is_empty() {
            pairs.append_pair("search", query.trim());
        }
        pairs
            .append_pair("filter", if format == "mlx" { "mlx" } else { "gguf" })
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
    let siblings = value
        .get("siblings")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for sibling in &siblings {
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
    options.sort_by(|a, b| {
        a.get("bytes")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            .cmp(&b.get("bytes").and_then(|value| value.as_u64()).unwrap_or(0))
    });
    if let Some(mlx) = mlx_download_files(&siblings) {
        options.push(mlx);
    }
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
    start_download_with(state, repo, option, "gguf".into(), Vec::new())
}

/// Starts a GGUF or multi-file MLX download. The job stores a repo-scoped
/// target directory and a per-file list so resume never has to guess which
/// bytes are already on disk.
pub fn start_download_with(
    state: AppState,
    repo: String,
    option: String,
    format: String,
    requested_files: Vec<String>,
) -> Result<Value, String> {
    let repo = safe_repo(&repo)?;
    let format = if format == "mlx" { "mlx" } else { "gguf" };
    let option = option.trim().to_string();
    if option.is_empty() {
        return Err("다운로드할 배포 형식을 선택하세요.".into());
    }
    for existing in state.store.all(MODEL_KIND) {
        if existing["repo"].as_str() == Some(repo.as_str())
            && existing["optionId"].as_str() == Some(option.as_str())
            && matches!(existing["status"].as_str(), Some("downloading" | "paused"))
        {
            return Ok(existing);
        }
    }
    let files = if format == "mlx" {
        let files = requested_files
            .into_iter()
            .filter(|path| safe_relative(path))
            .collect::<Vec<_>>();
        if files.is_empty()
            || !files.iter().any(|path| path == "config.json")
            || !files.iter().any(|path| path.ends_with(".safetensors"))
        {
            return Err("MLX 다운로드에는 config.json과 safetensors 파일이 필요합니다.".into());
        }
        files
    } else {
        let file = option.trim_start_matches('/').to_string();
        if !file.to_ascii_lowercase().ends_with(".gguf") || !safe_relative(&file) {
            return Err("다운로드할 GGUF 파일을 확인하세요.".into());
        }
        vec![file]
    };
    let dir = target_root(&state, format, &repo);
    let mut total = 0u64;
    let mut entries = Vec::new();
    for path in &files {
        let target = dir.join(path);
        let partial = PathBuf::from(format!("{}.part", target.display()));
        let downloaded = if target.is_file() {
            std::fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&partial).map(|meta| meta.len()).unwrap_or(0)
        };
        total += downloaded;
        entries.push(json!({"path": path, "downloaded": downloaded, "total": 0}));
    }
    let job = state.store.put(
        MODEL_KIND,
        &json!({
            "repo": repo,
            "optionId": option,
            "format": format,
            "status": "downloading",
            "downloaded": total,
            "total": 0,
            "current": files.first().cloned().unwrap_or_default(),
            "files": entries,
            "targetDir": dir.to_string_lossy(),
            "error": "",
            "startedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    let Some(job) = job else {
        return Err("다운로드 작업을 만들 수 없습니다.".into());
    };
    let job_id = job["id"].as_str().unwrap_or("").to_string();
    tokio::spawn(async move {
        download_worker(state, job_id).await;
    });
    Ok(job)
}

fn job_status(state: &AppState, id: &str) -> String {
    state
        .store
        .get(MODEL_KIND, id)
        .and_then(|job| job["status"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn patch_download(state: &AppState, id: &str, patch: Value) {
    if let Some(saved) = state.store.patch_if(
        MODEL_KIND,
        id,
        "status",
        &json!("downloading"),
        &patch,
    ) {
        state.bus.record("model_job", &saved);
    }
}

fn downloaded_total(files: &[Value]) -> u64 {
    files
        .iter()
        .map(|file| file["downloaded"].as_u64().unwrap_or(0))
        .sum()
}

fn declared_total(files: &[Value]) -> u64 {
    files
        .iter()
        .map(|file| {
            file["total"]
                .as_u64()
                .filter(|value| *value > 0)
                .unwrap_or_else(|| file["downloaded"].as_u64().unwrap_or(0))
        })
        .sum()
}

async fn download_worker(state: AppState, job_id: String) {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let Some(job) = state.store.get(MODEL_KIND, &job_id) else {
        return;
    };
    let repo = job["repo"].as_str().unwrap_or("").to_string();
    let format = job["format"].as_str().unwrap_or("gguf").to_string();
    let dir = PathBuf::from(job["targetDir"].as_str().unwrap_or(""));
    let mut files = job["files"].as_array().cloned().unwrap_or_default();
    if dir.as_os_str().is_empty() || files.is_empty() {
        patch_download(
            &state,
            &job_id,
            json!({"status": "failed", "error": "다운로드 작업 정보가 손상되었습니다."}),
        );
        return;
    }
    let client = reqwest::Client::new();
    for index in 0..files.len() {
        if job_status(&state, &job_id) != "downloading" {
            return;
        }
        let Some(path) = files[index]["path"].as_str().map(str::to_string) else {
            continue;
        };
        if !safe_relative(&path) {
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": "안전하지 않은 모델 파일 경로입니다."}),
            );
            return;
        }
        let target = dir.join(&path);
        if let Some(parent) = target.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if target.is_file() {
            let size = std::fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0);
            files[index]["downloaded"] = json!(size);
            patch_download(
                &state,
                &job_id,
                json!({"files": files, "downloaded": downloaded_total(&files), "total": declared_total(&files), "current": path}),
            );
            continue;
        }
        let partial = PathBuf::from(format!("{}.part", target.display()));
        let existing = std::fs::metadata(&partial).map(|meta| meta.len()).unwrap_or(0);
        let url = format!("https://huggingface.co/{repo}/resolve/main/{path}?download=true");
        let mut request = client.get(&url).timeout(Duration::from_secs(3600));
        if existing > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                patch_download(
                    &state,
                    &job_id,
                    json!({"status": "failed", "error": error.to_string()}),
                );
                return;
            }
        };
        if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && existing > 0 {
            let _ = tokio::fs::rename(&partial, &target).await;
            files[index]["downloaded"] = json!(existing);
            continue;
        }
        if !response.status().is_success() {
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": format!("다운로드 HTTP {}", response.status())}),
            );
            return;
        }
        let append = existing > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let mut output = match tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&partial)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                patch_download(
                    &state,
                    &job_id,
                    json!({"status": "failed", "error": error.to_string()}),
                );
                return;
            }
        };
        let declared = response.content_length().unwrap_or(0).saturating_add(if append { existing } else { 0 });
        files[index]["total"] = json!(declared);
        let mut downloaded = if append { existing } else { 0 };
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if job_status(&state, &job_id) != "downloading" {
                let _ = output.flush().await;
                return;
            }
            let Ok(chunk) = chunk else {
                patch_download(
                    &state,
                    &job_id,
                    json!({"status": "failed", "error": "다운로드 스트림 오류"}),
                );
                return;
            };
            if let Err(error) = output.write_all(&chunk).await {
                patch_download(
                    &state,
                    &job_id,
                    json!({"status": "failed", "error": error.to_string()}),
                );
                return;
            }
            downloaded += chunk.len() as u64;
            if downloaded % (2 * 1024 * 1024) < chunk.len() as u64 {
                files[index]["downloaded"] = json!(downloaded);
                let total = downloaded_total(&files);
                patch_download(
                    &state,
                    &job_id,
                    json!({"files": files, "downloaded": total, "total": declared_total(&files), "current": path}),
                );
            }
        }
        if let Err(error) = output.flush().await {
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": error.to_string()}),
            );
            return;
        }
        drop(output);
        if job_status(&state, &job_id) != "downloading" {
            return;
        }
        let _commit = DOWNLOAD_COMMIT.lock().unwrap();
        if target.exists() {
            let _ = std::fs::remove_file(&target);
        }
        if let Err(error) = std::fs::rename(&partial, &target) {
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": error.to_string()}),
            );
            return;
        }
        files[index]["downloaded"] = json!(downloaded);
    }
    let installed_id = if format == "mlx" {
        find_lms_key(&repo)
            .map(|key| format!("lms:{key}"))
            .unwrap_or_else(|| format!("mlx:{repo}"))
    } else {
        files
            .first()
            .and_then(|file| file["path"].as_str())
            .map(|path| format!("{}/{}", repo.replace('/', "--"), path))
            .unwrap_or_else(|| repo.replace('/', "--"))
    };
    let done = downloaded_total(&files);
    let declared_total = files
        .iter()
        .map(|file| {
            file["total"]
                .as_u64()
                .filter(|value| *value > 0)
                .unwrap_or_else(|| file["downloaded"].as_u64().unwrap_or(0))
        })
        .sum::<u64>()
        .max(done);
    patch_download(
        &state,
        &job_id,
        json!({
            "status": "completed",
            "installedId": installed_id,
            "downloaded": done,
            "total": declared_total,
            "finishedAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    state.store.audit(
        "model.downloaded",
        json!({"repo": repo, "format": format, "files": files.len(), "bytes": done}),
    );
}

pub fn pause_job(state: &AppState, id: &str) -> bool {
    let Some(saved) = state.store.patch_if(
        MODEL_KIND,
        id,
        "status",
        &json!("downloading"),
        &json!({"status": "paused", "pausedAt": chrono::Utc::now().to_rfc3339()}),
    ) else {
        return false;
    };
    state.bus.record("model_job", &saved);
    true
}

pub fn resume_job(state: &AppState, id: &str) -> Option<Value> {
    let job = state.store.get(MODEL_KIND, id)?;
    let status = job["status"].as_str().unwrap_or("");
    if !matches!(status, "paused" | "failed" | "cancelled") {
        return None;
    }
    let saved = state.store.patch_if(
        MODEL_KIND,
        id,
        "status",
        &json!(status),
        &json!({
            "status": "downloading",
            "error": "",
            "resumedAt": chrono::Utc::now().to_rfc3339()
        }),
    )?;
    state.bus.record("model_job", &saved);
    let worker_state = state.clone();
    let worker_id = id.to_string();
    tokio::spawn(async move {
        download_worker(worker_state, worker_id).await;
    });
    Some(saved)
}

pub fn cancel_job(state: &AppState, id: &str) -> bool {
    for status in ["downloading", "paused"] {
        if let Some(saved) = state.store.patch_if(
            MODEL_KIND,
            id,
            "status",
            &json!(status),
            &json!({
                "status": "cancelled",
                "finishedAt": chrono::Utc::now().to_rfc3339()
            }),
        ) {
            state.bus.record("model_job", &saved);
            return true;
        }
    }
    false
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
            .filter(|item| !matches!(item["source"].as_str(), Some("lmstudio" | "mlx-download")))
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

    #[test]
    fn rejects_repo_traversal_and_scopes_download_targets() {
        assert!(safe_repo("../etc").is_err());
        assert!(safe_repo("owner/../model").is_err());
        assert!(safe_repo("owner/model").is_ok());
        let (state, dir) = temp_state();
        assert_eq!(
            target_root(&state, "gguf", "owner/model"),
            models_dir(&state).join("owner--model")
        );
        assert_eq!(
            target_root(&state, "mlx", "owner/model"),
            mlx_models_root().unwrap().join("owner/model")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mlx_option_requires_config_and_keeps_safetensors() {
        let siblings = vec![
            json!({"rfilename":"config.json","size":100}),
            json!({"rfilename":"model.safetensors","size":1000}),
            json!({"rfilename":"tokenizer.json","size":50}),
            json!({"rfilename":"README.md","size":10}),
            json!({"rfilename":"original/model.safetensors","size":1000}),
        ];
        let option = mlx_download_files(&siblings).expect("mlx option");
        assert_eq!(option["format"], "mlx");
        let files = option["files"].as_array().unwrap();
        assert!(files.iter().any(|file| file == "config.json"));
        assert!(files.iter().any(|file| file == "model.safetensors"));
        assert!(files.iter().any(|file| file == "tokenizer.json"));
        assert!(!files.iter().any(|file| file == "README.md"));
        assert!(!files.iter().any(|file| file == "original/model.safetensors"));
        assert_eq!(option["shards"], 1);
    }
}
