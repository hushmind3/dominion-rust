//! Durable model download queue, progress and jobs.
//! Runtime implementations connected by engine::models.
use super::*;

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

pub(super) fn job_status(state: &AppState, id: &str) -> String {
    state
        .store
        .get(MODEL_KIND, id)
        .and_then(|job| job["status"].as_str().map(str::to_string))
        .unwrap_or_default()
}

pub(super) fn patch_download(state: &AppState, id: &str, patch: Value) {
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

pub(super) fn downloaded_total(files: &[Value]) -> u64 {
    files
        .iter()
        .map(|file| file["downloaded"].as_u64().unwrap_or(0))
        .sum()
}

pub(super) fn declared_total(files: &[Value]) -> u64 {
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

pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
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

pub(super) async fn hf_blob_metadata(
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

pub(super) async fn download_worker(state: AppState, job_id: String) {
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
    if active_downloads()
        .lock()
        .map(|active| active.contains(id))
        .unwrap_or(false)
    {
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

