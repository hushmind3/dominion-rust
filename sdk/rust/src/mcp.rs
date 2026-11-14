//! Native Rust callbacks exposed through Dominion's EXISTING MCP stdio client.
//! Host-owned middleware is explicit and does not replace Dominion's approvals.
use serde_json::{json, Value};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
type ToolFunction = Arc<dyn Fn(Value) -> ToolFuture + Send + Sync>;
type Before = Arc<dyn Fn(&str, &Value) -> Result<(), String> + Send + Sync>;
type After = Arc<dyn Fn(&str, &Value, &Result<Value, String>) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpTool {
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: Value) -> Self {
        Self { name: name.into(), description: description.into(), input_schema }
    }
}

struct Entry { tool: McpTool, handler: ToolFunction }

#[derive(Default)]
pub struct ToolRegistry {
    entries: BTreeMap<String, Entry>,
    before: Vec<Before>,
    after: Vec<After>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self::default() }

    /// Register an async Rust function. Only explicitly registered functions
    /// are exposed. The server's preexisting approval process still applies.
    pub fn register<F, Fut>(&mut self, tool: McpTool, callback: F) -> Result<(), String>
    where F: Fn(Value) -> Fut + Send + Sync + 'static,
          Fut: Future<Output = Result<Value, String>> + Send + 'static
    {
        if tool.name.is_empty() || tool.name.len() > 80 ||
            !tool.name.chars().all(|c|c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err("tool name must be 1-80 ASCII letters, digits, '_' or '-'".into());
        }
        if tool.input_schema["type"] != "object" {
            return Err("tool input schema must be a JSON object schema".into());
        }
        if self.entries.contains_key(&tool.name) {
            return Err(format!("duplicate tool: {}",tool.name));
        }
        let cb: ToolFunction = Arc::new(move |args| Box::pin(callback(args)));
        self.entries.insert(tool.name.clone(), Entry {tool, handler:cb});
        Ok(())
    }

    /// Host-supplied hook, prior to the actual Rust callback.
    pub fn before_tool<F>(&mut self, hook: F)
    where F: Fn(&str,&Value) -> Result<(),String> + Send + Sync + 'static {
        self.before.push(Arc::new(hook));
    }

    /// Host-supplied observer for the real result, including tool errors.
    pub fn after_tool<F>(&mut self, hook: F)
    where F: Fn(&str,&Value,&Result<Value,String>) + Send + Sync + 'static {
        self.after.push(Arc::new(hook));
    }

    pub fn list(&self) -> Vec<McpTool> {
        self.entries.values().map(|e|e.tool.clone()).collect()
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value,String> {
        let entry=self.entries.get(name).ok_or_else(||format!("unknown tool: {name}"))?;
        if !args.is_object() {return Err("tool arguments must be JSON object".into());}
        for hook in &self.before {hook(name,&args)?;}
        let result=(entry.handler)(args.clone()).await;
        for hook in &self.after {hook(name,&args,&result);}
        result
    }

    fn response(id: Value, result: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"result":result})
    }
    fn error(id: Value, code: i64, message: &str) -> Value {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
    }

    /// Handle one wire message. Notifications have no response.
    /// The protocol path is interoperable with Dominion StdioSession.
    pub async fn handle_jsonrpc(&self, req: Value) -> Option<Value> {
        let id=req.get("id").filter(|v|v.is_string() || v.is_number()).cloned()?;
        if req["jsonrpc"] != "2.0" {
            return Some(Self::error(id,-32600,"invalid JSON-RPC 2.0 message"));
        }
        let response=match req["method"].as_str() {
            Some("initialize") => Self::response(id,json!({
                "protocolVersion":"2024-11-05",
                "capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"dominion-sdk-rust","version":env!("CARGO_PKG_VERSION")}
            })),
            Some("ping") => Self::response(id,json!({})),
            Some("tools/list") => Self::response(id,json!({
                "tools":self.entries.values().map(|entry|json!({
                    "name":entry.tool.name,
                    "description":entry.tool.description,
                    "inputSchema":entry.tool.input_schema
                })).collect::<Vec<_>>()
            })),
            Some("tools/call") => {
                let name=req["params"]["name"].as_str().unwrap_or("");
                let args=req["params"].get("arguments").cloned().unwrap_or_else(||json!({}));
                if !self.entries.contains_key(name) || !args.is_object() {
                    Self::error(id,-32602,"unknown tool or non-object arguments")
                } else {
                    let result=self.call(name,args).await;
                    let (body,is_error)=match result {Ok(value)=>(value.to_string(),false),Err(error)=>(error,true)};
                    Self::response(id,json!({"content":[{"type":"text","text":body}],"isError":is_error}))
                }
            }
            _ => Self::error(id,-32601,"unknown RPC method"),
        };
        Some(response)
    }

    /// Reads newline-delimited MCP JSON-RPC from stdin, returns clean JSON
    /// on stdout. Existing Dominion Rust MCP client launches this process.
    pub async fn serve_stdio(&self) -> io::Result<()> {
        let mut input=BufReader::new(io::stdin()).lines();
        let mut output=io::stdout();
        while let Some(line)=input.next_line().await? {
            if line.trim().is_empty(){continue;}
            let response=match serde_json::from_str::<Value>(&line) {
                Ok(value)=>self.handle_jsonrpc(value).await,
                Err(_)=>Some(Self::error(Value::Null,-32700,"invalid JSON")),
            };
            if let Some(response)=response {
                output.write_all(response.to_string().as_bytes()).await?;
                output.write_all(b"\n").await?;
                output.flush().await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::{AtomicUsize,Ordering},Arc};
    #[tokio::test]
    async fn jsonrpc_tool_registration_and_hooks_execute_real_rust_callbacks() {
        let counter=Arc::new(AtomicUsize::new(0));
        let mut registry=ToolRegistry::new();
        registry.register(McpTool::new("count","Count words",json!({"type":"object"})),
            |args| async move {Ok(json!({"count":args["text"].as_str().unwrap_or("").split_whitespace().count()}))}
        ).unwrap();
        let copy=counter.clone();
        registry.after_tool(move |_,_,_|{copy.fetch_add(1,Ordering::SeqCst);});
        let init=registry.handle_jsonrpc(json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).await.unwrap();
        assert_eq!(init["result"]["protocolVersion"],"2024-11-05");
        let listing=registry.handle_jsonrpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await.unwrap();
        assert_eq!(listing["result"]["tools"][0]["name"],"count");
        assert!(registry.handle_jsonrpc(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await.is_none());
        let outcome=registry.handle_jsonrpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"count","arguments":{"text":"one two three"}}})).await.unwrap();
        let value:Value=serde_json::from_str(outcome["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(value["count"],3);
        assert_eq!(counter.load(Ordering::SeqCst),1);
    }
    #[tokio::test]
    async fn host_hook_acts_only_when_owner_registered_it() {
        let counter=Arc::new(AtomicUsize::new(0)); let copy=counter.clone();
        let mut registry=ToolRegistry::new();
        registry.register(McpTool::new("a","Task",json!({"type":"object"})),
            move |_|{copy.fetch_add(1,Ordering::SeqCst);async {Ok(json!({"ok":true}))}}).unwrap();
        registry.before_tool(|_,_|Err("owner hook declined".into()));
        let output=registry.handle_jsonrpc(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"a","arguments":{}}})).await.unwrap();
        assert_eq!(output["result"]["isError"],true);
        assert_eq!(counter.load(Ordering::SeqCst),0);
    }
}
