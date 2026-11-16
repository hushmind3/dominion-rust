use crate::{config::{ModelSettings, Settings}, domain::ToolCall};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct ModelClient {
    http: Client,
    strong: ModelSettings,
    economy: ModelSettings,
    strong_protocol:String,
    economy_protocol:String,
}
pub struct Reply {
    pub message: Value,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub cost_krw: f64,
    pub model: String,
}
impl ModelClient {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let http = Client::builder().timeout(Duration::from_secs(90))
            // Never forward operator-owned credentials to a redirect target.
            .redirect(reqwest::redirect::Policy::none())
            .build().map_err(|e| e.to_string())?;
        Ok(Self { http, strong: settings.main.clone(), economy: settings.economy.clone(),
            strong_protocol:"openai".into(),economy_protocol:"openai".into() })
    }
    /// Snapshot the selected provider for a single native Run. Requests
    /// never fall back to a different vendor or model without user choice.
    pub fn with_primary(&self,selected:ModelSettings)->Self{
        Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:"openai".into(),economy_protocol:"openai".into()}
    }
    pub fn with_primary_protocol(&self,selected:ModelSettings,protocol:&str)->Result<Self,String>{
        if !matches!(protocol,"openai"|"ollama"|"anthropic"){
            return Err("unsupported native model protocol".into());
        }
        Ok(Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:protocol.to_owned(),economy_protocol:protocol.to_owned()})
    }
    pub async fn complete(&self, messages: &[Value], tools: &[Value], economy: bool) -> Result<Reply, String> {
        if economy {
            match self.request(&self.economy,&self.economy_protocol,messages,tools).await {
                Ok(reply) => return Ok(reply),
                Err(error) if self.economy.base_url == self.strong.base_url &&
                    self.economy.name == self.strong.name &&
                    self.economy_protocol==self.strong_protocol => return Err(error),
                // A provider timeout, 5xx or truncated body can occur after
                // billable inference. Never duplicate the uncertain request.
                Err(error) if !safe_fallback(&error)=>return Err(error),
                Err(_) => {} // confirmed 429 rejection; main fallback is safe
            }
        }
        self.request(&self.strong,&self.strong_protocol,messages,tools).await
    }
    async fn request(&self,settings:&ModelSettings,protocol:&str,messages:&[Value],tools:&[Value])
        ->Result<Reply,String>{
        // Only a definite pre-execution rejection may be replayed. Transport
        // resets, 5xx and body failures can happen after billable inference.
        for attempt in 0..3u32 {
            match self.request_once(settings,protocol,messages,tools).await {
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
    async fn request_once(&self, settings:&ModelSettings,protocol:&str,
        messages:&[Value],tools:&[Value])->Result<Reply,String>{
        if protocol=="anthropic"{
            return self.request_anthropic(settings,messages,tools).await;
        }
        let mut body = json!({"model": settings.name, "messages": messages, "stream": false});
        // OpenAI-specific cache controls are explicitly gated to the native
        // endpoint so OpenAI-compatible third-party servers are not broken.
        if settings.base_url=="https://api.openai.com/v1"{
            if let Some(ttl)=&settings.cache_ttl{body["prompt_cache_options"]=json!({"ttl":ttl});}
            if let Some(key)=&settings.cache_key{body["prompt_cache_key"]=json!(key);}
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(settings.parallel_tool_calls);
        }
        let mut req = self.http.post(format!("{}/chat/completions", settings.base_url)).json(&body);
        if !settings.api_key.is_empty() { req = req.bearer_auth(&settings.api_key); }
        let response = req.send().await.map_err(|e| format!("provider transport: {e}"))?;
        let status = response.status();
        let bytes = bounded_body(response).await?;
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
        let cache_write_tokens=raw.pointer("/usage/prompt_tokens_details/cache_write_tokens")
            .or_else(||raw.pointer("/usage/input_tokens_details/cache_write_tokens"))
            .and_then(Value::as_u64).unwrap_or(0)
            .min(input_tokens.saturating_sub(cached_input_tokens));
        let cost_krw=estimate_cost(settings,input_tokens,cached_input_tokens,cache_write_tokens,output_tokens);
        Ok(Reply { message, text, calls, input_tokens, cached_input_tokens, cache_write_tokens, output_tokens, cost_krw, model: settings.name.clone() })
    }

    async fn request_anthropic(&self,settings:&ModelSettings,messages:&[Value],tools:&[Value])
        ->Result<Reply,String>{
        let (system,conversation)=anthropic_messages(messages)?;
        let mut body=json!({
            "model":settings.name,"messages":conversation,
            "max_tokens":4096,"stream":false
        });
        if !system.is_empty(){body["system"]=json!(system);}
        if !tools.is_empty(){
            let mut supported=Vec::new();
            for tool in tools{
                let function=tool.get("function").ok_or("missing native tool function schema")?;
                let name=function.get("name").and_then(Value::as_str)
                    .ok_or("native tool missing name")?;
                let schema=function.get("parameters").cloned().ok_or("tool parameters missing")?;
                if !schema.is_object(){return Err("Anthropic tool schema must be an object".into());}
                supported.push(json!({
                    "name":name,"description":function.get("description").and_then(Value::as_str).unwrap_or(""),
                    "input_schema":schema
                }));
            }
            body["tools"]=json!(supported);
            body["tool_choice"]=json!({"type":"auto"});
        }
        let url=format!("{}/messages",settings.base_url.trim_end_matches('/'));
        let mut request=self.http.post(url).json(&body)
            .header("anthropic-version","2023-06-01");
        if !settings.api_key.is_empty(){
            request=request.header("x-api-key",&settings.api_key);
        }
        let response=request.send().await.map_err(|e|format!("provider transport: {e}"))?;
        let status=response.status();
        let bytes=bounded_body(response).await?;
        if !status.is_success(){
            let text=String::from_utf8_lossy(&bytes);
            return Err(format!("provider HTTP {status}: {}",
                text.chars().take(500).collect::<String>()));
        }
        let raw:Value=serde_json::from_slice(&bytes)
            .map_err(|e|format!("invalid Anthropic JSON: {e}"))?;
        if raw.get("type").and_then(Value::as_str)!=Some("message"){
            return Err("Anthropic provider did not return a message object".into());
        }
        let blocks=raw.get("content").and_then(Value::as_array)
            .ok_or("Anthropic message has no content blocks")?;
        let mut text=Vec::new();
        let mut calls=Vec::new();
        let mut tool_calls=Vec::new();
        for block in blocks{
            match block.get("type").and_then(Value::as_str){
                Some("text")=>{
                    let value=block.get("text").and_then(Value::as_str)
                        .ok_or("Anthropic text block missing text")?;
                    text.push(value.to_owned());
                },
                Some("tool_use")=>{
                    let id=block.get("id").and_then(Value::as_str)
                        .ok_or("Anthropic tool_use missing ID")?.to_owned();
                    let name=block.get("name").and_then(Value::as_str)
                        .ok_or("Anthropic tool_use missing name")?.to_owned();
                    let arguments=block.get("input").cloned()
                        .ok_or("Anthropic tool_use missing input")?;
                    if !arguments.is_object(){return Err("Anthropic tool input must be object".into());}
                    tool_calls.push(json!({
                        "id":id,"type":"function",
                        "function":{"name":name,"arguments":arguments.to_string()}
                    }));
                    calls.push(ToolCall{id,name,arguments});
                },
                Some("thinking"|"redacted_thinking")=>{},
                Some(other)=>return Err(format!("unsupported Anthropic response content block: {other}")),
                None=>return Err("Anthropic content block missing type".into()),
            }
        }
        let joined=text.join("\n");
        let mut message=json!({"role":"assistant","content":joined});
        if !tool_calls.is_empty(){message["tool_calls"]=json!(tool_calls);}
        let usage=raw.get("usage").ok_or("Anthropic usage missing")?;
        let input_tokens=usage.get("input_tokens").and_then(Value::as_u64)
            .ok_or("Anthropic billed input token count missing")?;
        let output_tokens=usage.get("output_tokens").and_then(Value::as_u64)
            .ok_or("Anthropic billed output token count missing")?;
        let cached_input_tokens=usage.get("cache_read_input_tokens")
            .and_then(Value::as_u64).unwrap_or(0);
        let cache_write_tokens=usage.get("cache_creation_input_tokens")
            .and_then(Value::as_u64).unwrap_or(0);
        let input_total=input_tokens.saturating_add(cached_input_tokens)
            .saturating_add(cache_write_tokens);
        let cost_krw=estimate_cost(settings,input_total,cached_input_tokens,
            cache_write_tokens,output_tokens);
        Ok(Reply{
            message,text:joined,calls,input_tokens:input_total,
            cached_input_tokens,cache_write_tokens,output_tokens,
            cost_krw,model:raw.get("model").and_then(Value::as_str)
                .unwrap_or(&settings.name).to_owned()
        })
    }
}

fn anthropic_messages(original:&[Value])->Result<(String,Vec<Value>),String>{
    let mut system=Vec::<String>::new();
    let mut messages=Vec::<Value>::new();
    for record in original{
        let role=record.get("role").and_then(Value::as_str)
            .ok_or("model history entry missing role")?;
        let content=match record.get("content"){
            Some(Value::String(s))=>s.as_str(),
            Some(Value::Null)|None=>"",
            _=>return Err("Anthropic model adapter requires plain-text history content".into()),
        };
        match role{
            "system"|"developer"=>{if !content.is_empty(){system.push(content.to_owned());}},
            "user"=>messages.push(json!({"role":"user","content":[
                {"type":"text","text":content}
            ]})),
            "assistant"=>{
                let mut blocks=Vec::<Value>::new();
                if !content.is_empty(){blocks.push(json!({"type":"text","text":content}));}
                if let Some(tool_calls)=record.get("tool_calls").and_then(Value::as_array){
                    for call in tool_calls{
                        let id=call.get("id").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks ID")?;
                        let name=call.pointer("/function/name").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks name")?;
                        let raw=call.pointer("/function/arguments").and_then(Value::as_str)
                            .ok_or("previous native tool call lacks arguments")?;
                        let input:Value=serde_json::from_str(raw)
                            .map_err(|e|format!("previous tool input is invalid: {e}"))?;
                        blocks.push(json!({"type":"tool_use","id":id,"name":name,"input":input}));
                    }
                }
                if blocks.is_empty(){return Err("assistant message contains no content".into());}
                messages.push(json!({"role":"assistant","content":blocks}));
            },
            "tool"=>{
                let call_id=record.get("tool_call_id").and_then(Value::as_str)
                    .ok_or("tool receipt missing associated call ID")?;
                let result=json!({"type":"tool_result","tool_use_id":call_id,"content":content});
                if let Some(last)=messages.last_mut(){
                    if last.get("role").and_then(Value::as_str)==Some("user"){
                        if let Some(blocks)=last.get_mut("content").and_then(Value::as_array_mut){
                            if blocks.iter().all(|v|v["type"]=="tool_result"){
                                blocks.push(result);continue;
                            }
                        }
                    }
                }
                messages.push(json!({"role":"user","content":[result]}));
            },
            other=>return Err(format!("unsupported native message role for Anthropic: {other}")),
        }
    }
    if messages.is_empty(){return Err("Anthropic request has no conversation messages".into());}
    Ok((system.join("\n\n"),messages))
}
const MAX_PROVIDER_RESPONSE_BYTES:usize=8*1024*1024;

/// Bounded even if Content-Length is absent or falsified.
async fn bounded_body(mut response:reqwest::Response)->Result<Vec<u8>,String>{
 if response.content_length().is_some_and(|n|n>MAX_PROVIDER_RESPONSE_BYTES as u64){
  return Err("provider response exceeds 8 MiB cap".into());
 }
 let mut body=Vec::new();
 while let Some(chunk)=response.chunk().await.map_err(|e|format!("provider body: {e}"))?{
  if body.len().saturating_add(chunk.len())>MAX_PROVIDER_RESPONSE_BYTES{
   return Err("provider response exceeds 8 MiB cap".into());
  }
  body.extend_from_slice(&chunk);
 }
 Ok(body)
}
/// 429 is a definite rejection; all other failures have uncertain billing.
fn retryable(error:&str)->bool{error.starts_with("provider HTTP 429 ")}
fn safe_fallback(error:&str)->bool{retryable(error)}
fn estimate_cost(settings:&ModelSettings,input:u64,cached:u64,written:u64,output:u64)->f64{
    let cached=cached.min(input);
    let written=written.min(input-cached);
    let noncached=input-cached-written;
    ((noncached as f64*settings.input_krw_per_million)
        +(cached as f64*settings.cached_input_krw_per_million)
        +(written as f64*settings.cache_write_krw_per_million)
        +(output as f64*settings.output_krw_per_million))/1_000_000.0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_policy_is_precise(){
        assert!(retryable("provider HTTP 429 Too Many Requests: throttled"));
        assert!(!retryable("provider HTTP 503 Service Unavailable: maintenance"));
        assert!(!retryable("provider transport: connection reset"));
        assert!(!safe_fallback("provider body: connection reset"));
        assert!(safe_fallback("provider HTTP 429 Too Many Requests: throttled"));
        assert!(!retryable("provider HTTP 401 Unauthorized: bad token"));
        assert!(!retryable("invalid provider JSON: bad body"));
    }
    #[tokio::test]
    async fn retries_transient_provider_failure_then_accepts_real_response(){
        use axum::{extract::State,response::IntoResponse,routing::post,Json,Router};
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
    #[tokio::test]
    async fn ambiguous_server_failure_is_not_replayed_or_fallback_billed(){
        use axum::{extract::State,response::IntoResponse,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn ambiguous(State(count):State<Arc<AtomicUsize>>)->impl IntoResponse{
            count.fetch_add(1,Ordering::SeqCst);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
             Json(json!({"error":"upstream may have consumed tokens"})))
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let router=Router::new().route("/chat/completions",post(ambiguous)).with_state(count.clone());
        let server=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.economy.base_url=format!("http://{address}");
        cfg.economy.name="budget-model".into();cfg.economy.api_key.clear();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.name="strong-model".into();cfg.main.api_key.clear();
        let result=ModelClient::new(&cfg).unwrap().complete(
            &[json!({"role":"user","content":"a priced generation"})],&[],true).await;
        assert!(result.err().unwrap().contains("provider HTTP 500"));
        assert_eq!(count.load(Ordering::SeqCst),1,
            "uncertain billed completion must not be retried on any model");
        server.abort();
    }
    #[test]
    fn billed_usage_splits_cached_and_uncached_tokens(){
        let mut settings=Settings::from_env().unwrap().main;
        settings.input_krw_per_million=1000.0;
        settings.cached_input_krw_per_million=100.0;
        settings.output_krw_per_million=2000.0;
        settings.cache_write_krw_per_million=2500.0;
        let cost=estimate_cost(&settings,1_000_000,400_000,0,100_000);
        assert!((cost-840.0).abs()<0.00001);
        assert!((estimate_cost(&settings,100,200,0,0)-0.01).abs()<0.00001);
        let with_writes=estimate_cost(&settings,1_000_000,400_000,100_000,100_000);
        assert!((with_writes-990.0).abs()<0.00001);
    }
    #[test]
    fn empty_key_may_connect_to_local_server() {
        let settings = Settings::from_env().unwrap();
        assert!(ModelClient::new(&settings).is_ok());
    }
}
