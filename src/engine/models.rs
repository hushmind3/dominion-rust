use crate::state::AppState;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::process::{Child, Command};

const RUNTIME_PORT: u16 = 4319;
const PROVIDER_ID: &str = "dominion-native";
const MODEL_KIND: &str = "model_job";
const EXTERNAL_KIND: &str = "external_model";
pub static RUNTIME_CHANGE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static DOWNLOAD_COMMIT: Mutex<()> = Mutex::new(());
// Register a model-file owner atomically; per-job download guards cannot
// prevent separate jobs from writing to the same destination.
static DOWNLOAD_START_LOCK: Mutex<()> = Mutex::new(());
static ACTIVE_DOWNLOADS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

fn active_downloads() -> &'static Mutex<std::collections::HashSet<String>> {
    ACTIVE_DOWNLOADS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = active_downloads().lock() {
            active.remove(&self.0);
        }
    }
}

struct Runtime {
    child: Option<Child>,
    status: String,
    id: String,
    name: String,
    engine: String,
    backend: String,
    model_key: String,
    error: String,
    logs: Vec<String>,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Mutex::new(Runtime {
            child: None,
            status: "idle".into(),
            id: String::new(),
            name: String::new(),
            engine: String::new(),
            backend: String::new(),
            model_key: String::new(),
            error: String::new(),
            logs: Vec::new(),
        })
    })
}

// Model storage, runtime control, discovery and transfers evolve independently.
mod inventory;
use self::inventory::{safe_repo, safe_relative, mlx_models_root, mlx_search_roots, target_root, find_lms_key, gguf_shard_parts, local_gguf_parts, gguf_download_options, validated_gguf_files, mlx_download_files};
pub use self::inventory::{models_dir, installed};
mod runtime_install;
use self::runtime_install::{lms_bin, llama_server_bin_for, llama_installable, mlx_runtime_python, mlx_installable, mlx_dir_complete, mlx_model_dir, mlx_runtime_snapshot, brew_bin, set_install, install_active, install_progress};
pub use self::runtime_install::{install_snapshot, install_llama_server, runtimes, install_runtime};
mod registration;
use self::registration::{walk, external_model, managed_model_path, managed_model_dir};
pub use self::registration::{import_local, register_local, remove_installed};
mod session;

pub use self::session::{runtime_snapshot, load, mark_load_failure, unload};
mod catalog;
use self::catalog::{hf_catalog_url};
pub use self::catalog::{catalog, catalog_with_filters, details};
mod downloads;
use self::downloads::{valid_hf_revision, partial_download_path, file_sha256, promote_download_file, content_range_starts_at};
pub use self::downloads::{jobs, start_download, start_download_with, start_download_at_revision, pause_job, resume_job, cancel_job};

#[cfg(test)]
include!("../../tests/support/engine/models.tests.rs");
