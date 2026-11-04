use std::path::PathBuf;

/// Runtime configuration. Everything is resolved once at startup so the rest
/// of the application never touches the environment directly.
#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub workspace: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = PathBuf::from(env_or("DOMINION_DATA", "runtime"));
        let web_dir = PathBuf::from(env_or("DOMINION_WEB", "web/dist"));
        let workspace = std::env::var("DOMINION_WORKSPACE")
            .map(PathBuf::from)
            .ok()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            port: env_or("DOMINION_PORT", "4317").parse().unwrap_or(4317),
            data_dir,
            web_dir,
            workspace,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("dominion.sqlite")
    }

    pub fn key_path(&self) -> PathBuf {
        self.data_dir.join("secret.key")
    }
}

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string())
}
