use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};

const CONFIG_KIND: &str = "dev_config";
const CONFIG_ID: &str = "default";

struct Preview {
    child: Option<Child>,
    status: String,
    url: String,
    logs: Vec<String>,
}

fn preview() -> &'static Mutex<Preview> {
    static PREVIEW: OnceLock<Mutex<Preview>> = OnceLock::new();
    PREVIEW.get_or_init(|| {
        Mutex::new(Preview {
            child: None,
            status: "stopped".into(),
            url: String::new(),
            logs: Vec::new(),
        })
    })
}

pub fn config(state: &AppState) -> Value {
    state.store.get(CONFIG_KIND, CONFIG_ID).unwrap_or_else(|| {
        json!({
            "id": CONFIG_ID,
            "sourceDir": std::env::var("DOMINION_SOURCE_WORKSPACE").unwrap_or_default(),
            "repoUrl": "",
            "branch": "main",
            "previewPort": 5174,
        })
    })
}

pub fn save_config(state: &AppState, body: &Value) -> Value {
    let mut record = config(state);
    for key in ["sourceDir", "repoUrl", "branch", "previewPort"] {
        if let Some(value) = body.get(key) {
            record[key] = value.clone();
        }
    }
    record["id"] = json!(CONFIG_ID);
    state.store.put(CONFIG_KIND, &record).unwrap_or(record)
}

/// npm scripts may live at the repo root or under `web/` (this Rust repo).
fn npm() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

fn npx() -> &'static str {
    if cfg!(windows) { "npx.cmd" } else { "npx" }
}

fn vite_bin(package: &Path) -> PathBuf {
    if cfg!(windows) {
        package.join("node_modules/.bin/vite.cmd")
    } else {
        package.join("node_modules/.bin/vite")
    }
}

fn package_dir(dir: &Path) -> PathBuf {
    if dir.join("package.json").exists() {
        dir.to_path_buf()
    } else if dir.join("web/package.json").exists() {
        dir.join("web")
    } else {
        dir.to_path_buf()
    }
}

fn source_dir(state: &AppState) -> Option<PathBuf> {
    let dir = config(state);
    let path = dir
        .get("sourceDir")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if path.trim().is_empty() {
        // A source checkout launched from its own root should not require
        // a non-developer to configure the same folder again.
        let cwd = std::env::current_dir().ok()?;
        if cwd.join(".git").exists()
            && cwd.join("Cargo.toml").is_file()
            && cwd.join("src/engine/rsi.rs").is_file()
        {
            return Some(cwd);
        }
        return None;
    }
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|_| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    };
    Some(expanded)
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub async fn status(state: &AppState) -> Value {
    let config = config(state);
    let dir = source_dir(state);
    let exists = dir.as_ref().map(|path| path.exists()).unwrap_or(false);
    let node_modules = dir
        .as_ref()
        .map(|path| package_dir(path).join("node_modules").exists())
        .unwrap_or(false);

    let mut name = Value::Null;
    let mut version = Value::Null;
    if let Some(path) = &dir {
        if let Ok(text) = std::fs::read_to_string(path.join("package.json")) {
            if let Ok(package) = serde_json::from_str::<Value>(&text) {
                name = package.get("name").cloned().unwrap_or(Value::Null);
                version = package.get("version").cloned().unwrap_or(Value::Null);
            }
        }
    }

    let (mut git_ok, mut branch, mut commit, mut remote, mut dirty) =
        (false, String::new(), String::new(), String::new(), 0usize);
    if exists {
        if let Some(path) = &dir {
            git_ok = path.join(".git").exists();
            if git_ok {
                branch = git(path, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                commit = git(path, &["rev-parse", "HEAD"])
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                remote = git(path, &["remote", "get-url", "origin"])
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                dirty = git(path, &["status", "--porcelain"])
                    .await
                    .map(|text| text.lines().count())
                    .unwrap_or(0);
            }
        }
    }

    let (preview_status, preview_url, preview_logs) = {
        let mut guard = preview().lock().unwrap();
        if let Some(child) = guard.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    guard.status = "stopped".into();
                    guard.child = None;
                }
                _ => {}
            }
        }
        (
            guard.status.clone(),
            guard.url.clone(),
            guard
                .logs
                .iter()
                .rev()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>(),
        )
    };

    json!({
        "config": {
            "sourceDir": dir.as_ref()
                .map(|path| json!(path.to_string_lossy().to_string()))
                .unwrap_or_else(|| config.get("sourceDir").cloned().unwrap_or(Value::Null)),
            "repoUrl": config.get("repoUrl"),
            "branch": config.get("branch"),
            "previewPort": config.get("previewPort"),
        },
        "exists": exists,
        "nodeModules": node_modules,
        "version": version,
        "name": name,
        "git": git_ok,
        "branch": branch,
        "commit": commit,
        "remote": remote,
        "dirty": dirty,
        "preview": {
            "status": preview_status,
            "url": preview_url,
            "logs": preview_logs,
        },
        "installer": Value::Null,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "installedExe": "",
    })
}

pub async fn connect(state: &AppState, body: &Value) -> Result<Value, String> {
    let mut record = config(state);
    for key in ["sourceDir", "repoUrl", "branch"] {
        if let Some(value) = body.get(key) {
            record[key] = value.clone();
        }
    }
    record["id"] = json!(CONFIG_ID);
    state.store.put(CONFIG_KIND, &record);

    let Some(dir) = source_dir(state) else {
        return Err("소스 폴더가 필요합니다.".into());
    };
    let mut cloned = false;
    if dir.join(".git").exists() {
        // Existing checkout: nothing to clone.
    } else if dir.exists() {
        if let Ok(true) = git(&dir, &["rev-parse", "--is-inside-work-tree"])
            .await
            .map(|v| v.trim() == "true")
        {
            // Fine.
        }
    } else {
        let repo = body
            .get("repoUrl")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or("저장소 URL 이 필요합니다.")?;
        let branch = body
            .get("branch")
            .and_then(|value| value.as_str())
            .unwrap_or("main");
        if let Some(parent) = dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let output = Command::new("git")
            .args(["clone", "--branch", branch, repo])
            .arg(&dir)
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        cloned = true;
    }

    Ok(json!({ "ok": true, "cloned": cloned, "sourceDir": dir.to_string_lossy() }))
}

pub async fn diff(state: &AppState) -> Result<Value, String> {
    let Some(dir) = source_dir(state) else {
        return Err("소스 폴더가 설정되지 않았습니다.".into());
    };
    if !dir.exists() {
        return Err("소스 폴더가 존재하지 않습니다.".into());
    }
    if !dir.join(".git").exists() {
        return Err("Git 저장소가 아닙니다.".into());
    }
    let porcelain = git(&dir, &["status", "--porcelain"]).await?;
    let mut changes = Vec::new();
    for line in porcelain.lines().take(20) {
        if line.len() < 4 {
            continue;
        }
        let status = line[..2].trim().to_string();
        let path = line[3..].trim().to_string();
        let patch = if status.contains('?') {
            "(새 파일)".to_string()
        } else {
            git(&dir, &["diff", "--", &path])
                .await
                .unwrap_or_default()
                .chars()
                .take(20_000)
                .collect::<String>()
        };
        changes.push(json!({ "path": path, "status": status, "patch": patch }));
    }
    Ok(json!({
        "changes": changes,
        "count": porcelain.lines().count(),
        "truncated": porcelain.lines().count() > changes.len(),
    }))
}


static UPDATING: AtomicBool = AtomicBool::new(false);

fn toolchain_path() -> Option<std::ffi::OsString> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home)=std::env::var_os("HOME").map(PathBuf::from) {
        paths.insert(0,home.join(".cargo/bin"));
        paths.insert(0,home.join(".local/bin"));
        paths.insert(0,home.join(".bun/bin"));
    }
    if let Some(value)=std::env::var_os("PATH"){
        paths.extend(std::env::split_paths(&value));
    }
    std::env::join_paths(paths).ok()
}

/// Only source installs managed by the local macOS LaunchAgent can safely
/// rebuild and restart the running process. Packaged apps need signed releases.
fn live_source_root(state: &AppState) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("소스 자동 업데이트는 현재 macOS LaunchAgent 설치에서 지원합니다. 설치형 앱은 릴리스 업데이트를 사용하세요.".into());
    }
    let dir = source_dir(state).ok_or("DOMINION 소스 설치 폴더를 찾지 못했습니다.")?;
    let actual = std::env::current_exe().and_then(|path| path.canonicalize()).map_err(|e| e.to_string())?;
    let expected = dir.join("target/release/dominion-rust").canonicalize().map_err(|_| "현재 실행 중인 서버가 소스 빌드인지 확인하지 못했습니다.")?;
    if actual != expected {
        return Err("이 서버는 소스 설치형 Rust 엔진이 아닙니다. 설치형 앱의 배포 업데이트를 사용하세요.".into());
    }
    if !dir.join(".git").exists() {
        return Err("Git 저장소가 없어 소스 갱신을 진행할 수 없습니다.".into());
    }
    if !dir.join("scripts/install-launch-agent.sh").exists() {
        return Err("macOS 자동 재시작 스크립트가 없습니다.".into());
    }
    Ok(dir)
}

pub async fn update_status(state: &AppState) -> Value {
    let dir = match live_source_root(state) {
        Ok(dir) => dir,
        Err(message) => return json!({"supported":false,"available":false,"reason":message}),
    };
    let branch = git(&dir,&["branch","--show-current"]).await.unwrap_or_default();
    if branch.trim() != "main" {
        return json!({"supported":false,"available":false,"reason":"main 브랜치에서 실행한 경우에만 업데이트할 수 있습니다."});
    }
    let local = git(&dir,&["rev-parse","HEAD"]).await.unwrap_or_default().trim().to_string();
    let dirty = git(&dir,&["status","--porcelain"]).await.unwrap_or_default();
    if !dirty.trim().is_empty() {
        return json!({"supported":false,"available":false,"local":local,"reason":"수정된 소스 파일이 있어 업데이트를 중단했습니다. 변경 내용을 보존한 채 개발 작업에서 정리하세요."});
    }
    let remote = tokio::time::timeout(Duration::from_secs(15),
        Command::new("git").args(["ls-remote","--exit-code","origin","refs/heads/main"])
            .current_dir(&dir).env("GIT_TERMINAL_PROMPT","0").stdin(Stdio::null()).output()
    ).await;
    match remote {
        Ok(Ok(output)) if output.status.success() => {
            let sha = String::from_utf8_lossy(&output.stdout).split_whitespace().next().unwrap_or("").to_string();
            if sha.len()!=40 {
                return json!({"supported":true,"available":false,"local":local,"reason":"원격 버전 정보를 읽지 못했습니다."});
            }
            json!({"supported":true,"available":sha!=local,"local":local,"remote":sha,"running":option_env!("DOMINION_BUILD_COMMIT").unwrap_or("unknown"),"installMode":"source"})
        }
        _ => json!({"supported":true,"available":false,"local":local,"reason":"업데이트 서버에 연결할 수 없습니다. 인터넷 연결을 확인하세요."})
    }
}

pub fn start_update_job(state: AppState) -> Result<Value, String> {
    if UPDATING.swap(true, Ordering::AcqRel) {
        return Err("이미 업데이트를 진행하고 있습니다.".into());
    }
    match start_job(state,"update",json!({})) {
        Some(job) => Ok(job),
        None => { UPDATING.store(false,Ordering::Release); Err("업데이트 작업을 시작하지 못했습니다.".into()) }
    }
}

async fn update_source(state:&AppState) -> Result<(Vec<String>,Value),(Vec<String>,String)> {
    let dir = live_source_root(state).map_err(|e|(vec![],e))?;
    let branch=git(&dir,&["branch","--show-current"]).await.map_err(|e|(vec![],e))?;
    if branch.trim()!="main" { return Err((vec![],"main 브랜치에서만 업데이트할 수 있습니다.".into())); }
    let dirty=git(&dir,&["status","--porcelain"]).await.map_err(|e|(vec![],e))?;
    if !dirty.trim().is_empty(){return Err((vec![],"소스 수정 사항이 있어 작업을 중단했습니다. 파일은 변경하지 않았습니다.".into()));}
    let old=git(&dir,&["rev-parse","HEAD"]).await.map_err(|e|(vec![],e))?.trim().to_string();
    let mut logs=vec![format!("현재 버전: {}",old.chars().take(12).collect::<String>())];
    let commands:Vec<(&str,Vec<&str>,u64)> = vec![
        ("git",vec!["fetch","origin","main"],120),
        ("git",vec!["merge","--ff-only","FETCH_HEAD"],30),
        ("npm",vec!["ci","--prefix","web"],300),
        ("cargo",vec!["build","--locked","--release"],900),
        ("npm",vec!["run","build","--prefix","web"],180)
    ];
    for (program,args,seconds) in commands {
        logs.push(format!("{} {}",program,args.join(" ")));
        let mut command=Command::new(program);
        command.args(&args).current_dir(&dir).stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT","0").kill_on_drop(true);
        if let Some(path)=toolchain_path(){command.env("PATH",path);}
        let output=tokio::time::timeout(Duration::from_secs(seconds),command.output()).await;
        let output=match output {
            Ok(Ok(output))=>output,
            Ok(Err(e))=>return Err((logs,format!("{program} 실행 실패: {e}"))),
            Err(_)=>return Err((logs,format!("{program} 실행 제한 시간을 초과했습니다.")))
        };
        let stdout=String::from_utf8_lossy(&output.stdout);
        let stderr=String::from_utf8_lossy(&output.stderr);
        for output in [&stdout[..],&stderr[..]] {
            let mut recent=output.lines().rev().take(45).map(str::to_string).collect::<Vec<_>>();
            recent.reverse();
            logs.extend(recent);
        }
        if logs.len()>180 { logs=logs.split_off(logs.len()-180); }
        if !output.status.success() {
            return Err((logs,format!("{} 실패 (종료 코드 {:?}). 실행 중인 DOMINION은 유지됩니다.",program,output.status.code())));
        }
    }
    let new=git(&dir,&["rev-parse","HEAD"]).await.unwrap_or_default().trim().to_string();
    logs.push(format!("새 버전 빌드 완료: {}",new.chars().take(12).collect::<String>()));
    Ok((logs,json!({"previous":old,"installed":new,"restartRequired":true})))
}

pub fn start_job(state: AppState, kind: &str, input: Value) -> Option<Value> {
    let mut record = json!({
        "kind": kind,
        "status": "running",
        "logs": [],
        "input": input,
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let job = state.store.put("dev_job", &record)?;
    let id = job.get("id").and_then(|value| value.as_str())?.to_string();
    record["id"] = json!(id);
    let state_clone = state.clone();
    let kind = kind.to_string();
    let input = input.clone();
    let job_id = id.clone();
    tokio::spawn(async move {
        run_job(state_clone, job_id, kind, input).await;
    });
    Some(job)
}

async fn run_job(state: AppState, job_id: String, kind: String, input: Value) {
    let result = if kind=="update" { update_source(&state).await } else { execute(&state, &kind, &input).await };
    let needs_restart = kind=="update" && result.as_ref().ok()
        .and_then(|(_,value)|value["restartRequired"].as_bool()).unwrap_or(false);
    let mut job = state
        .store
        .get("dev_job", &job_id)
        .unwrap_or_else(|| json!({}));
    match result {
        Ok((logs, summary)) => {
            job["status"] = json!("completed");
            job["logs"] = json!(logs);
            job["result"] = summary;
        }
        Err((logs, error)) => {
            job["status"] = json!("failed");
            job["logs"] = json!(logs);
            job["error"] = json!(error);
        }
    }
    job["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("dev_job", &job) {
        state.bus.record("dev_job", &saved);
    }
    state.store.audit("dev.job", json!({ "jobId": job_id, "kind": kind }));
    if kind=="update" {
        UPDATING.store(false,Ordering::Release);
        if needs_restart {
            // The platform installer registers (or refreshes) the LaunchAgent.
            // It executes after the successful job is committed to SQLite.
            if let Some(dir)=source_dir(&state) {
                let script=dir.join("scripts/install-launch-agent.sh");
                let _=Command::new("/bin/zsh").arg(script).current_dir(dir)
                    .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
                    .spawn();
            }
        }
    }
}

async fn execute(
    state: &AppState,
    kind: &str,
    input: &Value,
) -> Result<(Vec<String>, Value), (Vec<String>, String)> {
    let Some(dir) = source_dir(state) else {
        return Err((vec![], "소스 폴더가 설정되지 않았습니다.".into()));
    };
    if !dir.exists() {
        return Err((vec![], "소스 폴더가 존재하지 않습니다.".into()));
    }

    let commands: Vec<Vec<String>> = match kind {
        "setup" => vec![vec![
            npm().into(),
            "install".into(),
            "--no-audit".into(),
            "--no-fund".into(),
        ]],
        "test" => vec![vec![
            npm().into(),
            "run".into(),
            "test".into(),
            "--if-present".into(),
        ]],
        "build" => vec![vec![npm().into(), "run".into(), "build".into()]],
        "package" => vec![vec![
            npm().into(),
            "run".into(),
            "package".into(),
            "--if-present".into(),
        ]],
        "commit" => {
            let message = input
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if message.is_empty() {
                return Err((vec![], "커밋 메시지가 필요합니다.".into()));
            }
            vec![
                vec!["git".into(), "add".into(), "-A".into()],
                vec!["git".into(), "commit".into(), "-m".into(), message],
            ]
        }
        other => return Err((vec![], format!("지원하지 않는 개발 작업입니다: {other}"))),
    };

    let mut logs = Vec::new();
    let mut result = json!({});
    for command in commands {
        let (program, args) = command.split_first().expect("command");
        let working = if program == "git" {
            dir.clone()
        } else {
            package_dir(&dir)
        };
        logs.push(format!("$ {} {}", program, args.join(" ")));
        let output = tokio::time::timeout(
            Duration::from_secs(900),
            Command::new(program)
                .args(args)
                .current_dir(&working)
                .env("CI", "1")
                .stdin(Stdio::null())
                .output(),
        )
        .await;

        let output = match output {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err((logs, error.to_string())),
            Err(_) => return Err((logs, "명령이 15분 후 종료되었습니다.".into())),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        logs.extend(stdout.lines().map(str::to_string));
        logs.extend(stderr.lines().map(str::to_string));
        if logs.len() > 800 {
            let keep = logs.split_off(logs.len() - 800);
            logs = keep;
        }
        if !output.status.success() {
            return Err((
                logs,
                format!("{} 실패 (exit {:?})", kind, output.status.code()),
            ));
        }
        if program == "git" && args.first().map(|value| value.as_str()) == Some("commit") {
            result["oid"] = json!(git(&dir, &["rev-parse", "HEAD"])
                .await
                .unwrap_or_default()
                .trim());
        }
    }
    Ok((logs, result))
}

pub async fn start_preview(state: &AppState) -> Result<Value, String> {
    let Some(dir) = source_dir(state) else {
        return Err("소스 폴더가 설정되지 않았습니다.".into());
    };
    if !dir.exists() {
        return Err("소스 폴더가 존재하지 않습니다.".into());
    }
    let config = config(state);
    let port = config
        .get("previewPort")
        .and_then(|value| value.as_u64())
        .unwrap_or(5174);

    stop_preview();

    // Preview only the React bundle. The Rust backend stays the only server;
    // the Node toolchain is used solely as the Vite dev compiler.
    let package = package_dir(&dir);
    let vite = vite_bin(&package);
    let mut command = if vite.exists() {
        let mut command = Command::new(&vite);
        command.args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--strictPort",
        ]);
        command
    } else {
        let mut command = Command::new(npx());
        command.args([
            "vite",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--strictPort",
        ]);
        command
    };
    command
        .current_dir(&package)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;

    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                push_log(format!("[out] {line}"));
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                push_log(format!("[err] {line}"));
            }
        });
    }

    let url = format!("http://127.0.0.1:{port}");
    {
        let mut guard = preview().lock().unwrap();
        guard.child = Some(child);
        guard.status = "running".into();
        guard.url = url.clone();
        guard.logs.clear();
    }
    Ok(json!({ "status": "running", "url": url }))
}

fn push_log(line: String) {
    let mut guard = preview().lock().unwrap();
    guard.logs.push(line);
    if guard.logs.len() > 200 {
        let excess = guard.logs.len() - 200;
        guard.logs.drain(0..excess);
    }
}

pub fn stop_preview() -> Value {
    let mut guard = preview().lock().unwrap();
    if let Some(child) = guard.child.as_mut() {
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            // npm spawns vite as a grandchild: terminate the whole group.
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &format!("-{pid}")])
                .status();
        }
        let _ = child.start_kill();
    }
    guard.child = None;
    guard.status = "stopped".into();
    guard.url.clear();
    json!({ "status": "stopped", "url": "" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_dir_prefers_root_then_web() {
        let dir = std::env::temp_dir().join(format!("dominion-dev-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("web")).unwrap();
        assert_eq!(package_dir(&dir), dir);

        std::fs::write(dir.join("web/package.json"), "{}").unwrap();
        assert_eq!(package_dir(&dir), dir.join("web"));

        std::fs::write(dir.join("package.json"), "{}").unwrap();
        assert_eq!(package_dir(&dir), dir);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn config_round_trips() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::state::AppState;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-devcfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&cfg.db_path()).unwrap();
        let vault = Vault::open(&cfg.key_path());
        let state = AppState::new(cfg, store, vault);
        let saved = save_config(
            &state,
            &serde_json::json!({
                "sourceDir": "/tmp/source",
                "branch": "dev",
            }),
        );
        assert_eq!(saved["sourceDir"], "/tmp/source");
        assert_eq!(saved["branch"], "dev");
        assert_eq!(config(&state)["sourceDir"], "/tmp/source");
        let _ = std::fs::remove_dir_all(dir);
    }
}
