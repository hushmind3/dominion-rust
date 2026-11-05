use std::path::{Path, PathBuf};

pub(super) fn stop_service() -> Result<(), String> {
    let label = service_label()?;
    let status = std::process::Command::new("launchctl")
        .args(["bootout", &label])
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("launchctl could not stop {label}: {status}"));
    }
    println!("DOMINION stop requested ({label})");
    Ok(())
}
pub(super) fn start_service() -> Result<(), String> {
    let label = service_label()?;
    let started = std::process::Command::new("launchctl")
        .args(["kickstart", "-k", &label])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !started {
        let home = std::env::var_os("HOME").ok_or("User home directory is unavailable")?;
        let plist =
            PathBuf::from(home).join("Library/LaunchAgents/com.hushmind.dominion.rust.plist");
        let user = user_id()?;
        let status = std::process::Command::new("launchctl")
            .args(["bootstrap", &format!("gui/{user}")])
            .arg(plist)
            .status()
            .map_err(|error| error.to_string())?;
        if !status.success() {
            return Err(format!("launchctl could not start {label}: {status}"));
        }
    }
    println!("DOMINION start requested ({label})");
    Ok(())
}
fn user_id() -> Result<String, String> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|error| error.to_string())?;
    let id = String::from_utf8(output.stdout)
        .map_err(|error| error.to_string())?
        .trim()
        .to_string();
    if !output.status.success() || id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("Cannot determine the login user ID".into());
    }
    Ok(id)
}
fn service_label() -> Result<String, String> {
    Ok(format!("gui/{}/com.hushmind.dominion.rust", user_id()?))
}

/// Compatibility launcher for existing development installs. Native releases
/// are produced by the separate Tauri package, not this repository shortcut.
pub(super) fn install_app(target: &Path) -> Result<PathBuf, String> {
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
mod tests {
    use super::*;
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
