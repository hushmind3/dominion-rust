//! Runtime composition. The process claims its port before touching durable
//! state, so opening an already-running app cannot interrupt its approvals.
use crate::{
    api, config::Config, db::Store, engine, migrations, platform, state::AppState, vault::Vault,
};
use std::future::IntoFuture;

pub async fn run(args: Vec<String>) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--shim") {
        let port = args
            .iter()
            .position(|arg| arg == "--port")
            .and_then(|index| args.get(index + 1))
            .and_then(|value| value.parse::<u16>().ok())
            .or_else(|| std::env::var("SHIM_PORT").ok().and_then(|value| value.parse().ok()))
            .unwrap_or(4399);
        return crate::shim::run(port).await;
    }
    if platform::handle_control(&args)? {
        return Ok(());
    }
    let open_browser = platform::prepare_launch(&args);
    serve(
        Config::from_env(),
        open_browser,
        std::env::var_os("DOMINION_DESKTOP_PARENT").is_some(),
    )
    .await
}
async fn serve(mut config: Config, open_browser: bool, parent_bound: bool) -> Result<(), String> {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            println!("DOMINION already running on port {}", config.port);
            if open_browser {
                platform::open_url(config.port);
            }
            return Ok(());
        }
        Err(error) => return Err(format!("cannot bind 127.0.0.1:{}: {error}", config.port)),
    };
    config.port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    std::fs::create_dir_all(&config.data_dir).map_err(|error| {
        format!(
            "cannot create data directory {}: {error}",
            config.data_dir.display()
        )
    })?;
    let store = Store::open(&config.db_path()).map_err(|error| {
        format!(
            "cannot open database {}: {error}",
            config.db_path().display()
        )
    })?;
    let vault = Vault::open(&config.key_path());
    let state = AppState::new(config.clone(), store, vault);
    migrations::run(&state)?;
    engine::jobs::recover(&state);
    engine::wake::recover(&state);
    engine::improve::recover_finalizations(&state);
    engine::jobs::start(state.clone());
    engine::discord::start(state.clone());
    engine::telegram::start(state.clone());
    let app = api::router(state);
    println!(
        "DOMINION (rust engine) listening on http://127.0.0.1:{}",
        config.port
    );
    println!("  data      : {}", config.data_dir.display());
    println!("  web       : {}", config.web_dir.display());
    println!("  workspace : {}", config.workspace.display());
    if open_browser {
        platform::open_url(config.port);
    }
    if parent_bound {
        serve_until_parent_disconnect(listener, app, tokio::io::stdin()).await
    } else {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|error| format!("server error: {error}"))
    }
}
async fn serve_until_parent_disconnect(
    input_listener: tokio::net::TcpListener,
    app: axum::Router,
    mut parent: impl tokio::io::AsyncRead + Unpin,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let parent_closed = async {
        let mut bytes = [0u8; 64];
        loop {
            match parent.read(&mut bytes).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    };
    // SSE clients can stay open during the native host's Exit callback. Drop
    // serving immediately on EOF; returning from main drops worker/tool guards.
    tokio::select! {
        result=axum::serve(input_listener,app).into_future()=>result.map_err(|error|format!("server error: {error}")),
        _=parent_closed=>Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn duplicate_start_does_not_modify_the_running_engines_database() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let root =
            std::env::temp_dir().join(format!("dominion-duplicate-start-{}", uuid::Uuid::new_v4()));
        let config = Config {
            port: listener.local_addr().unwrap().port(),
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        store.put("approval", &json!({"id":"live","status":"pending"}));
        store.put("provider", &json!({"id":"live","apiKey":"do-not-touch"}));
        serve(config, false, false).await.unwrap();
        assert_eq!(store.get("approval", "live").unwrap()["status"], "pending");
        assert_eq!(
            store.get("provider", "live").unwrap()["apiKey"],
            "do-not-touch"
        );
        assert!(!root.join("secret.key").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn native_parent_eof_drops_server_even_with_a_live_sse_client() {
        use axum::{
            response::sse::{Event, Sse},
            routing::get,
        };
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let app=axum::Router::new().route("/events",get(||async {Sse::new(async_stream::stream! {yield Ok::<_,std::convert::Infallible>(Event::default().data("ready"));std::future::pending::<()>().await;})}));
        let (writer, reader) = tokio::io::duplex(64);
        let server = tokio::spawn(serve_until_parent_disconnect(listener, app, reader));
        let response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/events"))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        drop(writer);
        tokio::time::timeout(std::time::Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(response);
    }
}
