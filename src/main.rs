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

    // Migrate keys written by early builds that kept `apiKey` inline. The
    // record must never carry a plaintext secret again.
    for provider in state.store.all("provider") {
        let Some(plaintext) = provider
            .get("apiKey")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(id) = provider.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        if !state.store.has_secret(&format!("provider:{id}")) {
            state.save_secret(&format!("provider:{id}"), plaintext);
        }
        let mut clean = provider.clone();
        if let serde_json::Value::Object(ref mut map) = clean {
            map.remove("apiKey");
        }
        state.store.put("provider", &clean);
        println!("migrated plaintext provider key into the vault: {id}");
    }

    // Early Rust builds stored collection routes under their plural URL
    // segment. Move them to the singular kinds the engine uses.
    for (plural, singular) in [
        ("memories", "memory"),
        ("skills", "skill"),
        ("automations", "automation"),
        ("workflows", "workflow"),
        ("rooms", "room"),
        ("channels", "channel"),
        ("jobs", "job"),
        ("executions", "execution"),
        ("goals", "goal"),
    ] {
        for record in state.store.all(plural) {
            if let Some(id) = record.get("id").and_then(|value| value.as_str()) {
                state.store.put(singular, &record);
                state.store.remove(plural, id);
            }
        }
    }

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

    engine::jobs::start(state.clone());

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
