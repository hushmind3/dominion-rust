//! H10 data-key (vault) rotation.
//!
//! Provider secrets and legacy `.enc` backups are migrated to a new
//! `data/secret.key` under maintenance mode. The old key file is copied aside
//! first; on any failure the old key remains available and the new key is not
//! finalized.
use crate::state::AppState;
use rand::RngCore;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug)]
pub struct RotationReport {
    pub secrets_reencrypted: usize,
    pub legacy_backups_migrated: usize,
    pub previous_keys: usize,
    pub old_key_file: Option<PathBuf>,
}

fn legacy_backups(state: &AppState) -> Vec<PathBuf> {
    let directory = state.config.data_dir.join("backups");
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("enc"))
        .filter(|path| {
            std::fs::read(path)
                .map(|bytes| !crate::engine::backup_keys::KeyRing::is_versioned(&bytes))
                .unwrap_or(false)
        })
        .collect()
}

/// Verifies that every sealed secret in a snapshot DB can be decrypted with
/// the key that would be installed by a restore. Used before touching the live
/// database/key so an unavailable old key fails safely.
pub fn verify_snapshot_secrets(
    key: &[u8; 32],
    database: &[u8],
    temp_dir: &std::path::Path,
) -> Result<(), String> {
    let path = temp_dir.join(format!(".restore-verify-{}.sqlite", uuid::Uuid::new_v4()));
    std::fs::write(&path, database).map_err(|error| error.to_string())?;
    let result = (|| -> Result<(), String> {
        let store = crate::db::Store::open(&path).map_err(|error| error.to_string())?;
        for (name, sealed) in store.all_secrets() {
            if crate::vault::Vault::open_secret_with_key(key, &sealed).is_none() {
                return Err(format!("secret {name} cannot be decrypted by the snapshot key"));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&path);
    result
}

pub fn rotate(state: &AppState, keep_old: bool) -> Result<RotationReport, String> {
    // Decrypt everything before touching the key file. An unreadable secret
    // aborts the rotation with zero changes.
    let secrets = state.store.all_secrets();
    let mut plaintexts = Vec::new();
    for (name, sealed) in &secrets {
        let plain = state
            .vault
            .open_secret(sealed)
            .ok_or_else(|| format!("secret {name} cannot be decrypted with the current key"))?;
        plaintexts.push((name.clone(), plain));
    }

    // Move legacy snapshots to the versioned backup keyring before the vault
    // key changes.
    let ring = crate::engine::backup_keys::KeyRing::load_or_create(&state.config.data_dir)?;
    let mut migrated = 0;
    for path in legacy_backups(state) {
        let sealed = std::fs::read(&path).map_err(|error| error.to_string())?;
        let plain = state
            .vault
            .open_bytes(&sealed)
            .ok_or_else(|| format!("legacy backup key unavailable for {}", path.display()))?;
        let resealed = ring.encrypt(&plain)?;
        let temp = path.with_extension("enc.tmp");
        std::fs::write(&temp, resealed).map_err(|error| error.to_string())?;
        std::fs::rename(&temp, &path).map_err(|error| error.to_string())?;
        migrated += 1;
    }

    let old_key_path = state.vault.key_path().to_path_buf();
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let old_backup = state
        .config
        .data_dir
        .join(format!("secret.key.old-{stamp}"));
    std::fs::copy(&old_key_path, &old_backup).map_err(|error| error.to_string())?;

    let mut new_key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut new_key);
    state.vault.install_primary(new_key);
    let temp = old_key_path.with_extension("key.new");
    state
        .vault
        .write_key_file(&temp)
        .map_err(|error| error.to_string())?;
    std::fs::rename(&temp, &old_key_path).map_err(|error| error.to_string())?;

    for (name, plain) in &plaintexts {
        let sealed = state.vault.seal(plain);
        state.store.set_secret(name, &sealed);
    }
    for (name, _) in &plaintexts {
        let Some(sealed) = state.store.get_secret(name) else {
            return Err(format!(
                "secret {name} disappeared during rotation; old key retained"
            ));
        };
        if state.vault.open_with_primary(&sealed).is_none() {
            return Err(format!(
                "secret {name} failed primary-key verification; old key retained"
            ));
        }
    }

    let retained_path = if keep_old {
        Some(old_backup.clone())
    } else {
        state.vault.drop_previous();
        let _ = std::fs::remove_file(&old_backup);
        None
    };
    let report = RotationReport {
        secrets_reencrypted: plaintexts.len(),
        legacy_backups_migrated: migrated,
        previous_keys: state.vault.previous_count(),
        old_key_file: retained_path,
    };
    state.store.audit(
        "vault.key_rotated",
        json!({
            "secrets": report.secrets_reencrypted,
            "legacyBackups": report.legacy_backups_migrated,
            "keepOld": keep_old,
            "previousKeys": report.previous_keys,
        }),
    );
    Ok(report)
}

pub fn status(state: &AppState) -> Value {
    let old_files = std::fs::read_dir(&state.config.data_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.starts_with("secret.key.old-"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "previousKeys": state.vault.previous_count(),
        "oldKeyFiles": old_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-vault-rot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.set_secret("provider:p1", &state.vault.seal("sk-secret"));
        (state, dir)
    }

    #[test]
    fn rotation_reencrypts_secrets_and_migrates_legacy_backups() {
        let (state, dir) = state();
        let old_sealed = state.store.get_secret("provider:p1").unwrap();
        // Simulate a legacy snapshot encrypted with the vault key.
        let backup_dir = dir.join("backups");
        std::fs::create_dir_all(&backup_dir).unwrap();
        let legacy = state.vault.seal_bytes(b"SQLite format 3legacy").unwrap();
        std::fs::write(backup_dir.join("dominion-legacy.sqlite.enc"), &legacy).unwrap();

        let report = rotate(&state, false).unwrap();
        assert_eq!(report.secrets_reencrypted, 1);
        assert_eq!(report.legacy_backups_migrated, 1);
        assert_eq!(report.previous_keys, 0);

        let new_sealed = state.store.get_secret("provider:p1").unwrap();
        assert_ne!(new_sealed, old_sealed);
        assert_eq!(
            state.vault.open_secret(&new_sealed).as_deref(),
            Some("sk-secret")
        );
        assert!(state.vault.open_secret(&old_sealed).is_none());
        // A fresh process reads the new key and cannot read the old ciphertext.
        let reopened = Vault::open(&dir.join("secret.key"));
        assert_eq!(
            reopened.open_secret(&new_sealed).as_deref(),
            Some("sk-secret")
        );
        assert!(reopened.open_secret(&old_sealed).is_none());

        let migrated = std::fs::read(backup_dir.join("dominion-legacy.sqlite.enc")).unwrap();
        let ring = crate::engine::backup_keys::KeyRing::load_or_create(&dir).unwrap();
        assert!(crate::engine::backup_keys::KeyRing::is_versioned(&migrated));
        assert_eq!(
            ring.decrypt(&migrated).as_deref(),
            Some(b"SQLite format 3legacy".as_slice())
        );
        let old_files = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with("secret.key.old-")
            })
            .count();
        assert_eq!(old_files, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snapshot_verification_rejects_an_unavailable_old_key() {
        let (state, dir) = state();
        // Snapshot includes committed WAL content, unlike a raw DB file read.
        let snapshot = dir.join("snapshot.sqlite");
        state.store.backup_to(&snapshot).unwrap();
        let database = std::fs::read(&snapshot).unwrap();
        let old_key = state.vault.primary_key();
        assert!(verify_snapshot_secrets(&old_key, &database, &dir).is_ok());
        let mut new_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut new_key);
        let error = verify_snapshot_secrets(&new_key, &database, &dir).unwrap_err();
        assert!(error.contains("cannot be decrypted"), "{error}");
        // No temp verification DB is left behind.
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with(".restore-verify-")
            })
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keeping_the_old_key_allows_explicit_recovery() {
        let (state, dir) = state();
        let old_sealed = state.store.get_secret("provider:p1").unwrap();
        let report = rotate(&state, true).unwrap();
        assert_eq!(report.previous_keys, 1);
        assert!(report.old_key_file.as_ref().is_some_and(|path| path.is_file()));
        assert_eq!(
            state.vault.open_secret(&old_sealed).as_deref(),
            Some("sk-secret")
        );
        // Restart loads the retained key file as a fallback.
        let reopened = Vault::open(&dir.join("secret.key"));
        assert_eq!(
            reopened.open_secret(&old_sealed).as_deref(),
            Some("sk-secret")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_undecryptable_secret_aborts_before_the_key_changes() {
        let (state, dir) = state();
        state.store.set_secret("provider:broken", "not-a-valid-envelope");
        let before = std::fs::read(dir.join("secret.key")).unwrap();
        let error = rotate(&state, false).unwrap_err();
        assert!(error.contains("cannot be decrypted"), "{error}");
        assert_eq!(std::fs::read(dir.join("secret.key")).unwrap(), before);
        assert_eq!(state.vault.previous_count(), 0);
        assert_eq!(
            state.vault.open_secret(&state.store.get_secret("provider:p1").unwrap())
                .as_deref(),
            Some("sk-secret")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
