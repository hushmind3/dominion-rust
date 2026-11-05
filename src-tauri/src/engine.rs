use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopSettings {
    #[serde(default)]
    last_port: Option<u16>,
    #[serde(default)]
    workspace: Option<PathBuf>,
    #[serde(default)]
    source_workspace: Option<PathBuf>,
}

/// Only a process spawned by this host is owned and stopped by it. A verified
/// user's existing gateway is borrowed and is never killed when a window quits.
pub struct EngineConnection {
    pub origin: String,
    child: Option<Child>,
}
impl Drop for EngineConnection {
    fn drop(&mut self) {
        self.stop();
    }
}
impl EngineConnection {
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(25));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn connect(resources: &Path, app_data: &Path) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_millis(400))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let standalone = std::env::var("DOMINION_DESKTOP_STANDALONE").ok().as_deref() == Some("1");
        if !standalone && health(&client, "http://127.0.0.1:4317", None) {
            return Ok(Self {
                origin: "http://127.0.0.1:4317".into(),
                child: None,
            });
        }
        fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
        let settings_path = app_data.join("desktop.json");
        let mut settings: DesktopSettings = fs::read(&settings_path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        let workspace = std::env::var_os("DOMINION_WORKSPACE")
            .map(PathBuf::from)
            .or_else(|| settings.workspace.clone())
            .unwrap_or_else(|| app_data.join("workspace"));
        fs::create_dir_all(&workspace)
            .map_err(|e| format!("Cannot open workspace {}: {e}", workspace.display()))?;
        let source_workspace = std::env::var_os("DOMINION_SOURCE_WORKSPACE")
            .map(PathBuf::from)
            .or_else(|| settings.source_workspace.clone());
        // Editing source is optional. Moving/deleting a development checkout
        // must not break an otherwise self-contained installed application.
        let source_workspace = source_workspace.filter(|source| source.is_dir());
        let web = resources.join("web");
        if !web.join("index.html").is_file() {
            return Err(format!("Bundled interface is missing: {}", web.display()));
        }
        let engine = find_engine(resources)?;
        let runtime = app_data.join("runtime");
        fs::create_dir_all(&runtime).map_err(|e| e.to_string())?;
        let logs = app_data.join("logs");
        fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join("engine.log"))
            .map_err(|e| e.to_string())?;
        let listener = preferred_listener(settings.last_port)?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        drop(listener);
        let instance = uuid::Uuid::new_v4().to_string();
        let mut command = Command::new(&engine);
        command
            .current_dir(&workspace)
            .env("DOMINION_HOME", resources)
            .env("DOMINION_DATA", &runtime)
            .env("DOMINION_WEB", &web)
            .env("DOMINION_ASSETS", resources.join("assets"))
            .env("DOMINION_WORKSPACE", &workspace)
            .env("DOMINION_PORT", port.to_string())
            .env("DOMINION_NO_REEXEC", "1")
            .env("DOMINION_NATIVE", "1")
            .env("DOMINION_ENGINE_INSTANCE", &instance)
            .env("DOMINION_DESKTOP_PARENT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(log));
        if let Some(source) = source_workspace {
            command.env("DOMINION_SOURCE_WORKSPACE", source);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW, no console flash.
        }
        let child = command
            .spawn()
            .map_err(|e| format!("Cannot start bundled Rust engine {}: {e}", engine.display()))?;
        let mut connection = Self {
            origin: format!("http://127.0.0.1:{port}"),
            child: Some(child),
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if health(&client, &connection.origin, Some(&instance)) {
                settings.last_port = Some(port);
                let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
                fs::write(&settings_path, bytes).map_err(|e| e.to_string())?;
                return Ok(connection);
            }
            if let Some(status) = connection
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
            {
                return Err(format!(
                    "Rust engine stopped during startup ({status}). Check {}.",
                    logs.join("engine.log").display()
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "Bundled Rust engine did not become ready. Check {}.",
            logs.join("engine.log").display()
        ))
    }
}

fn preferred_listener(port: Option<u16>) -> Result<TcpListener, String> {
    if let Some(port) = port.filter(|port| *port >= 1024) {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            return Ok(listener);
        }
    }
    TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())
}
fn health(client: &Client, origin: &str, instance: Option<&str>) -> bool {
    client
        .get(format!("{origin}/api/health"))
        .send()
        .ok()
        .filter(|response| response.status().is_success())
        .and_then(|response| response.json::<Value>().ok())
        .is_some_and(|health| is_dominion(&health, instance))
}
fn is_dominion(value: &Value, instance: Option<&str>) -> bool {
    value["ok"] == true
        && value["service"] == "dominion"
        && value["runtime"] == "rust"
        && value["engine"] == "rust"
        && instance.is_none_or(|expected| value["instanceId"].as_str() == Some(expected))
}
fn find_engine(resources: &Path) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "dominion-engine.exe"
    } else {
        "dominion-engine"
    };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    for candidate in [
        executable.parent().unwrap_or(resources).join(name),
        resources.join(name),
        resources.join("binaries").join(name),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    // Cargo/Tauri development keeps the architecture suffix before bundling.
    #[cfg(debug_assertions)]
    {
        let binaries = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        if let Ok(entries) = fs::read_dir(binaries) {
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().to_string();
                if file.starts_with(&format!("dominion-engine-{}-", std::env::consts::ARCH))
                    && entry.path().is_file()
                {
                    return Ok(entry.path());
                }
            }
        }
    }
    Err("Bundled Rust engine is missing. Build with scripts/build-desktop.mjs.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn existing_server_must_be_dominion_and_owned_server_must_match_instance() {
        let healthy = json!({"ok":true,"service":"dominion","runtime":"rust","engine":"rust","instanceId":"ours"});
        assert!(is_dominion(&healthy, None));
        assert!(is_dominion(&healthy, Some("ours")));
        assert!(!is_dominion(&healthy, Some("other")));
        assert!(!is_dominion(
            &json!({"ok":true,"service":"other","runtime":"rust","engine":"rust"}),
            None
        ));
    }
    #[test]
    fn occupied_remembered_port_uses_another_available_port() {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert_ne!(
            preferred_listener(Some(port))
                .unwrap()
                .local_addr()
                .unwrap()
                .port(),
            port
        );
    }
    #[test]
    fn settings_keep_source_workspace_optional_and_paths_with_spaces() {
        let config: DesktopSettings =
            serde_json::from_value(json!({"workspace":"/tmp/DOMINION workspace/한글"})).unwrap();
        assert!(config.source_workspace.is_none());
        assert_eq!(
            config.workspace.unwrap(),
            PathBuf::from("/tmp/DOMINION workspace/한글")
        );
    }
    #[cfg(unix)]
    #[test]
    fn owned_engine_receives_eof_and_exits_before_forced_termination() {
        let dir =
            std::env::temp_dir().join(format!("dominion-native-child-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let child = Command::new("/bin/sh")
            .args(["-c", "cat >/dev/null; printf graceful > stopped.txt"])
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut connection = EngineConnection {
            origin: "http://127.0.0.1:1".into(),
            child: Some(child),
        };
        connection.stop();
        assert_eq!(
            fs::read_to_string(dir.join("stopped.txt")).unwrap(),
            "graceful"
        );
        assert!(connection.child.is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
