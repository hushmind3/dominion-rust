//! Installed GGUF/MLX and LM Studio discovery.
//! Runtime implementations connected by engine::models.
use super::*;

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
pub(super) fn local_mlx_models(state: &AppState) -> Vec<Value> {
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

pub(super) fn mlx_model_from_dir(dir: &Path, root: &Path) -> Option<Value> {
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

pub(super) fn walk_dirs(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
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
pub(super) fn lmstudio_models() -> Vec<Value> {
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

pub(super) fn lms_bin() -> Option<PathBuf> {
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

pub(super) fn llama_server_bin() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }))
        .chain(
            ["/opt/homebrew/bin/llama-server", "/usr/local/bin/llama-server"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|path| path.is_file())
}

pub(super) fn llama_available() -> bool {
    llama_server_bin().is_some()
}

pub(super) fn llama_server_bin_for(state: &AppState) -> Option<PathBuf> {
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
pub(super) fn llama_installable(state: &AppState) -> bool {
    llama_server_bin_for(state).is_none()
        && matches!(std::env::consts::OS, "macos" | "linux")
        && matches!(std::env::consts::ARCH, "aarch64" | "x86_64")
}

pub(super) fn mlx_runtime_dir(state: &AppState) -> PathBuf {
    state.config.data_dir.join("mlx-runtime")
}

pub(super) fn mlx_runtime_python(state: &AppState) -> Option<PathBuf> {
    let dir = mlx_runtime_dir(state);
    let python = dir.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" });
    (python.is_file() && dir.join(".ready").is_file()).then_some(python)
}

pub(super) fn mlx_installable(state: &AppState) -> bool {
    mlx_runtime_python(state).is_none()
        && cfg!(target_os = "macos")
        && cfg!(target_arch = "aarch64")
}

/// Resolve a downloaded MLX repository below the managed models root.
pub(super) fn mlx_dir_complete(dir: &Path) -> bool {
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

pub(super) fn mlx_model_dir(state: &AppState, repo: &str) -> Option<PathBuf> {
    mlx_search_roots(state)
        .iter()
        .find_map(|root| managed_model_dir(root, repo))
        .filter(|dir| mlx_dir_complete(dir))
}

pub(super) fn mlx_runtime_snapshot(state: &AppState) -> Value {
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

