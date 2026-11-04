mod api;
mod config;
mod db;
mod engine;
mod events;
mod state;
mod vault;

use config::Config;
use db::Store;
use serde_json::json;
use state::AppState;
use vault::Vault;

#[tokio::main]
async fn main() {
    let config = Config::from_env();
    if let Err(error) = std::fs::create_dir_all(&config.data_dir) {
        eprintln!(
            "cannot create data directory {}: {error}",
            config.data_dir.display()
        );
        std::process::exit(1);
    }

    let store = match Store::open(&config.db_path()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open database {}: {error}", config.db_path().display());
            std::process::exit(1);
        }
    };
    let vault = Vault::open(&config.key_path());
    let state = AppState::new(config.clone(), store, vault);

    // A restart cannot resume an in-flight approval prompt. Mark them so the
    // UI never offers a stale "approve" that would do nothing.
    for approval in state.store.all("approval") {
        if approval.get("status").and_then(|value| value.as_str()) == Some("pending") {
            let mut stale = approval.clone();
            stale["status"] = json!("interrupted");
            stale["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            state.store.put("approval", &stale);
        }
    }

    let app = api::router(state);

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("cannot bind 127.0.0.1:{}: {error}", config.port);
            std::process::exit(1);
        }
    };

    println!(
        "DOMINION (rust engine) listening on http://127.0.0.1:{}",
        config.port
    );
    println!("  data      : {}", config.data_dir.display());
    println!("  web       : {}", config.web_dir.display());
    println!("  workspace : {}", config.workspace.display());

    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("server error: {error}");
    }
}
