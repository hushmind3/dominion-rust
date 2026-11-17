//! Compatibility for the **unchanged** latest-main React frontend.
//! Every implemented endpoint reads/writes the existing Dominion Rust services.
//! Unsupported operations reject explicitly; no fake success responses.
use crate::{
    domain::{CreateRun,ReconcileTool,Run,RunStatus,now_ms},
    engine::Engine,
    frontend_state::{UiAgent,UiMessage,UiSession,timestamp},
    sdk::Dominion,
};
use axum::{
    extract::{Path,State,Query},http::StatusCode,
    response::{IntoResponse,Response,sse::{Event,KeepAlive,Sse}},
    routing::{get,post},Json,Router,
};
use futures_core::Stream;
use serde::Deserialize;
use serde_json::{json,Value};
use std::{convert::Infallible,sync::Arc,time::Duration};
use uuid::Uuid;

mod legacy;
mod system;
mod agents;
mod sessions;
mod chat;
mod operations;
mod memory;
mod sdk_endpoints;
use self::{legacy::*,system::*,agents::*,sessions::*,chat::*,operations::*,memory::*,sdk_endpoints::*};
type Fault=(StatusCode,Json<Value>);
fn bad(msg:impl Into<String>)->Fault{(StatusCode::BAD_REQUEST,Json(json!({"error":msg.into()})))}
fn missing(msg:impl Into<String>)->Fault{(StatusCode::NOT_FOUND,Json(json!({"error":msg.into()})))}
fn unsupported(msg:&str)->Fault{(StatusCode::NOT_IMPLEMENTED,Json(json!({"error":msg})))}
fn broken(msg:String)->Fault{(StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":msg})))}
fn busy(msg:impl Into<String>)->Fault{(StatusCode::CONFLICT,Json(json!({"error":msg.into()})))}

pub fn router()->Router<Arc<Engine>>{
    Router::new()
        .route("/health",get(health))
        .route("/status",get(status))
        .route("/system/legacy-sqlite/preview",post(legacy_sqlite_preview))
        .route("/system/legacy-sqlite/import",post(legacy_sqlite_import))
        .route("/system/legacy-sqlite/archive",post(legacy_sqlite_archive))
        .route("/system/legacy-archive",get(legacy_archive_list))
        .route("/system/legacy-archive/item",get(legacy_archive_item))
        .route("/system/legacy-archive/promote-memory",post(legacy_archive_promote_memory))
        .route("/events",get(global_events))
        .route("/settings/owner-profile",get(owner_profile).post(save_owner_profile))
        .route("/agents",get(agents).post(save_agent))
        .route("/agents/bulk-model",post(bulk_agent_model))
        .route("/agents/{id}",axum::routing::delete(delete_agent))
        .route("/sessions",get(sessions).post(create_session))
        .route("/sessions/{id}",get(get_session).delete(delete_session))
        .route("/sessions/{id}/messages",get(session_messages))
        .route("/sessions/{id}/usage",get(session_usage))
        .route("/sessions/{id}/rename",post(rename_session))
        .route("/sessions/{id}/clear",post(clear_session))
        .route("/sessions/{id}/fork",post(fork_session))
        .route("/sessions/{id}/cancel",post(cancel_session))
        .route("/chat",post(chat))
        .route("/approvals",get(approvals).post(resolve_approval))
        .route("/policy",get(policy).post(save_policy))
        .route("/jobs",get(jobs).post(create_job))
        .route("/jobs/{id}",get(job))
        .route("/jobs/{id}/cancel",post(cancel_job))
        .route("/jobs/{id}/resume",post(resume_job))
        .route("/tools",get(tools_catalog))
        .route("/executions",get(executions))
        .route("/executions/{id}/operations",get(execution_operations))
        .route("/executions/{id}/cancel",post(cancel_job))
        // Local Ollama runtime routes are owned by frontend_local_models.
        // Real external channel routes are owned by frontend_channels.
        .route("/memories",get(memories).post(save_memory))
        .route("/memories/{id}",axum::routing::delete(delete_memory))
        .route("/sdk/runs",get(sdk_runs).post(sdk_start))
        .route("/sdk/runs/{id}",get(sdk_run))
        .route("/sdk/runs/{id}/approve",post(sdk_approve))
        .route("/sdk/runs/{id}/resume",post(sdk_resume))
        .route("/sdk/runs/{id}/cancel",post(sdk_cancel))
        .route("/sdk/runs/{id}/reconcile",post(sdk_reconcile))
        // Unimplemented main features return a precise JSON error instead of
        // silently serving index.html (which the UI could mistake for success).
        .merge(crate::frontend_rooms::routes())
        .merge(crate::frontend_providers::routes())
        .merge(crate::frontend_extensions::routes())
        .merge(crate::frontend_growth::routes())
        .merge(crate::frontend_personas::routes())
        .merge(crate::frontend_system::routes())
        .merge(crate::frontend_local_models::routes())
        .merge(crate::frontend_channels::routes())
        .merge(crate::frontend_skill_import::routes())
        .merge(crate::frontend_oauth::routes())
        .merge(crate::money::api::routes())
        .fallback(unsupported_endpoint)
}
async fn unsupported_endpoint()->Fault{
    unsupported("This main frontend action has no native Rust implementation yet; the UI is preserved without fabricated functionality.")
}
