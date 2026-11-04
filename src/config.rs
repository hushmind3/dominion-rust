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
        // A bundled .app stores its repo path in Contents/Resources/home so the
        // binary works no matter what cwd Finder launches it with.
        let home = std::env::var("DOMINION_HOME")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .or_else(bundle_home);

        let data_dir = std::env::var("DOMINION_DATA")
            .map(PathBuf::from)
            .ok()
            .or_else(|| home.as_ref().map(|root| root.join("runtime")))
            .unwrap_or_else(|| PathBuf::from("runtime"));

        let web_dir = std::env::var("DOMINION_WEB")
            .map(PathBuf::from)
            .ok()
            .or_else(|| home.as_ref().map(|root| root.join("web/dist")))
            .unwrap_or_else(|| PathBuf::from("web/dist"));

        let workspace = std::env::var("DOMINION_WORKSPACE")
            .map(PathBuf::from)
            .ok()
            .or_else(|| home.clone())
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

pub fn bundle_home() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let resources = exe.parent()?.parent()?.join("Resources/home");
    let text = std::fs::read_to_string(resources).ok()?;
    let path = PathBuf::from(text.trim());
    path.exists().then_some(path)
}
