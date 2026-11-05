mod api;
mod bootstrap;
mod config;
mod db;
mod engine;
mod events;
mod migrations;
mod platform;
mod shim;
mod state;
mod vault;

#[tokio::main]
async fn main() {
    if let Err(error) = bootstrap::run(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
