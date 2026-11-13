//! Runtime dependency installation state and installation jobs.
//! Runtime implementations connected by engine::models.
use super::*;

pub(super) async fn install_mlx_runtime(state: &AppState) -> Result<Value, String> {
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

pub(super) fn brew_bin() -> Option<PathBuf> {
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

pub(super) fn verify_llama_server(path: &Path) -> Result<String, String> {
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

pub(super) fn llama_release_suffix() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "bin-macos-arm64.zip",
        ("macos", "x86_64") => "bin-macos-x64.zip",
        ("linux", "x86_64") => "bin-ubuntu-x64.zip",
        ("linux", "aarch64") => "bin-ubuntu-arm64.zip",
        _ => "",
    }
}

pub(super) static INSTALL_STATE: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

pub(super) fn install_state() -> &'static Mutex<Option<Value>> {
    INSTALL_STATE.get_or_init(|| Mutex::new(None))
}

pub fn install_snapshot() -> Value {
    install_state()
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or(Value::Null)
}

pub(super) fn set_install(value: Value) {
    if let Ok(mut guard) = install_state().lock() {
        *guard = Some(value);
    }
}

pub(super) fn install_active() -> bool {
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

pub(super) fn install_progress(
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

pub(super) async fn install_from_release(state: &AppState) -> Result<(PathBuf, String), String> {
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

pub(super) async fn run_llama_install(state: &AppState) -> Result<Value, String> {
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
