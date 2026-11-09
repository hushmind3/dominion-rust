mod response;
#[cfg(test)]
mod tests;
pub mod algorithms;
pub mod rsi;
pub mod translation;
pub mod settings;
pub mod goals;
pub mod improvements;
pub mod executions;
pub mod tasks;
pub mod experience;
pub mod memory;
pub mod browser;
pub mod extensions;
pub mod approvals;
pub mod sessions;
pub mod agents;
pub mod providers;
pub mod benchmark;
pub mod experiments;
pub mod chat;
pub mod jobs;
pub mod system;
pub mod attachments;
pub mod channels;
pub mod collections;
pub mod compat;
pub mod dev;
pub mod gateways;
pub mod models;
pub mod oauth;
pub mod personas;
pub mod rooms;
pub mod video;

use crate::state::AppState;
use axum::{
    extract::{OriginalUri, Request, State},
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir};

pub fn router(state: AppState) -> Router {
    let mut router = Router::new()
        // core
        .route("/api/health", get(system::health))
        .route("/api/benchmark/strategies", post(benchmark::seed_strategy))
        .route("/api/benchmark/experiments", post(benchmark::record_experiment))
        .route("/api/benchmark/policies", post(benchmark::register_policy))
        .route(
            "/api/benchmark/policies/experiment",
            post(benchmark::policy_experiment),
        )
        .route(
            "/api/benchmark/policies/state",
            post(benchmark::policy_state),
        )
        .route("/api/benchmark/policy-search", post(benchmark::policy_search))
        .route("/api/benchmark/policy-patches", post(benchmark::register_patch))
        .route(
            "/api/benchmark/policy-patches/experiment",
            post(benchmark::patch_experiment),
        )
        .route("/api/benchmark/identity", get(system::benchmark_identity))
        .route("/api/status", get(system::status))
        .route(
            "/api/settings/assistant",
            get(settings::assistant_settings).post(settings::save_assistant_settings),
        )
        .route("/api/attachments", post(attachments::upload))
        .route("/api/attachments/:id/content", get(attachments::content))
        .route("/api/events", get(system::events))
        .route("/api/model-usage", get(system::model_usage))
        .route("/api/run-wakes", get(system::run_wakes))
        .route("/api/experiment-requests", get(system::experiment_requests))
        .route("/api/optimizers", get(experiments::optimizers))
        .route("/api/rsi", get(rsi::list).post(rsi::propose))
        .route("/api/rsi/start", post(rsi::start))
        .route("/api/rsi/signals", get(rsi::signals))
        .route("/api/rsi/campaigns", get(rsi::campaigns))
        .route("/api/rsi/campaigns/:id/choose", post(rsi::choose))
        .route("/api/rsi/campaigns/:id", get(rsi::campaign))
        .route("/api/rsi/:id", get(rsi::one))
        .route("/api/rsi/:id/evaluate", post(rsi::evaluate))
        .route("/api/rsi/:id/apply", post(rsi::apply))
        .route("/api/rsi/:id/compare", post(rsi::compare))
        .route("/api/rsi/:id/decide", post(rsi::decide))
        .route("/api/rsi/:id/deployment", post(rsi::deployment))
        .route("/api/algorithms", get(algorithms::list).post(algorithms::register))
        .route("/api/algorithms/evaluate", get(algorithms::evaluate))
        .route("/api/algorithms/evaluations", get(algorithms::evaluations))
        .route("/api/algorithms/generate", post(algorithms::generate))
        .route("/api/algorithms/:id/activate", post(algorithms::activate))
        .route("/api/algorithms/:id/disable", post(algorithms::disable))
        .route("/api/experiment-requests/claim", post(experiments::claim))
        .route(
            "/api/experiment-requests/release-runner",
            post(experiments::release_runner),
        )
        .route("/api/experiment-requests/:id/complete", post(experiments::complete))
        .route("/api/experiment-requests/:id/release", post(experiments::release))
        .route(
            "/api/benchmark/experiment-requests",
            post(experiments::enqueue),
        )
        .route("/api/policies", get(system::policies))
        .route("/api/scheduler-runs", get(system::scheduler_runs))
        .route("/api/policy-searches", get(system::policy_searches))
        .route("/api/policy-patches", get(system::policy_patches))
        .route("/api/system/backup", post(system::create_backup))
        .route("/api/system/backup-keys", get(system::backup_keys))
        .route(
            "/api/system/backup-keys/rotate",
            post(system::rotate_backup_keys),
        )
        .route("/api/system/secret-key", get(system::secret_key))
        .route(
            "/api/system/secret-key/rotate",
            post(system::rotate_secret_key),
        )
        .route("/api/system/backups", get(system::list_backups))
        .route("/api/system/maintenance", post(system::maintenance))
        .route("/api/system/restore", post(system::restore_backup))
        .route("/api/chat", post(chat::chat))
        .route("/api/jobs",get(jobs::list).post(jobs::create))
        .route("/api/jobs/:id",get(jobs::one))
        .route("/api/jobs/:id/cancel",post(jobs::cancel))
        .route("/api/tasks", get(tasks::list))
        .route("/api/tasks/:id", get(tasks::one))
        .route("/api/tools", get(extensions::list_tools))
        .route("/api/browser/status", get(browser::status))
        .route("/api/browser/screenshot", get(browser::screenshot))
        .route("/api/browser/action", post(browser::action))
        .route("/api/policy", get(approvals::get_policy).post(approvals::save_policy))
        .route("/api/audit", get(system::audit_log))
        .route("/api/model-route", get(system::model_route))
        .route("/api/memory/overview", get(memory::memory_overview))
        .route("/api/translate", post(translation::translate))
        .route("/api/translate/config", get(translation::translate_config))
        .route("/api/upload", post(compat::upload))
        // providers
        .route(
            "/api/providers",
            get(providers::list_providers).post(providers::save_provider),
        )
        .route(
            "/api/providers/:id",
            axum::routing::delete(providers::delete_provider),
        )
        .route("/api/providers/catalog", get(providers::provider_catalog))
        .route("/api/providers/free-catalog", get(providers::free_catalog))
        .route("/api/providers/free-catalog/refresh", post(providers::free_catalog_refresh))
        .route("/api/providers/cli", get(providers::cli_credentials))
        .route("/api/providers/codex/login", get(providers::codex_login_start))
        .route("/api/providers/codex/device", post(providers::codex_device_start))
        .route("/api/providers/codex/login/status", get(providers::codex_login_status))
        .route("/api/providers/cli/import", post(providers::import_cli_credential))
        .route("/api/providers/:id/models", get(providers::provider_models))
        .route("/api/providers/:id/test", post(providers::test_provider))
        .route("/api/gateways/freellmapi", get(gateways::freellmapi))
        .route("/api/providers/:id/gateway", get(gateways::inspect))
        // agents
        .route("/api/agents", get(agents::list_agents).post(agents::save_agent))
        .route("/api/agents/bulk-model", post(agents::bulk_agent_model))
        .route("/api/personas/import/preview", get(personas::preview))
        .route("/api/personas/import", post(personas::import))
        .route("/api/agents/:id/growth", get(agents::growth))
        .route("/api/synergy", get(agents::synergy))
        .route("/api/agents/:id/avatar", get(personas::avatar))
        .route("/api/agents/:id", axum::routing::delete(agents::delete_agent))
        .route("/api/rooms", get(rooms::list).post(rooms::save))
        .route(
            "/api/rooms/:id",
            get(rooms::one).post(rooms::update).delete(rooms::remove),
        )
        .route(
            "/api/rooms/:id/messages",
            get(rooms::messages).post(rooms::send_message),
        )
        .route("/api/rooms/:id/state", get(rooms::state))
        .route("/api/rooms/:id/knowledge", get(rooms::knowledge))
        .route("/api/rooms/:id/system-prompt", get(rooms::system_prompt))
        .route("/api/rooms/:id/knowledge/learn", post(rooms::learn))
        .route("/api/rooms/:id/run", post(rooms::run))
        .route("/api/rooms/:id/reset", post(rooms::reset))
        .route("/api/rooms/:id/stop", post(rooms::stop))
        // sessions
        .route(
            "/api/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/sessions/:id",
            get(sessions::get_session)
                .delete(sessions::delete_session)
                .post(sessions::rename_session),
        )
        .route("/api/sessions/:id/messages", get(sessions::session_messages))
        .route("/api/sessions/:id/usage", get(sessions::session_usage))
        .route("/api/sessions/:id/rename", post(sessions::rename_session))
        .route("/api/sessions/:id/clear", post(sessions::clear_session))
        .route("/api/sessions/:id/fork", post(sessions::fork_session))
        .route("/api/sessions/:id/compact", post(sessions::compact_session))
        .route("/api/sessions/:id/cancel", post(sessions::cancel_session))
        .route("/api/sessions/:id/archive", post(sessions::archive_session))
        .route("/api/sessions/:id/learn", post(sessions::learn))
        .route(
            "/api/executions/:id/operations",
            get(executions::execution_operations),
        )
        .route("/api/skills/:id/export", get(extensions::export_skill))
        .route("/api/goals", get(goals::list_goals).post(goals::create_goal))
        .route(
            "/api/goals/:id",
            get(goals::get_goal).delete(goals::delete_goal),
        )
        // approvals
        .route(
            "/api/approvals",
            get(approvals::list_approvals).post(approvals::resolve_approval),
        )
        // capabilities + commands
        .route("/api/capabilities", get(system::capabilities))
        .route("/api/commands", get(system::commands))
        // compatibility surface
        .route("/api/mcp/presets", get(extensions::mcp_presets))
        .route("/api/mcp/catalog", get(extensions::mcp_catalog))
        .route("/api/mcp/discover", get(extensions::mcp_discover))
        .route("/api/skill-catalog", get(extensions::skill_catalog))
        .route("/api/skill-catalog/meta", get(extensions::skill_catalog_meta))
        .route("/api/skill-catalog/import", post(extensions::import_skill))
        .route("/api/skills/import-repo", post(extensions::import_skill_repo))
        .route("/api/model-library/catalog", get(models::catalog))
        .route("/api/model-library/details", get(models::details))
        .route("/api/model-library/installed", get(models::installed))
        .route("/api/model-library/installed/:id", axum::routing::delete(models::remove_installed))
        .route("/api/model-library/pick-file", post(compat::pick_model))
        .route("/api/model-library/jobs", get(models::jobs))
        .route("/api/model-library/runtime", get(models::runtime))
        .route("/api/model-library/runtime/load", post(models::load))
        .route("/api/model-library/runtime/unload", post(models::unload))
        .route("/api/model-library/runtime/install", post(models::install_runtime))
        .route("/api/model-library/runtimes", get(models::runtimes))
        .route("/api/model-library/download", post(models::download))
        .route("/api/model-library/import", post(models::import))
        .route(
            "/api/model-library/jobs/:id/:action",
            post(models::job_action),
        )
        .route("/api/dev/status", get(dev::status))
        .route(
            "/api/dev/config",
            get(dev::get_config).post(dev::save_config),
        )
        .route("/api/dev/connect", post(dev::connect))
        .route("/api/dev/diff", get(dev::diff))
        .route("/api/dev/jobs/:id", get(dev::get_job))
        .route("/api/dev/preview/:action", post(dev::preview))
        .route("/api/dev/update", post(dev::update))
        .route("/api/dev/:kind", post(dev::start_job))
        .route("/api/oauth/providers", get(oauth::providers))
        .route("/api/oauth/connections", get(channels::connections))
        .route("/api/oauth/providers/:id", post(oauth::save))
        .route("/api/oauth/:id/start", get(oauth::start))
        .route("/api/oauth/:id/open", post(oauth::open))
        .route("/api/oauth/:id/callback", get(oauth::callback))
        .route("/api/oauth/:id", axum::routing::delete(oauth::disconnect))
        .route("/api/youtube/status", get(compat::youtube_status))
        .route("/api/youtube/uploads", get(compat::empty_list))
        .route("/api/youtube/uploads/:id", get(video::job))
        .route("/api/youtube/:action", post(compat::youtube_action))
        .route("/api/video/status", get(video::status))
        .route(
            "/api/video/config",
            get(video::get_config).post(video::save_config),
        )
        .route("/api/video/outputs", get(video::outputs_handler))
        .route("/api/video/locations", get(video::locations))
        .route("/api/video/models", get(video::empty_list))
        .route("/api/video/catalog", get(video::empty_list))
        .route("/api/video/files", get(video::empty_list))
        .route("/api/video/jobs/:id", get(video::job))
        .route(
            "/api/video/jobs",
            get(video::list_jobs).post(video::create_job),
        )
        .route("/api/video/jobs/:id/upload", post(video::upload))
        .route("/api/video/jobs/:id/content", get(video::content))
        .route("/api/video/jobs/:id/reveal", post(video::reveal))
        .route("/api/video/open-models", post(video::not_available))
        .route("/api/video/workflows", get(video::workflows))
        .route("/api/video/:action", post(video::not_available))
        .route("/api/system/pick-folder", post(compat::pick_folder))
        .route("/api/channels/catalog", get(channels::catalog_handler))
        .route("/api/channels", get(channels::list).post(channels::save))
        .route(
            "/api/channels/:id",
            get(channels::one)
                .post(channels::update)
                .delete(channels::remove),
        )
        .route("/api/channels/:id/diagnose", post(channels::diagnose))
        .route("/api/executions", get(executions::list))
        .route("/api/executions/active", get(executions::active_executions))
        .route("/api/improvements", get(improvements::list))
        .route("/api/experience", get(experience::list))
        .route("/api/transfer", get(experience::transfer))
        .route("/api/improvements/:id/:action", post(improvements::decide))
        .route("/api/executions/:id", get(executions::one))
        .route("/api/executions/:id/cancel", post(executions::cancel))
        .route("/api/webhooks/:id", post(compat::webhook));

    for kind in collections::COLLECTIONS {
        router = router
            .route(
                &format!("/api/{kind}"),
                get(collections::list).post(collections::save),
            )
            .route(
                &format!("/api/{kind}/:id"),
                get(collections::one)
                    .delete(collections::remove)
                    .post(collections::save),
            )
            .route(
                &format!("/api/{kind}/:id/:action"),
                post(collections::action),
            );
    }

    let static_files =
        ServeDir::new(state.config.web_dir.clone()).append_index_html_on_directories(true);

    let port = state.config.port;
    let origins = vec![
        format!("http://127.0.0.1:{port}")
            .parse::<HeaderValue>()
            .expect("origin"),
        format!("http://localhost:{port}")
            .parse::<HeaderValue>()
            .expect("origin"),
    ];
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-dominion-token"),
        ]);

    router
        .route("/api/*rest", any(api_not_found))
        .fallback_service(static_files)
        .layer(middleware::from_fn(no_stale_ui))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maintenance_guard,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), guard_writes))
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(12 * 1024 * 1024))
        .with_state(state)
}

async fn api_not_found(OriginalUri(uri): OriginalUri) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": format!("Rust 백엔드에 없는 API 경로입니다: {}", uri.path()),
            "ported": false,
        })),
    )
        .into_response()
}

/// HTML is served without a long-lived cache so a rebuilt UI is picked up
/// immediately instead of showing an old bundle after a restart.
async fn no_stale_ui(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

/// While maintenance mode is on, only reads and `/api/system/*` mutations are
/// allowed. Restore cannot race with normal writes.
async fn maintenance_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.maintenance_mode() {
        return next.run(request).await;
    }
    if request.method() == Method::GET || request.uri().path().starts_with("/api/system/") {
        return next.run(request).await;
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "유지보수 모드입니다. 복구가 끝난 뒤 다시 시도하세요.",
            "maintenance": true
        })),
    )
        .into_response()
}

/// Rejects cross-origin writes so a random website cannot drive the local
/// computer-control API. An optional admin token protects writes further.
async fn guard_writes(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    if !matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        let port = state.config.port;
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
        {
            let allowed = origin == format!("http://127.0.0.1:{port}")
                || origin == format!("http://localhost:{port}");
            if !allowed {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "cross-origin write blocked" })),
                )
                    .into_response();
            }
        }
        if let Ok(token) = std::env::var("DOMINION_ADMIN_TOKEN") {
            if !token.trim().is_empty() {
                let provided = request
                    .headers()
                    .get("x-dominion-token")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                if provided != token {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(json!({ "error": "admin token required" })),
                    )
                        .into_response();
                }
            }
        }
    }
    next.run(request).await
}
