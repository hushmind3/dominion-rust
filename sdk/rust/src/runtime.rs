//! Owned subprocess host for Dominion's *own* Rust autonomous runtime.
//! Never substitutes a third-party decision engine, SDK or model policy.
use crate::{DominionClient, DominionError, Result};
use serde_json::Value;
use std::{net::TcpListener, path::PathBuf, process::Stdio, time::Duration};
use tokio::{process::{Child, Command}, time::Instant};

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub binary: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub web_dir: Option<PathBuf>,
    pub port: Option<u16>,
    pub startup_timeout: Duration,
}

impl RuntimeConfig {
    pub fn new(binary: impl Into<PathBuf>, workspace: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(), workspace: workspace.into(),
            data_dir: data_dir.into(), web_dir: None, port: None,
            startup_timeout: Duration::from_secs(30),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }
}

#[derive(Debug)]
pub struct DominionRuntime {
    child: Option<Child>,
    client: DominionClient,
    id: String,
    port: u16,
}

impl DominionRuntime {
    /// Spawn an independently isolated native process and verify exact
    /// instance-id, PID, workspace and SQLite root before returning a client.
    /// A running server on an occupied port is NEVER silently adopted.
    pub async fn spawn(config: RuntimeConfig) -> Result<Self> {
        if config.startup_timeout < Duration::from_secs(1) {
            return Err(DominionError::Configuration(
                "startup timeout must be at least one second".into()));
        }
        let binary = config.binary.canonicalize().map_err(|e|
            DominionError::Configuration(format!("binary path: {e}")))?;
        if !binary.is_file() {
            return Err(DominionError::Configuration("binary is not a file".into()));
        }
        let workspace = config.workspace.canonicalize().map_err(|e|
            DominionError::Configuration(format!("workspace path: {e}")))?;
        if !workspace.is_dir() {
            return Err(DominionError::Configuration("workspace is not a directory".into()));
        }
        std::fs::create_dir_all(&config.data_dir)?;
        let data_dir = config.data_dir.canonicalize()?;
        let port = match config.port {
            Some(0) => return Err(DominionError::Configuration("port 0 cannot be addressed".into())),
            Some(port) => port,
            None => {
                let reservation = TcpListener::bind(("127.0.0.1", 0))?;
                reservation.local_addr()?.port()
            }
        };
        let id = uuid::Uuid::new_v4().to_string();
        let token = format!("{}{}",uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple());
        let mut command = Command::new(binary);
        command.current_dir(&workspace)
            .env("DOMINION_PORT", port.to_string())
            .env("DOMINION_DATA", &data_dir)
            .env("DOMINION_WORKSPACE", &workspace)
            .env("DOMINION_ENGINE_INSTANCE", &id)
            .env("DOMINION_ADMIN_TOKEN", &token)
            .env("DOMINION_DESKTOP_PARENT","1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(dir) = config.web_dir { command.env("DOMINION_WEB",dir); }
        let mut child = command.spawn()?;
        let pid = child.id().ok_or_else(||
            DominionError::Runtime("native runtime process has no PID".into()))?;
        let base = format!("http://127.0.0.1:{port}");
        let client = DominionClient::new(&base)?.with_admin_token(&token)?;
        let probe = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(300))
            .timeout(Duration::from_secs(2))
            .build()?;
        let started=Instant::now();
        loop {
            if let Some(status)=child.try_wait()? {
                return Err(DominionError::Runtime(format!(
                    "Dominion process exited before readiness: {status}")));
            }
            if started.elapsed() >= config.startup_timeout {
                return Err(DominionError::Runtime("native runtime startup timeout".into()));
            }
            let response=probe.get(format!("{base}/api/benchmark/identity")).send().await;
            if let Ok(response) = response {
                if let Ok(identity) = response.json::<Value>().await {
                    if identity["instanceId"].as_str() == Some(id.as_str())
                        && identity["pid"].as_u64() == Some(u64::from(pid))
                        && identity["workspace"].as_str() == workspace.to_str()
                        && identity["dataRoot"].as_str() == data_dir.to_str()
                    {
                        return Ok(Self { child:Some(child), client, id, port });
                    }
                    // Refuse foreign or stale server identity; never execute
                    // any write against a guessed localhost process.
                    return Err(DominionError::Runtime(
                        "native runtime identity mismatch; possible occupied port".into()));
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    pub fn client(&self) -> DominionClient { self.client.clone() }
    pub fn port(&self) -> u16 { self.port }
    pub fn instance_id(&self) -> &str { &self.id }

    /// Close the parent's stdin. The engine's existing native EOF listener
    /// drains the listener; if it does not exit, terminate the owned child.
    /// Durable job/checkpoint records remain on disk.
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            drop(child.stdin.take());
            match tokio::time::timeout(Duration::from_secs(5),child.wait()).await {
                Ok(Ok(_)) => (),
                Ok(Err(error)) => return Err(DominionError::Io(error)),
                Err(_) => {
                    child.kill().await?;
                    child.wait().await?;
                }
            }
        }
        Ok(())
    }
}
