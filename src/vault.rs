use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Provider API keys are never written into the record store. They live in a
/// dedicated table, sealed with a key file that is created with 0600 mode.
pub struct Vault {
    cipher: ChaCha20Poly1305,
}

impl Vault {
    pub fn open(key_path: &Path) -> Self {
        let key = match fs::read(key_path) {
            Ok(bytes) if bytes.len() == 32 => bytes,
            _ => {
                let mut key = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut key);
                if let Some(parent) = key_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if let Ok(mut file) = fs::File::create(key_path) {
                    let _ = file.write_all(&key);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(key_path, fs::Permissions::from_mode(0o600));
                }
                key.to_vec()
            }
        };
        let key: [u8; 32] = key[..32].try_into().expect("32 byte key");
        Self {
            cipher: ChaCha20Poly1305::new(&key.into()),
        }
    }

    pub fn seal(&self, plaintext: &str) -> String {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        match self.cipher.encrypt(nonce, plaintext.as_bytes()) {
            Ok(ciphertext) => {
                let mut out = nonce_bytes.to_vec();
                out.extend_from_slice(&ciphertext);
                STANDARD.encode(out)
            }
            Err(_) => String::new(),
        }
    }

    /// Binary envelope: 12-byte nonce followed by ChaCha20-Poly1305 ciphertext.
    /// Backup keyrings now own encryption; this remains for legacy snapshots
    /// and binary-secret callers.
    #[allow(dead_code)]
    pub fn seal_bytes(&self, plaintext: &[u8]) -> Option<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self.cipher.encrypt(nonce, plaintext).ok()?;
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ciphertext);
        Some(out)
    }

    pub fn open_bytes(&self, sealed: &[u8]) -> Option<Vec<u8>> {
        if sealed.len() < 13 {
            return None;
        }
        let (nonce_bytes, ciphertext) = sealed.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher.decrypt(nonce, ciphertext).ok()
    }

    pub fn open_secret(&self, sealed: &str) -> Option<String> {
        let bytes = STANDARD.decode(sealed).ok()?;
        if bytes.len() < 13 {
            return None;
        }
        let (nonce_bytes, ciphertext) = bytes.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plain = self.cipher.decrypt(nonce, ciphertext).ok()?;
        String::from_utf8(plain).ok()
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
