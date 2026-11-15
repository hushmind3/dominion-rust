//! Thin HTTP adapter; the in-process Rust SDK is Dominion's primary API.
use dominion::{Dominion, Settings};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("dominion=info")))
        .init();
    let settings = Settings::from_env()?;
    let bind = settings.bind;
    let sdk = Dominion::open(settings).await?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("DOMINION Rust SDK · HTTP adapter on http://{bind}");
    axum::serve(listener, sdk.http_router()).await?;
    Ok(())
}
