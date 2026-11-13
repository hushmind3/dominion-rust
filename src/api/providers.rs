use crate::{engine::codex_oauth, state::AppState};
use axum::{extract::{Path,State},http::StatusCode,response::{IntoResponse,Response},Json};
use serde_json::{json,Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use super::response::error;

// Auth sources, provider registry and live model discovery stay independently evolvable.
mod auth_sources;
use self::auth_sources::*;
mod discovery;
use self::discovery::*;
pub use self::discovery::{provider_catalog, refresh_free_catalog, free_catalog, free_catalog_refresh};
mod cli_credentials;
use self::cli_credentials::*;
pub use self::cli_credentials::{set_cli_model, cli_credentials, import_cli_credential};
mod codex_login;
use self::codex_login::*;
pub use self::codex_login::{codex_login_start, codex_device_start, codex_login_status};
mod provider_store;
use self::provider_store::*;
pub use self::provider_store::{list_providers, save_provider, delete_provider};
mod connection_test;
use self::connection_test::*;
pub use self::connection_test::{test_provider};
mod model_list;
use self::model_list::*;
pub use self::model_list::{provider_models};

#[cfg(test)]
include!("../../tests/support/api/providers.tests.rs");
