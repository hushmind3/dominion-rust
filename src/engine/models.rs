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

#[path = "models/installed.rs"]
mod installed;
pub use installed::{installed};
pub(crate) use installed::*;

#[path = "models/install.rs"]
mod install;
pub use install::{install_snapshot,install_llama_server};
pub(crate) use install::*;

#[path = "models/runtimes.rs"]
mod runtimes;
pub use runtimes::{runtimes,install_runtime};
pub(crate) use runtimes::*;

#[path = "models/storage.rs"]
mod storage;
pub use storage::{import_local,register_local,remove_installed};
pub(crate) use storage::*;

#[path = "models/load.rs"]
mod load;
pub use load::{runtime_snapshot,load,mark_load_failure,unload};
pub(crate) use load::*;

#[path = "models/catalog.rs"]
mod catalog;
pub use catalog::{catalog,details};
pub(crate) use catalog::*;

#[path = "models/download.rs"]
mod download;
pub use download::{jobs,start_download,start_download_with,pause_job,resume_job,cancel_job};
pub(crate) use download::*;

#[cfg(test)]
#[path = "models/tests.rs"]
mod tests;
