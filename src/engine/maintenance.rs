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
            path.extension().and_then(|value| value.to_str()) == Some("sqlite")
                && path
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

pub fn backup_now(state: &AppState) -> Result<PathBuf, String> {
    let directory = state.config.data_dir.join("backups");
    let name = format!(
        "dominion-{}.sqlite",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = directory.join(name);
    state.store.backup_to(&path)?;
    prune_backups(&directory, keep_count());
    Ok(path)
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
            match backup_now(&state) {
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
