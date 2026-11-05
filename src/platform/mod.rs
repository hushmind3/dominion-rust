//! Explicit platform integration and compatibility launch commands. The Rust
//! server and native Tauri host do not depend on a browser or launchd at runtime.
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
mod macos;

/// Handle legacy control flags before opening a database or starting a runtime.
pub fn handle_control(args: &[String]) -> Result<bool, String> {
    if args.iter().any(|arg| arg == "--stop") {
        #[cfg(target_os = "macos")]
        {
            macos::stop_service()?;
            return Ok(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("--stop requires the macOS launchd development service; close the desktop app or stop your own server service on this platform.".into());
        }
    }
    if args.iter().any(|arg| arg == "--start") {
        #[cfg(target_os = "macos")]
        {
            macos::start_service()?;
            return Ok(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("--start requires the macOS launchd development service; use the native desktop app or run the Rust server on this platform.".into());
        }
    }
    if let Some(index) = args.iter().position(|arg| arg == "--install-app") {
        let target = args
            .get(index + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/Applications/DOMINION.app"));
        #[cfg(target_os = "macos")]
        {
            let path = macos::install_app(&target)?;
            println!("installed {}", path.display());
            println!("double-click it from Finder/Applications");
            return Ok(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = target;
            return Err("--install-app is the macOS compatibility launcher. Build the native desktop installer with scripts/build-desktop.mjs.".into());
        }
    }
    Ok(false)
}

/// Preserve the legacy Finder repository handoff, while native sidecars stay
/// headless even when explicitly pointed at an old development resource root.
pub fn prepare_launch(args: &[String]) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == "--home") {
        if let Some(home) = args.get(index + 1) {
            std::env::set_var("DOMINION_HOME", home);
        }
    }
    let bundled = crate::config::bundle_home().is_some();
    let native = std::env::var_os("DOMINION_NATIVE").is_some()
        || std::env::var_os("DOMINION_DESKTOP_PARENT").is_some();
    if !native {
        reexec_repo_binary();
    }
    should_open_browser(args, bundled, native)
}
fn should_open_browser(args: &[String], bundled: bool, native: bool) -> bool {
    !native && (bundled || args.iter().any(|arg| arg == "--open"))
}

fn reexec_repo_binary() {
    if std::env::var_os("DOMINION_NO_REEXEC").is_some() {
        return;
    }
    let Some(home) = crate::config::bundle_home() else {
        return;
    };
    let repo = home.join("target/release/dominion-rust");
    if !repo.is_file() || std::env::current_exe().ok().as_deref() == Some(repo.as_path()) {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = repo_launch_command(&home).exec();
    }
}
fn repo_launch_command(home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(home.join("target/release/dominion-rust"));
    command
        .arg("--open")
        .env("DOMINION_HOME", home)
        .current_dir(home);
    command
}

/// Used only by explicit --open or the old development launcher. The Tauri
/// host displays its own webview and never reaches this compatibility helper.
pub fn open_url(port: u16) {
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

#[cfg(test)]
mod tests {
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
    #[test]
    fn native_host_sidecars_never_open_an_external_browser() {
        let open = vec!["--open".to_string()];
        assert!(should_open_browser(&open, false, false));
        assert!(should_open_browser(&[], true, false));
        assert!(!should_open_browser(&open, true, true));
        assert!(!should_open_browser(&[], false, false));
    }
}
