//! Native model runtime detection, installation and provisioning.
//! Split from models.rs; engine behavior remains on its original API paths.
use super::*;

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

static INSTALL_STATE: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

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

