//! H10 versioned backup keys and rotation.
//!
//! Backups carry their key version in a `DOMBAK2` envelope. Rotation creates a
//! new key, re-encrypts every existing `.enc` backup atomically, and only then
//! makes the new version active. Old keys are removed only after a successful
//! re-encryption unless the caller explicitly retains them.
use crate::state::AppState;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 7] = b"DOMBAK2";
const KEY_LEN: usize = 32;

#[derive(Clone)]
pub struct KeyRing {
    active: u32,
    keys: BTreeMap<u32, [u8; KEY_LEN]>,
    directory: PathBuf,
}

fn key_file(directory: &Path, version: u32) -> PathBuf {
    directory.join(format!("key-{version}.key"))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

impl KeyRing {
    pub fn load_or_create(data_dir: &Path) -> Result<Self, String> {
        let directory = data_dir.join("backup-keys");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let mut keys = BTreeMap::new();
        if let Ok(entries) = std::fs::read_dir(&directory) {
            for entry in entries.filter_map(|entry| entry.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(version) = name
                    .strip_prefix("key-")
                    .and_then(|rest| rest.strip_suffix(".key"))
                    .and_then(|value| value.parse::<u32>().ok())
                else {
                    continue;
                };
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    if bytes.len() == KEY_LEN {
                        let mut key = [0u8; KEY_LEN];
                        key.copy_from_slice(&bytes);
                        keys.insert(version, key);
                    }
                }
            }
        }
        if keys.is_empty() {
            let mut key = [0u8; KEY_LEN];
            rand::thread_rng().fill_bytes(&mut key);
            write_private(&key_file(&directory, 1), &key)?;
            keys.insert(1, key);
        }
        let active = std::fs::read_to_string(directory.join("active"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .filter(|version| keys.contains_key(version))
            .unwrap_or_else(|| *keys.keys().next_back().unwrap_or(&1));
        let _ = std::fs::write(directory.join("active"), active.to_string());
        Ok(Self {
            active,
            keys,
            directory,
        })
    }

    pub fn active_version(&self) -> u32 {
        self.active
    }

    pub fn versions(&self) -> Vec<u32> {
        self.keys.keys().cloned().collect()
    }

    fn cipher(key: &[u8; KEY_LEN]) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(key.into())
    }

    fn encrypt_with(&self, version: u32, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let key = self
            .keys
            .get(&version)
            .ok_or_else(|| format!("backup key version {version} is missing"))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = Self::cipher(key)
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .map_err(|_| "backup encryption failed".to_string())?;
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        self.encrypt_with(self.active, plaintext)
    }

    pub fn is_versioned(data: &[u8]) -> bool {
        data.starts_with(MAGIC)
    }

    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < MAGIC.len() + 4 + 12 + 16 || !Self::is_versioned(data) {
            return None;
        }
        let version = u32::from_le_bytes(data[7..11].try_into().ok()?);
        let key = self.keys.get(&version)?;
        let nonce = Nonce::from_slice(&data[11..23]);
        Self::cipher(key).decrypt(nonce, &data[23..]).ok()
    }
}

pub struct RotationReport {
    pub active_version: u32,
    pub previous_version: u32,
    pub reencrypted: usize,
    pub retained_versions: Vec<u32>,
}

fn backup_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("enc"))
        .collect()
}

/// Re-encrypts every backup under a newly generated key version.
pub fn rotate(state: &AppState, keep_old: bool) -> Result<RotationReport, String> {
    let mut ring = KeyRing::load_or_create(&state.config.data_dir)?;
    let previous_version = ring.active_version();
    let directory = state.config.data_dir.join("backups");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut payloads = Vec::new();
    for path in backup_files(&directory) {
        let sealed = std::fs::read(&path).map_err(|error| error.to_string())?;
        let plain = if KeyRing::is_versioned(&sealed) {
            ring.decrypt(&sealed)
                .ok_or_else(|| format!("cannot decrypt {}", path.display()))?
        } else {
            // Legacy snapshots were sealed with the vault key.
            state
                .vault
                .open_bytes(&sealed)
                .ok_or_else(|| format!("legacy backup key unavailable for {}", path.display()))?
        };
        payloads.push((path, plain));
    }

    let next = ring.versions().into_iter().max().unwrap_or(0) + 1;
    let mut new_key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut new_key);
    ring.keys.insert(next, new_key);
    ring.active = next;

    // Encrypt to temp files first; originals stay valid until every write
    // succeeds.
    let mut staged = Vec::new();
    for (path, plain) in &payloads {
        let sealed = ring.encrypt_with(next, plain)?;
        let temp = path.with_extension("enc.tmp");
        std::fs::write(&temp, &sealed).map_err(|error| error.to_string())?;
        staged.push((temp, path.clone()));
    }
    write_private(&key_file(&ring.directory, next), &new_key)?;
    std::fs::write(ring.directory.join("active"), next.to_string())
        .map_err(|error| error.to_string())?;
    for (temp, path) in &staged {
        std::fs::rename(temp, path).map_err(|error| error.to_string())?;
    }

    if !keep_old {
        for version in ring.versions() {
            if version != next {
                let _ = std::fs::remove_file(key_file(&ring.directory, version));
            }
        }
    }
    let report = RotationReport {
        active_version: next,
        previous_version,
        reencrypted: payloads.len(),
        retained_versions: KeyRing::load_or_create(&state.config.data_dir)?.versions(),
    };
    state.store.audit(
        "backup.keys_rotated",
        json!({
            "activeVersion": report.active_version,
            "previousVersion": report.previous_version,
            "reencrypted": report.reencrypted,
            "retained": report.retained_versions,
            "keepOld": keep_old,
        }),
    );
    Ok(report)
}

pub fn decrypt_backup(state: &AppState, sealed: &[u8]) -> Option<Vec<u8>> {
    let ring = KeyRing::load_or_create(&state.config.data_dir).ok()?;
    if KeyRing::is_versioned(sealed) {
        return ring.decrypt(sealed);
    }
    state.vault.open_bytes(sealed)
}

pub fn status(state: &AppState) -> Value {
    match KeyRing::load_or_create(&state.config.data_dir) {
        Ok(ring) => json!({
            "activeVersion": ring.active_version(),
            "versions": ring.versions(),
        }),
        Err(error) => json!({"error": error}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-keyring-{}", uuid::Uuid::new_v4()));
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
    fn rotation_reencrypts_backups_and_drops_old_keys() {
        let (state, dir) = state();
        state.store.put(
            "provider",
            &json!({"id":"p1","name":"rotation","baseUrl":"http://x/v1"}),
        );
        let backup = crate::engine::maintenance::backup_now(&state).unwrap();
        let sealed = std::fs::read(&backup).unwrap();
        assert!(KeyRing::is_versioned(&sealed));
        assert_eq!(KeyRing::load_or_create(&dir).unwrap().active_version(), 1);

        let report = rotate(&state, false).unwrap();
        assert_eq!(report.active_version, 2);
        assert_eq!(report.previous_version, 1);
        assert_eq!(report.retained_versions, vec![2]);
        let ring = KeyRing::load_or_create(&dir).unwrap();
        let resealed = std::fs::read(&backup).unwrap();
        let plain = ring.decrypt(&resealed).expect("new key decrypts");
        assert!(plain.starts_with(b"SQLite format 3"));
        let restored_path = dir.join("rotated.sqlite");
        std::fs::write(&restored_path, &plain).unwrap();
        assert_eq!(
            Store::open(&restored_path).unwrap().get("provider", "p1").unwrap()["name"],
            "rotation"
        );
        assert!(!key_file(&dir.join("backup-keys"), 1).exists());
    }

    #[test]
    fn keep_old_retains_the_previous_key_version() {
        let (state, dir) = state();
        let _ = crate::engine::maintenance::backup_now(&state).unwrap();
        let report = rotate(&state, true).unwrap();
        assert_eq!(report.retained_versions, vec![1, 2]);
        assert!(key_file(&dir.join("backup-keys"), 1).exists());
    }

    #[test]
    fn a_missing_key_version_cannot_decrypt() {
        let (state, dir) = state();
        let _ = crate::engine::maintenance::backup_now(&state).unwrap();
        let report = rotate(&state, false).unwrap();
        // Forge an envelope claiming the removed version.
        let mut forged = MAGIC.to_vec();
        forged.extend_from_slice(&report.previous_version.to_le_bytes());
        forged.extend_from_slice(&[0u8; 12]);
        forged.extend_from_slice(&[0u8; 16]);
        let ring = KeyRing::load_or_create(&dir).unwrap();
        assert!(ring.decrypt(&forged).is_none());
    }
}
