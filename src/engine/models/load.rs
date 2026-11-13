//! Process-scoped loaded model lifecycle and snapshots.
//! Runtime implementations connected by engine::models.
use super::*;

pub(crate) fn total_memory() -> u64 {
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

pub(crate) async fn load_mlx_standalone(
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

pub(crate) async fn load_mlx_from_dir(
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

pub(crate) async fn load_lmstudio(
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

pub(crate) fn push_log(line: String) {
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
