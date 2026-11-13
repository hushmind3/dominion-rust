use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::process::{Child, Command};

const RUNTIME_PORT: u16 = 4319;
const PROVIDER_ID: &str = "dominion-native";
const MODEL_KIND: &str = "model_job";
const EXTERNAL_KIND: &str = "external_model";
pub static RUNTIME_CHANGE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static DOWNLOAD_COMMIT: Mutex<()> = Mutex::new(());
// Registration and resume must claim file destinations as one operation.
static DOWNLOAD_REGISTRATION: Mutex<()> = Mutex::new(());
static ACTIVE_DOWNLOADS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn active_downloads() -> &'static Mutex<std::collections::HashSet<String>> {
    ACTIVE_DOWNLOADS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = active_downloads().lock() {
            active.remove(&self.0);
        }
    }
}

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

fn mlx_models_root(state: &AppState) -> PathBuf {
    models_dir(state).join("mlx")
}

fn legacy_mlx_models_root() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = PathBuf::from(home).join(".lmstudio/models");
    path.is_dir().then_some(path)
}

/// New downloads stay in DOMINION's data directory. Models downloaded by
/// older builds from the LM Studio models folder remain readable without
/// duplicating multi-gigabyte files.
fn mlx_search_roots(state: &AppState) -> Vec<PathBuf> {
    let mut roots = vec![mlx_models_root(state)];
    if let Some(legacy) = legacy_mlx_models_root() {
        if !roots.contains(&legacy) {
            roots.push(legacy);
        }
    }
    roots
}

fn target_root(state: &AppState, format: &str, repo: &str) -> PathBuf {
    if format == "mlx" {
        mlx_models_root(state).join(repo)
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
        .chain(local_mlx_models(state).into_iter().filter(|model| {
            model["repo"]
                .as_str()
                .is_some_and(|repo| !indexed.contains(repo))
        }))
        .collect()
}

/// MLX repositories downloaded directly from Hugging Face. LM Studio indexes
/// them from its own model directory; until it does, they are still visible in
/// the library so the download is not silently lost.
fn local_mlx_models(state: &AppState) -> Vec<Value> {
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in mlx_search_roots(state) {
        let mut found = Vec::new();
        walk_dirs(&root, 0, &mut found);
        for model in found.into_iter().filter_map(|dir| mlx_model_from_dir(&dir, &root)) {
            if let Some(repo) = model["repo"].as_str() {
                if seen.insert(repo.to_string()) {
                    models.push(model);
                }
            }
        }
    }
    models
}

fn mlx_model_from_dir(dir: &Path, root: &Path) -> Option<Value> {
            let config = dir.join("config.json");
            if !config.is_file() || !mlx_dir_complete(&dir) {
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
                .strip_prefix(root)
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
                "engine": "MLX (DOMINION runtime)",
                "installedAt": Value::Null,
            }))
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

fn llama_server_bin_for(state: &AppState) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DOMINION_LLAMA_SERVER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let managed = state
        .config
        .data_dir
        .join("bin")
        .join(if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        });
    if managed.is_file() {
        return Some(managed);
    }
    llama_server_bin()
}

/// Automatic installation is limited to platforms where the official
/// llama.cpp release assets are known to run. Homebrew is preferred on macOS.
fn llama_installable(state: &AppState) -> bool {
    llama_server_bin_for(state).is_none()
        && matches!(std::env::consts::OS, "macos" | "linux")
        && matches!(std::env::consts::ARCH, "aarch64" | "x86_64")
}

fn mlx_runtime_dir(state: &AppState) -> PathBuf {
    state.config.data_dir.join("mlx-runtime")
}

fn mlx_runtime_python(state: &AppState) -> Option<PathBuf> {
    let dir = mlx_runtime_dir(state);
    let python = dir.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" });
    (python.is_file() && dir.join(".ready").is_file()).then_some(python)
}

fn mlx_installable(state: &AppState) -> bool {
    mlx_runtime_python(state).is_none()
        && cfg!(target_os = "macos")
        && cfg!(target_arch = "aarch64")
}

/// Resolve a downloaded MLX repository below the managed models root.
fn mlx_dir_complete(dir: &Path) -> bool {
    if !dir.join("config.json").is_file() {
        return false;
    }
    let mut has_weights = false;
    let mut has_tokenizer = dir.join("tokenizer.json").is_file()
        || dir.join("tokenizer_config.json").is_file()
        || dir.join("tokenizer.model").is_file();
    for entry in std::fs::read_dir(dir).ok().into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".safetensors") {
            has_weights = true;
        }
        if name == "tokenizer.json" || name == "tokenizer_config.json" || name == "tokenizer.model" {
            has_tokenizer = true;
        }
    }
    has_weights && has_tokenizer
}

fn mlx_model_dir(state: &AppState, repo: &str) -> Option<PathBuf> {
    mlx_search_roots(state)
        .iter()
        .find_map(|root| managed_model_dir(root, repo))
        .filter(|dir| mlx_dir_complete(dir))
}

fn mlx_runtime_snapshot(state: &AppState) -> Value {
    let standalone = mlx_runtime_python(state);
    json!({
        "available": standalone.is_some() || lms_bin().is_some(),
        "backend": if standalone.is_some() { "mlx-lm" } else if lms_bin().is_some() { "LM Studio MLX" } else { "" },
        "standalone": standalone.is_some(),
        "installable": mlx_installable(state),
        "path": standalone.map(|path| path.to_string_lossy().to_string()),
        "installMethod": "python-venv",
    })
}

async fn install_mlx_runtime(state: &AppState) -> Result<Value, String> {
    let dir = mlx_runtime_dir(state);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(dir.join(".ready"));
    let python = std::env::var("DOMINION_PYTHON").unwrap_or_else(|_| "python3".into());
    set_install(json!({
        "status": "checking", "engine": "mlx", "backend": "mlx-lm",
        "tag": "mlx-lm", "downloaded": 0, "total": 0,
        "file": "python -m venv mlx-runtime", "error": "",
    }));
    let venv = tokio::time::timeout(
        Duration::from_secs(180),
        Command::new(&python).args(["-m", "venv"]).arg(&dir).output(),
    )
    .await
    .map_err(|_| "Python 가상환경 생성 시간 초과".to_string())?
    .map_err(|error| format!("Python 실행 실패: {error}"))?;
    if !venv.status.success() {
        return Err(format!(
            "Python 가상환경 생성 실패: {}",
            String::from_utf8_lossy(&venv.stderr).trim()
        ));
    }
    let pip = dir.join(if cfg!(windows) { "Scripts/pip.exe" } else { "bin/pip" });
    set_install(json!({
        "status": "downloading", "engine": "mlx", "backend": "mlx-lm",
        "tag": "mlx-lm", "downloaded": 0, "total": 0,
        "file": "pip install mlx-lm", "error": "",
    }));
    let installed = tokio::time::timeout(
        Duration::from_secs(900),
        Command::new(&pip)
            .args(["install", "--upgrade", "pip"])
            .output(),
    )
    .await
    .map_err(|_| "pip 업그레이드 시간 초과".to_string())?
    .map_err(|error| format!("pip 실행 실패: {error}"))?;
    if !installed.status.success() {
        return Err(format!(
            "pip 업그레이드 실패: {}",
            String::from_utf8_lossy(&installed.stderr).trim()
        ));
    }
    let installed = tokio::time::timeout(
        Duration::from_secs(1800),
        Command::new(&pip).args(["install", "mlx-lm"]).output(),
    )
    .await
    .map_err(|_| "mlx-lm 설치 시간 초과".to_string())?
    .map_err(|error| format!("pip 실행 실패: {error}"))?;
    if !installed.status.success() {
        return Err(format!(
            "mlx-lm 설치 실패: {}",
            String::from_utf8_lossy(&installed.stderr).trim()
        ));
    }
    set_install(json!({
        "status": "verifying", "engine": "mlx", "backend": "mlx-lm",
        "tag": "mlx-lm", "downloaded": 0, "total": 0,
        "file": "import mlx_lm", "error": "",
    }));
    let check = tokio::time::timeout(
        Duration::from_secs(120),
        Command::new(dir.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" }))
            .args(["-c", "import mlx_lm; print(mlx_lm.__version__)"])
            .output(),
    )
    .await
    .map_err(|_| "mlx-lm 검증 시간 초과".to_string())?
    .map_err(|error| format!("mlx-lm 검증 실패: {error}"))?;
    if !check.status.success() {
        return Err(format!(
            "mlx-lm을 불러오지 못했습니다: {}",
            String::from_utf8_lossy(&check.stderr).trim()
        ));
    }
    let version = String::from_utf8_lossy(&check.stdout).trim().to_string();
    std::fs::write(dir.join(".ready"), &version).map_err(|error| error.to_string())?;
    Ok(json!({
        "status": "installed",
        "engine": "mlx",
        "backend": "mlx-lm",
        "path": mlx_runtime_python(state).map(|path| path.to_string_lossy().to_string()),
        "version": version,
        "tag": "mlx-lm",
        "downloaded": 0,
        "total": 0,
        "file": "",
        "error": "",
    }))
}

fn brew_bin() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("DOMINION_BREW_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn verify_llama_server(path: &Path) -> Result<String, String> {
    for argument in ["--version", "--help"] {
        let output = std::process::Command::new(path)
            .arg(argument)
            .output()
            .map_err(|error| format!("llama-server 실행 확인 실패: {error}"))?;
        if output.status.success() {
            let version = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .chars()
            .take(500)
            .collect::<String>();
            return Ok(version);
        }
    }
    Err("llama-server가 설치되었지만 실행 확인에 실패했습니다.".into())
}

fn llama_release_suffix() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "bin-macos-arm64.zip",
        ("macos", "x86_64") => "bin-macos-x64.zip",
        ("linux", "x86_64") => "bin-ubuntu-x64.zip",
        ("linux", "aarch64") => "bin-ubuntu-arm64.zip",
        _ => "",
    }
}

static INSTALL_STATE: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

fn install_state() -> &'static Mutex<Option<Value>> {
    INSTALL_STATE.get_or_init(|| Mutex::new(None))
}

pub fn install_snapshot() -> Value {
    install_state()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or(Value::Null)
}

fn set_install(value: Value) {
    if let Ok(mut guard) = install_state().lock() {
        *guard = Some(value);
    }
}

fn install_active() -> bool {
    install_state()
        .lock()
        .ok()
        .and_then(|guard| {
            guard
                .as_ref()
                .and_then(|value| value["status"].as_str().map(str::to_owned))
        })
        .is_some_and(|status| {
            matches!(
                status.as_str(),
                "checking" | "downloading" | "extracting" | "verifying"
            )
        })
}

fn install_progress(
    status: &str,
    backend: &str,
    tag: &str,
    downloaded: u64,
    total: u64,
    file: &str,
) -> Value {
    json!({
        "status": status,
        "backend": backend,
        "tag": tag,
        "downloaded": downloaded,
        "total": total,
        "file": file,
        "error": "",
        "updatedAt": chrono::Utc::now().to_rfc3339(),
    })
}

async fn install_from_release(state: &AppState) -> Result<(PathBuf, String), String> {
    let suffix = llama_release_suffix();
    if suffix.is_empty() {
        return Err("이 플랫폼의 llama.cpp 자동 설치 자산이 없습니다.".into());
    }
    let client = reqwest::Client::new();
    let release = client
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .header("User-Agent", "dominion-rust")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|error| format!("llama.cpp 릴리스 조회 실패: {error}"))?
        .error_for_status()
        .map_err(|error| format!("llama.cpp 릴리스 HTTP 오류: {error}"))?
        .json::<Value>()
        .await
        .map_err(|error| format!("llama.cpp 릴리스 응답 오류: {error}"))?;
    let asset = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|asset| {
            asset["name"]
                .as_str()
                .is_some_and(|name| name.ends_with(suffix))
        })
        .ok_or_else(|| format!("llama.cpp 릴리스에 {suffix} 자산이 없습니다."))?;
    let asset_name = asset["name"]
        .as_str()
        .unwrap_or("llama.cpp")
        .to_string();
    let url = asset["browser_download_url"]
        .as_str()
        .ok_or("llama.cpp 다운로드 주소가 없습니다.")?;
    let temp = state
        .config
        .data_dir
        .join(format!("llama-install-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let archive = temp.join("llama.zip");
    let mut response = client
        .get(url)
        .header("User-Agent", "dominion-rust")
        .timeout(Duration::from_secs(900))
        .send()
        .await
        .map_err(|error| format!("llama.cpp 다운로드 실패: {error}"))?
        .error_for_status()
        .map_err(|error| format!("llama.cpp 다운로드 HTTP 오류: {error}"))?;
    let total = response.content_length().unwrap_or(0);
    set_install(install_progress(
        "downloading",
        "release",
        &asset_name,
        0,
        total,
        &asset_name,
    ));
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("llama.cpp 다운로드 본문 오류: {error}"))?
    {
        bytes.extend_from_slice(&chunk);
        set_install(install_progress(
            "downloading",
            "release",
            &asset_name,
            bytes.len() as u64,
            total,
            &asset_name,
        ));
    }
    std::fs::write(&archive, &bytes).map_err(|error| error.to_string())?;
    set_install(install_progress(
        "extracting",
        "release",
        &asset_name,
        bytes.len() as u64,
        total,
        &asset_name,
    ));
    let extracted = temp.join("extracted");
    std::fs::create_dir_all(&extracted).map_err(|error| error.to_string())?;
    let output = if cfg!(target_os = "macos") {
        std::process::Command::new("ditto")
            .args(["-x", "-k"])
            .arg(&archive)
            .arg(&extracted)
            .output()
    } else {
        std::process::Command::new("unzip")
            .arg("-q")
            .arg(&archive)
            .arg("-d")
            .arg(&extracted)
            .output()
    }
    .map_err(|error| format!("llama.cpp 압축 해제 실행 실패: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "llama.cpp 압축 해제 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut found = Vec::new();
    walk(&extracted, 0, &mut found);
    let binary = found
        .into_iter()
        .find(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some("llama-server")
                || path.file_name().and_then(|name| name.to_str()) == Some("llama-server.exe")
        })
        .ok_or("다운로드한 llama.cpp에 llama-server가 없습니다.")?;
    let source_dir = binary.parent().ok_or("llama-server 경로가 올바르지 않습니다.")?;
    let bin_dir = state.config.data_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source_dir).map_err(|error| error.to_string())?.flatten() {
        let source = entry.path();
        if source.is_file() {
            let target = bin_dir.join(entry.file_name());
            std::fs::copy(&source, &target)
                .map_err(|error| format!("llama.cpp 파일 복사 실패: {error}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&source)
                    .map(|meta| meta.permissions().mode())
                    .unwrap_or(0o755);
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&temp);
    let installed = bin_dir.join(if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    });
    Ok((installed, asset_name))
}

async fn run_llama_install(state: &AppState) -> Result<Value, String> {
    let prefer_brew = cfg!(target_os = "macos") && brew_bin().is_some();
    set_install(install_progress(
        "checking",
        if prefer_brew { "homebrew" } else { "release" },
        "",
        0,
        0,
        "",
    ));
    if let Some(brew) = brew_bin().filter(|_| cfg!(target_os = "macos")) {
        set_install(install_progress(
            "downloading",
            "homebrew",
            "llama.cpp",
            0,
            0,
            "brew install llama.cpp",
        ));
        let result = tokio::time::timeout(
            Duration::from_secs(900),
            tokio::process::Command::new(&brew)
                .args(["install", "llama.cpp"])
                .output(),
        )
        .await;
        if let Ok(Ok(output)) = result {
            if output.status.success() {
                set_install(install_progress(
                    "verifying",
                    "homebrew",
                    "llama.cpp",
                    0,
                    0,
                    "",
                ));
                if let Some(path) = llama_server_bin_for(state) {
                    let version = verify_llama_server(&path)?;
                    return Ok(json!({
                        "status": "installed",
                        "method": "homebrew",
                        "backend": "homebrew",
                        "path": path.to_string_lossy(),
                        "version": version,
                        "tag": "llama.cpp",
                        "downloaded": 0,
                        "total": 0,
                        "file": path.to_string_lossy(),
                        "error": "",
                    }));
                }
            }
        }
    }
    let (path, asset) = install_from_release(state).await?;
    set_install(install_progress(
        "verifying",
        "release",
        &asset,
        0,
        0,
        &path.to_string_lossy(),
    ));
    let version = verify_llama_server(&path)?;
    Ok(json!({
        "status": "installed",
        "method": "release",
        "backend": "release",
        "path": path.to_string_lossy(),
        "version": version,
        "tag": asset,
        "downloaded": 0,
        "total": 0,
        "file": path.to_string_lossy(),
        "error": "",
    }))
}

pub async fn install_llama_server(state: AppState) -> Result<Value, String> {
    if let Some(path) = llama_server_bin_for(&state) {
        let version = verify_llama_server(&path)?;
        set_install(json!({
            "status": "installed",
            "backend": "existing",
            "path": path.to_string_lossy(),
            "version": version,
            "tag": "",
            "downloaded": 0,
            "total": 0,
            "file": path.to_string_lossy(),
            "error": "",
        }));
        return Ok(json!({
            "status": "already_installed",
            "path": path.to_string_lossy(),
            "version": version,
            "setup": install_snapshot(),
            "capabilities": runtime_snapshot(&state)["capabilities"],
        }));
    }
    if !llama_installable(&state) {
        return Err("이 플랫폼에서는 llama.cpp 자동 설치를 지원하지 않습니다.".into());
    }
    if install_active() {
        return Ok(json!({
            "status": "installing",
            "setup": install_snapshot(),
            "capabilities": runtime_snapshot(&state)["capabilities"],
        }));
    }
    set_install(install_progress(
        "checking",
        if cfg!(target_os = "macos") && brew_bin().is_some() {
            "homebrew"
        } else {
            "release"
        },
        "",
        0,
        0,
        "",
    ));
    let task_state = state.clone();
    tokio::spawn(async move {
        let result = match run_llama_install(&task_state).await {
            Ok(result) => result,
            Err(error) => json!({
                "status": "failed",
                "backend": "",
                "tag": "",
                "downloaded": 0,
                "total": 0,
                "file": "",
                "error": error,
                "updatedAt": chrono::Utc::now().to_rfc3339(),
            }),
        };
        set_install(result);
    });
    Ok(json!({
        "status": "installing",
        "setup": install_snapshot(),
        "capabilities": runtime_snapshot(&state)["capabilities"],
    }))
}

/// Installable runtime catalog. The UI renders this list, so adding an engine
/// only needs an entry here plus an install branch where automatic setup is
/// possible. External engines are listed with their official download page.
pub fn runtimes(state: &AppState) -> Value {
    let setup = install_snapshot();
    let installing = setup["status"]
        .as_str()
        .is_some_and(|status| matches!(status, "checking" | "downloading" | "extracting" | "verifying"));
    let setup_engine = setup["engine"].as_str().unwrap_or("llama-cpp");
    let active = |engine: &str| {
        installing
            && (setup_engine == engine
                || (engine == "llama-cpp" && setup["engine"].as_str().is_none()))
    };
    let mlx_version = std::fs::read_to_string(mlx_runtime_dir(state).join(".ready"))
        .unwrap_or_default();
    let bin = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    };
    let app = |path: &str| {
        let path = PathBuf::from(path);
        path.is_dir().then_some(path)
    };
    let ollama = bin("ollama");
    let vllm = bin("vllm");
    let sglang = bin("sglang");
    let kobold = bin("koboldcpp").or_else(|| app("/Applications/KoboldCpp.app"));
    let jan = app("/Applications/Jan.app");
    let lmstudio = lms_bin();
    let ollama_installable = ollama.is_none()
        && cfg!(target_os = "macos")
        && brew_bin().is_some();
    json!({
        "setup": setup,
        "items": [
            {
                "id": "llama-cpp", "name": "llama.cpp", "formats": ["GGUF"],
                "installed": llama_server_bin_for(state).is_some(),
                "path": llama_server_bin_for(state).map(|path| path.to_string_lossy().to_string()),
                "installable": llama_installable(state), "installing": active("llama-cpp"),
                "installMethod": if cfg!(target_os="macos") && brew_bin().is_some() { "homebrew" } else { "release" },
                "version": "", "url": "https://github.com/ggml-org/llama.cpp",
            },
            {
                "id": "mlx-lm", "name": "MLX (mlx-lm)", "formats": ["MLX"],
                "installed": mlx_runtime_python(state).is_some(),
                "path": mlx_runtime_python(state).map(|path| path.to_string_lossy().to_string()),
                "installable": mlx_installable(state), "installing": active("mlx"),
                "installMethod": "python-venv", "version": mlx_version.trim(),
                "url": "https://github.com/ml-explore/mlx-lm",
            },
            {
                "id": "ollama", "name": "Ollama", "formats": ["GGUF"],
                "installed": ollama.is_some(),
                "path": ollama.map(|path| path.to_string_lossy().to_string()),
                "installable": ollama_installable, "installing": active("ollama"),
                "installMethod": "homebrew", "version": "",
                "url": "https://ollama.com/download",
            },
            {
                "id": "lm-studio", "name": "LM Studio", "formats": ["GGUF", "MLX"],
                "installed": lmstudio.is_some(),
                "path": lmstudio.map(|path| path.to_string_lossy().to_string()),
                "installable": false, "installing": false,
                "installMethod": "external", "version": "",
                "url": "https://lmstudio.ai/download",
            },
            {
                "id": "vllm", "name": "vLLM", "formats": ["HF", "GGUF"],
                "installed": vllm.is_some(),
                "path": vllm.map(|path| path.to_string_lossy().to_string()),
                "installable": false, "installing": false,
                "installMethod": "external", "version": "",
                "url": "https://docs.vllm.ai/en/latest/getting_started/installation.html",
            },
            {
                "id": "sglang", "name": "SGLang", "formats": ["HF"],
                "installed": sglang.is_some(),
                "path": sglang.map(|path| path.to_string_lossy().to_string()),
                "installable": false, "installing": false,
                "installMethod": "external", "version": "",
                "url": "https://docs.sglang.ai/get_started/install.html",
            },
            {
                "id": "koboldcpp", "name": "KoboldCpp", "formats": ["GGUF"],
                "installed": kobold.is_some(),
                "path": kobold.map(|path| path.to_string_lossy().to_string()),
                "installable": false, "installing": false,
                "installMethod": "external", "version": "",
                "url": "https://github.com/LostRuins/koboldcpp/releases",
            },
            {
                "id": "jan", "name": "Jan", "formats": ["GGUF"],
                "installed": jan.is_some(),
                "path": jan.map(|path| path.to_string_lossy().to_string()),
                "installable": false, "installing": false,
                "installMethod": "external", "version": "",
                "url": "https://jan.ai/download",
            },
        ],
    })
}

pub async fn install_runtime(state: AppState, engine: &str) -> Result<Value, String> {
    if engine == "ollama" {
        if let Some(path) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join("ollama"))
            .find(|path| path.is_file())
        {
            return Ok(json!({
                "status": "already_installed", "engine": "ollama",
                "path": path.to_string_lossy(),
                "setup": install_snapshot(),
            }));
        }
        let brew = brew_bin().ok_or("Ollama 자동 설치에는 Homebrew가 필요합니다.")?;
        if install_active() {
            return Ok(json!({"status": "installing", "setup": install_snapshot()}));
        }
        set_install(json!({
            "status": "downloading", "engine": "ollama", "backend": "homebrew",
            "tag": "ollama", "downloaded": 0, "total": 0,
            "file": "brew install ollama", "error": "",
        }));
        let task_state = state.clone();
        tokio::spawn(async move {
            let result = match tokio::time::timeout(
                Duration::from_secs(900),
                Command::new(&brew).args(["install", "ollama"]).output(),
            )
            .await
            {
                Ok(Ok(output)) if output.status.success() => json!({
                    "status": "installed", "engine": "ollama", "backend": "homebrew",
                    "tag": "ollama", "downloaded": 0, "total": 0,
                    "file": "", "error": "",
                }),
                Ok(Ok(output)) => json!({
                    "status": "failed", "engine": "ollama", "backend": "homebrew",
                    "tag": "ollama", "downloaded": 0, "total": 0,
                    "file": "", "error": String::from_utf8_lossy(&output.stderr).trim(),
                }),
                Ok(Err(error)) => json!({
                    "status": "failed", "engine": "ollama", "backend": "homebrew",
                    "tag": "ollama", "downloaded": 0, "total": 0,
                    "file": "", "error": error.to_string(),
                }),
                Err(_) => json!({
                    "status": "failed", "engine": "ollama", "backend": "homebrew",
                    "tag": "ollama", "downloaded": 0, "total": 0,
                    "file": "", "error": "Ollama installation timed out",
                }),
            };
            set_install(result);
            let _ = task_state;
        });
        return Ok(json!({
            "status": "installing",
            "setup": install_snapshot(),
            "capabilities": runtime_snapshot(&state)["capabilities"],
        }));
    }
    if !matches!(engine, "mlx" | "mlx-lm") {
        return install_llama_server(state).await;
    }
    if let Some(python) = mlx_runtime_python(&state) {
        let version = std::fs::read_to_string(mlx_runtime_dir(&state).join(".ready"))
            .unwrap_or_default();
        set_install(json!({
            "status": "installed", "engine": "mlx", "backend": "mlx-lm",
            "path": python.to_string_lossy(), "version": version, "tag": "mlx-lm",
            "downloaded": 0, "total": 0, "file": "", "error": "",
        }));
        return Ok(json!({
            "status": "already_installed",
            "engine": "mlx",
            "path": python.to_string_lossy(),
            "version": version,
            "setup": install_snapshot(),
            "capabilities": runtime_snapshot(&state)["capabilities"],
        }));
    }
    if !(cfg!(target_os = "macos") && cfg!(target_arch = "aarch64")) {
        return Err("MLX 실행기는 Apple Silicon macOS에서만 자동 설치할 수 있습니다.".into());
    }
    if install_active() {
        return Ok(json!({"status": "installing", "setup": install_snapshot()}));
    }
    set_install(json!({
        "status": "checking", "engine": "mlx", "backend": "mlx-lm",
        "tag": "mlx-lm", "downloaded": 0, "total": 0,
        "file": "python -m venv mlx-runtime", "error": "",
    }));
    let task_state = state.clone();
    tokio::spawn(async move {
        let result = match install_mlx_runtime(&task_state).await {
            Ok(result) => result,
            Err(error) => json!({
                "status": "failed", "engine": "mlx", "backend": "mlx-lm",
                "tag": "mlx-lm", "downloaded": 0, "total": 0,
                "file": "", "error": error,
                "updatedAt": chrono::Utc::now().to_rfc3339(),
            }),
        };
        set_install(result);
    });
    Ok(json!({
        "status": "installing",
        "setup": install_snapshot(),
        "capabilities": runtime_snapshot(&state)["capabilities"],
    }))
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
    if metadata.is_file() && label.to_ascii_lowercase().ends_with(".gguf") {
        return Some(json!({
            "id": format!("external:{id}"), "repo": label, "label": label,
            "source": "external", "format": "gguf", "bytes": metadata.len(),
            "externalPath": path, "installedAt": null
        }));
    }
    if metadata.is_dir() && path.join("config.json").is_file() {
        let mut bytes = 0u64;
        let mut shards = 0usize;
        for entry in std::fs::read_dir(path).ok()?.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".safetensors") {
                shards += 1;
                bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            }
        }
        if shards == 0 {
            return None;
        }
        return Some(json!({
            "id": format!("external:{id}"), "repo": label, "label": label,
            "source": "external", "format": "mlx", "bytes": bytes,
            "externalPath": path, "installedAt": null
        }));
    }
    None
}

pub fn register_local(state: &AppState, file_path: &str) -> Result<Value, String> {
    let path = Path::new(file_path.trim()).canonicalize().map_err(|_| "파일을 찾을 수 없습니다.")?;
    let previous = state.store.all(EXTERNAL_KIND).into_iter()
        .find(|record| record["path"].as_str() == path.to_str());
    let id = previous.as_ref().and_then(|record| record["id"].as_str()).map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model = external_model(&id, &path).ok_or("GGUF 파일 또는 MLX 모델 폴더를 선택하세요.")?;
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

/// Resolve a repository id below a models root without allowing traversal.
/// Lexical `starts_with` is not enough: `root/a/../../b` still starts with
/// `root` component-wise. Canonicalize and re-check the final directory, and
/// reject symlinks that escape the root.
fn managed_model_dir(root: &Path, repo: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let relative = Path::new(repo);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    let path = root.join(relative).canonicalize().ok()?;
    if path == root || !path.starts_with(&root) || !path.is_dir() {
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
        let path = mlx_model_dir(state, repo)
            .ok_or("다운로드한 MLX 모델을 찾을 수 없습니다.")?;
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
        "setup": install_snapshot(),
        "capabilities": {
            "gguf": { "available": lms_bin().is_some() || llama_server_bin_for(state).is_some(), "backend": "llama.cpp", "installable": llama_installable(state), "path": llama_server_bin_for(state).map(|path| path.to_string_lossy().to_string()), "installMethod": if cfg!(target_os="macos") && brew_bin().is_some() { "homebrew" } else { "release" } },
            "mlx": mlx_runtime_snapshot(state),
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

async fn load_mlx_standalone(
    state: &AppState,
    id: &str,
    repo: &str,
    context: u64,
) -> Result<Value, String> {
    let dir = mlx_model_dir(state, repo)
        .ok_or("다운로드한 MLX 모델 디렉터리를 찾을 수 없습니다.")?;
    let name = repo.rsplit('/').next().unwrap_or(repo);
    load_mlx_from_dir(state, id, name, &dir, context).await
}

async fn load_mlx_from_dir(
    state: &AppState,
    id: &str,
    name: &str,
    dir: &Path,
    context: u64,
) -> Result<Value, String> {
    let python = mlx_runtime_python(state)
        .ok_or("MLX 실행기가 설치되지 않았습니다. 모델 관리의 실행기 탭에서 설치하세요.")?;
    if !mlx_dir_complete(dir) {
        return Err("MLX 모델 파일이 완전하지 않습니다. config.json, safetensors, tokenizer 파일을 확인하세요.".into());
    }
    unload(state);
    {
        let mut guard = runtime().lock().unwrap();
        guard.status = "loading".into();
        guard.id = id.to_string();
        guard.name = name.to_string();
        guard.engine = "MLX".into();
        guard.backend = "mlx-lm".into();
        guard.model_key.clear();
        guard.error.clear();
        guard.logs.clear();
    }
    let log_path = state.config.data_dir.join("mlx-server.log");
    let log = std::fs::File::create(&log_path).map_err(|error| error.to_string())?;
    let child = Command::new(python)
        .args([
            "-m",
            "mlx_lm.server",
            "--model",
            &dir.to_string_lossy(),
            "--host",
            "127.0.0.1",
            "--port",
            &RUNTIME_PORT.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log))
        .spawn()
        .map_err(|error| format!("MLX 실행기 시작 실패: {error}"))?;
    runtime().lock().unwrap().child = Some(child);
    let client = reqwest::Client::new();
    for _ in 0..600 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let exited = {
            let mut guard = runtime().lock().unwrap();
            guard
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten())
        };
        if let Some(status) = exited {
            let tail = std::fs::read_to_string(&log_path)
                .unwrap_or_default()
                .lines()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            let message = format!("MLX 실행기가 종료되었습니다 (exit {status}): {tail}");
            {
                let mut guard = runtime().lock().unwrap();
                guard.status = "error".into();
                guard.error = message.clone();
            }
            return Err(message);
        }
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{RUNTIME_PORT}/v1/models"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    if let Some(served) = value["data"]
                        .as_array()
                        .and_then(|models| models.first())
                        .and_then(|model| model["id"].as_str())
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
                                "model": served,
                                "contextTokens": context.clamp(512, 32768),
                                "description": "MLX runtime managed by the Rust engine",
                            }),
                        )
                        .ok_or("모델 제공자를 저장할 수 없습니다.")?;
                        runtime().lock().unwrap().status = "ready".into();
                        state.bus.record("provider", &saved);
                        return Ok(runtime_snapshot(state));
                    }
                }
            }
        }
    }
    runtime().lock().unwrap().status = "error".into();
    runtime().lock().unwrap().error = "MLX 실행기 응답 시간 초과".into();
    Err("MLX 실행기 응답 시간 초과".into())
}

pub async fn load(state: &AppState, id: &str, context: u64) -> Result<Value, String> {
    if let Some(key) = id.strip_prefix("lms:") {
        return load_lmstudio(state, id, key, context).await;
    }
    if let Some(repo) = id.strip_prefix("mlx:") {
        if mlx_runtime_python(state).is_some() {
            return load_mlx_standalone(state, id, repo, context).await;
        }
        let key = find_lms_key(repo).ok_or(
            "MLX 실행기가 없습니다. 모델 관리에서 MLX 실행기를 설치하거나 LM Studio를 설치하세요.",
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
    if resolved.is_dir() {
        if mlx_runtime_python(state).is_none() {
            return Err("MLX 실행기가 필요합니다. 모델 관리의 실행기 탭에서 설치하세요.".into());
        }
        let name = resolved
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("local-mlx")
            .to_string();
        return load_mlx_from_dir(state, id, &name, &resolved, context).await;
    }
    let llama_server = llama_server_bin_for(state)
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
                        "toolMode": "native",
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
    if format == "all" {
        let limit = limit.clamp(1, 100);
        let (gguf, mlx) = tokio::join!(
            catalog_filtered(query, limit, "gguf"),
            catalog_filtered(query, limit, "mlx")
        );
        let mut items = gguf.unwrap_or_default();
        items.extend(mlx.unwrap_or_default());
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| {
            item["id"]
                .as_str()
                .is_some_and(|id| seen.insert(id.to_string()))
        });
        items.sort_by(|a, b| {
            b["downloads"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["downloads"].as_u64().unwrap_or(0))
        });
        items.truncate(limit);
        return Ok(items);
    }
    catalog_filtered(query, limit, format).await
}

async fn catalog_filtered(query: &str, limit: usize, format: &str) -> Result<Vec<Value>, String> {
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

/// File claims are based on actual destinations, not just job IDs or option names.
fn download_targets(job: &Value) -> std::collections::HashSet<PathBuf> {
    let Some(root)=job["targetDir"].as_str().filter(|root|!root.is_empty()) else {
        return std::collections::HashSet::new();
    };
    job["files"].as_array().into_iter().flatten()
        .filter_map(|file|file["path"].as_str().filter(|path|safe_relative(path)))
        .map(|path|PathBuf::from(root).join(path))
        .collect()
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
    let files = if format == "mlx" {
        if requested_files.iter().any(|path|!safe_relative(path)) {
            return Err("MLX 모델 파일 경로가 올바르지 않습니다.".into());
        }
        let mut unique=std::collections::HashSet::new();
        let files = requested_files.into_iter()
            .filter(|path|unique.insert(path.clone()))
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
    let targets=files.iter().map(|path|dir.join(path))
        .collect::<std::collections::HashSet<_>>();
    // Check + write are serialized; otherwise two simultaneous requests can
    // both observe an empty job list and overwrite the same .part file.
    let _registration=DOWNLOAD_REGISTRATION.lock()
        .map_err(|_|"모델 다운로드 등록 상태를 읽지 못했습니다.".to_string())?;
    let workers=active_downloads().lock()
        .map_err(|_|"실행 중인 모델 다운로드를 확인하지 못했습니다.".to_string())?
        .clone();
    for existing in state.store.all(MODEL_KIND) {
        let is_claimed=matches!(existing["status"].as_str(),Some("downloading"|"paused"))
            || existing["id"].as_str().is_some_and(|id|workers.contains(id));
        if !is_claimed {continue;}
        let used=download_targets(&existing);
        let same_option=existing["repo"]==repo && existing["format"]==format
            && existing["optionId"]==option;
        if same_option && used==targets {return Ok(existing);}
        if same_option || !targets.is_disjoint(&used) {
            return Err("같은 모델 파일을 사용하는 다운로드가 이미 진행 중이거나 일시 중지됐습니다. 기존 작업을 완료하거나 취소하세요.".into());
        }
    }
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

fn file_sha256(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn hf_blob_metadata(
    client: &reqwest::Client,
    repo: &str,
) -> std::collections::HashMap<String, (u64, Option<String>)> {
    let url = format!("https://huggingface.co/api/models/{repo}?blobs=true");
    let Ok(response) = client.get(url).timeout(Duration::from_secs(20)).send().await else {
        return Default::default();
    };
    let Ok(value) = response.json::<Value>().await else {
        return Default::default();
    };
    let mut map = std::collections::HashMap::new();
    for sibling in value["siblings"].as_array().into_iter().flatten() {
        let Some(path) = sibling["rfilename"].as_str() else {
            continue;
        };
        let size = sibling["size"].as_u64().unwrap_or(0);
        let sha = sibling["lfs"]["oid"]
            .as_str()
            .filter(|value| value.len() == 64)
            .or_else(|| sibling["blobId"].as_str().filter(|value| value.len() == 64))
            .map(str::to_string);
        map.insert(path.to_string(), (size, sha));
    }
    map
}

async fn download_worker(state: AppState, job_id: String) {
    use futures_util::StreamExt;
    if !active_downloads()
        .lock()
        .map(|mut active| active.insert(job_id.clone()))
        .unwrap_or(false)
    {
        return;
    }
    let _download_guard = DownloadGuard(job_id.clone());
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
    let blobs = hf_blob_metadata(&client, &repo).await;
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
            let (remote_size, remote_sha) = blobs.get(&path).cloned().unwrap_or((0, None));
            let expected = remote_size.max(files[index]["total"].as_u64().unwrap_or(0));
            let hash_ok = remote_sha.as_deref().is_none_or(|sha| {
                file_sha256(&target).is_ok_and(|actual| actual == sha)
            });
            if (expected > 0 && size != expected) || !hash_ok {
                let _ = std::fs::remove_file(&target);
            } else {
                files[index]["downloaded"] = json!(size);
                patch_download(
                    &state,
                    &job_id,
                    json!({"files": files, "downloaded": downloaded_total(&files), "total": declared_total(&files), "current": path}),
                );
                continue;
            }
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
            let (remote_size, remote_sha) = blobs.get(&path).cloned().unwrap_or((0, None));
            let expected = remote_size.max(files[index]["total"].as_u64().unwrap_or(0));
            let hash_ok = remote_sha.as_deref().is_none_or(|sha| {
                file_sha256(&partial).is_ok_and(|actual| actual == sha)
            });
            if expected > 0 && existing == expected && hash_ok {
                let _ = tokio::fs::rename(&partial, &target).await;
                files[index]["downloaded"] = json!(existing);
                continue;
            }
            let _ = tokio::fs::remove_file(&partial).await;
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": "부분 파일 크기가 원격 파일과 맞지 않습니다. 다시 다운로드하세요."}),
            );
            return;
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
        if declared > 0 && downloaded != declared {
            patch_download(
                &state,
                &job_id,
                json!({"status": "failed", "error": format!("다운로드 크기 불일치: {downloaded}/{declared} 바이트. 재개하면 이어받습니다.")}),
            );
            return;
        }
        if let Some((_, Some(sha))) = blobs.get(&path).cloned() {
            let verify_path = partial.clone();
            let actual = tokio::task::spawn_blocking(move || file_sha256(&verify_path))
                .await
                .ok()
                .and_then(Result::ok);
            if actual.as_deref() != Some(sha.as_str()) {
                let _ = tokio::fs::remove_file(&partial).await;
                patch_download(
                    &state,
                    &job_id,
                    json!({"status": "failed", "error": "다운로드 파일 해시가 원격 파일과 일치하지 않습니다. 다시 다운로드하세요."}),
                );
                return;
            }
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
    // Resume must use the same file-ownership gate as initial registration.
    let _registration=DOWNLOAD_REGISTRATION.lock().ok()?;
    let running=active_downloads().lock().ok()?.clone();
    if running.contains(id) {return None;}
    let targets=download_targets(&job);
    if state.store.all(MODEL_KIND).iter().any(|other| {
        other["id"]!=id
            && (matches!(other["status"].as_str(),Some("downloading"|"paused"))
                || other["id"].as_str().is_some_and(|other_id|running.contains(other_id)))
            && !download_targets(other).is_disjoint(&targets)
    }) {
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
#[path = "models/tests.rs"]
mod tests;
