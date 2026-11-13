use crate::{engine::codex_oauth, state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use super::response::error;

// Auth sources, provider registry and live model discovery stay independently evolvable.
mod auth_sources;
use self::auth_sources::{home_file, codex_home_file, codex_auth, opencode_auth, opencode_go_key_in_auth, local_opencode_go_key, official_opencode_go_url, direct_opencode_go, import_opencode_go, opencode_api_entries, opencode_target, provider_slug, import_opencode, codex_model};
mod discovery;

pub use self::discovery::{provider_catalog, refresh_free_catalog, free_catalog, free_catalog_refresh};
mod cli_credentials;

pub use self::cli_credentials::{set_cli_model, cli_credentials, import_cli_credential};
mod codex_login;
use self::codex_login::{save_codex_provider};
pub use self::codex_login::{codex_login_start, codex_device_start, codex_login_status};
mod provider_store;
use self::provider_store::{needs_account_discovery, discover_cloudflare_account, public_provider};
pub use self::provider_store::{list_providers, save_provider, delete_provider};
mod connection_test;
use self::connection_test::{model_free_status, model_is_free};
pub use self::connection_test::{test_provider};
mod model_list;
use self::model_list::{parse_codex_models, codex_models_live};
pub use self::model_list::{provider_models};

#[cfg(test)]
include!("../../tests/support/api/providers.tests.rs");
