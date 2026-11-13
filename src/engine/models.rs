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


// Executable engine boundaries: public names are re-exported for unchanged API consumers.
#[path = "models/installer.rs"]
mod installer;
use installer::*;
pub use installer::{install_snapshot,install_llama_server,runtimes,install_runtime};
#[path = "models/management.rs"]
mod management;
use management::*;
pub use management::{import_local,register_local,remove_installed};
#[path = "models/loader.rs"]
mod loader;
use loader::*;
pub use loader::{runtime_snapshot,load,mark_load_failure,unload};
#[path = "models/catalog.rs"]
mod catalog;
use catalog::*;
pub use catalog::{catalog,details};
#[path = "models/download.rs"]
mod download;
use download::*;
pub use download::{jobs,start_download,start_download_with,pause_job,resume_job,cancel_job};

#[cfg(test)]
#[path = "models/tests.rs"]
mod tests;
