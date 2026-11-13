//! Local model import, registration and removal.
use super::{
    AppState,
    json,
    Value,
    Path,
    PathBuf,
    EXTERNAL_KIND,
    runtime,
    models_dir,
    gguf_shard_parts,
    local_gguf_parts,
    installed,
    mlx_model_dir,
};

pub(super) fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
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
    let parts = local_gguf_parts(&source)?;
    let dir = models_dir(state);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let name = source
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or("파일 이름이 없습니다.")?;
    // Preserve every sibling when importing a sharded model. Copying only the
    // selected first shard made it disappear from the installed-model list.
    for part in parts {
        let part_name = part.file_name().ok_or("GGUF 조각 파일 이름이 없습니다.")?;
        let target = dir.join(part_name);
        if part != target {
            std::fs::copy(&part, &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(installed(state)
        .into_iter()
        .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(name.as_str()))
        .unwrap_or_else(|| json!({ "id": name })))
}

pub(super) fn external_model(id: &str, path: &Path) -> Option<Value> {
    let label = path.file_name()?.to_str()?;
    let metadata = path.metadata().ok()?;
    if metadata.is_file() && label.to_ascii_lowercase().ends_with(".gguf") {
        let parts = local_gguf_parts(path).ok()?;
        let bytes = parts.iter().try_fold(0u64, |sum, part| {
            sum.checked_add(part.metadata().ok()?.len())
        })?;
        return Some(json!({
            "id": format!("external:{id}"), "repo": label, "label": label,
            "source": "external", "format": "gguf", "bytes": bytes,
            "shards": parts.len(), "externalPath": path, "installedAt": null
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

pub(super) fn managed_model_path(state: &AppState, id: &str) -> Option<PathBuf> {
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
pub(super) fn managed_model_dir(root: &Path, repo: &str) -> Option<PathBuf> {
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
        if let Some((prefix, number, total)) = gguf_shard_parts(&path.to_string_lossy()) {
            if number != 1 {
                return Err("분할 GGUF 모델은 첫 조각에서 제거해야 합니다.".into());
            }
            // Delete the full model, not just shard one. Otherwise gigabytes
            // remain orphaned on disk but disappear from the installed list.
            let parts = (1..=total)
                .map(|index| PathBuf::from(format!("{prefix}-{index:05}-of-{total:05}.gguf")))
                .collect::<Vec<_>>();
            if !parts.iter().all(|part| part.is_file()) {
                return Err("분할 GGUF 조각이 누락되어 모델을 제거할 수 없습니다.".into());
            }
            for part in parts {
                std::fs::remove_file(part).map_err(|error| error.to_string())?;
            }
        } else {
            std::fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    state.bus.record(EXTERNAL_KIND, &json!({"id":id}));
    Ok(())
}

