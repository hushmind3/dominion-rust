//! Managed model paths, shard validation, installed-model discovery.
use super::*;

pub fn models_dir(state: &AppState) -> PathBuf {
    state.config.data_dir.join("models")
}

pub(super) fn safe_repo(repo: &str) -> Result<String, String> {
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

pub(super) fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn mlx_models_root(state: &AppState) -> PathBuf {
    models_dir(state).join("mlx")
}

pub(super) fn legacy_mlx_models_root() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = PathBuf::from(home).join(".lmstudio/models");
    path.is_dir().then_some(path)
}

/// New downloads stay in DOMINION's data directory. Models downloaded by
/// older builds from the LM Studio models folder remain readable without
/// duplicating multi-gigabyte files.
pub(super) fn mlx_search_roots(state: &AppState) -> Vec<PathBuf> {
    let mut roots = vec![mlx_models_root(state)];
    if let Some(legacy) = legacy_mlx_models_root() {
        if !roots.contains(&legacy) {
            roots.push(legacy);
        }
    }
    roots
}

pub(super) fn target_root(state: &AppState, format: &str, repo: &str) -> PathBuf {
    if format == "mlx" {
        mlx_models_root(state).join(repo)
    } else {
        models_dir(state).join(repo.replace('/', "--"))
    }
}

pub(super) fn find_lms_key(repo: &str) -> Option<String> {
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


/* A split GGUF is ONE model: llama.cpp needs every numbered part beside
 * the first shard. Never advertise a partial set as a usable model. */
pub(super) fn gguf_shard_parts(path: &str) -> Option<(String, usize, usize)> {
    let stem = path.strip_suffix(".gguf")?;
    let (head, total_text) = stem.rsplit_once("-of-")?;
    let (prefix, number_text) = head.rsplit_once('-')?;
    if number_text.len() != 5 || total_text.len() != 5
        || !number_text.bytes().all(|b| b.is_ascii_digit())
        || !total_text.bytes().all(|b| b.is_ascii_digit())
    { return None; }
    let number = number_text.parse::<usize>().ok()?;
    let total = total_text.parse::<usize>().ok()?;
    (number > 0 && total > 1 && total <= 256 && number <= total)
        .then(|| (prefix.to_string(), number, total))
}

/// Local split GGUF imports and external registrations must include all
/// numbered shards. A single shard is not a usable model.
pub(super) fn local_gguf_parts(file: &Path) -> Result<Vec<PathBuf>, String> {
    let name = file.file_name().and_then(|name| name.to_str())
        .ok_or("GGUF 파일 이름을 읽을 수 없습니다.")?;
    let Some((prefix, number, total)) = gguf_shard_parts(name) else {
        return Ok(vec![file.to_path_buf()]);
    };
    if number != 1 {
        return Err("분할 GGUF 모델의 첫 번째 조각을 선택하세요.".into());
    }
    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    let parts = (1..=total)
        .map(|index| parent.join(format!("{prefix}-{index:05}-of-{total:05}.gguf")))
        .collect::<Vec<_>>();
    if parts.iter().any(|part| !part.is_file()) {
        return Err("분할 GGUF 모델의 조각이 누락되었습니다. 같은 폴더에 모든 조각을 두세요.".into());
    }
    Ok(parts)
}

pub(super) fn gguf_download_options(siblings: &[Value]) -> Vec<Value> {
    let mut options = Vec::new();
    let mut groups = std::collections::BTreeMap::<
        (String, usize), std::collections::BTreeMap<usize, (String, u64)>
    >::new();
    for sibling in siblings {
        let Some(file) = sibling["rfilename"].as_str() else { continue };
        if !file.to_ascii_lowercase().ends_with(".gguf") || !safe_relative(file) { continue; }
        let size = sibling["size"].as_u64().unwrap_or(0);
        if let Some((prefix, number, total)) = gguf_shard_parts(file) {
            groups.entry((prefix, total)).or_default()
                .insert(number, (file.to_string(), size));
            continue;
        }
        let quantization = file.strip_suffix(".gguf").unwrap_or(file)
            .rsplit('-').next().unwrap_or("").to_uppercase();
        options.push(json!({
            "id": file, "format": "gguf",
            "label": file.rsplit('/').next().unwrap_or(file),
            "quantization": quantization, "bytes": size,
            "sizeLabel": format_bytes(size), "files": [file], "shards": 1
        }));
    }
    for ((prefix, total), group) in groups {
        if group.len() != total || !(1..=total).all(|i| group.contains_key(&i)) {
            continue;
        }
        let files = (1..=total).map(|i| group[&i].0.clone()).collect::<Vec<_>>();
        let bytes = group.values().map(|(_, size)| *size).sum::<u64>();
        let name = prefix.rsplit('/').next().unwrap_or(&prefix);
        options.push(json!({
            "id": files[0], "format": "gguf",
            "label": format!("{name} · {total} parts"),
            "quantization": "", "bytes": bytes, "sizeLabel": format_bytes(bytes),
            "files": files, "shards": total
        }));
    }
    options
}

pub(super) fn validated_gguf_files(option: &str, requested: Vec<String>) -> Result<Vec<String>, String> {
    let file = option.trim_start_matches('/').to_string();
    if !file.to_ascii_lowercase().ends_with(".gguf") || !safe_relative(&file) {
        return Err("다운로드할 GGUF 파일을 확인하세요.".into());
    }
    let files = if requested.is_empty() { vec![file.clone()] } else { requested };
    if files.len() == 1 && files[0] == file && gguf_shard_parts(&file).is_none() {
        return Ok(files);
    }
    let Some((prefix, number, total)) = gguf_shard_parts(&file) else {
        return Err("GGUF 파일 목록과 선택한 모델이 일치하지 않습니다.".into());
    };
    if number != 1 || files.len() != total
        || files.iter().enumerate().any(|(index, path)| {
            !safe_relative(path) || gguf_shard_parts(path).is_none_or(|(p, n, t)| {
                p != prefix || n != index + 1 || t != total
            })
        })
    {
        return Err("분할 GGUF 모델은 모든 조각을 순서대로 함께 다운로드해야 합니다.".into());
    }
    Ok(files)
}

pub(super) fn mlx_download_files(siblings: &[Value]) -> Option<Value> {
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

pub(super) fn format_bytes(bytes: u64) -> String {
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
    let gguf = found.into_iter()
        .filter(|path| path.extension().and_then(|value| value.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf")))
        .collect::<Vec<_>>();
    let available = gguf.iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<std::collections::HashSet<_>>();
    gguf.into_iter()
        .filter(|path| {
            let path = path.to_string_lossy();
            match gguf_shard_parts(&path) {
                None => true,
                Some((prefix, number, total)) => number == 1
                    && (2..=total).all(|index|
                        available.contains(&format!("{prefix}-{index:05}-of-{total:05}.gguf"))),
            }
        })
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            let bytes = if let Some((prefix, _, total)) =
                gguf_shard_parts(&path.to_string_lossy()) {
                (1..=total).map(|index|
                    std::fs::metadata(format!("{prefix}-{index:05}-of-{total:05}.gguf"))
                        .map(|m| m.len()).unwrap_or(0)).sum()
            } else { metadata.len() };
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
                "bytes": bytes,
                "sizeLabel": format_bytes(bytes),
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

