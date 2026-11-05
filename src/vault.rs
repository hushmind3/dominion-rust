use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEY_LEN: usize = 32;

struct Keys {
    primary: [u8; KEY_LEN],
    /// Previous keys are kept only while a rotation is in progress or when the
    /// caller explicitly retains the old key file.
    previous: Vec<[u8; KEY_LEN]>,
}

/// Provider API keys are never written into the record store. They live in a
/// dedicated table, sealed with a key file that is created with 0600 mode.
/// The vault supports a primary key plus previous keys so a rotation can read
/// records encrypted at either side of the swap.
pub struct Vault {
    keys: Mutex<Keys>,
    key_path: PathBuf,
}

fn read_key(path: &Path) -> Option<[u8; KEY_LEN]> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() != KEY_LEN {
        return None;
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Some(key)
}

fn write_key(path: &Path, key: &[u8; KEY_LEN]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

impl Vault {
    pub fn open(key_path: &Path) -> Self {
        let primary = match read_key(key_path) {
            Some(key) => key,
            None => {
                let mut key = [0u8; KEY_LEN];
                rand::thread_rng().fill_bytes(&mut key);
                if let Some(parent) = key_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = write_key(key_path, &key);
                key
            }
        };
        // Old key files left by a rotation are trusted fallbacks on restart so
        // a crash during migration cannot make every secret unreadable.
        let mut previous = Vec::new();
        if let Some(parent) = key_path.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.filter_map(|entry| entry.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("secret.key.old-") {
                        if let Some(key) = read_key(&entry.path()) {
                            previous.push(key);
                        }
                    }
                }
            }
        }
        Self {
            keys: Mutex::new(Keys { primary, previous }),
            key_path: key_path.to_path_buf(),
        }
    }

    fn cipher(key: &[u8; KEY_LEN]) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(key.into())
    }

    fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Option<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = Self::cipher(key)
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .ok()?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ciphertext);
        Some(out)
    }

    fn decrypt(key: &[u8; KEY_LEN], sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < 13 {
            return None;
        }
        let (nonce, ciphertext) = sealed.split_at(12);
        Self::cipher(key)
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .ok()
    }

    pub fn seal(&self, plaintext: &str) -> String {
        let key = self.keys.lock().unwrap().primary;
        match Self::encrypt(&key, plaintext.as_bytes()) {
            Some(out) => STANDARD.encode(out),
            None => String::new(),
        }
    }

    #[allow(dead_code)] // used by legacy-snapshot tests and migration tooling
    pub fn seal_bytes(&self, plaintext: &[u8]) -> Option<Vec<u8>> {
        let key = self.keys.lock().unwrap().primary;
        Self::encrypt(&key, plaintext)
    }

    fn open_with_keys(&self, sealed: &[u8], primary_only: bool) -> Option<Vec<u8>> {
        let keys = self.keys.lock().unwrap();
        if let Some(plain) = Self::decrypt(&keys.primary, sealed) {
            return Some(plain);
        }
        if primary_only {
            return None;
        }
        keys.previous
            .iter()
            .find_map(|key| Self::decrypt(key, sealed))
    }

    pub fn open_secret(&self, sealed: &str) -> Option<String> {
        let bytes = STANDARD.decode(sealed).ok()?;
        String::from_utf8(self.open_with_keys(&bytes, false)?).ok()
    }

    pub fn open_bytes(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        self.open_with_keys(sealed, false)
    }

    pub fn open_with_primary(&self, sealed: &str) -> Option<String> {
        let bytes = STANDARD.decode(sealed).ok()?;
        String::from_utf8(self.open_with_keys(&bytes, true)?).ok()
    }

    pub fn primary_key(&self) -> [u8; KEY_LEN] {
        self.keys.lock().unwrap().primary
    }

    /// Restores a key from a backup container: writes it atomically and makes
    /// it the only primary/previous key, so restored secrets match exactly.
    pub fn restore_primary(&self, key: [u8; KEY_LEN]) -> std::io::Result<()> {
        let temp = self.key_path.with_extension("key.restore");
        write_key(&temp, &key)?;
        fs::rename(&temp, &self.key_path)?;
        let mut keys = self.keys.lock().unwrap();
        keys.primary = key;
        keys.previous.clear();
        Ok(())
    }

    pub fn open_secret_with_key(key: &[u8; KEY_LEN], sealed: &str) -> Option<String> {
        let bytes = STANDARD.decode(sealed).ok()?;
        String::from_utf8(Self::decrypt(key, &bytes)?).ok()
    }

    pub fn key_path(&self) -> &Path {
        &self.key_path
    }

    /// Installs a new primary key and keeps the old one available as previous
    /// until the caller verifies the migration and calls `drop_previous`.
    pub fn install_primary(&self, new_key: [u8; KEY_LEN]) {
        let mut keys = self.keys.lock().unwrap();
        let old = keys.primary;
        keys.primary = new_key;
        if !keys.previous.contains(&old) {
            keys.previous.push(old);
        }
    }

    pub fn drop_previous(&self) {
        self.keys.lock().unwrap().previous.clear();
    }

    pub fn previous_count(&self) -> usize {
        self.keys.lock().unwrap().previous.len()
    }

    pub fn write_key_file(&self, write_path: &Path) -> std::io::Result<()> {
        let key = self.primary_key();
        write_key(write_path, &key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_secrets_round_trip_and_reject_tampering() {
        let dir = std::env::temp_dir().join(format!("dominion-vault-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let vault = Vault::open(&dir.join("secret.key"));
        let sealed = vault.seal("sk-test-123");
        assert_ne!(sealed, "sk-test-123");
        assert_eq!(vault.open_secret(&sealed).as_deref(), Some("sk-test-123"));

        let reopened = Vault::open(&dir.join("secret.key"));
        assert_eq!(
            reopened.open_secret(&sealed).as_deref(),
            Some("sk-test-123")
        );

        let mut tampered = STANDARD.decode(&sealed).unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert_eq!(vault.open_secret(&STANDARD.encode(tampered)), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn binary_backups_round_trip_and_reject_tampering() {
        let dir = std::env::temp_dir().join(format!("dominion-vault-bin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let vault = Vault::open(&dir.join("secret.key"));
        let payload = b"SQLite format 3\0backup bytes";
        let sealed = vault.seal_bytes(payload).unwrap();
        assert_ne!(sealed.as_slice(), payload.as_slice());
        assert_eq!(vault.open_bytes(&sealed).as_deref(), Some(payload.as_slice()));
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert!(vault.open_bytes(&tampered).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_previous_key_is_only_a_transitional_fallback() {
        let dir = std::env::temp_dir().join(format!("dominion-vault-rot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.key");
        let vault = Vault::open(&path);
        let old = vault.seal("old-secret");
        let mut new_key = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut new_key);
        vault.install_primary(new_key);
        assert_eq!(vault.open_secret(&old).as_deref(), Some("old-secret"));
        assert!(vault.open_with_primary(&old).is_none());
        vault.write_key_file(&path).unwrap();
        vault.drop_previous();
        assert!(vault.open_secret(&old).is_none());
        let reopened = Vault::open(&path);
        assert!(reopened.open_secret(&old).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn key_file_is_private() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = std::env::temp_dir().join(format!("dominion-key-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("secret.key");
            let _ = Vault::open(&path);
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
