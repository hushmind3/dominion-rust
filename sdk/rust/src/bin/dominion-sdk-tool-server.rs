use dominion_sdk::{McpTool,ToolRegistry};
use serde_json::json;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut registry=ToolRegistry::new();
    registry.register(
        McpTool::new("word_count","Count whitespace-separated words",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]})),
        |args| async move {
            let text=args["text"].as_str().ok_or("text must be a string")?;
            Ok(json!({"count":text.split_whitespace().count()}))
        },
    ).expect("valid registration");
    registry.serve_stdio().await
}
