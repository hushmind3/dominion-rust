//! Sub4 main-UI system adapter; endpoint behavior preserved.
use super::*;

pub(super) async fn health()->Json<Value>{
    Json(json!({
        "ok":true,"service":"dominion","engine":"rust","runtime":"rust",
        "version":env!("CARGO_PKG_VERSION"),
        "branch":crate::build_identity::running().branch,
        "commit":crate::build_identity::running().commit,
        "dirty":crate::build_identity::running().dirty,
        "identitySource":crate::build_identity::running().source
    }))
}
pub(super) fn is_configured(engine:&Engine)->bool{
    !engine.settings.main.api_key.is_empty()
        || engine.settings.main.base_url.contains("localhost")
        || engine.settings.main.base_url.contains("127.0.0.1")
}
pub(super) async fn status(State(engine):State<Arc<Engine>>)->Json<Value>{
    let runs=engine.store.list().await;
    let agents=engine.frontend.agents().await;
    let sessions=engine.frontend.sessions().await;
    let active=runs.iter().filter(|run|matches!(run.status,RunStatus::Queued|RunStatus::Running)).count();
    Json(json!({
        "engine":"rust","runtime":"rust",
        "providers":usize::from(is_configured(&engine))+engine.providers.list().iter().filter(|p|p.enabled).count(),
        "agents":agents.len(),"sessions":sessions.len(),"jobs":active,
        "executions":runs.len(),"mcp":engine.mcp.server_count()
    }))
}
