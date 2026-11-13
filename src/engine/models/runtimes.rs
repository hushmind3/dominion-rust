//! Runtime choices, capabilities and install routing.
//! Runtime implementations connected by engine::models.
use super::*;

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

