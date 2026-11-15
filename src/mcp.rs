//! Stateless MCP Streamable HTTP client (2026-07-28), with explicit tool approval.
//! Server endpoints and tokens are configured by the process owner, never the model.
use crate::domain::ToolCall;
use reqwest::{header, Client, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, net::IpAddr, time::{Duration, Instant}};
use tokio::sync::{Mutex,watch};

const VERSION: &str = "2026-07-28";
const BODY_MAX: usize = 262_144;
const CACHE_SECONDS: u64 = 60;

/// HTTP transport stream cancellation never proves that the remote system
/// rolled back its work. The owner must reconcile a potentially committed
/// remote side effect before permitting another mutation.
#[derive(Debug)]
pub enum McpCallOutcome {
    Settled(Result<String,String>),
    Uncertain(String),
}
#[derive(Debug)]
enum RpcOutcome {
    Settled(Result<Value,String>),
    Uncertain(String),
}
fn classify_response(method:&str,response:Result<Value,String>)->RpcOutcome{
    match response {
        Ok(value)=>RpcOutcome::Settled(Ok(value)),
        // A valid JSON-RPC error is an observed server answer, not a
        // disconnected stream. Other protocol corruption cannot prove the
        // preceding mutation did not occur.
        Err(error) if method=="tools/call" &&
            !error.starts_with("MCP JSON-RPC error:")=>
            RpcOutcome::Uncertain(format!("MCP final response unconfirmed: {error}")),
        Err(error)=>RpcOutcome::Settled(Err(error)),
    }
}
async fn cancellation_signal(cancel:&mut Option<watch::Receiver<bool>>){
    if let Some(rx)=cancel {
        loop {
            if *rx.borrow(){return;}
            if rx.changed().await.is_err(){
                std::future::pending::<()>().await;
            }
        }
    }
    std::future::pending::<()>().await;
}
fn sse_frame_end(buffer:&[u8])->Option<(usize,usize)>{
    let lf=buffer.windows(2).position(|chunk|chunk==b"\n\n").map(|end|(end,2));
    let crlf=buffer.windows(4).position(|chunk|chunk==b"\r\n\r\n").map(|end|(end,4));
    match (lf,crlf){
        (Some(a),Some(b))=>Some(if a.0<=b.0{a}else{b}),
        (Some(a),None)=>Some(a),(None,Some(b))=>Some(b),(None,None)=>None
    }
}
fn sse_payload(frame:&[u8])->Result<Option<Value>,String>{
    let text=std::str::from_utf8(frame).map_err(|e|format!("MCP SSE UTF-8: {e}"))?;
    let mut parts=Vec::new();
    for line in text.lines(){
        let line=line.trim_end_matches('\r');
        if let Some(rest)=line.strip_prefix("data:"){
            parts.push(rest.trim_start_matches(' '));
        }
    }
    if parts.is_empty(){return Ok(None);}
    let value=serde_json::from_str(&parts.join("\n"))
        .map_err(|e|format!("MCP SSE JSON: {e}"))?;
    Ok(Some(value))
}
fn validate_rpc_payload(payload:Value,id:&str)->Result<Value,String>{
    if payload.get("jsonrpc").and_then(Value::as_str)!=Some("2.0")
        || payload.get("id").and_then(Value::as_str)!=Some(id){
        return Err("MCP JSON-RPC response does not match request".into());
    }
    if let Some(error)=payload.get("error"){
        return Err(format!("MCP JSON-RPC error: {}",error.to_string().chars().take(1000).collect::<String>()));
    }
    payload.get("result").cloned().ok_or("MCP response has no result".into())
}


#[derive(Clone, Debug, Deserialize)]
pub struct Server {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub bearer_token: String,
}

#[derive(Clone, Debug)]
struct HeaderParam {
    name: String,
    path: Vec<String>,
}

#[derive(Clone, Debug)]
struct RemoteTool {
    name: String,
    description: String,
    input_schema: Value,
    headers: Vec<HeaderParam>,
}

pub struct Mcp {
    http: Client,
    servers: Vec<Server>,
    cached: Mutex<HashMap<String, (Instant, Vec<RemoteTool>)>>,
}

fn safe_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 &&
        value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn validate_endpoint(raw: &str) -> Result<(), String> {
    let url=Url::parse(raw).map_err(|e|format!("invalid MCP URL: {e}"))?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("MCP URL may not contain credentials or fragments".into());
    }
    match url.scheme() {
        "https" => {},
        "http" => {
            let host=url.host_str().ok_or("MCP URL has no host")?;
            let local=host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok_and(|ip|ip.is_loopback());
            if !local {return Err("Non-local MCP endpoint must use HTTPS".into());}
        },
        _ => return Err("MCP URL must use HTTPS or loopback HTTP".into()),
    }
    Ok(())
}

fn nested_annotation(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(k,v)|k=="x-mcp-header" || nested_annotation(v)),
        Value::Array(items) => items.iter().any(nested_annotation),
        _ => false
    }
}
fn header_name_valid(name:&str)->bool {
    !name.is_empty() && name.len()<=64 && name.bytes().all(|c|
        c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
}
fn collect_headers(schema:&Value)->Result<Vec<HeaderParam>,String>{
    fn walk(schema:&Value, path:Vec<String>, depth:usize, found:&mut Vec<HeaderParam>)->Result<(),String>{
        if depth>8{return Err("tool schema too deeply nested".into());}
        let Some(map)=schema.as_object() else{return Err("tool input schema must be an object".into());};
        if let Some(value)=map.get("x-mcp-header"){
            let name=value.as_str().ok_or("invalid MCP header annotation")?;
            if !header_name_valid(name) || path.is_empty() {return Err("invalid MCP header annotation".into());}
            if !matches!(map.get("type").and_then(Value::as_str),Some("string"|"integer"|"boolean")) {
                return Err("MCP mirrored headers require a primitive property".into());
            }
            found.push(HeaderParam{name:name.into(),path:path.clone()});
        }
        for (key,value) in map{
            if key=="properties" {
                let properties=value.as_object().ok_or("properties must be object")?;
                for (name,child) in properties {
                    let mut child_path=path.clone();child_path.push(name.clone());
                    walk(child,child_path,depth+1,found)?;
                }
            } else if key!="x-mcp-header" && nested_annotation(value) {
                return Err("MCP header annotation occurs outside properties".into());
            }
        }
        Ok(())
    }
    let mut collected=vec![];
    walk(schema,vec![],0,&mut collected)?;
    let mut names=HashSet::new();
    for item in &collected {
        if !names.insert(item.name.to_ascii_lowercase()){return Err("duplicate MCP header annotation".into());}
    }
    Ok(collected)
}
fn encode_header(raw:&str)->String{
    let clean=!raw.is_empty() && raw.trim()==raw &&
        !raw.starts_with("=?base64?") && raw.bytes().all(|b|(32..=126).contains(&b));
    if clean {raw.into()} else {
        use base64::Engine;
        format!("=?base64?{}?=",base64::engine::general_purpose::STANDARD.encode(raw))
    }
}

impl Mcp {
    pub fn from_env()->Result<Self,String>{
        let raw=std::env::var("DOMINION_MCP_SERVERS").unwrap_or_else(|_|"[]".into());
        let servers:Vec<Server>=serde_json::from_str(&raw).map_err(|e|format!("DOMINION_MCP_SERVERS: {e}"))?;
        Self::new(servers)
    }
    pub fn new(servers:Vec<Server>)->Result<Self,String>{
        if servers.len()>8{return Err("at most 8 MCP endpoints are supported".into());}
        let mut names=HashSet::new();
        for s in &servers {
            if !safe_name(&s.name) || s.name.contains("__"){return Err("MCP server name must be an ASCII identifier without double underscore".into());}
            if !names.insert(s.name.clone()){return Err("duplicate MCP server name".into());}
            validate_endpoint(&s.url)?;
        }
        let http=Client::builder().timeout(Duration::from_secs(35)).redirect(reqwest::redirect::Policy::none())
            .build().map_err(|e|e.to_string())?;
        Ok(Self{http,servers,cached:Mutex::new(HashMap::new())})
    }
    pub fn server_count(&self)->usize{self.servers.len()}

    async fn rpc(&self,server:&Server,method:&str,params:Value,name:Option<&str>,
        extra_headers:&[(String,String)])->Result<Value,String>{
        match self.rpc_cancellable(server,method,params,name,extra_headers,None).await?{
            RpcOutcome::Settled(result)=>result,
            RpcOutcome::Uncertain(reason)=>Err(reason)
        }
    }

    async fn rpc_cancellable(&self,server:&Server,method:&str,mut params:Value,
        name:Option<&str>,extra_headers:&[(String,String)],
        mut cancellation:Option<watch::Receiver<bool>>)->Result<RpcOutcome,String>{
        let id=uuid::Uuid::new_v4().to_string();
        params["_meta"]=json!({
            "io.modelcontextprotocol/protocolVersion":VERSION,
            "io.modelcontextprotocol/clientInfo":{"name":"dominion-rust","version":"0.1.0"},
            "io.modelcontextprotocol/clientCapabilities":{}
        });
        let request=json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let mut builder=self.http.post(&server.url)
            .header(header::ACCEPT,"application/json, text/event-stream")
            .header("MCP-Protocol-Version",VERSION)
            .header("Mcp-Method",method)
            .json(&request);
        if let Some(name)=name{builder=builder.header("Mcp-Name",encode_header(name));}
        for (key,value) in extra_headers{
            builder=builder.header(key.as_str(),value.as_str());
        }
        if !server.bearer_token.is_empty(){builder=builder.bearer_auth(&server.bearer_token);}
        // Once HTTP dispatch starts, a disconnected or interrupted request
        // leaves unknown remote effects. A cancelled Streamable HTTP SSE
        // response is dropped here, sending the protocol's close signal.
        if cancellation.as_ref().is_some_and(|rx|*rx.borrow()){
            return Ok(RpcOutcome::Settled(Err("MCP cancelled before remote dispatch".into())));
        }
        let send=builder.send();
        tokio::pin!(send);
        let mut response=tokio::select!{
            biased;
            _=cancellation_signal(&mut cancellation)=>{
                return Ok(RpcOutcome::Uncertain("Remote MCP request cancelled during HTTP dispatch; server completion unknown".into()));
            }
            outcome=&mut send=>match outcome{
                Ok(response)=>response,
                Err(error)=>{
                    let detail=format!("MCP transport: {error}");
                    return Ok(if method=="tools/call"{RpcOutcome::Uncertain(detail)}
                        else{RpcOutcome::Settled(Err(detail))});
                }
            }
        };
        let status=response.status();
        if !status.is_success(){
            let error=format!("MCP HTTP {status}");
            // Authentication/validation rejection is an observed refusal.
            // A 5xx or request timeout may occur after remote execution.
            let ambiguous=status.is_server_error()||status.as_u16()==408;
            return Ok(if method=="tools/call"&&ambiguous{
                RpcOutcome::Uncertain(error)
            }else{
                RpcOutcome::Settled(Err(error))
            });
        }
        let content_type=response.headers().get(header::CONTENT_TYPE)
            .and_then(|v|v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let sse=content_type.contains("text/event-stream");
        let json_body=content_type.contains("application/json");
        if !sse&&!json_body{
            return Ok(classify_response(method,Err("MCP unsupported response Content-Type".into())));
        }
        let mut pending=Vec::<u8>::new();
        let mut consumed=0usize;
        loop{
            let part=tokio::select!{
                biased;
                _=cancellation_signal(&mut cancellation)=>{
                    // Dropping the HTTP response drops its SSE stream. Per MCP
                    // 2026-07-28 no notifications/cancelled POST is sent.
                    return Ok(RpcOutcome::Uncertain(
                        "MCP response stream closed on cancellation; remote outcome unconfirmed".into()));
                }
                outcome=response.chunk()=>outcome
            };
            let part=match part{
                Ok(part)=>part,
                Err(error)=>{
                    let detail=format!("MCP stream transport error: {error}");
                    return Ok(if method=="tools/call"{RpcOutcome::Uncertain(detail)}
                        else{RpcOutcome::Settled(Err(detail))});
                }
            };
            let Some(part)=part else{break};
            consumed=consumed.saturating_add(part.len());
            if consumed>BODY_MAX{
                let message="MCP response exceeds 256 KB".to_string();
                return Ok(if method=="tools/call"{RpcOutcome::Uncertain(message)}
                    else{RpcOutcome::Settled(Err(message))});
            }
            pending.extend_from_slice(&part);
            if sse{
                while let Some((end,delimiter))=sse_frame_end(&pending){
                    let frame=pending.drain(..end+delimiter).collect::<Vec<_>>();
                    let payload=match sse_payload(&frame[..end]){
                        Ok(Some(payload))=>payload,
                        Ok(None)=>continue,
                        Err(error)=>return Ok(classify_response(method,Err(error))),
                    };
                    if payload.get("id").and_then(Value::as_str)==Some(id.as_str()){
                        return Ok(classify_response(method,validate_rpc_payload(payload,&id)));
                    }
                    // Unrelated messages (progress, logs) are advisory only.
                    // Never mistake a notification for a final tool receipt.
                }
            }
        }
        let response=if sse{
            // Strictly require a complete matching response frame.
            Err("MCP SSE stream ended without a matching final response".into())
        }else{
            serde_json::from_slice::<Value>(&pending).map_err(|e|format!("MCP JSON: {e}"))
                .and_then(|payload|validate_rpc_payload(payload,&id))
        };
        Ok(classify_response(method,response))
    }
    async fn list(&self,name:&str)->Result<Vec<RemoteTool>,String>{
        {
            let cache=self.cached.lock().await;
            if let Some((at,tools))=cache.get(name){
                if at.elapsed()<Duration::from_secs(CACHE_SECONDS){return Ok(tools.clone());}
            }
        }
        let server=self.servers.iter().find(|s|s.name==name).ok_or("MCP server not found")?;
        let result=self.rpc(server,"tools/list",json!({}),None,&[]).await?;
        let raw=result.get("tools").and_then(Value::as_array).ok_or("MCP tools/list has no tools array")?;
        if raw.len()>80{return Err("MCP server advertises too many tools".into());}
        let mut seen=HashSet::new();
        let mut tools=Vec::new();
        for item in raw {
            let Some(name)=item.get("name").and_then(Value::as_str) else {continue;};
            if !safe_name(name) || !seen.insert(name.to_string()){continue;}
            let schema=item.get("inputSchema").cloned().unwrap_or(json!({"type":"object","properties":{}}));
            if schema.get("type").and_then(Value::as_str)!=Some("object"){continue;}
            let headers=match collect_headers(&schema){
                Ok(value)=>value,Err(error)=>{tracing::warn!(%error,%name,"ignored invalid MCP tool");continue;}
            };
            let description=item.get("description").and_then(Value::as_str).unwrap_or("").chars().take(1500).collect::<String>();
            tools.push(RemoteTool{name:name.into(),description,input_schema:schema,headers});
        }
        self.cached.lock().await.insert(name.into(),(Instant::now(),tools.clone()));
        Ok(tools)
    }
    pub async fn definitions(&self)->Vec<Value>{
        let mut combined=vec![];
        for server in &self.servers {
            match self.list(&server.name).await {
                Ok(tools)=>for item in tools{
                    let alias=format!("mcp__{}__{}",server.name,item.name);
                    combined.push(json!({"type":"function","function":{
                        "name":alias,
                        "description":format!("External MCP tool (approval required). {}: {}",server.name,item.description),
                        "parameters":item.input_schema
                    }}));
                },
                Err(error)=>tracing::warn!(name=%server.name,%error,"MCP tool discovery unavailable")
            }
        }
        combined
    }
    pub async fn call(&self,call:&ToolCall)->Result<String,String>{
        match self.call_inner(call,None).await?{
            McpCallOutcome::Settled(result)=>result,
            McpCallOutcome::Uncertain(reason)=>Err(reason),
        }
    }
    pub async fn call_cooperative(&self,call:&ToolCall,cancellation:watch::Receiver<bool>)
        ->Result<McpCallOutcome,String>{
        self.call_inner(call,Some(cancellation)).await
    }
    async fn call_inner(&self,call:&ToolCall,mut cancellation:Option<watch::Receiver<bool>>)
        ->Result<McpCallOutcome,String>{
        if !call.name.starts_with("mcp__"){return Err("not an MCP tool".into());}
        let suffix=&call.name["mcp__".len()..];
        let (server_name,tool_name)=suffix.split_once("__").ok_or("invalid MCP tool alias")?;
        if !safe_name(server_name)||!safe_name(tool_name) {return Err("invalid MCP tool name".into());}
        let server=self.servers.iter().find(|s|s.name==server_name).ok_or("unknown MCP server")?;
        if !call.arguments.is_object(){return Err("MCP arguments must be object".into());}
        let listed=tokio::select!{
            biased;
            _=cancellation_signal(&mut cancellation)=>{
                return Ok(McpCallOutcome::Settled(Err("MCP cancelled before tool call".into())));
            }
            listed=self.list(&server.name)=>listed?
        };
        if cancellation.as_ref().is_some_and(|rx|*rx.borrow()){
            return Ok(McpCallOutcome::Settled(Err("MCP cancelled before tool call".into())));
        }
        let tool=listed.iter().find(|t|t.name==tool_name).ok_or("MCP tool is not currently advertised")?;
        let mut headers=vec![];
        for parameter in &tool.headers {
            let mut value=&call.arguments;
            for part in &parameter.path {value=value.get(part).unwrap_or(&Value::Null);}
            if value.is_null(){continue;}
            let text=match value {
                Value::String(s)=>s.clone(),
                Value::Bool(b)=>b.to_string(),
                Value::Number(n) if n.is_i64()||n.is_u64()=>n.to_string(),
                _=>return Err("MCP header parameter requires string, bool or integer".into()),
            };
            if text.len()>2048{return Err("MCP header value too long".into());}
            headers.push((format!("Mcp-Param-{}",parameter.name),encode_header(&text)));
        }
        let result=match self.rpc_cancellable(server,"tools/call",
            json!({"name":tool_name,"arguments":call.arguments}),Some(tool_name),
            &headers,cancellation).await?{
            RpcOutcome::Settled(Ok(value))=>value,
            RpcOutcome::Settled(Err(error))=>return Ok(McpCallOutcome::Settled(Err(error))),
            RpcOutcome::Uncertain(reason)=>return Ok(McpCallOutcome::Uncertain(reason)),
        };
        if result.get("resultType").and_then(Value::as_str)==Some("input_required"){
            return Err("MCP tool requires further user input; this capability is not connected".into());
        }
        if result.get("isError").and_then(Value::as_bool)==Some(true){
            return Err(format!("MCP tool returned error: {}",result.to_string().chars().take(2500).collect::<String>()));
        }
        let summary=serde_json::to_string(&result).map_err(|e|e.to_string())?;
        Ok(McpCallOutcome::Settled(Ok(summary.chars().take(16000).collect())))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, routing::post, Json,Router};

    use std::{convert::Infallible,sync::{Arc,atomic::{AtomicUsize,Ordering}}};
    use axum::{body::{Body,Bytes},extract::State,response::IntoResponse};
    use tokio::sync::Notify;

    #[derive(Clone)]
    struct StreamFixture{
        started:Arc<Notify>,
        notifications:Arc<AtomicUsize>,
    }
    async fn streamable_rpc(State(fixture):State<StreamFixture>,Json(body):Json<Value>)
        ->axum::response::Response{
        let id=body["id"].clone();
        let method=body["method"].as_str().unwrap_or("");
        if method=="tools/list"{
            return Json(json!({"jsonrpc":"2.0","id":id,"result":{
                "tools":[{"name":"run","description":"Streaming test tool","inputSchema":{
                    "type":"object","properties":{"mode":{"type":"string"}},
                    "required":["mode"]}}
                ]
            }})).into_response();
        }
        if method!="tools/call"{
            fixture.notifications.fetch_add(1,Ordering::SeqCst);
            return (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":"unsupported"}))).into_response();
        }
        let mode=body["params"]["arguments"]["mode"].as_str().unwrap_or("");
        if mode=="http-500"{
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let hold=mode=="hold";
        let incomplete=mode=="incomplete";
        let started=Arc::clone(&fixture.started);
        let response=async_stream::stream!{
            let progress=json!({"jsonrpc":"2.0","method":"notifications/progress",
                "params":{"progressToken":"opaque","progress":1}});
            yield Ok::<Bytes,Infallible>(Bytes::from(format!(
                "event: message\r\ndata: {}\r\n\r\n",progress
            )));
            started.notify_one();
            if hold {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            if incomplete{return;}
            let completed=json!({"jsonrpc":"2.0","id":id,
                "result":{"content":[{"type":"text","text":"completed"}]}});
            // Split final event across chunks so parsing cannot rely on
            // frame alignment with network reads.
            let whole=format!("event: message\ndata: {}\n\n",completed);
            let split=whole.len()/2;
            yield Ok::<Bytes,Infallible>(Bytes::from(whole[..split].to_string()));
            yield Ok::<Bytes,Infallible>(Bytes::from(whole[split..].to_string()));
            // A conforming SSE server may keep a stream alive after its final
            // reply. The client must return on the matching response frame.
            tokio::time::sleep(Duration::from_secs(10)).await;
        };
        ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(response)).into_response()
    }
    async fn fixture_server()->(Mcp,StreamFixture,tokio::task::JoinHandle<()>){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let fixture=StreamFixture{
            started:Arc::new(Notify::new()),notifications:Arc::new(AtomicUsize::new(0))
        };
        let state=fixture.clone();
        let handle=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/mcp",post(streamable_rpc))
                .with_state(state)).await.unwrap();
        });
        let client=Mcp::new(vec![Server{name:"test".into(),url,bearer_token:String::new()}]).unwrap();
        (client,fixture,handle)
    }
    fn test_call(mode:&str)->ToolCall{
        ToolCall{id:"client-tool-1".into(),name:"mcp__test__run".into(),
            arguments:json!({"mode":mode})}
    }
    #[tokio::test]
    async fn sse_stream_completes_on_matching_frame_without_waiting_for_close(){
        let (client,fixture,server)=fixture_server().await;
        let response=tokio::time::timeout(Duration::from_secs(2),client.call(&test_call("fast")))
            .await.expect("SSE frame was not processed before socket close")
            .unwrap();
        assert!(response.contains("completed"));
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0);
        server.abort();
    }
    #[tokio::test]
    async fn http_stream_cancel_is_unconfirmed_not_success_or_user_error(){
        let (client,fixture,server)=fixture_server().await;
        let (tx,rx)=watch::channel(false);
        let task=tokio::spawn(async move{client.call_cooperative(&test_call("hold"),rx).await});
        tokio::time::timeout(Duration::from_secs(2),fixture.started.notified())
            .await.expect("remote MCP stream did not start");
        tx.send_replace(true);
        let outcome=tokio::time::timeout(Duration::from_secs(2),task).await
            .expect("cancelled SSE did not close promptly").unwrap().unwrap();
        assert!(matches!(outcome,McpCallOutcome::Uncertain(_)),
            "closing a remote response never proves the side effect stopped");
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0,
            "MCP 2026 HTTP must not send a cancelled notification POST");
        server.abort();
    }

    #[tokio::test]
    async fn incomplete_or_server_failed_remote_mcp_call_never_auto_retries(){
        let (client,fixture,server)=fixture_server().await;
        let (_tx,rx)=watch::channel(false);
        let incomplete=tokio::time::timeout(Duration::from_secs(2),
            client.call_cooperative(&test_call("incomplete"),rx.clone()))
            .await.expect("incomplete response should close")
            .unwrap();
        assert!(matches!(incomplete,McpCallOutcome::Uncertain(_)));
        let failed=client.call_cooperative(&test_call("http-500"),rx).await.unwrap();
        assert!(matches!(failed,McpCallOutcome::Uncertain(_)));
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0);
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_before_remote_dispatch_is_definitively_non_mutating(){
        let (client,_fixture,server)=fixture_server().await;
        let (tx,rx)=watch::channel(true);
        // Pre-cancelled tool invocation may fetch no remote result but
        // cannot dispatch tools/call.
        let outcome=client.call_cooperative(&test_call("hold"),rx).await.unwrap();
        assert!(matches!(outcome,McpCallOutcome::Settled(Err(_))));
        drop(tx);
        server.abort();
    }

    #[test]
    fn validates_endpoints_and_header_schema() {
        assert!(validate_endpoint("http://127.0.0.1:8080/mcp").is_ok());
        assert!(validate_endpoint("http://external.example.com/mcp").is_err());
        let schema=json!({"type":"object","properties":{"city":{"type":"string","x-mcp-header":"City"}}});
        assert_eq!(collect_headers(&schema).unwrap().len(),1);
        assert!(collect_headers(&json!({"type":"object","anyOf":[{"x-mcp-header":"City"}]})).is_err());
        assert_eq!(encode_header("서울"),"=?base64?7ISc7Jq4?=");
    }
    async fn endpoint(headers:HeaderMap,Json(body):Json<Value>)->Json<Value>{
        assert_eq!(headers.get("MCP-Protocol-Version").unwrap(),"2026-07-28");
        let method=body["method"].as_str().unwrap();
        assert_eq!(headers.get("Mcp-Method").unwrap(),method);
        let id=body["id"].clone();
        let result=if method=="tools/list"{
            json!({"tools":[{"name":"echo","description":"Echo value","inputSchema":{
                "type":"object","properties":{"word":{"type":"string","x-mcp-header":"Word"}},"required":["word"]}}]})
        }else{
            assert_eq!(headers.get("Mcp-Name").unwrap(),"echo");
            assert_eq!(headers.get("Mcp-Param-Word").unwrap(),"test");
            assert_eq!(body["params"]["arguments"]["word"],"test");
            json!({"content":[{"type":"text","text":"echo successful"}]})
        };
        Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    #[tokio::test]
    async fn lists_and_invokes_2026_stateless_http_tools(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,Router::new().route("/mcp",post(endpoint))).await.unwrap();});
        let mcp=Mcp::new(vec![Server{name:"local".into(),url,bearer_token:"".into()}]).unwrap();
        let tools=mcp.definitions().await;
        assert_eq!(tools[0]["function"]["name"],"mcp__local__echo");
        let answer=mcp.call(&ToolCall{id:"id".into(),name:"mcp__local__echo".into(),arguments:json!({"word":"test"})}).await.unwrap();
        assert!(answer.contains("echo successful"));
        server.abort();
    }
}
