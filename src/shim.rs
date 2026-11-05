use axum::{
    body::{Body, Bytes},
    extract::{OriginalUri, State},
    http::{header, HeaderMap, HeaderValue, Method, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

const UPSTREAM: &str = "https://opencode.ai/zen/go/v1";

#[derive(Clone)]
struct Shim {
    client: reqwest::Client,
    key: String,
    session: String,
    counter: std::sync::Arc<AtomicU64>,
}

/// OpenAI-compatible local proxy for the OpenCode Go endpoint.
///
/// DOMINION -> http://127.0.0.1:4399/v1 -> https://opencode.ai/zen/go/v1
/// Streams the upstream response through unchanged.
pub async fn run(port: u16) -> Result<(), String> {
    let key = api_key()?;
    let shim = Shim {
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .map_err(|error| error.to_string())?,
        key,
        session: format!(
            "dominion-rust-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or(0)
        ),
        counter: std::sync::Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/v1/*rest", any(proxy))
        .route("/v1", any(proxy))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(shim);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| error.to_string())?;
    println!("opencode-go shim (rust) on http://127.0.0.1:{port}/v1");
    axum::serve(listener, app)
        .await
        .map_err(|error| error.to_string())
}

fn api_key() -> Result<String, String> {
    if let Ok(key) = std::env::var("OPENCODE_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    let path = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".openclaw/openclaw.json"))
        .map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    value
        .pointer("/env/vars/OPENCODE_API_KEY")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "OPENCODE_API_KEY 를 찾을 수 없습니다.".to_string())
}

async fn proxy(
    State(shim): State<Shim>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let path = uri.path().trim_start_matches("/v1");
    let query = uri
        .query()
        .map(|value| format!("?{value}"))
        .unwrap_or_default();
    let url = format!("{UPSTREAM}{path}{query}");
    let ticket = shim.counter.fetch_add(1, Ordering::Relaxed);
    let session = format!("{}-{ticket}", shim.session);

    let mut request = shim
        .client
        .request(method, &url)
        .header(header::AUTHORIZATION, format!("Bearer {}", shim.key))
        .header("x-opencode-session", session);
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(accept) = headers.get(header::ACCEPT) {
        request = request.header(header::ACCEPT, accept);
    }
    if !body.is_empty() {
        request = request.body(body);
    }

    match request.send().await {
        Ok(upstream) => {
            let status = upstream.status();
            let content_type = upstream
                .headers()
                .get(header::CONTENT_TYPE)
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("application/json"));
            let stream = upstream.bytes_stream().map(|chunk| {
                chunk.map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))
            });
            let mut response = Response::new(Body::from_stream(stream));
            *response.status_mut() = status;
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, content_type);
            response.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            response
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}
