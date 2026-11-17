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
    let stats=engine.store.stats().await;
    let (agents,sessions)=engine.frontend.counts().await;
    Json(json!({
        "engine":"rust","runtime":"rust",
        "providers":usize::from(is_configured(&engine))+engine.providers.list().iter().filter(|p|p.enabled).count(),
        "agents":agents,"sessions":sessions,"jobs":stats.active,
        "executions":stats.total,"mcp":engine.mcp.server_count()
    }))
}
