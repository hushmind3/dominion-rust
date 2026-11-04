pub mod channels;
pub mod collections;
pub mod compat;
pub mod core;
pub mod dev;
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
        .route("/api/health", get(core::health))
        .route("/api/status", get(core::status))
        .route("/api/events", get(core::events))
        .route("/api/chat", post(core::chat))
        .route("/api/tools", get(core::list_tools))
        .route("/api/policy", get(core::get_policy).post(core::save_policy))
        .route("/api/audit", get(core::audit_log))
        .route("/api/model-route", get(core::model_route))
        .route("/api/memory/overview", get(core::memory_overview))
        .route("/api/translate", post(core::translate))
        .route("/api/translate/config", get(core::translate_config))
        .route("/api/upload", post(compat::upload))
        // providers
        .route(
            "/api/providers",
            get(core::list_providers).post(core::save_provider),
        )
        .route(
            "/api/providers/:id",
            axum::routing::delete(core::delete_provider),
        )
        .route("/api/providers/:id/models", get(core::provider_models))
        // agents
        .route("/api/agents", get(core::list_agents).post(core::save_agent))
        .route("/api/agents/bulk-model", post(core::bulk_agent_model))
        .route("/api/personas/import/preview", get(personas::preview))
        .route("/api/personas/import", post(personas::import))
        .route("/api/agents/:id/avatar", get(personas::avatar))
        .route("/api/agents/:id", axum::routing::delete(core::delete_agent))
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
        .route("/api/rooms/:id/run", post(rooms::run))
        .route("/api/rooms/:id/stop", post(rooms::stop))
        // sessions
        .route(
            "/api/sessions",
            get(core::list_sessions).post(core::create_session),
        )
        .route(
            "/api/sessions/:id",
            get(core::get_session)
                .delete(core::delete_session)
                .post(core::rename_session),
        )
        .route("/api/sessions/:id/messages", get(core::session_messages))
        .route("/api/sessions/:id/rename", post(core::rename_session))
        .route("/api/sessions/:id/clear", post(core::clear_session))
        .route("/api/sessions/:id/fork", post(core::fork_session))
        .route("/api/sessions/:id/compact", post(core::compact_session))
        .route("/api/sessions/:id/cancel", post(core::cancel_session))
        .route(
            "/api/executions/:id/operations",
            get(core::execution_operations),
        )
        .route("/api/skills/:id/export", get(core::export_skill))
        .route("/api/goals", get(core::list_goals).post(core::create_goal))
        .route(
            "/api/goals/:id",
            get(core::get_goal).delete(core::delete_goal),
        )
        // approvals
        .route(
            "/api/approvals",
            get(core::list_approvals).post(core::resolve_approval),
        )
        // capabilities + commands
        .route("/api/capabilities", get(core::capabilities))
        .route("/api/commands", get(core::commands))
        // compatibility surface
        .route("/api/mcp/presets", get(core::mcp_presets))
        .route("/api/mcp/catalog", get(core::mcp_catalog))
        .route("/api/mcp/discover", get(core::mcp_discover))
        .route("/api/skill-catalog", get(core::skill_catalog))
        .route("/api/skill-catalog/meta", get(core::skill_catalog_meta))
        .route("/api/skill-catalog/import", post(core::import_skill))
        .route("/api/skills/import-repo", post(core::import_skill_repo))
        .route("/api/model-library/catalog", get(models::catalog))
        .route("/api/model-library/details", get(models::details))
        .route("/api/model-library/installed", get(models::installed))
        .route("/api/model-library/jobs", get(models::jobs))
        .route("/api/model-library/runtime", get(models::runtime))
        .route("/api/model-library/runtime/load", post(models::load))
        .route("/api/model-library/runtime/unload", post(models::unload))
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
        .route("/api/oauth/:id/callback", get(oauth::callback))
        .route("/api/oauth/:id", axum::routing::delete(oauth::disconnect))
        .route("/api/youtube/status", get(compat::youtube_status))
        .route("/api/youtube/uploads", get(compat::empty_list))
        .route("/api/youtube/uploads/:id", get(video::job))
        .route("/api/youtube/:action", post(compat::not_ported))
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
        .route("/api/video/open-models", post(video::not_available))
        .route("/api/video/workflows", get(video::empty_list))
        .route("/api/video/:action", post(video::not_available))
        .route("/api/system/pick-folder", post(compat::not_ported))
        .route("/api/channels/catalog", get(channels::catalog_handler))
        .route("/api/channels", get(channels::list).post(channels::save))
        .route(
            "/api/channels/:id",
            get(channels::one)
                .post(channels::update)
                .delete(channels::remove),
        )
        .route("/api/channels/:id/diagnose", post(channels::diagnose))
        .route("/api/executions/active", get(core::active_executions))
        .route("/api/webhooks/:id", post(compat::not_ported));

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
        .layer(middleware::from_fn_with_state(state.clone(), guard_writes))
        .layer(cors)
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
