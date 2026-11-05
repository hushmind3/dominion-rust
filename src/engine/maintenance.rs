//! Scheduled maintenance: bounded database backups.
use crate::state::AppState;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn interval_seconds() -> u64 {
    std::env::var("DOMINION_BACKUP_INTERVAL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3_600)
}

fn keep_count() -> usize {
    std::env::var("DOMINION_BACKUP_KEEP")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1)
}

/// Removes the oldest snapshots beyond `keep`. Returns the removed paths.
pub fn prune_backups(directory: &Path, keep: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("sqlite" | "enc")
            ) && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("dominion-"))
        })
        .collect::<Vec<_>>();
    files.sort();
    if files.len() <= keep {
        return Vec::new();
    }
    let remove_count = files.len() - keep;
    files
        .into_iter()
        .take(remove_count)
        .filter(|path| std::fs::remove_file(path).is_ok())
        .collect()
}

fn offsite_directory() -> Option<PathBuf> {
    std::env::var("DOMINION_BACKUP_OFFSITE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Creates an encrypted snapshot. The plaintext SQLite file exists only as a
/// short-lived temp during VACUUM INTO and is removed before returning.
pub fn create_snapshot(state: &AppState) -> Result<PathBuf, String> {
    let directory = state.config.data_dir.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let temp = directory.join(format!(".backup-{}.tmp", uuid::Uuid::new_v4()));
    state.store.backup_to(&temp)?;
    let bytes = std::fs::read(&temp).map_err(|error| error.to_string())?;
    let ring = crate::engine::backup_keys::KeyRing::load_or_create(&state.config.data_dir)?;
    // The snapshot carries the vault key that its provider secrets were sealed
    // with, so restoring an older snapshot remains decryptable after rotation.
    let mut container = b"DOMVAULT1".to_vec();
    container.extend_from_slice(&state.vault.primary_key());
    container.extend_from_slice(&bytes);
    let sealed = ring.encrypt(&container)?;
    let path = directory.join(format!("dominion-{stamp}.sqlite.enc"));
    std::fs::write(&path, &sealed).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(&temp);
    if let Some(offsite) = offsite_directory() {
        std::fs::create_dir_all(&offsite).map_err(|error| error.to_string())?;
        std::fs::copy(&path, offsite.join(path.file_name().unwrap_or_default()))
            .map_err(|error| error.to_string())?;
        prune_backups(&offsite, keep_count());
    }
    prune_backups(&directory, keep_count());
    state.store.audit(
        "backup.encrypted",
        serde_json::json!({
            "path": path.to_string_lossy(),
            "bytes": sealed.len(),
            "offsite": offsite_directory().map(|dir| dir.to_string_lossy().to_string()),
        }),
    );
    Ok(path)
}

/// Scheduled path: create locally, then best-effort remote transfer. A remote
/// failure never deletes the local snapshot.
pub async fn backup_now(state: &AppState) -> Result<PathBuf, String> {
    let path = create_snapshot(state)?;
    let _ = crate::engine::remote_backup::upload_if_configured(state, &path).await;
    Ok(path)
}

/// API path: report the remote transfer outcome to the caller.
pub async fn backup_now_with_remote(
    state: &AppState,
) -> Result<(PathBuf, Option<crate::engine::remote_backup::RemoteReport>), String> {
    let path = create_snapshot(state)?;
    let report = crate::engine::remote_backup::upload_if_configured(state, &path).await;
    Ok((path, report))
}

/// Starts the background backup loop. `DOMINION_BACKUP_INTERVAL_SECS=0`
/// disables it; the default is hourly with five retained snapshots.
pub fn start(state: AppState) {
    let seconds = interval_seconds();
    if seconds == 0 {
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(seconds));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if state.maintenance_mode() {
                continue;
            }
            match backup_now(&state).await {
                Ok(path) => state.store.audit(
                    "backup.created",
                    serde_json::json!({"path": path.to_string_lossy()}),
                ),
                Err(error) => state.store.audit(
                    "backup.failed",
                    serde_json::json!({"error": error}),
                ),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-backup-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[test]
    fn encrypted_backup_round_trips_and_reopens_as_sqlite() {
        let (state, dir) = state();
        state.store.put(
            "provider",
            &serde_json::json!({"id":"p1","name":"encrypted backup","baseUrl":"http://x/v1"}),
        );
        let path = create_snapshot(&state).unwrap();
        assert_eq!(path.extension().and_then(|value| value.to_str()), Some("enc"));
        let sealed = std::fs::read(&path).unwrap();
        assert!(!sealed.starts_with(b"SQLite format 3"));
        let plain =
            crate::engine::backup_keys::decrypt_backup(&state, &sealed).expect("decrypt");
        assert!(plain.starts_with(b"DOMVAULT1"));
        let database = &plain[41..];
        assert!(database.starts_with(b"SQLite format 3"));
        let restored_path = dir.join("restored.sqlite");
        std::fs::write(&restored_path, database).unwrap();
        let restored = Store::open(&restored_path).unwrap();
        assert_eq!(
            restored.get("provider", "p1").unwrap()["name"],
            "encrypted backup"
        );
        // The plaintext temp used for VACUUM INTO is removed.
        let leftovers = std::fs::read_dir(dir.join("backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    == Some("tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_keeps_the_newest_snapshots_only() {
        let dir = std::env::temp_dir().join(format!("dominion-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for index in 1..=5 {
            std::fs::write(dir.join(format!("dominion-2026110{index}-000000.sqlite")), b"x")
                .unwrap();
        }
        std::fs::write(dir.join("other.sqlite"), b"x").unwrap();
        let removed = prune_backups(&dir, 2);
        assert_eq!(removed.len(), 3);
        let remaining = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|name| name.starts_with("dominion-"))
            })
            .count();
        assert_eq!(remaining, 2);
        assert!(dir.join("other.sqlite").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
