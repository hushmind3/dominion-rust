use crate::{config::{ModelSettings, Settings}, domain::ToolCall};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use sha2::{Digest,Sha256};

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
    /// Inert per-client fallback. First-party OpenCode Go still gets a stable
    /// non-global session ID if a raw caller did not supply Run/Room identity.
    opencode_default_session:String,
    /// Frozen for one native Run. Defaults never emit model-specific knobs.
    run_reasoning_effort:Option<String>,
    run_thinking:Option<bool>,
}
/// A Responses API continuation receipt. Legacy Chat Completions has no
/// remote session handle, so response_id is None and keeps full prompts.
pub struct StatefulReply {
    pub reply:Reply,
    pub response_id:Option<String>,
    /// True only when the completed request really referenced remote history.
    /// A definitively rejected stale pointer can trigger a clean rebase.
    pub used_previous_response:bool,
    pub invalidated_previous_response:bool,
}
pub struct Reply {
    pub message: Value,
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    /// True only if the provider supplied cache-read telemetry, not inferred.
    pub cache_usage_reported: bool,
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
            session_affinity:None,
            opencode_default_session:uuid::Uuid::new_v4().to_string(),
            run_reasoning_effort:None,run_thinking:None })
    }
    /// Snapshot the selected provider for a single native Run. Requests
    /// never fall back to a different vendor or model without user choice.
    pub fn with_primary(&self,selected:ModelSettings)->Self{
        Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:"openai".into(),economy_protocol:"openai".into(),
            session_affinity:self.session_affinity.clone(),
            opencode_default_session:self.opencode_default_session.clone(),
            run_reasoning_effort:self.run_reasoning_effort.clone(),
            run_thinking:self.run_thinking}
    }
    pub fn with_primary_protocol(&self,selected:ModelSettings,protocol:&str)->Result<Self,String>{
        if !matches!(protocol,"openai"|"ollama"|"anthropic"|"responses"){
            return Err("unsupported native model protocol".into());
        }
        Ok(Self{http:self.http.clone(),strong:selected.clone(),economy:selected,
            strong_protocol:protocol.to_owned(),economy_protocol:protocol.to_owned(),
            session_affinity:self.session_affinity.clone(),
            opencode_default_session:self.opencode_default_session.clone(),
            run_reasoning_effort:self.run_reasoning_effort.clone(),
            run_thinking:self.run_thinking})
    }
    /// The UI receives the same exact support matrix enforced before paid
    /// dispatch. Unknown vendors remain disabled instead of suggesting that
    /// a setting can be applied when the adapter would discard it.
    pub fn run_reasoning_support(&self)->Value{
        let protocol=self.strong_protocol.as_str();
        let model=self.strong.name.to_ascii_lowercase();
        let official=self.strong.base_url=="https://api.openai.com/v1";
        let openai=official&&matches!(protocol,"openai"|"responses")
            &&matches!(model.as_str(),"gpt-5.6"|"gpt-6");
        let ollama=protocol=="ollama";
        let disable=ollama&&(model.starts_with("qwen3")||
            model.starts_with("deepseek-r1"));
        let levels=openai||(ollama&&model.starts_with("gpt-oss"));
        json!({"thinkingDisable":disable,
            "reasoningEfforts":if levels{vec!["low","medium","high"]}
                else{Vec::<&str>::new()},
            "verifiedEndpoint":disable||levels,
            "model":self.strong.name})
    }
    /// Only verified first-party transport/model combinations expose Run
    /// inference overrides. "Unknown" or operator-declared generic reasoning
    /// capability never justifies sending arbitrary vendor-specific fields.
    pub fn with_run_reasoning(&self,effort:Option<&str>,thinking:Option<bool>)
        ->Result<Self,String>{
        let effort=effort.filter(|value|!value.is_empty());
        let protocol=self.strong_protocol.as_str();
        let model=self.strong.name.to_ascii_lowercase();
        let official=self.strong.base_url=="https://api.openai.com/v1";
        let openai_reasoning=official
            &&matches!(protocol,"openai"|"responses")
            &&(model=="gpt-5.6"||model=="gpt-6");
        let ollama_boolean=protocol=="ollama" &&
            (model.starts_with("qwen3")||model.starts_with("deepseek-r1"));
        let ollama_levels=protocol=="ollama" && model.starts_with("gpt-oss");
        if thinking==Some(false) && effort.is_some(){
            return Err("choose either thinking disable or reasoning effort for one Run".into());
        }
        if thinking==Some(false) && !ollama_boolean{
            return Err("per-Run thinking disable unsupported by this exact model/endpoint".into());
        }
        if let Some(level)=effort{
            let allowed=matches!(level,"low"|"medium"|"high");
            if !allowed || !(openai_reasoning || ollama_levels){
                return Err("per-Run reasoning effort unsupported by this exact model/endpoint".into());
            }
        }
        let mut result=self.clone();
        result.run_reasoning_effort=effort.map(str::to_owned);
        result.run_thinking=thinking.filter(|enabled|!*enabled);
        Ok(result)
    }
    /// Apply ONLY provider-recognized fields to the exact outbound request.
    /// A proxy or unsupported model must not receive guessed settings.
    fn apply_run_reasoning(&self,body:&mut Value)->Result<(),String>{
        // The same validation is applied at Chat admission and here before
        // any paid request so a changed provider cannot bypass the contract.
        self.with_run_reasoning(self.run_reasoning_effort.as_deref(),
            self.run_thinking)?;
        if self.run_thinking==Some(false){
            body["think"]=json!(false);
        }
        if let Some(level)=self.run_reasoning_effort.as_deref(){
            if self.strong_protocol=="ollama"{
                body["think"]=json!(level);
            }else if self.strong_protocol=="responses"{
                body["reasoning"]=json!({"effort":level});
            }else{
                body["reasoning_effort"]=json!(level);
            }
        }
        Ok(())
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
    /// Route by conversation, never by branch-wide shared constant.
    /// Exact first-party HTTPS origin/path checks prevent leaking a session
    /// identifier to arbitrary OpenAI-compatible gateways or lookalike hosts.
    /// Only the official OpenAI endpoint receives prompt_cache_key. The
    /// operator's configured key has priority; otherwise the persisted
    /// native chat/Run affinity is stable across calls and model retries.
    fn openai_cache_key_for(&self,settings:&ModelSettings)->Option<String>{
        if settings.base_url!="https://api.openai.com/v1"{return None}
        match (settings.cache_key.as_deref(),self.session_affinity.as_deref()){
            (Some(operator),Some(session))=>{
                // A shared operator key by itself makes unrelated chats
                // contend for one ~15 RPM cache route. Bind it to a stable,
                // one-way session fingerprint without logging account IDs.
                let hash=Sha256::digest(session.as_bytes());
                let suffix=hash[..16].iter().map(|v|format!("{v:02x}")).collect::<String>();
                Some(format!("{}:{suffix}",operator.chars().take(80).collect::<String>()))
            },
            (Some(operator),None)=>Some(operator.to_owned()),
            (None,Some(session))=>Some(session.to_owned()),
            (None,None)=>None,
        }
    }
    /// DOMINION's own Cloud gateway is the only proxy that receives a
    /// per-chat cache affinity. Third-party OpenAI-compatible providers do
    /// not receive product-session identifiers or proprietary headers.
    fn dominion_cloud_affinity_for(&self,settings:&ModelSettings)->Option<String>{
        let url=reqwest::Url::parse(&settings.base_url).ok()?;
        if url.scheme()!="https"
            ||url.host_str()!=Some("dominion-cloud.dominion-hushmind.workers.dev")
            ||url.path().trim_end_matches('/')!="/v1" {
            return None;
        }
        let session=self.session_affinity.as_deref()?;
        let digest=Sha256::digest(session.as_bytes());
        Some(digest[..16].iter().map(|v|format!("{v:02x}")).collect())
    }
    /// Provider-specific header for Dominion's own Worker gateway only.
    /// The delimiter is a *message index*, not an untrusted prompt substring.
    /// The changing latest user question and pending tool results never
    /// become paid cache-write boundaries.
    fn dominion_cloud_cache_boundary_for(&self,settings:&ModelSettings,
        messages:&[Value])->Option<usize>{
        self.dominion_cloud_affinity_for(settings)?;
        messages.iter().enumerate()
            .filter(|(index,message)|*index+1<messages.len()
                &&message.get("dominion_cache_anchor")==Some(&Value::Bool(true))
                &&matches!(message["role"].as_str(),Some("user"|"system"|"developer"))
                &&message["content"].as_str().is_some_and(|text|!text.is_empty()))
            .map(|(index,_)|index).last()
    }
    fn opencode_session_for(&self,settings:&ModelSettings)->Option<&str>{
        let url=reqwest::Url::parse(&settings.base_url).ok()?;
        if url.scheme()=="https" && url.host_str()==Some("opencode.ai")
            && (url.path().starts_with("/zen/") || url.path().starts_with("/inference/go/")){
            self.session_affinity.as_deref()
                .or(Some(self.opencode_default_session.as_str()))
        }else{None}
    }
    /// Stateless Chat Completions MUST receive the full prompt on every
    /// request. Only explicit Responses transport (or the exact first-party
    /// OpenAI endpoint) may send a delta with a durable response handle.
    /// This saves uploaded bytes; prior server-side context can still be
    /// metered/billed, so the provider usage receipt remains authoritative.
    pub fn room_stateful_identity(&self,economy:bool)->Option<String>{
        let (settings,protocol)=if economy{
            (&self.economy,self.economy_protocol.as_str())
        }else{(&self.strong,self.strong_protocol.as_str())};
        if protocol=="responses"||
            (protocol=="openai"&&settings.base_url=="https://api.openai.com/v1"){
            // Qwen Code's 2026 session/cache comparison requires matching
            // endpoint, resolved model and auth identity. Persist only a
            // one-way credential fingerprint: never store the secret key.
            let credential=Sha256::digest(settings.api_key.as_bytes());
            let fingerprint=credential[..16].iter()
                .map(|byte|format!("{byte:02x}")).collect::<String>();
            Some(format!("{}|{}|{}|{}",settings.base_url,protocol,
                settings.name,fingerprint))
        }else{None}
    }
    pub async fn complete_room_continuation(&self,full:&[Value],
        delta:&[Value],previous_response_id:Option<&str>,economy:bool)
        ->Result<StatefulReply,String>{
        if self.room_stateful_identity(economy).is_none(){
            return self.complete(full,&[],economy).await.map(|reply|
                StatefulReply{reply,response_id:None,
                    used_previous_response:false,invalidated_previous_response:false});
        }
        let (settings,protocol)=if economy{
            (&self.economy,self.economy_protocol.as_str())
        }else{(&self.strong,self.strong_protocol.as_str())};
        // Single explicit request: a transport failure can follow billable
        // inference. Do not silently issue a second full-prompt generation.
        debug_assert!(protocol=="responses"||
            settings.base_url=="https://api.openai.com/v1");
        let result=self.request_responses(settings,
            if previous_response_id.is_some(){delta}else{full},
            previous_response_id).await;
        match result{
            // A 400/404 with *explicit* previous_response_id error means
            // the remote state was rejected before inference. Recover once
            // from the exact durable Room projection, not from an invented
            // model summary. Do NOT replay ambiguous 5xx or transport errors.
            Err(ref error) if previous_response_id.is_some()
                &&error.starts_with("provider previous_response_id unavailable:")=>{
                tracing::warn!(model=%settings.name,
                    "remote Responses history unavailable; rebasing once from durable room");
                let mut fresh=self.request_responses(settings,full,None).await?;
                fresh.invalidated_previous_response=true;
                Ok(fresh)
            }
            other=>other
        }
    }
    async fn request_responses(&self,settings:&ModelSettings,input:&[Value],
        previous_response_id:Option<&str>)->Result<StatefulReply,String>{
        if input.is_empty(){return Err("Responses continuation has no new input".into());}
        let mut items=Vec::with_capacity(input.len());
        for entry in input{
            let role=entry.get("role").and_then(Value::as_str)
                .ok_or("Responses input message has no role")?;
            if !matches!(role,"system"|"developer"|"user"|"assistant"){
                return Err("unsupported Responses input role".into());
            }
            let content=entry.get("content").and_then(Value::as_str)
                .ok_or("Responses input message has no text")?;
            let mut message=json!({"role":role,"content":content});
            if entry.get("dominion_cache_anchor")==Some(&Value::Bool(true)){
                message["dominion_cache_anchor"]=json!(true);
            }
            items.push(message);
        }
        let mut body=json!({"model":settings.name,"input":items,"store":true});
        self.apply_run_reasoning(&mut body)?;
        if let Some(id)=previous_response_id{
            if id.is_empty(){return Err("empty previous_response_id".into());}
            body["previous_response_id"]=json!(id);
        }
        let cache_key=self.openai_cache_key_for(settings);
        configure_openai_cache(&mut body,settings,
            cache_key.as_deref(),"input");
        let mut req=self.http.post(format!("{}/responses",settings.base_url))
            .json(&body);
        if !settings.api_key.is_empty(){req=req.bearer_auth(&settings.api_key);}
        let response=req.send().await
            .map_err(|e|format!("provider transport: {e}"))?;
        let status=response.status();
        let bytes=bounded_body(response).await?;
        if !status.is_success(){
            let detail=String::from_utf8_lossy(&bytes)
                .chars().take(500).collect::<String>();
            if previous_response_id.is_some()
                &&definitely_missing_previous_response(status.as_u16(),&bytes){
                return Err(format!(
                    "provider previous_response_id unavailable: HTTP {status}: {detail}"));
            }
            return Err(format!("provider HTTP {status}: {detail}"));
        }
        let raw:Value=serde_json::from_slice(&bytes)
            .map_err(|e|format!("invalid Responses provider JSON: {e}"))?;
        if raw.get("status").and_then(Value::as_str)!=Some("completed"){
            return Err("Responses provider did not complete generation".into());
        }
        let response_id=raw.get("id").and_then(Value::as_str)
            .filter(|id|!id.is_empty())
            .ok_or("Responses provider omitted response id")?.to_owned();
        let text=raw.get("output_text").and_then(Value::as_str)
            .filter(|s|!s.trim().is_empty()).map(str::to_owned)
            .or_else(||{
                let items=raw.get("output")?.as_array()?;
                let content=items.iter().filter(|item|
                    item.get("type").and_then(Value::as_str)==Some("message")
                    &&item.get("role").and_then(Value::as_str)==Some("assistant"))
                    .filter_map(|item|item.get("content").and_then(Value::as_array))
                    .flatten()
                    .filter(|block|block.get("type").and_then(Value::as_str)
                        ==Some("output_text"))
                    .filter_map(|block|block.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>().join("\n");
                if content.trim().is_empty(){None}else{Some(content)}
            }).ok_or("Responses provider returned no assistant output text")?;
        let (input_tokens,output_tokens,cached_input_tokens,cache_write_tokens,
             usage_reported,read_cache_known)=openai_compatible_usage(&raw);
        let cache_usage_reported=reliable_cache_read_write_receipt(&raw,settings,read_cache_known);
        let cost_krw=estimate_cost(settings,input_tokens,cached_input_tokens,
            cache_write_tokens,output_tokens);
        Ok(StatefulReply{
            reply:Reply{
                message:json!({"role":"assistant","content":text}),
                text,calls:vec![],input_tokens,cached_input_tokens,
                cache_write_tokens,cache_usage_reported,usage_reported,output_tokens,cost_krw,
                model:settings.name.clone()
            },
            response_id:Some(response_id),
            used_previous_response:previous_response_id.is_some(),
            invalidated_previous_response:false
        })
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
        if protocol=="responses"{
            if !tools.is_empty(){
                return Err("Responses transport does not yet support tool calls".into());
            }
            return self.request_responses(settings,messages,None).await.map(|v|v.reply);
        }
        if protocol=="anthropic"{
            return self.request_anthropic(settings,messages,tools).await;
        }
        if protocol=="ollama"{
            return self.request_ollama(settings,messages,tools).await;
        }
        let mut body = json!({"model": settings.name, "messages": messages, "stream": false});
        self.apply_run_reasoning(&mut body)?;
        // GPT-5.6+ requires an explicit stable-prefix boundary to avoid
        // repeatedly charging cache writes for a changing final message.
        // Compatibility gateways and earlier OpenAI models only see the
        // original string-message contract (internal markers are removed).
        let cache_key=self.openai_cache_key_for(settings);
        configure_openai_cache(&mut body,settings,
            cache_key.as_deref(),"messages");
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = json!("auto");
            body["parallel_tool_calls"] = json!(settings.parallel_tool_calls);
        }
        let mut req = self.http.post(format!("{}/chat/completions", settings.base_url)).json(&body);
        if let Some(affinity)=self.affinity_for(settings){
            req=req.header("x-session-affinity",affinity);
        }
        if let Some(fingerprint)=self.dominion_cloud_affinity_for(settings){
            req=req.header("x-dominion-chat-affinity",fingerprint);
            if let Some(boundary)=self.dominion_cloud_cache_boundary_for(settings,messages){
                req=req.header("x-dominion-cache-anchor",boundary.to_string());
            }
        }
        if !settings.api_key.is_empty() { req = req.bearer_auth(&settings.api_key); }
        // OpenCode Go conversation affinity survives retries/restarts for
        // native Runs and is isolated across unrelated Rooms and Runs.
        if let Some(session)=self.opencode_session_for(settings){
            req=req.header("x-opencode-session",session);
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
             usage_reported,read_cache_known)=openai_compatible_usage(&raw);
        let cache_usage_reported=reliable_cache_read_write_receipt(&raw,settings,read_cache_known);
        let cost_krw=estimate_cost(settings,input_tokens,cached_input_tokens,cache_write_tokens,output_tokens);
        Ok(Reply { message, text, calls, input_tokens, cached_input_tokens,
            cache_write_tokens,cache_usage_reported,usage_reported,output_tokens,cost_krw,
            model: settings.name.clone() })
    }

    /// Native Ollama /api/chat is not the OpenAI-compatible /v1 endpoint.
    /// Keep the native JSON-object tool arguments and observed token counts,
    /// then normalize the response into the one durable Run/tool journal.
    async fn request_ollama(&self,settings:&ModelSettings,messages:&[Value],tools:&[Value])
        ->Result<Reply,String>{
        let mut body=json!({"model":settings.name,
            "messages":ollama_messages(messages)?,"stream":false});
        self.apply_run_reasoning(&mut body)?;
        if !tools.is_empty(){body["tools"]=Value::Array(tools.to_vec());}
        let root=settings.base_url.trim_end_matches('/');
        let root=root.strip_suffix("/v1").unwrap_or(root);
        let mut request=self.http.post(format!("{root}/api/chat")).json(&body);
        if !settings.api_key.is_empty(){request=request.bearer_auth(&settings.api_key);}
        if let Some(session)=self.opencode_session_for(settings){
            request=request.header("x-opencode-session",session);
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
        // Ollama 2026 returns cache-inclusive prompt_eval_count. This is
        // provider-measured KV prefix reuse, not a prompt-length estimate.
        let cached_input_tokens=raw.get("prompt_eval_cached_count")
            .and_then(Value::as_u64).unwrap_or(0).min(input_tokens);
        let output_tokens=raw.get("eval_count").and_then(Value::as_u64)
            .ok_or("Ollama response missing eval_count")?;
        let mut message=json!({"role":"assistant","content":text});
        if !normalized_calls.is_empty(){message["tool_calls"]=json!(normalized_calls);}
        Ok(Reply{
            message,text,calls,input_tokens,cached_input_tokens,cache_write_tokens:0,
            output_tokens,usage_reported:true,
            cache_usage_reported:raw.get("prompt_eval_cached_count").and_then(Value::as_u64).is_some(),
            cost_krw:estimate_cost(settings,input_tokens,cached_input_tokens,0,output_tokens),
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
            usage_reported:true,
            cache_usage_reported:usage.get("cache_read_input_tokens").and_then(Value::as_u64).is_some()
                &&usage.get("cache_creation_input_tokens").and_then(Value::as_u64).is_some(),
            cost_krw,model:raw.get("model").and_then(Value::as_str)
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
/// Fail closed on ambiguous remote errors. Only an explicit provider-side
/// rejection of the previous_response_id parameter is safe for rebase/retry.
/// A network/5xx failure may have already performed billable inference.
fn definitely_missing_previous_response(status:u16,body:&[u8])->bool{
    if status!=400&&status!=404{return false;}
    let Ok(json)=serde_json::from_slice::<Value>(body) else{return false;};
    let Some(error)=json.get("error") else{return false;};
    if error.get("param").and_then(Value::as_str)!=Some("previous_response_id"){
        return false;
    }
    let code=error.get("code").and_then(Value::as_str)
        .unwrap_or("").to_ascii_lowercase();
    let msg=error.get("message").and_then(Value::as_str)
        .unwrap_or("").to_ascii_lowercase();
    matches!(code.as_str(),"previous_response_not_found"|
        "invalid_previous_response_id"|"response_not_found"|
        "not_found"|"invalid_request_error")
        &&(msg.contains("not found")||msg.contains("does not exist")||
            msg.contains("expired")||msg.contains("invalid"))
}
/// Normalize provider-reported token counts without estimating absent usage.
fn openai_compatible_usage(raw:&Value)->(u64,u64,u64,u64,bool,bool){
    let input=raw.pointer("/usage/prompt_tokens").and_then(Value::as_u64)
        .or_else(||raw.pointer("/usage/input_tokens").and_then(Value::as_u64));
    let output=raw.pointer("/usage/completion_tokens").and_then(Value::as_u64)
        .or_else(||raw.pointer("/usage/output_tokens").and_then(Value::as_u64));
    let reported=input.is_some() && output.is_some();
    let input=input.unwrap_or(0);
    let output=output.unwrap_or(0);
    let raw_cache=raw.pointer("/usage/prompt_tokens_details/cached_tokens")
        .or_else(||raw.pointer("/usage/input_tokens_details/cached_tokens"))
        .or_else(||raw.pointer("/usage/cache_read_input_tokens"))
        .and_then(Value::as_u64);
    let cached=raw_cache.unwrap_or(0).min(input);
    let written=raw.pointer("/usage/prompt_tokens_details/cache_write_tokens")
        .or_else(||raw.pointer("/usage/input_tokens_details/cache_write_tokens"))
        .or_else(||raw.pointer("/usage/cache_creation_input_tokens"))
        .and_then(Value::as_u64).unwrap_or(0)
        .min(input.saturating_sub(cached));
    (input,output,cached,written,reported,raw_cache.is_some())
}
/// For first-party GPT-5.6+ explicit cache accounting, knowing only
/// cache READ tokens does not establish that cache WRITE cost was zero.
/// Legacy providers without a separate write tariff retain their original
/// read-cache receipt semantics.
fn reliable_cache_read_write_receipt(raw:&Value,settings:&ModelSettings,read_known:bool)->bool{
    if !read_known{return false;}
    if settings.base_url!="https://api.openai.com/v1"
        ||!openai_explicit_cache_model(&settings.name){return true;}
    raw.pointer("/usage/prompt_tokens_details/cache_write_tokens")
        .or_else(||raw.pointer("/usage/input_tokens_details/cache_write_tokens"))
        .and_then(Value::as_u64).is_some()
}
/// Explicit breakpoints are a GPT-5.6+ 2026 provider contract, not a
/// generic OpenAI-compatible option. Never attach an unsupported request
/// extension to local, proxy, or older API backends.
fn openai_explicit_cache_model(name:&str)->bool{
    let Some(version)=name.strip_prefix("gpt-")else{return false};
    let first=version.split(|c:char|!c.is_ascii_digit()).next().unwrap_or("");
    let major=first.parse::<u32>().unwrap_or(0);
    if major>5{return true;}
    if major!=5{return false;}
    let Some(minor)=version.strip_prefix("5.")else{return false};
    let digits=minor.split(|c:char|!c.is_ascii_digit()).next().unwrap_or("");
    digits.parse::<u32>().is_ok_and(|n|n>=6)
}
/// Convert stable run/room annotations to the supported GPT-5.6+ typed
/// content-block shape. Markers are always stripped from the wire. When
/// no stable cache boundary exists, leave caching at its provider default.
fn configure_openai_cache(body:&mut Value,settings:&ModelSettings,
    key:Option<&str>,field:&str){
    let modern=settings.base_url=="https://api.openai.com/v1"
        &&openai_explicit_cache_model(&settings.name);
    let mut anchors=0usize;
    if let Some(items)=body.get_mut(field).and_then(Value::as_array_mut){
        for item in items {
            let marked=item.get("dominion_cache_anchor")==Some(&Value::Bool(true));
            if let Some(object)=item.as_object_mut(){
                object.remove("dominion_cache_anchor");
            }
            if marked && modern && key.is_some() {
                if let Some(text)=item.get("content").and_then(Value::as_str){
                    let kind=if field=="input"{"input_text"}else{"text"};
                    let block=if field=="input"{
                        json!({"type":kind,"text":text,
                            "prompt_cache_breakpoint":{"mode":"explicit"}})
                    }else{
                        json!({"type":kind,"text":text,
                            "prompt_cache_breakpoint":{"mode":"explicit"}})
                    };
                    item["content"]=json!([block]);
                    anchors+=1;
                }
            }
        }
    }
    if settings.base_url=="https://api.openai.com/v1"{
        if let Some(key)=key{body["prompt_cache_key"]=json!(key);}
        if modern {
            // Avoid the GPT-5.6+ default implicit cache WRITE at a changing
            // user/tool suffix. A continuation with no new explicit marker
            // may reuse an older server-side breakpoint, but never pay
            // to write a new, usually one-off trailing prompt.
            let _=anchors; // annotations were already converted above
            body["prompt_cache_options"]=json!({"mode":"explicit","ttl":"30m"});
        }
    }
}
mod contract;
use contract::{anthropic_native_cache,anthropic_messages,validate_completion,
    bounded_body,retryable,safe_fallback,estimate_cost};
#[cfg(test)]
mod tests;
