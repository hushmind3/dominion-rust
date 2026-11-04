pub mod collections;
pub mod compat;
pub mod core;

use crate::state::AppState;
use axum::{
    extract::OriginalUri,
    http::StatusCode,
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
        .route("/api/providers/:id", axum::routing::delete(core::delete_provider))
        .route("/api/providers/:id/models", get(core::provider_models))
        // agents
        .route("/api/agents", get(core::list_agents).post(core::save_agent))
        .route("/api/agents/:id", axum::routing::delete(core::delete_agent))
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
        .route("/api/sessions/:id/compact", post(compat::not_ported))
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
        .route("/api/mcp/catalog", get(compat::empty_list))
        .route("/api/mcp/discover", get(core::mcp_discover))
        .route("/api/skill-catalog", get(compat::skill_catalog))
        .route("/api/skill-catalog/meta", get(compat::empty_object))
        .route("/api/model-library/catalog", get(compat::empty_list))
        .route("/api/model-library/installed", get(compat::empty_list))
        .route("/api/model-library/jobs", get(compat::empty_list))
        .route("/api/model-library/runtime", get(compat::model_runtime))
        .route(
            "/api/model-library/:action",
            post(compat::not_ported).get(compat::empty_object),
        )
        .route("/api/dev/status", get(compat::dev_status))
        .route("/api/dev/:action", post(compat::not_ported).get(compat::empty_object))
        .route("/api/oauth/providers", get(compat::oauth_providers))
        .route("/api/oauth/providers/:id", post(compat::not_ported))
        .route("/api/oauth/:id/start", get(compat::not_ported))
        .route("/api/youtube/status", get(compat::youtube_status))
        .route("/api/youtube/:action", get(compat::empty_list).post(compat::not_ported))
        .route("/api/video/config", get(compat::empty_object).post(compat::not_ported))
        .route("/api/video/open-models", get(compat::empty_list))
        .route("/api/video/workflows", get(compat::empty_list))
        .route("/api/video/:action", post(compat::not_ported).get(compat::empty_object))
        .route("/api/channels/catalog", get(compat::empty_list))
        .route("/api/executions/active", get(compat::empty_list))
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
            .route(&format!("/api/{kind}/:id/:action"), post(collections::action));
    }

    let static_files = ServeDir::new(state.config.web_dir.clone())
        .append_index_html_on_directories(true);

    router
        .route("/api/*rest", any(api_not_found))
        .fallback_service(static_files)
        .layer(CorsLayer::permissive())
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
