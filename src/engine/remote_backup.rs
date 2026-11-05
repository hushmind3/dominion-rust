//! H10 remote offsite backup transfer (HTTP PUT target, S3-compatible).
//!
//! Uploads retry, verify the remote copy by SHA-256 (echo header or GET), and
//! never delete the local snapshot on remote failure. Outcomes are audited.
use crate::state::AppState;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct RemoteConfig {
    pub base_url: String,
    pub token: String,
    pub retries: u32,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug)]
pub struct RemoteReport {
    pub uploaded: bool,
    pub verified: bool,
    pub url: String,
    pub sha256: String,
    pub attempts: u32,
    pub error: String,
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn from_env() -> Option<RemoteConfig> {
    let base_url = env_string("DOMINION_BACKUP_REMOTE_URL")?;
    Some(RemoteConfig {
        base_url,
        token: env_string("DOMINION_BACKUP_REMOTE_TOKEN").unwrap_or_default(),
        retries: env_string("DOMINION_BACKUP_REMOTE_RETRIES")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(2)
            .min(5),
        timeout_secs: env_string("DOMINION_BACKUP_REMOTE_TIMEOUT_SECS")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(30)
            .clamp(1, 300),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn verify_get(client: &reqwest::Client, url: &str, expected: &str) -> bool {
    let Ok(response) = client.get(url).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    match response.bytes().await {
        Ok(body) => sha256_hex(&body) == expected,
        Err(_) => false,
    }
}

pub async fn upload(path: &Path, config: &RemoteConfig) -> RemoteReport {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return RemoteReport {
                uploaded: false,
                verified: false,
                url: String::new(),
                sha256: String::new(),
                attempts: 0,
                error: error.to_string(),
            }
        }
    };
    let checksum = sha256_hex(&bytes);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("backup.enc");
    let url = format!("{}/{}", config.base_url.trim_end_matches('/'), name);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return RemoteReport {
                uploaded: false,
                verified: false,
                url,
                sha256: checksum,
                attempts: 0,
                error: error.to_string(),
            }
        }
    };
    let total_attempts = config.retries.saturating_add(1);
    let mut last_error = String::new();
    for attempt in 1..=total_attempts {
        let mut request = client
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .header("x-dominion-sha256", &checksum)
            .body(bytes.clone());
        if !config.token.is_empty() {
            request = request.bearer_auth(&config.token);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let echoed = response
                    .headers()
                    .get("x-dominion-sha256")
                    .and_then(|value| value.to_str().ok())
                    == Some(checksum.as_str());
                if echoed || verify_get(&client, &url, &checksum).await {
                    return RemoteReport {
                        uploaded: true,
                        verified: true,
                        url,
                        sha256: checksum,
                        attempts: attempt,
                        error: String::new(),
                    };
                }
                last_error = "remote checksum verification failed".to_string();
            }
            Ok(response) => {
                last_error = format!("remote PUT failed: HTTP {}", response.status());
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
        if attempt < total_attempts {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    RemoteReport {
        uploaded: false,
        verified: false,
        url,
        sha256: checksum,
        attempts: total_attempts,
        error: last_error,
    }
}

pub async fn upload_if_configured(
    state: &AppState,
    path: &Path,
) -> Option<RemoteReport> {
    let config = from_env()?;
    let report = upload(path, &config).await;
    if report.uploaded && report.verified {
        state.store.audit(
            "backup.remote_uploaded",
            json!({
                "path": path.to_string_lossy(),
                "url": report.url,
                "sha256": report.sha256,
                "attempts": report.attempts,
            }),
        );
    } else {
        state.store.audit(
            "backup.remote_failed",
            json!({
                "path": path.to_string_lossy(),
                "url": report.url,
                "attempts": report.attempts,
                "error": report.error,
            }),
        );
    }
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn sink(failures_first: usize, echo: bool) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hashes = Arc::new(Mutex::new(Vec::new()));
        let observed = hashes.clone();
        std::thread::spawn(move || {
            let mut seen = 0usize;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                seen += 1;
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let end = loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(at) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                if seen <= failures_first {
                    let _ = stream.write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let body = &bytes[end..end + length];
                let hash = sha256_hex(body);
                observed.lock().unwrap().push(hash.clone());
                let header = if echo { &hash } else { "wrong" };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nx-dominion-sha256: {header}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (port, hashes)
    }

    fn backup_file() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dominion-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dominion-test.sqlite.enc");
        std::fs::write(&path, b"encrypted-backup-bytes").unwrap();
        path
    }

    #[tokio::test]
    async fn upload_verifies_the_remote_checksum() {
        let (port, hashes) = sink(0, true);
        let path = backup_file();
        let config = RemoteConfig {
            base_url: format!("http://127.0.0.1:{port}/backups"),
            token: "secret".into(),
            retries: 0,
            timeout_secs: 5,
        };
        let report = upload(&path, &config).await;
        assert!(report.uploaded && report.verified, "{report:?}");
        assert_eq!(report.attempts, 1);
        assert_eq!(hashes.lock().unwrap().as_slice(), &[report.sha256.clone()]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn upload_retries_then_succeeds() {
        let (port, _) = sink(1, true);
        let path = backup_file();
        let config = RemoteConfig {
            base_url: format!("http://127.0.0.1:{port}/backups"),
            token: String::new(),
            retries: 1,
            timeout_secs: 5,
        };
        let report = upload(&path, &config).await;
        assert!(report.uploaded, "{report:?}");
        assert_eq!(report.attempts, 2);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_checksum_mismatch_is_a_failure_and_keeps_the_local_file() {
        let (port, _) = sink(0, false);
        let path = backup_file();
        let config = RemoteConfig {
            base_url: format!("http://127.0.0.1:{port}/backups"),
            token: String::new(),
            retries: 0,
            timeout_secs: 2,
        };
        let report = upload(&path, &config).await;
        assert!(!report.uploaded, "{report:?}");
        assert!(report.error.contains("checksum"), "{report:?}");
        assert!(path.is_file(), "local snapshot must be retained");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
