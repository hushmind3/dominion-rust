mod api;
mod config;
mod db;
mod engine;
mod events;
mod state;
mod vault;

use config::Config;
use db::Store;
use serde_json::json;
use state::AppState;
use std::path::{Path, PathBuf};
use vault::Vault;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--stop") {
        let label = format!("gui/{}/com.hushmind.dominion.rust", user_id());
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &label])
            .status();
        println!("DOMINION stop requested ({label})");
        return;
    }
    if args.iter().any(|arg| arg == "--start") {
        let label = format!("gui/{}/com.hushmind.dominion.rust", user_id());
        let started = std::process::Command::new("launchctl")
            .args(["kickstart", "-k", &label])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !started {
            let plist = format!(
                "{}/Library/LaunchAgents/com.hushmind.dominion.rust.plist",
                std::env::var("HOME").unwrap_or_default()
            );
            let _ = std::process::Command::new("launchctl")
                .args(["bootstrap", &format!("gui/{}", user_id()), &plist])
                .status();
        }
        println!("DOMINION start requested ({label})");
        return;
    }
    if args.iter().any(|arg| arg == "--install-app") {
        let target = args
            .iter()
            .position(|arg| arg == "--install-app")
            .and_then(|index| args.get(index + 1))
            .cloned()
            .unwrap_or_else(|| "/Applications/DOMINION.app".to_string());
        match install_app(Path::new(&target)) {
            Ok(path) => {
                println!("installed {}", path.display());
                println!("double-click it from Finder/Applications");
                return;
            }
            Err(error) => {
                eprintln!("install failed: {error}");
                std::process::exit(1);
            }
        }
    }

    if let Some(index) = args.iter().position(|arg| arg == "--home") {
        if let Some(home) = args.get(index + 1) {
            std::env::set_var("DOMINION_HOME", home);
        }
    }
    let bundled = config::bundle_home().is_some();
    reexec_repo_binary();
    let open_browser = bundled || args.iter().any(|arg| arg == "--open");

    let config = Config::from_env();
    if let Err(error) = std::fs::create_dir_all(&config.data_dir) {
        eprintln!(
            "cannot create data directory {}: {error}",
            config.data_dir.display()
        );
        std::process::exit(1);
    }

    let store = match Store::open(&config.db_path()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "cannot open database {}: {error}",
                config.db_path().display()
            );
            std::process::exit(1);
        }
    };
    let vault = Vault::open(&config.key_path());
    let state = AppState::new(config.clone(), store, vault);

    // Migrate keys written by early builds that kept `apiKey` inline.
    for provider in state.store.all("provider") {
        let Some(plaintext) = provider
            .get("apiKey")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(id) = provider.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        if !state.store.has_secret(&format!("provider:{id}")) {
            state.save_secret(&format!("provider:{id}"), plaintext);
        }
        let mut clean = provider.clone();
        if let serde_json::Value::Object(ref mut map) = clean {
            map.remove("apiKey");
        }
        state.store.put("provider", &clean);
        println!("migrated plaintext provider key into the vault: {id}");
    }

    // Early Rust builds stored collection routes under their plural URL
    // segment. Move them to the singular kinds the engine uses.
    for (plural, singular) in [
        ("memories", "memory"),
        ("skills", "skill"),
        ("automations", "automation"),
        ("workflows", "workflow"),
        ("rooms", "room"),
        ("channels", "channel"),
        ("jobs", "job"),
        ("executions", "execution"),
        ("goals", "goal"),
    ] {
        for record in state.store.all(plural) {
            if let Some(id) = record.get("id").and_then(|value| value.as_str()) {
                state.store.put(singular, &record);
                state.store.remove(plural, id);
            }
        }
    }

    // A restart cannot resume an in-flight approval prompt. Mark them so the
    // UI never offers a stale "approve" that would do nothing.
    for approval in state.store.all("approval") {
        if approval.get("status").and_then(|value| value.as_str()) == Some("pending") {
            let mut stale = approval.clone();
            stale["status"] = json!("interrupted");
            stale["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            state.store.put("approval", &stale);
        }
    }

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            // Double-clicking the app while it already runs just opens the UI.
            println!("DOMINION already running on port {}", config.port);
            if open_browser {
                open_url(config.port);
            }
            return;
        }
        Err(error) => {
            eprintln!("cannot bind 127.0.0.1:{}: {error}", config.port);
            std::process::exit(1);
        }
    };

    engine::jobs::recover(&state);
    engine::jobs::start(state.clone());
    engine::discord::start(state.clone());
    let app = api::router(state);

    println!(
        "DOMINION (rust engine) listening on http://127.0.0.1:{}",
        config.port
    );
    println!("  data      : {}", config.data_dir.display());
    println!("  web       : {}", config.web_dir.display());
    println!("  workspace : {}", config.workspace.display());

    if open_browser {
        open_url(config.port);
    }

    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("server error: {error}");
    }
}

fn user_id() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|| "501".to_string())
}

/// A bundled app is only a bootstrap: hand over to the current repo binary so
/// rebuilds take effect without reinstalling the .app.
fn reexec_repo_binary() {
    if std::env::var("DOMINION_NO_REEXEC").is_ok() {
        return;
    }
    let Some(home) = config::bundle_home() else {
        return;
    };
    let repo = home.join("target/release/dominion-rust");
    if !repo.is_file() {
        return;
    }
    if std::env::current_exe().ok().as_deref() == Some(repo.as_path()) {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = repo_launch_command(&home).exec();
    }
}

/// Finder does not supply the repository working directory. After exec the
/// current executable is outside the app bundle, so bundle_home can no longer
/// recover it; pass that resolved path through explicitly.
fn repo_launch_command(home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(home.join("target/release/dominion-rust"));
    command
        .arg("--open")
        .env("DOMINION_HOME", home)
        .current_dir(home);
    command
}

/// Opens the UI in Chrome when available, otherwise the default browser.
fn open_url(port: u16) {
    let url = format!("http://127.0.0.1:{port}");
    #[cfg(target_os = "macos")]
    {
        let chrome = std::process::Command::new("open")
            .args(["-a", "Google Chrome", &url])
            .status();
        if chrome.map(|status| !status.success()).unwrap_or(true) {
            let _ = std::process::Command::new("open").arg(&url).status();
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).status();
    }
}

/// Builds a macOS app bundle whose executable *is* this Rust binary, so
/// double-clicking starts the server without a shell or AppleScript wrapper.
#[cfg(not(target_os = "macos"))]
fn install_app(_target: &Path) -> Result<PathBuf, String> {
    Err("--install-app 는 macOS 전용입니다.".into())
}

#[cfg(target_os = "macos")]
fn install_app(target: &Path) -> Result<PathBuf, String> {
    let home = std::env::current_dir().map_err(|error| error.to_string())?;
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let macos = target.join("Contents/MacOS");
    let resources = target.join("Contents/Resources");
    std::fs::create_dir_all(&macos).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&resources).map_err(|error| error.to_string())?;

    let binary = macos.join("DOMINION");
    std::fs::copy(&exe, &binary).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755));
    }

    std::fs::write(resources.join("home"), home.to_string_lossy().as_bytes())
        .map_err(|error| error.to_string())?;

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>DOMINION</string>
<key>CFBundleDisplayName</key><string>DOMINION</string>
<key>CFBundleIdentifier</key><string>com.hushmind.dominion.rust</string>
<key>CFBundleExecutable</key><string>DOMINION</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{}</string>
<key>CFBundleVersion</key><string>{}</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#,
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(target.join("Contents/Info.plist"), plist).map_err(|error| error.to_string())?;
    Ok(target.to_path_buf())
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn finder_reexec_keeps_repo_home_and_working_directory_with_spaces() {
        let home = PathBuf::from("/tmp/DOMINION project with spaces/러스트");
        let command = repo_launch_command(&home);
        assert_eq!(
            command.get_program(),
            home.join("target/release/dominion-rust")
        );
        assert_eq!(command.get_current_dir(), Some(home.as_path()));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("--open")]
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "DOMINION_HOME" && value == Some(home.as_os_str())));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reinstall_replaces_old_bundled_executable_and_home() {
        let root =
            std::env::temp_dir().join(format!("dominion app reinstall {}", uuid::Uuid::new_v4()));
        let target = root.join("DOMINION.app");
        let macos = target.join("Contents/MacOS");
        let resources = target.join("Contents/Resources");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(macos.join("DOMINION"), b"old app executable").unwrap();
        std::fs::write(resources.join("home"), b"/old/repository").unwrap();
        install_app(&target).unwrap();
        assert_eq!(
            std::fs::read(macos.join("DOMINION")).unwrap(),
            std::fs::read(std::env::current_exe().unwrap()).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(resources.join("home")).unwrap(),
            std::env::current_dir().unwrap().to_string_lossy()
        );
        assert!(std::fs::read_to_string(target.join("Contents/Info.plist"))
            .unwrap()
            .contains("<string>DOMINION</string>"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
