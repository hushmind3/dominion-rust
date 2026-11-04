use crate::config::Config;
use crate::db::Store;
use crate::events::Bus;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub store: Store,
    pub bus: Bus,
    pub vault: Arc<Vault>,
    /// Pending tool approvals: approval id -> waiter.
    pub approvals: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
    /// Sessions the user asked to stop. The engine checks between rounds/tools.
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    /// Sessions currently executing. Prevents two turns racing on one history.
    pub active: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    pub fn new(config: Config, store: Store, vault: Vault) -> Self {
        Self {
            config,
            store,
            bus: Bus::new(),
            vault: Arc::new(vault),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn save_secret(&self, name: &str, plaintext: &str) {
        let sealed = self.vault.seal(plaintext);
        self.store.set_secret(name, &sealed);
    }

    pub fn read_secret(&self, name: &str) -> Option<String> {
        let sealed = self.store.get_secret(name)?;
        self.vault.open_secret(&sealed)
    }

    pub fn cancel_session(&self, session_id: &str) {
        self.cancelled
            .lock()
            .unwrap()
            .insert(session_id.to_string());
        self.store.put(
            "session_cancel",
            &serde_json::json!({"id":session_id,"cancelledAt":chrono::Utc::now().to_rfc3339()}),
        );
        // Cancellation survives a restart and cannot be cleared for old queued
        // work merely because the user starts a fresh foreground turn.
        for job in self.store.all("job") {
            let matches = [
                "/sessionId",
                "/input/sessionId",
                "/goalState/sessionId",
                "/roomState/sessionId",
                "/workflowState/sessionId",
            ]
            .iter()
            .any(|path| job.pointer(path).and_then(serde_json::Value::as_str) == Some(session_id));
            if matches && matches!(job["status"].as_str(), Some("queued" | "running")) {
                if let Some(id) = job["id"].as_str() {
                    if let Some(saved) = self.store.patch_if("job",id,"status",&job["status"],&serde_json::json!({
                        "status":"cancelled","cancelRequested":true,"finishedAt":chrono::Utc::now().to_rfc3339(),"error":"사용자 요청으로 실행이 중단되었습니다."
                    })) { self.bus.record("job",&saved); }
                }
            }
        }
        for mut goal in self.store.all("goal") {
            if goal["sessionId"].as_str() == Some(session_id)
                && matches!(goal["status"].as_str(), Some("queued" | "running"))
            {
                goal["status"] = serde_json::json!("cancelled");
                goal["phase"] = serde_json::json!("cancelled");
                goal["finishedAt"] = serde_json::json!(chrono::Utc::now().to_rfc3339());
                if let Some(saved) = self.store.put("goal", &goal) {
                    self.bus.record("goal", &saved);
                }
            }
        }
    }

    pub fn clear_cancel(&self, session_id: &str) {
        self.cancelled.lock().unwrap().remove(session_id);
        self.store.remove("session_cancel", session_id);
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(session_id)
            || self.store.get("session_cancel", session_id).is_some()
    }

    /// Returns false when the session already has a running turn.
    pub fn begin_session(&self, session_id: &str) -> bool {
        self.active.lock().unwrap().insert(session_id.to_string())
    }

    pub fn end_session(&self, session_id: &str) {
        self.active.lock().unwrap().remove(session_id);
    }

    pub fn session_guard(&self, session_id: &str) -> SessionGuard {
        SessionGuard {
            state: self.clone(),
            id: session_id.to_string(),
        }
    }
}

/// Releases the session lock even when the turn returns early via `?`.
pub struct SessionGuard {
    state: AppState,
    id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.state.end_session(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&cfg.db_path()).unwrap();
        let vault = Vault::open(&cfg.key_path());
        (AppState::new(cfg, store, vault), dir)
    }

    #[test]
    fn session_lock_rejects_a_second_turn() {
        let (state, dir) = temp_state();
        assert!(state.begin_session("s1"));
        assert!(!state.begin_session("s1"));
        state.end_session("s1");
        assert!(state.begin_session("s1"));
        {
            let _guard = state.session_guard("s1");
            assert!(!state.begin_session("s1"));
        }
        assert!(state.begin_session("s1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancellation_survives_restart_and_old_jobs_stay_cancelled() {
        let (state, dir) = temp_state();
        state.store.put("job",&serde_json::json!({"id":"old","kind":"prompt","status":"queued","input":{"sessionId":"s"}}));
        state.store.put(
            "job",
            &serde_json::json!({"id":"running","kind":"room","status":"running","sessionId":"s"}),
        );
        state.cancel_session("s");
        let restarted = AppState::new(
            state.config.clone(),
            state.store.clone(),
            Vault::open(&state.config.key_path()),
        );
        assert!(restarted.is_cancelled("s"));
        restarted.clear_cancel("s");
        assert!(!restarted.is_cancelled("s"));
        assert_eq!(
            restarted.store.get("job", "old").unwrap()["status"],
            "cancelled"
        );
        assert!(
            crate::engine::jobs::JobContext::new(restarted.clone(), "running".into())
                .checkpoint(serde_json::json!({"status":"completed"}))
                .is_err()
        );
        assert_eq!(
            restarted.store.get("job", "running").unwrap()["status"],
            "cancelled"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
