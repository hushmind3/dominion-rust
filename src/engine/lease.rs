//! Durable run lease shared by chat, goal, room and workflow turns.
//!
//! The in-memory session set only protects one process. A lease adds an owner
//! and heartbeat in the database, so a restarted engine can tell a dead run
//! from a live one and refuse a duplicate owner.
use crate::db::Store;
use crate::state::AppState;
use std::time::Duration;

pub const LEASE_TTL: Duration = Duration::from_secs(120);

pub struct LeaseGuard {
    store: Store,
    id: String,
    owner: String,
    heartbeat: Option<tokio::task::JoinHandle<()>>,
}

impl LeaseGuard {
    /// Acquires the session lease or returns None when another live owner
    /// holds it. The heartbeat keeps it alive while the turn awaits approval or
    /// a long model call.
    pub fn acquire(state: &AppState, scope: &str) -> Option<Self> {
        Self::acquire_with(state, scope, LEASE_TTL)
    }

    pub fn acquire_with(state: &AppState, scope: &str, ttl: Duration) -> Option<Self> {
        let id = format!("session:{scope}");
        let ttl_ms = ttl.as_millis() as i64;
        // A lease owner is one turn, not one process: two turns in the same
        // engine must not both believe they own the session.
        let owner = format!("{}:{}", state.instance_id, uuid::Uuid::new_v4());
        state.store.acquire_lease(&id, &owner, ttl_ms)?;
        let store = state.store.clone();
        let heartbeat_store = store.clone();
        let heartbeat_id = id.clone();
        let heartbeat_owner = owner.clone();
        let interval_ms = (ttl.as_millis() / 3).max(1_000) as u64;
        let heartbeat = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if heartbeat_store
                    .renew_lease(&heartbeat_id, &heartbeat_owner, ttl_ms)
                    .is_none()
                {
                    break;
                }
            }
        });
        Some(Self {
            store,
            id,
            owner,
            heartbeat: Some(heartbeat),
        })
    }

}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.heartbeat.take() {
            handle.abort();
        }
        self.store.release_lease(&self.id, &self.owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    #[tokio::test]
    async fn a_session_has_one_live_owner_and_is_released_on_drop() {
        let (state, dir) = state();
        let first = LeaseGuard::acquire(&state, "session-a").expect("first owner");
        assert!(LeaseGuard::acquire(&state, "session-a").is_none());
        assert!(state
            .store
            .active_lease("session:session-a")
            .is_some_and(|lease| {
                lease["owner"]
                    .as_str()
                    .is_some_and(|owner| owner.starts_with(&state.instance_id))
            }));
        drop(first);
        assert!(state.store.active_lease("session:session-a").is_none());
        let second = LeaseGuard::acquire(&state, "session-a").expect("released lease");
        drop(second);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn heartbeat_keeps_a_long_turn_alive() {
        let (state, dir) = state();
        let guard = LeaseGuard::acquire_with(&state, "session-b", Duration::from_millis(1_500))
            .expect("lease");
        tokio::time::sleep(Duration::from_millis(1_700)).await;
        let lease = state
            .store
            .active_lease("session:session-b")
            .expect("lease renewed");
        let expires = lease["expiresAtMs"].as_i64().unwrap_or(0);
        assert!(expires > chrono::Utc::now().timestamp_millis());
        drop(guard);
        let _ = std::fs::remove_dir_all(dir);
    }
}
