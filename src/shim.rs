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
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

const UPSTREAM: &str = "https://opencode.ai/zen/go/v1";

#[derive(Clone)]
struct Shim {
    client: reqwest::Client,
    key: String,
    session: String,
    counter: std::sync::Arc<AtomicU64>,
    upstream: String,
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
        upstream: UPSTREAM.into(),
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
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|error| error.to_string())?;
    let path = std::path::PathBuf::from(home).join(".openclaw/openclaw.json");
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
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
    let url = format!("{}{path}{query}", shim.upstream);
    let session = upstream_session(&shim, &headers, &body);

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

fn upstream_session(shim: &Shim, headers: &HeaderMap, body: &[u8]) -> String {
    for name in ["x-dominion-conversation", "x-opencode-session"] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty() && value.len() <= 256)
        {
            return format!("dominion-{:x}", Sha256::digest(value.as_bytes()));
        }
    }
    if let Ok(body) = serde_json::from_slice::<Value>(body) {
        if let Some(key) = body["prompt_cache_key"]
            .as_str()
            .filter(|key| !key.is_empty())
        {
            return format!(
                "dominion-{:x}",
                Sha256::digest(format!("{}:{key}", body["model"]).as_bytes())
            );
        }
    }
    // Unscoped callers keep independent requests. A DOMINION participant uses
    // its durable route identity, rather than a new upstream session per turn.
    let ticket = shim.counter.fetch_add(1, Ordering::Relaxed);
    format!("{}-{ticket}", shim.session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn proxy_preserves_participant_affinity_and_provider_usage() {
        type Seen = Arc<Mutex<Vec<(String, Value)>>>;
        async fn upstream(
            State(seen): State<Seen>,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            seen.lock().unwrap().push((
                headers["x-opencode-session"].to_str().unwrap().to_string(),
                body,
            ));
            Json(
                json!({"usage":{"prompt_tokens":2200,"completion_tokens":17,"total_tokens":2217,
                "prompt_tokens_details":{"cached_tokens":2048}}}),
            )
        }
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/chat/completions", post(upstream))
            .with_state(seen.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let shim = Shim {
            client: reqwest::Client::new(),
            key: "local-test".into(),
            session: "fallback".into(),
            counter: Arc::new(AtomicU64::new(0)),
            upstream: format!("http://{address}/v1"),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let proxy_app = Router::new()
            .route("/v1/chat/completions", post(proxy))
            .with_state(shim.clone());
        let proxy_server =
            tokio::spawn(async move { axum::serve(listener, proxy_app).await.unwrap() });
        for (index, participant) in [
            "room-a-bot-a-route-a",
            "room-a-bot-a-route-a",
            "room-a-bot-b-route-a",
            "room-b-bot-a-route-a",
        ]
        .into_iter()
        .enumerate()
        {
            let body = json!({"model":"test","messages":[{"role":"user","content":format!("turn-{index}")} ]});
            let response = reqwest::Client::new()
                .post(format!("http://{address}/v1/chat/completions"))
                .header("x-dominion-conversation", participant)
                .json(&body)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap();
            assert_eq!(
                response["usage"]["prompt_tokens_details"]["cached_tokens"],
                2048
            );
            assert_eq!(response["usage"]["prompt_tokens"], 2200);
        }
        let rows = seen.lock().unwrap();
        assert_eq!(
            rows[0].0, rows[1].0,
            "another message must not create another upstream session"
        );
        assert_ne!(rows[0].0, rows[2].0, "participants stay separate");
        assert_ne!(rows[0].0, rows[3].0, "rooms stay separate");
        assert_eq!(
            rows[1].1["messages"][0]["content"], "turn-1",
            "the proxy must preserve the actual request"
        );
        let headers = HeaderMap::new();
        let a = upstream_session(
            &shim,
            &headers,
            br#"{"model":"test","prompt_cache_key":"scope-a"}"#,
        );
        let b = upstream_session(
            &shim,
            &headers,
            br#"{"model":"test","prompt_cache_key":"scope-a","messages":["next"]}"#,
        );
        assert_eq!(
            a, b,
            "scoped clients without headers can also retain affinity"
        );
        drop(rows);
        server.abort();
        proxy_server.abort();
    }
}
