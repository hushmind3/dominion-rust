use crate::config::Config;
use crate::db::Store;
use crate::events::Bus;
use crate::vault::Vault;
use std::collections::HashMap;
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
}

impl AppState {
    pub fn new(config: Config, store: Store, vault: Vault) -> Self {
        Self {
            config,
            store,
            bus: Bus::new(),
            vault: Arc::new(vault),
            approvals: Arc::new(Mutex::new(HashMap::new())),
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
}
