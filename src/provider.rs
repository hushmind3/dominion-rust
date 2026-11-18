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
    /// Stable routing hint for providers that explicitly support prompt
    /// affinity. It is not evidence of a cache hit or billed-token discount.
    session_affinity:Option<String>,
}
pub struct Reply {
    pub message: Value,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    /// Actual provider reported both counters; missing is not free usage.
    pub usage_reported: bool,
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
            strong_protocol:"openai".into(),economy_protocol:"openai".into(),
            session_affinity:None })
    }
    /// Snapshot the selected provider for a single native Run. Requests
    /// never fall back to a different vendor or model without user choice.
    pub fn with_primary(&self,selected:ModelSettings)->Self{
        Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:"openai".into(),economy_protocol:"openai".into(),
            session_affinity:self.session_affinity.clone()}
    }
    pub fn with_primary_protocol(&self,selected:ModelSettings,protocol:&str)->Result<Self,String>{
        if !matches!(protocol,"openai"|"ollama"|"anthropic"){
            return Err("unsupported native model protocol".into());
        }
        Ok(Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:protocol.to_owned(),economy_protocol:protocol.to_owned(),
            session_affinity:self.session_affinity.clone()})
    }
    /// Keep the same UUID-derived routing hint across participants in one
    /// room. Only Cloudflare's documented Workers AI endpoint receives it.
    pub fn with_session_affinity(&self,identifier:&str)->Self{
        let mut selected=self.clone();
        if !identifier.is_empty()&&identifier.len()<=128
            &&identifier.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){
            selected.session_affinity=Some(identifier.to_owned());
        }
        selected
    }
    fn affinity_for(&self,settings:&ModelSettings)->Option<&str>{
        let url=reqwest::Url::parse(&settings.base_url).ok()?;
        if url.scheme()=="https"&&url.host_str()==Some("api.cloudflare.com")
            &&url.path().contains("/ai/"){
            self.session_affinity.as_deref()
        }else{None}
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
        if protocol=="ollama"{
            return self.request_ollama(settings,messages,tools).await;
        }
        let mut body = json!({"model": settings.name, "messages": messages, "stream": false});
        // Internal group-only markers are Anthropic content-block controls.
        // Never leak them as unknown JSON keys to OpenAI-compatible gateways.
        strip_internal_cache_markers(&mut body);
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
        if let Some(affinity)=self.affinity_for(settings){
            req=req.header("x-session-affinity",affinity);
        }
        if !settings.api_key.is_empty() { req = req.bearer_auth(&settings.api_key); }
        // OpenCode Go requires a routing session header on every request.
        if settings.base_url.contains("opencode.ai") {
            req = req.header("x-opencode-session", "dominion-sub4");
        }
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
        validate_completion("openai",
            raw.pointer("/choices/0/finish_reason").and_then(Value::as_str),
            !text.trim().is_empty(), !calls.is_empty())?;
        // Keep only stable, provider-accepted fields in subsequent history.
        if !message.is_object() { return Err("provider message is not an object".into()); }
        message.as_object_mut().unwrap().retain(|key, _| ["role", "content", "tool_calls"].contains(&key.as_str()));
        // Accept both Chat Completions and Responses-style gateway usage keys.
        let (input_tokens,output_tokens,cached_input_tokens,cache_write_tokens,
             usage_reported)=openai_compatible_usage(&raw);
        let cost_krw=estimate_cost(settings,input_tokens,cached_input_tokens,cache_write_tokens,output_tokens);
        Ok(Reply { message, text, calls, input_tokens, cached_input_tokens,
            cache_write_tokens,usage_reported,output_tokens,cost_krw,
            model: settings.name.clone() })
    }

    /// Native Ollama /api/chat is not the OpenAI-compatible /v1 endpoint.
    /// Keep the native JSON-object tool arguments and observed token counts,
    /// then normalize the response into the one durable Run/tool journal.
    async fn request_ollama(&self,settings:&ModelSettings,messages:&[Value],tools:&[Value])
        ->Result<Reply,String>{
        let mut body=json!({"model":settings.name,
            "messages":ollama_messages(messages)?,"stream":false});
        if !tools.is_empty(){body["tools"]=Value::Array(tools.to_vec());}
        let root=settings.base_url.trim_end_matches('/');
        let root=root.strip_suffix("/v1").unwrap_or(root);
        let mut request=self.http.post(format!("{root}/api/chat")).json(&body);
        if !settings.api_key.is_empty(){request=request.bearer_auth(&settings.api_key);}
        if settings.base_url.contains("opencode.ai"){
            request=request.header("x-opencode-session","dominion-sub4");
        }
        let response=request.send().await.map_err(|e|format!("provider transport: {e}"))?;
        let status=response.status();
        let bytes=bounded_body(response).await?;
        if !status.is_success(){
            let body=String::from_utf8_lossy(&bytes);
            return Err(format!("provider HTTP {status}: {}",
                body.chars().take(500).collect::<String>()));
        }
        let raw:Value=serde_json::from_slice(&bytes)
            .map_err(|e|format!("invalid Ollama JSON: {e}"))?;
        if raw.get("done")!=Some(&Value::Bool(true)){
            return Err("Ollama response is incomplete: done=true missing; upstream inference may already have occurred".into());
        }
        let original=raw.get("message").and_then(Value::as_object)
            .ok_or("Ollama response has no message")?;
        if original.get("role").and_then(Value::as_str)!=Some("assistant"){
            return Err("Ollama response message is not assistant".into());
        }
        let text=original.get("content").and_then(Value::as_str)
            .unwrap_or_default().to_owned();
        let mut calls=Vec::new();
        let mut normalized_calls=Vec::new();
        if let Some(raw_calls)=original.get("tool_calls"){
            let entries=raw_calls.as_array().ok_or("Ollama tool_calls must be an array")?;
            if entries.len()>16{return Err("Ollama tool_calls exceed 16 calls".into());}
            for (index,entry) in entries.iter().enumerate(){
                let name=entry.pointer("/function/name").and_then(Value::as_str)
                    .ok_or("Ollama tool call missing function name")?;
                let args=entry.pointer("/function/arguments")
                    .ok_or("Ollama tool call missing arguments")?;
                if !args.is_object(){return Err("Ollama tool arguments must be JSON objects".into());}
                let id=entry.get("id").and_then(Value::as_str)
                    .filter(|s|!s.trim().is_empty()).map(str::to_owned)
                    .unwrap_or_else(||format!("ollama-{index}-{}",uuid::Uuid::new_v4()));
                normalized_calls.push(json!({"id":id,"type":"function",
                    "function":{"name":name,"arguments":args.to_string()}}));
                calls.push(ToolCall{id,name:name.to_owned(),arguments:args.clone()});
            }
        }
        validate_completion("openai",Some(if calls.is_empty(){"stop"}else{"tool_calls"}),
            !text.trim().is_empty(),!calls.is_empty())?;
        let input_tokens=raw.get("prompt_eval_count").and_then(Value::as_u64)
            .ok_or("Ollama response missing prompt_eval_count")?;
        let output_tokens=raw.get("eval_count").and_then(Value::as_u64)
            .ok_or("Ollama response missing eval_count")?;
        let mut message=json!({"role":"assistant","content":text});
        if !normalized_calls.is_empty(){message["tool_calls"]=json!(normalized_calls);}
        Ok(Reply{
            message,text,calls,input_tokens,cached_input_tokens:0,cache_write_tokens:0,
            output_tokens,usage_reported:true,
            cost_krw:estimate_cost(settings,input_tokens,0,0,output_tokens),
            model:raw.get("model").and_then(Value::as_str)
                .unwrap_or(&settings.name).to_owned()
        })
    }

    async fn request_anthropic(&self,settings:&ModelSettings,messages:&[Value],tools:&[Value])
        ->Result<Reply,String>{
        let native_cache=anthropic_native_cache(&settings.base_url);
        let (system,conversation)=anthropic_messages(messages,native_cache)?;
        let mut body=json!({
            "model":settings.name,"messages":conversation,
            "max_tokens":4096,"stream":false
        });
        if !system.is_empty(){body["system"]=json!(system);}
        // Anthropic first-party 2026 automatic caching is explicit opt-in.
        // Gated to the canonical host so protocol-compatible proxies are not
        // sent a vendor-specific extension. Provider receipts, not the flag,
        // remain the source of truth for cached-token accounting.
        if native_cache{
            body["cache_control"]=json!({"type":"ephemeral"});
        }
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
        validate_completion("anthropic",raw.get("stop_reason").and_then(Value::as_str),
            !joined.trim().is_empty(),!calls.is_empty())?;
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
            usage_reported:true,cost_krw,model:raw.get("model").and_then(Value::as_str)
                .unwrap_or(&settings.name).to_owned()
        })
    }
}

/// Convert the persisted OpenAI-shaped transcript to Ollama's native
/// messages. Tool results must carry their originating tool name; the tool
/// ID stays in the Run journal and is not delegated to the model server.
fn ollama_messages(original:&[Value])->Result<Vec<Value>,String>{
    let mut converted=Vec::with_capacity(original.len());
    let mut pending=std::collections::HashMap::<String,String>::new();
    for record in original {
        let role=record.get("role").and_then(Value::as_str)
            .ok_or("model history entry missing role")?;
        let content=match record.get("content"){
            Some(Value::String(s))=>s.as_str(),
            Some(Value::Null)|None=>"",
            _=>return Err("Ollama history requires plain text content".into())
        };
        match role{
            "system"|"developer"=>converted.push(json!({"role":"system","content":content})),
            "user"=>converted.push(json!({"role":"user","content":content})),
            "assistant"=>{
                let mut message=json!({"role":"assistant","content":content});
                if let Some(calls)=record.get("tool_calls").and_then(Value::as_array){
                    let mut native=Vec::new();
                    for call in calls{
                        let id=call.get("id").and_then(Value::as_str)
                            .ok_or("Ollama history tool call missing ID")?;
                        let name=call.pointer("/function/name").and_then(Value::as_str)
                            .ok_or("Ollama history tool call missing name")?;
                        let raw=call.pointer("/function/arguments").and_then(Value::as_str)
                            .ok_or("Ollama history tool call missing arguments")?;
                        let arguments:Value=serde_json::from_str(raw)
                            .map_err(|e|format!("Ollama previous tool arguments invalid: {e}"))?;
                        if !arguments.is_object(){return Err("Ollama previous tool input must be an object".into());}
                        if pending.insert(id.to_owned(),name.to_owned()).is_some(){
                            return Err("Ollama history repeated tool call ID".into());
                        }
                        native.push(json!({"function":{"name":name,"arguments":arguments}}));
                    }
                    if !native.is_empty(){message["tool_calls"]=json!(native);}
                }
                converted.push(message);
            },
            "tool"=>{
                let id=record.get("tool_call_id").and_then(Value::as_str)
                    .ok_or("Ollama tool result missing call ID")?;
                let name=pending.remove(id)
                    .ok_or("Ollama orphan tool result without its assistant call")?;
                converted.push(json!({"role":"tool","tool_name":name,"content":content}));
            },
            other=>return Err(format!("unsupported native message role for Ollama: {other}"))
        }
    }
    Ok(converted)
}
/// Normalize provider-reported token counts without estimating absent usage.
fn openai_compatible_usage(raw:&Value)->(u64,u64,u64,u64,bool){
    let input=raw.pointer("/usage/prompt_tokens").and_then(Value::as_u64)
        .or_else(||raw.pointer("/usage/input_tokens").and_then(Value::as_u64));
    let output=raw.pointer("/usage/completion_tokens").and_then(Value::as_u64)
        .or_else(||raw.pointer("/usage/output_tokens").and_then(Value::as_u64));
    let reported=input.is_some() && output.is_some();
    let input=input.unwrap_or(0);
    let output=output.unwrap_or(0);
    let cached=raw.pointer("/usage/prompt_tokens_details/cached_tokens")
        .or_else(||raw.pointer("/usage/input_tokens_details/cached_tokens"))
        .or_else(||raw.pointer("/usage/cache_read_input_tokens"))
        .and_then(Value::as_u64).unwrap_or(0).min(input);
    let written=raw.pointer("/usage/prompt_tokens_details/cache_write_tokens")
        .or_else(||raw.pointer("/usage/input_tokens_details/cache_write_tokens"))
        .or_else(||raw.pointer("/usage/cache_creation_input_tokens"))
        .and_then(Value::as_u64).unwrap_or(0)
        .min(input.saturating_sub(cached));
    (input,output,cached,written,reported)
}
fn strip_internal_cache_markers(body:&mut Value){
    if let Some(list)=body["messages"].as_array_mut(){
        for message in list{
            if let Some(object)=message.as_object_mut(){
                object.remove("dominion_cache_anchor");
            }
        }
    }
}
fn anthropic_native_cache(base_url:&str)->bool{
    reqwest::Url::parse(base_url).ok().is_some_and(|url|
        url.scheme()=="https"&&url.host_str()==Some("api.anthropic.com")
            &&url.port_or_known_default()==Some(443))
}
fn anthropic_messages(original:&[Value],native_cache:bool)->Result<(String,Vec<Value>),String>{
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
            "user"=>{
                let mut block=json!({"type":"text","text":content});
                if native_cache && record.get("dominion_cache_anchor")==Some(&Value::Bool(true)){
                    block["cache_control"]=json!({"type":"ephemeral"});
                }
                messages.push(json!({"role":"user","content":[block]}));
            },
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
/// Provider finish status is part of the execution contract. A model that
/// exhausted its output budget or returned filtered/paused content has not
/// delivered a complete plan, tool request or final answer. Never feed it to
/// the harness verifier as a successful completion. This error is not
/// retryable: model usage may already have been charged upstream.
fn validate_completion(protocol:&str,reason:Option<&str>,
    has_text:bool,has_calls:bool)->Result<(),String>{
 let valid=match (protocol,reason){
  ("openai",Some("stop"))|("anthropic",Some("end_turn"|"stop_sequence"))=>
   has_text||has_calls,
  ("openai",Some("tool_calls"))|("anthropic",Some("tool_use"))=>
   has_calls,
  ("openai"|"anthropic",None)=>has_text||has_calls, // local mock/compat
  _=>false
 };
 if valid{return Ok(());}
 let label=reason.unwrap_or("unspecified");
 Err(format!(
  "model provider response incomplete or unusable ({protocol} stop_reason={label}); \
the reply was not executed or verified. Upstream model tokens may already be billed; inspect usage before retrying"))
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
    fn native_ollama_history_maps_tool_names_and_object_arguments(){
        let original=vec![
            json!({"role":"system","content":"tools"}),
            json!({"role":"user","content":"inspect"}),
            json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"call-x","type":"function","function":{
                    "name":"read_file","arguments":r#"{"path":"main.rs"}"#}}]}),
            json!({"role":"tool","tool_call_id":"call-x","content":"rust"}),
        ];
        let normalized=ollama_messages(&original).unwrap();
        assert_eq!(normalized[2]["tool_calls"][0]["function"]["arguments"]["path"],"main.rs");
        assert_eq!(normalized[3]["tool_name"],"read_file");
        assert!(normalized[2]["tool_calls"][0].get("id").is_none());
        let mut invalid=original.clone();
        invalid[3]["tool_call_id"]=json!("invented");
        assert!(ollama_messages(&invalid).unwrap_err().contains("orphan"));
    }
    #[tokio::test]
    async fn native_ollama_calls_real_api_chat_normalizes_calls_and_bills_tokens(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        async fn server(State(received):State<Arc<Mutex<Vec<Value>>>>,
            Json(body):Json<Value>)->Json<Value>{
            let mut requests=received.lock().unwrap();
            requests.push(body);
            if requests.len()==1{
                Json(json!({"model":"qwen-local","done":true,
                    "message":{"role":"assistant","content":"",
                        "tool_calls":[{"function":{"name":"read_file",
                            "arguments":{"path":"main.rs"}}}]},
                    "prompt_eval_count":120,"eval_count":13}))
            }else{
                Json(json!({"model":"qwen-local","done":true,
                    "message":{"role":"assistant","content":"Artifact inspected"},
                    "prompt_eval_count":155,"eval_count":8}))
            }
        }
        let received=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/api/chat",post(server)).with_state(received.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut settings=Settings::from_env().unwrap();
        settings.main.api_key.clear();
        settings.main.base_url=format!("http://{address}");
        settings.main.name="qwen-local".into();
        settings.main.input_krw_per_million=1000.0;
        settings.main.output_krw_per_million=2000.0;
        let client=ModelClient::new(&settings).unwrap()
            .with_primary_protocol(settings.main.clone(),"ollama").unwrap();
        let tool=json!({"type":"function","function":{"name":"read_file",
            "description":"inspect file","parameters":{"type":"object",
                "properties":{"path":{"type":"string"}},"required":["path"]}}});
        let mut history=vec![json!({"role":"user","content":"inspect main.rs"})];
        let result=client.complete(&history,&[tool],false).await.unwrap();
        assert_eq!(result.calls.len(),1);
        assert_eq!(result.calls[0].arguments["path"],"main.rs");
        assert_eq!(result.input_tokens,120);
        assert_eq!(result.output_tokens,13);
        assert!((result.cost_krw-0.146).abs()<0.000001);
        history.push(result.message);
        history.push(json!({"role":"tool","tool_call_id":result.calls[0].id,
            "content":"file text"}));
        let final_reply=client.complete(&history,&[],false).await.unwrap();
        assert_eq!(final_reply.text,"Artifact inspected");
        assert_eq!(received.lock().unwrap()[1]["messages"][2]["tool_name"],"read_file");
        assert_eq!(received.lock().unwrap()[0]["tools"][0]["function"]["name"],"read_file");
        server.abort();
    }
    #[test]
    fn explicit_anthropic_group_anchor_is_first_party_only(){
        let history=vec![
            json!({"role":"system","content":"shared"}),
            json!({"role":"user","content":"indexed owner decisions",
                "dominion_cache_anchor":true}),
            json!({"role":"user","content":"operator-selected identity"}),
        ];
        let (system,native)=anthropic_messages(&history,true).unwrap();
        assert_eq!(system,"shared");
        assert_eq!(native[0]["content"][0]["cache_control"]["type"],"ephemeral");
        assert!(native[1]["content"][0].get("cache_control").is_none());
        let (_,proxy)=anthropic_messages(&history,false).unwrap();
        assert!(proxy[0]["content"][0].get("cache_control").is_none());
    }
    #[test]
    fn annotated_group_messages_never_reach_openai_or_ollama_json_schemas(){
        let original=vec![
            json!({"role":"system","content":"common"}),
            json!({"role":"user","content":"facts","dominion_cache_anchor":true}),
            json!({"role":"user","content":"persona"})
        ];
        let mut openai=json!({"model":"mock","messages":original});
        strip_internal_cache_markers(&mut openai);
        assert!(openai["messages"][1].get("dominion_cache_anchor").is_none());
        assert!(ollama_messages(&original).unwrap().iter()
            .all(|entry|entry.get("dominion_cache_anchor").is_none()));
    }
    #[test]
    fn actual_gateway_token_usage_is_accounted_and_missing_usage_is_labelled(){
        let chat=json!({"usage":{"prompt_tokens":270,"completion_tokens":35,
            "prompt_tokens_details":{"cached_tokens":150}}});
        assert_eq!(openai_compatible_usage(&chat),(270,35,150,0,true));
        let responses=json!({"usage":{"input_tokens":480,"output_tokens":60,
            "input_tokens_details":{"cached_tokens":240,"cache_write_tokens":30}}});
        assert_eq!(openai_compatible_usage(&responses),(480,60,240,30,true));
        assert_eq!(openai_compatible_usage(&json!({"usage":{}})),(0,0,0,0,false));
        assert_eq!(openai_compatible_usage(&json!({"usage":{"input_tokens":67}})),
            (67,0,0,0,false));
    }
    #[test]
    fn cloudflare_room_affinity_is_explicit_scoped_and_not_sent_to_other_providers(){
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url="https://api.cloudflare.com/client/v4/accounts/example/ai/v1".into();
        cfg.main.name="@cf/zai-org/glm-4.7-flash".into();
        let client=ModelClient::new(&cfg).unwrap()
            .with_session_affinity("dominion-room-a1");
        assert_eq!(client.affinity_for(&cfg.main),Some("dominion-room-a1"));
        let another=ModelSettings{base_url:"https://api.openai.com/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&another),None);
        let local=ModelSettings{base_url:"http://127.0.0.1:11434/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&local),None);
        let malicious=ModelSettings{base_url:"https://api.cloudflare.com.attacker.test/ai/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&malicious),None);
        let selected=client.with_primary(cfg.main.clone());
        assert_eq!(selected.affinity_for(&cfg.main),Some("dominion-room-a1"));
        let selected=client.with_primary_protocol(cfg.main.clone(),"openai").unwrap();
        assert_eq!(selected.affinity_for(&cfg.main),Some("dominion-room-a1"));
    }
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
    fn incomplete_or_filtered_provider_output_never_looks_like_completion(){
        assert!(validate_completion("openai",Some("stop"),true,false).is_ok());
        assert!(validate_completion("openai",Some("tool_calls"),false,true).is_ok());
        assert!(validate_completion("openai",None,true,false).is_ok());
        assert!(validate_completion("openai",Some("length"),true,false).is_err());
        assert!(validate_completion("openai",Some("tool_calls"),false,false).is_err());
        assert!(validate_completion("openai",Some("content_filter"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("max_tokens"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("pause_turn"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("tool_use"),false,true).is_ok());
        assert!(validate_completion("anthropic",Some("tool_use"),false,false).is_err());
        assert!(validate_completion("anthropic",Some("end_turn"),false,false).is_err());
    }
    #[tokio::test]
    async fn truncated_billable_reply_is_not_retried_or_accepted(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn cut_off(State(count):State<Arc<AtomicUsize>>)->Json<Value>{
            count.fetch_add(1,Ordering::SeqCst);
            Json(json!({"choices":[{"finish_reason":"length","message":{
                "role":"assistant","content":"I finished the task"
            }}],"usage":{"prompt_tokens":100,"completion_tokens":1000}}))
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/chat/completions",post(cut_off))
            .with_state(Arc::clone(&count));
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.api_key.clear();
        let error=ModelClient::new(&cfg).unwrap().complete(
            &[json!({"role":"user","content":"return a file"})],&[],false).await
            .err().expect("truncated response must be rejected");
        assert!(error.contains("incomplete"));
        assert!(error.contains("already be billed"));
        assert_eq!(count.load(Ordering::SeqCst),1);
        server.abort();
    }
    #[test]
    fn automatic_anthropic_cache_is_first_party_only(){
        assert!(anthropic_native_cache("https://api.anthropic.com/v1"));
        assert!(!anthropic_native_cache("http://api.anthropic.com/v1"));
        assert!(!anthropic_native_cache("https://api.anthropic.com.evil.test/v1"));
        assert!(!anthropic_native_cache("https://proxy.example.org/anthropic"));
        assert!(!anthropic_native_cache("http://localhost:8111/v1"));
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
