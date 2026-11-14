use crate::config::Config;
use crate::db::Store;
use crate::events::Bus;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::oneshot;

type FileMutations = Arc<Mutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>>;

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
    /// Coordinates checked and unchecked writes to the same canonical path.
    pub file_mutations: FileMutations,
    pub principle_learning: Arc<Mutex<()>>,
    pub policy_settlement: Arc<Mutex<()>>,
    /// Identity of this engine process, used to own durable run leases.
    pub instance_id: String,
    /// Explicit maintenance flag. While true, non-GET writes are rejected.
    pub maintenance: Arc<std::sync::atomic::AtomicBool>,
}

impl AppState {
    pub fn new(config: Config, store: Store, vault: Vault) -> Self {
        let bus = Bus::new(store.clone());
        let instance_id = std::env::var("DOMINION_ENGINE_INSTANCE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            config,
            store,
            bus,
            vault: Arc::new(vault),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            active: Arc::new(Mutex::new(HashSet::new())),
            file_mutations: Arc::new(Mutex::new(HashMap::new())),
            principle_learning: Arc::new(Mutex::new(())),
            policy_settlement: Arc::new(Mutex::new(())),
            instance_id,
            maintenance: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn set_maintenance(&self, enabled: bool) {
        self.maintenance
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
        self.store.audit(
            "system.maintenance",
            serde_json::json!({"enabled": enabled}),
        );
    }

    pub fn maintenance_mode(&self) -> bool {
        self.maintenance.load(std::sync::atomic::Ordering::SeqCst)
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
        // One transaction closes every descendant's durable cancellation epoch,
        // then marks their queued/running jobs and goals cancelled. A concurrent
        // delegation reservation sees the parent's stopped epoch under the
        // same database lock and cannot start an orphan paid worker.
        match self.store.cancel_session_family(session_id) {
            Ok(stopped) => {
                let mut local=self.cancelled.lock().unwrap();
                local.extend(stopped.session_ids.iter().cloned());
                drop(local);
                for job in &stopped.jobs {self.bus.record("job",job);}
                for goal in &stopped.goals {self.bus.record("goal",goal);}
                // Drop live approval senders now. A stopped child must not
                // hold a worker slot waiting for the owner approval timeout.
                // close_session_approvals is durable and CAS-protected.
                for id in &stopped.session_ids {
                    crate::engine::chat::close_session_approvals(self,id);
                }
                self.store.audit("session.cancel_tree",serde_json::json!({
                    "rootSessionId":session_id,
                    "sessions":stopped.session_ids.len(),
                    "jobs":stopped.jobs.len(),
                    "goals":stopped.goals.len(),
                }));
            }
            Err(error) => {
                // Stop the local turn even when disk persistence fails;
                // report the failure rather than claiming durable cancellation.
                self.cancelled.lock().unwrap().insert(session_id.to_string());
                crate::engine::chat::close_session_approvals(self,session_id);
                self.store.audit("session.cancel_persistence_failed",
                    serde_json::json!({"sessionId":session_id,"error":error}));
            }
        }
    }

    pub fn clear_cancel(&self, session_id: &str) {
        self.resume_session_if_generation(session_id, self.cancellation_generation(session_id));
    }

    pub fn cancellation_generation(&self, session_id: &str) -> u64 {
        self.store
            .get("session_cancel", session_id)
            .and_then(|record| record["generation"].as_u64())
            .unwrap_or(0)
    }

    pub fn resume_session_if_generation(&self, session_id: &str, generation: u64) -> bool {
        if !self.store.resume_session_epoch(session_id, generation) {
            return false;
        }
        self.cancelled.lock().unwrap().remove(session_id);
        true
    }

    pub fn is_cancelled(&self, session_id: &str) -> bool {
        self.cancelled.lock().unwrap().contains(session_id)
            || self
                .store
                .get("session_cancel", session_id)
                .is_some_and(|record| record["cancelled"].as_bool().unwrap_or(true))
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

    #[test]
    fn stop_propagates_to_nested_delegates_and_keeps_unrelated_sessions_running() {
        let (state,dir)=temp_state();
        for (id,parent,delegated) in [
            ("root",None,false),("child",Some("root"),true),
            ("grandchild",Some("child"),true),("sibling",None,false)
        ] {
            state.store.put("session",&serde_json::json!({
                "id":id,"parentSessionId":parent,"delegated":delegated
            })).unwrap();
            state.store.put("job",&serde_json::json!({
                "id":format!("job-{id}"),"kind":"prompt",
                "status":if id=="grandchild" {"queued"} else {"running"},
                "input":{"sessionId":id,"delegation":delegated,
                    "parentSessionId":parent}
            })).unwrap();
        }
        state.store.put("goal",&serde_json::json!({
            "id":"child-goal","sessionId":"grandchild","status":"running"
        })).unwrap();
        state.cancel_session("root");
        let restarted=AppState::new(state.config.clone(),state.store.clone(),
            Vault::open(&state.config.key_path()));
        for id in ["root","child","grandchild"] {
            assert!(restarted.is_cancelled(id),"{id} must remain stopped after restart");
            let job=restarted.store.get("job",&format!("job-{id}")).unwrap();
            assert_eq!(job["status"],"cancelled");
            assert_eq!(job["cancelRequested"],true);
        }
        assert_eq!(restarted.store.get("goal","child-goal").unwrap()["status"],"cancelled");
        assert!(!restarted.is_cancelled("sibling"));
        assert_eq!(restarted.store.get("job","job-sibling").unwrap()["status"],"running");
        let session=serde_json::json!({"id":"late","delegated":true,
            "parentSessionId":"child"});
        let job=serde_json::json!({"id":"late-job","kind":"prompt","status":"queued",
            "input":{"delegation":true,"parentSessionId":"child","sessionId":"late"}});
        assert!(restarted.store.reserve_delegation("child",6,&session,&job).is_err());
        assert!(restarted.store.get("session","late").is_none());
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_stop_and_delegation_never_leave_an_active_orphan() {
        let (state,dir)=temp_state();
        state.store.put("session",&serde_json::json!({"id":"root"})).unwrap();
        std::thread::scope(|scope| {
            let store=state.store.clone();
            let child=scope.spawn(move||{
                let session=serde_json::json!({"id":"racing-child","delegated":true,
                    "parentSessionId":"root"});
                let job=serde_json::json!({"id":"racing-job","kind":"prompt",
                    "status":"queued","input":{"sessionId":"racing-child",
                        "parentSessionId":"root","delegation":true}});
                store.reserve_delegation("root",6,&session,&job)
            });
            state.cancel_session("root");
            let inserted=child.join().unwrap();
            match inserted {
                Ok(true)=>assert_eq!(state.store.get("job","racing-job").unwrap()["status"],
                    "cancelled","child created first must be stopped"),
                Err(_)=>assert!(state.store.get("job","racing-job").is_none(),
                    "Stop first must reject new child"),
                Ok(false)=>panic!("Fresh parent should never hit its quota"),
            }
        });
        assert!(state.is_cancelled("root"));
        assert!(!state.store.get("job","racing-job")
            .is_some_and(|job|matches!(job["status"].as_str(),
                Some("queued"|"running"))));
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resuming_a_new_epoch_cannot_revive_an_old_worker() {
        let (state, dir) = temp_state();
        let initial = state.cancellation_generation("room:r");
        assert!(state.resume_session_if_generation("room:r", initial));
        state.cancel_session("room:r");
        let stopped = state.cancellation_generation("room:r");
        assert!(stopped > initial);
        assert!(!state.resume_session_if_generation("room:r", initial));
        assert!(state.is_cancelled("room:r"));
        assert!(state.resume_session_if_generation("room:r", stopped));
        assert!(!state.is_cancelled("room:r"));
        assert_ne!(state.cancellation_generation("room:r"), initial);
        let _ = std::fs::remove_dir_all(dir);
    }
}
