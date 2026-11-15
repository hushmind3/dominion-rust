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
