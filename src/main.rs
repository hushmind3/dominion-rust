mod api;
mod config;
mod domain;
mod engine;
mod provider;
mod store;
mod tools;

use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_|EnvFilter::new("dominion=info"))).init();
    let settings = config::Settings::from_env()?;
    tokio::fs::create_dir_all(&settings.workspace_dir).await?;
    let store = Arc::new(store::Store::open(settings.data_dir.join("runs")).await?);
    let bind = settings.bind;
    let engine = engine::Engine::new(settings,store)?;
    let app = api::router(engine);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("Dominion listening on http://{bind}");
    axum::serve(listener,app).await?;
    Ok(())
}
