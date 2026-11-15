//! Stateless MCP Streamable HTTP client (2026-07-28), with explicit tool approval.
//! Server endpoints and tokens are configured by the process owner, never the model.
use crate::domain::ToolCall;
use reqwest::{header, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, net::IpAddr, time::{Duration, Instant}};
use tokio::sync::Mutex;

const VERSION: &str = "2026-07-28";
const BODY_MAX: usize = 262_144;
const CACHE_SECONDS: u64 = 60;

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

    async fn rpc(&self,server:&Server,method:&str,params:Value,name:Option<&str>,extra_headers:&[(String,String)])->Result<Value,String>{
        let id=uuid::Uuid::new_v4().to_string();
        let mut params=params;
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
        if let Some(name)=name{
            builder=builder.header("Mcp-Name",encode_header(name));
        }
        for (key,value) in extra_headers {
            builder=builder.header(key.as_str(),value.as_str());
        }
        if !server.bearer_token.is_empty(){builder=builder.bearer_auth(&server.bearer_token);}
        let mut response=builder.send().await.map_err(|e|format!("MCP transport: {e}"))?;
        let status=response.status();
        let content_type=response.headers().get(header::CONTENT_TYPE).and_then(|v|v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let mut bytes=Vec::new();
        while let Some(part)=response.chunk().await.map_err(|e|e.to_string())? {
            if bytes.len().saturating_add(part.len())>BODY_MAX{return Err("MCP response exceeds 256 KB".into());}
            bytes.extend_from_slice(&part);
        }
        if !status.is_success() {
            return Err(format!("MCP HTTP {status}: {}",String::from_utf8_lossy(&bytes).chars().take(300).collect::<String>()));
        }
        let payload = if content_type.contains("text/event-stream"){
            // A stream may contain progress notifications before the final JSON-RPC response.
            let text=String::from_utf8(bytes).map_err(|e|e.to_string())?;
            let normalized=text.replace("\r\n","\n");
            let mut response=None;
            for frame in normalized.split("\n\n") {
                let data=frame.lines().filter_map(|l|l.strip_prefix("data:").map(str::trim_start)).collect::<Vec<_>>().join("\n");
                if data.is_empty(){continue;}
                let value:Value=serde_json::from_str(&data).map_err(|e|format!("MCP SSE data: {e}"))?;
                if value.get("id").and_then(Value::as_str)==Some(id.as_str()) {response=Some(value);break;}
            }
            response.ok_or("MCP SSE stream did not return a matching response")?
        } else {
            serde_json::from_slice::<Value>(&bytes).map_err(|e|format!("MCP JSON: {e}"))?
        };
        if payload.get("jsonrpc").and_then(Value::as_str)!=Some("2.0") ||
            payload.get("id").and_then(Value::as_str)!=Some(id.as_str()){
            return Err("MCP JSON-RPC response does not match request".into());
        }
        if let Some(error)=payload.get("error"){
            return Err(format!("MCP JSON-RPC error: {}",error.to_string().chars().take(1000).collect::<String>()));
        }
        payload.get("result").cloned().ok_or("MCP response has no result".into())
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
        if !call.name.starts_with("mcp__"){return Err("not an MCP tool".into());}
        let suffix=&call.name["mcp__".len()..];
        let (server_name,tool_name)=suffix.split_once("__").ok_or("invalid MCP tool alias")?;
        if !safe_name(server_name)||!safe_name(tool_name) {return Err("invalid MCP tool name".into());}
        let server=self.servers.iter().find(|s|s.name==server_name).ok_or("unknown MCP server")?;
        if !call.arguments.is_object(){return Err("MCP arguments must be object".into());}
        let listed=self.list(&server.name).await?;
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
        let result=self.rpc(server,"tools/call",json!({"name":tool_name,"arguments":call.arguments}),Some(tool_name),&headers).await?;
        if result.get("resultType").and_then(Value::as_str)==Some("input_required"){
            return Err("MCP tool requires further user input; this capability is not connected".into());
        }
        if result.get("isError").and_then(Value::as_bool)==Some(true){
            return Err(format!("MCP tool returned error: {}",result.to_string().chars().take(2500).collect::<String>()));
        }
        let summary=serde_json::to_string(&result).map_err(|e|e.to_string())?;
        Ok(summary.chars().take(16000).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::HeaderMap, routing::post, Json,Router};
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
