//! Resumable model downloads and durable transfer jobs.
use super::{
    AppState,
    json,
    Value,
    Path,
    PathBuf,
    Sha256,
    Duration,
    MODEL_KIND,
    DOWNLOAD_COMMIT,
    DOWNLOAD_START_LOCK,
    active_downloads,
    DownloadGuard,
    safe_repo,
    safe_relative,
    target_root,
    find_lms_key,
    validated_gguf_files,
    installed,
    Digest,
};

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
pub(super) fn valid_hf_revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Keep partial downloads from different immutable snapshots independent.
/// A cancelled build A must never be appended to by resumed build B.
pub(super) fn partial_download_path(target: &Path, revision: &str) -> PathBuf {
    let suffix = if valid_hf_revision(revision) {
        format!(".part.{revision}")
    } else {
        ".part".to_string()
    };
    PathBuf::from(format!("{}{}", target.display(), suffix))
}

pub fn start_download_with(
    state: AppState,
    repo: String,
    option: String,
    format: String,
    requested_files: Vec<String>,
) -> Result<Value, String> {
    start_download_at_revision(state, repo, option, format, requested_files, None)
}

/// Pin a single immutable Hugging Face commit for every shard and resume.
pub fn start_download_at_revision(
    state: AppState,
    repo: String,
    option: String,
    format: String,
    requested_files: Vec<String>,
    revision: Option<&str>,
) -> Result<Value, String> {
    let repo = safe_repo(&repo)?;
    let revision = revision.filter(|value| !value.is_empty()).unwrap_or("main");
    if revision != "main" && !valid_hf_revision(revision) {
        return Err("Hugging Face 모델 버전이 올바른 커밋 해시가 아닙니다.".into());
    }
    let format = if format == "mlx" { "mlx" } else { "gguf" };
    let option = option.trim().to_string();
    if option.is_empty() {
        return Err("다운로드할 배포 형식을 선택하세요.".into());
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
        validated_gguf_files(&option, requested_files)?
    };
    let dir = target_root(&state, format, &repo);
    let requested_targets = files.iter().map(|path|dir.join(path))
        .collect::<std::collections::HashSet<_>>();
    // The lock covers conflict inspection AND persistent job registration.
    // This is the actual ownership boundary, not the later worker guard.
    let _registration = DOWNLOAD_START_LOCK.lock()
        .map_err(|_|"모델 다운로드 등록 잠금을 사용할 수 없습니다.".to_string())?;
    for existing in state.store.all(MODEL_KIND) {
        let running = matches!(existing["status"].as_str(),Some("downloading"|"paused"));
        let live_worker = existing["id"].as_str().is_some_and(|id|
            active_downloads().lock().map(|active|active.contains(id)).unwrap_or(true));
        if !running && !live_worker {continue;}
        let same_selection=existing["repo"].as_str()==Some(repo.as_str())
            && existing["optionId"].as_str()==Some(option.as_str())
            && existing["format"].as_str().unwrap_or("gguf")==format;
        if same_selection {
            if existing["revision"].as_str().unwrap_or("main")!=revision {
                return Err("이 모델의 다른 버전 다운로드가 진행 중입니다. 기존 작업을 먼저 완료하거나 취소하세요.".into());
            }
            let existing_files=existing["files"].as_array().map(|rows|rows.iter()
                .filter_map(|row|row["path"].as_str())
                .collect::<std::collections::HashSet<_>>());
            let same_manifest=existing_files.as_ref().is_none_or(|manifest|
                manifest.len()==files.len() && files.iter().all(|path|manifest.contains(path.as_str())));
            if same_manifest {return Ok(existing);}
            return Err("동일 모델의 다운로드 파일 구성이 다릅니다. 기존 작업을 먼저 완료하거나 취소하세요.".into());
        }
        let previous_root=existing["targetDir"].as_str().filter(|root|!root.is_empty());
        let overlaps=previous_root.is_some_and(|root|existing["files"]
            .as_array().into_iter().flatten().filter_map(|row|row["path"].as_str())
            .filter(|path|safe_relative(path))
            .any(|path|requested_targets.contains(&Path::new(root).join(path))));
        if overlaps {
            return Err("동일한 대상 파일을 사용하는 다운로드가 이미 있습니다. 기존 작업을 완료하거나 취소하세요.".into());
        }
    }
    let mut total = 0u64;
    let mut entries = Vec::new();
    for path in &files {
        let target = dir.join(path);
        let partial = partial_download_path(&target, revision);
        // A pre-existing target may belong to a different revision. Until the
        // worker validates its hash, only actual .part bytes are progress.
        let downloaded = std::fs::metadata(&partial)
            .map(|meta| meta.len()).unwrap_or(0);
        total += downloaded;
        entries.push(json!({"path": path, "downloaded": downloaded, "total": 0}));
    }
    let job = state.store.put(
        MODEL_KIND,
        &json!({
            "repo": repo,
            "optionId": option,
            "revision": revision,
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
    revision: &str,
) -> std::collections::HashMap<String, (u64, Option<String>)> {
    let url = format!("https://huggingface.co/api/models/{repo}/revision/{revision}?blobs=true");
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

/// A 206 response is safe to append only when the server confirms that the
/// returned byte range starts exactly after the partial file on disk.
pub(super) fn promote_download_file(partial: &Path, target: &Path) -> Result<(), String> {
    let _lock = DOWNLOAD_COMMIT.lock()
        .map_err(|_| "모델 파일 변경 잠금을 획득하지 못했습니다.".to_string())?;
    if !partial.is_file() {
        return Err("완료된 부분 파일을 찾을 수 없습니다.".into());
    }
    // On macOS/Linux this is an atomic replacement. Windows may reject a
    // rename onto an existing file; preserve the old model as a fallback.
    match std::fs::rename(partial, target) {
        Ok(()) => Ok(()),
        Err(first) if target.is_file() => {
            let backup = PathBuf::from(format!("{}.dominion-previous", target.display()));
            if backup.exists() {
                return Err(format!("이전 모델 백업 파일이 남아 있습니다: {}", backup.display()));
            }
            std::fs::rename(target, &backup).map_err(|error| error.to_string())?;
            match std::fs::rename(partial, target) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&backup);
                    Ok(())
                }
                Err(error) => {
                    let restore = std::fs::rename(&backup, target);
                    Err(format!("모델 교체 실패: {error}; 기존 모델 복구: {}",
                        if restore.is_ok() { "완료" } else { "실패" }))
                }
            }
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(super) fn content_range_starts_at(header: Option<&str>, requested: u64) -> bool {
    let Some((range, total)) = header.and_then(|value| value.strip_prefix("bytes "))
        .and_then(|value| value.split_once('/')) else { return false };
    let Some((start, end)) = range.split_once('-') else { return false };
    let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) else {
        return false;
    };
    start == requested && end >= start
        && (total == "*" || total.parse::<u64>().is_ok_and(|n| n > end))
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
    let revision = job["revision"].as_str()
        .filter(|value| valid_hf_revision(value))
        .unwrap_or("main").to_string();
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
    let blobs = hf_blob_metadata(&client, &repo, &revision).await;
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
            if size == 0 || expected == 0 || size != expected || !hash_ok {
                // Do not erase an installed revision before the replacement
                // has finished downloading and passed integrity checks.
                files[index]["downloaded"] = json!(0);
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
        let partial = partial_download_path(&target, &revision);
        let existing = std::fs::metadata(&partial).map(|meta| meta.len()).unwrap_or(0);
        let url = format!("https://huggingface.co/{repo}/resolve/{revision}/{path}?download=true");
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
                if let Err(error) = promote_download_file(&partial, &target) {
                    patch_download(&state, &job_id, json!({
                        "status": "failed", "error": error
                    }));
                    return;
                }
                files[index]["downloaded"] = json!(existing);
                files[index]["total"] = json!(expected);
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
        if append && !content_range_starts_at(
            response.headers().get(reqwest::header::CONTENT_RANGE)
                .and_then(|value| value.to_str().ok()),
            existing,
        ) {
            // Never append unknown bytes onto a valid partial download.
            // Discard it so the next resume starts from byte zero.
            let _ = tokio::fs::remove_file(&partial).await;
            patch_download(&state, &job_id, json!({
                "status": "failed",
                "error": "서버의 이어받기 바이트 구간이 일치하지 않습니다. 다시 재개하면 처음부터 다운로드합니다."
            }));
            return;
        }
        let remote_size = blobs.get(&path).map(|(size, _)| *size).unwrap_or(0);
        let response_size = response.content_length()
            .unwrap_or(0).saturating_add(if append { existing } else { 0 });
        let declared = if remote_size > 0 { remote_size } else { response_size };
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
        if downloaded == 0 || (declared > 0 && downloaded != declared) {
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
        if let Err(error) = promote_download_file(&partial, &target) {
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
    // Serialize resume with new registrations; otherwise a stopped job can
    // reacquire a destination already assigned to another download.
    let _registration = DOWNLOAD_START_LOCK.lock().ok()?;
    let job = state.store.get(MODEL_KIND, id)?;
    let status = job["status"].as_str().unwrap_or("");
    if !matches!(status, "paused" | "failed" | "cancelled") {
        return None;
    }
    if active_downloads()
        .lock()
        .map(|active| active.contains(id))
        .unwrap_or(true)
    {
        return None;
    }
    let target_dir=job["targetDir"].as_str().filter(|root|!root.is_empty())?;
    let own_paths=job["files"].as_array()?
        .iter().filter_map(|row|row["path"].as_str())
        .filter(|path|safe_relative(path))
        .map(|path|Path::new(target_dir).join(path))
        .collect::<std::collections::HashSet<_>>();
    if own_paths.is_empty() {return None;}
    let others=state.store.all(MODEL_KIND);
    for other in others.iter().filter(|other|other["id"].as_str()!=Some(id)) {
        let other_active=matches!(other["status"].as_str(),Some("downloading"|"paused"))
            || other["id"].as_str().is_some_and(|other_id|
                active_downloads().lock().map(|active|active.contains(other_id)).unwrap_or(true));
        if !other_active {continue;}
        let overlapping=other["targetDir"].as_str().filter(|root|!root.is_empty())
            .is_some_and(|root|other["files"].as_array().into_iter().flatten()
                .filter_map(|row|row["path"].as_str())
                .filter(|path|safe_relative(path))
                .any(|path|own_paths.contains(&Path::new(root).join(path))));
        if overlapping {return None;}
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

