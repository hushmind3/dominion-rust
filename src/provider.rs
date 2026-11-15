use crate::{config::{ModelSettings, Settings}, domain::ToolCall};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct ModelClient {
    http: Client,
    strong: ModelSettings,
    economy: ModelSettings,
}
pub struct Reply {
    pub message: Value,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub cost_krw: f64,
    pub model: String,
}
impl ModelClient {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let http = Client::builder().timeout(Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
        Ok(Self { http, strong: settings.main.clone(), economy: settings.economy.clone() })
    }
    pub async fn complete(&self, messages: &[Value], tools: &[Value], economy: bool) -> Result<Reply, String> {
        if economy {
            match self.request(&self.economy, messages, tools).await {
                Ok(reply) => return Ok(reply),
                Err(error) if self.economy.base_url == self.strong.base_url && self.economy.name == self.strong.name => return Err(error),
                Err(_) => {} // explicit fallback: main model, with its actual metered cost
            }
        }
        self.request(&self.strong, messages, tools).await
    }
    async fn request(&self, settings: &ModelSettings, messages: &[Value], tools: &[Value]) -> Result<Reply, String> {
        // Retry only transient HTTP/transport failures. A completed model reply
        // is never replayed through the harness; tool side effects are journaled separately.
        for attempt in 0..3u32 {
            match self.request_once(settings,messages,tools).await {
                Ok(reply)=>return Ok(reply),
                Err(error) if retryable(&error) && attempt<2=>{
                    let delay=250u64.saturating_mul(1u64<<attempt);
                    tracing::warn!(model=%settings.name,attempt=attempt+1,delay_ms=delay,"transient provider failure; retrying");
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(error)=>return Err(error)
            }
        }
        Err("provider retries exhausted".into())
    }
    async fn request_once(&self, settings: &ModelSettings, messages: &[Value], tools: &[Value]) -> Result<Reply, String> {
        let mut body = json!({"model": settings.name, "messages": messages, "stream": false});
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(false);
        }
        let mut req = self.http.post(format!("{}/chat/completions", settings.base_url)).json(&body);
        if !settings.api_key.is_empty() { req = req.bearer_auth(&settings.api_key); }
        let response = req.send().await.map_err(|e| format!("provider transport: {e}"))?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| format!("provider body: {e}"))?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            return Err(format!("provider HTTP {status}: {}", body.chars().take(500).collect::<String>()));
        }
        let raw: Value = serde_json::from_slice(&bytes).map_err(|e| format!("invalid provider JSON: {e}"))?;
        let mut message = raw.pointer("/choices/0/message").cloned().ok_or("provider missing choices[0].message")?;
        let text = message.get("content").and_then(Value::as_str).unwrap_or_default().to_string();
        let mut calls = vec![];
        if let Some(raw_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in raw_calls {
                let id = call.get("id").and_then(Value::as_str).ok_or("tool call has no id")?.to_string();
                let name = call.pointer("/function/name").and_then(Value::as_str).ok_or("tool call has no function name")?.to_string();
                let args = call.pointer("/function/arguments").and_then(Value::as_str).ok_or("tool call has no arguments")?;
                let arguments = serde_json::from_str(args).map_err(|e| format!("invalid tool arguments: {e}"))?;
                calls.push(ToolCall { id, name, arguments });
            }
        }
        // Keep only stable, provider-accepted fields in subsequent history.
        if !message.is_object() { return Err("provider message is not an object".into()); }
        message.as_object_mut().unwrap().retain(|key, _| ["role", "content", "tool_calls"].contains(&key.as_str()));
        let input_tokens = raw.pointer("/usage/prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
        let output_tokens = raw.pointer("/usage/completion_tokens").and_then(Value::as_u64).unwrap_or(0);
        let cached_input_tokens=raw.pointer("/usage/prompt_tokens_details/cached_tokens")
            .or_else(||raw.pointer("/usage/input_tokens_details/cached_tokens"))
            .and_then(Value::as_u64).unwrap_or(0).min(input_tokens);
        let cost_krw=estimate_cost(settings,input_tokens,cached_input_tokens,output_tokens);
        Ok(Reply { message, text, calls, input_tokens, cached_input_tokens, output_tokens, cost_krw, model: settings.name.clone() })
    }
}
fn retryable(error:&str)->bool{
    error.starts_with("provider transport:") ||
    error.starts_with("provider body:") ||
    ["429","500","502","503","504"].iter().any(|status|error.starts_with(&format!("provider HTTP {status} ")))
}
fn estimate_cost(settings:&ModelSettings,input:u64,cached:u64,output:u64)->f64{
    let cached=cached.min(input);
    let noncached=input-cached;
    ((noncached as f64*settings.input_krw_per_million)
        +(cached as f64*settings.cached_input_krw_per_million)
        +(output as f64*settings.output_krw_per_million))/1_000_000.0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_policy_is_precise(){
        assert!(retryable("provider HTTP 429 Too Many Requests: throttled"));
        assert!(retryable("provider HTTP 503 Service Unavailable: maintenance"));
        assert!(retryable("provider transport: connection reset"));
        assert!(!retryable("provider HTTP 401 Unauthorized: bad token"));
        assert!(!retryable("invalid provider JSON: bad body"));
    }
    #[tokio::test]
    async fn retries_transient_provider_failure_then_accepts_real_response(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn flaky(State(count):State<Arc<AtomicUsize>>)->impl axum::response::IntoResponse{
            let attempt=count.fetch_add(1,Ordering::SeqCst);
            if attempt==0 {
                (axum::http::StatusCode::TOO_MANY_REQUESTS,Json(json!({"error":"slow down"}))).into_response()
            }else{
                (axum::http::StatusCode::OK,Json(json!({"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))).into_response()
            }
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let router=Router::new().route("/chat/completions",post(flaky)).with_state(count.clone());
        let handle=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();
        let client=ModelClient::new(&settings).unwrap();
        let reply=client.complete(&[json!({"role":"user","content":"hi"})],&[],false).await.unwrap();
        assert_eq!(reply.text,"ok");
        assert_eq!(count.load(Ordering::SeqCst),2);
        handle.abort();
    }
    #[test]
    fn billed_usage_splits_cached_and_uncached_tokens(){
        let mut settings=Settings::from_env().unwrap().main;
        settings.input_krw_per_million=1000.0;
        settings.cached_input_krw_per_million=100.0;
        settings.output_krw_per_million=2000.0;
        let cost=estimate_cost(&settings,1_000_000,400_000,100_000);
        assert!((cost-840.0).abs()<0.00001);
        assert!((estimate_cost(&settings,100,200,0)-0.01).abs()<0.00001);
    }
    #[test]
    fn empty_key_may_connect_to_local_server() {
        let settings = Settings::from_env().unwrap();
        assert!(ModelClient::new(&settings).is_ok());
    }
}
