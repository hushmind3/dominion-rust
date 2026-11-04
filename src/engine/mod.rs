pub mod chat;
pub mod compact;
pub mod skills;
pub mod dev;
pub mod goals;
pub mod llm;
pub mod learning;
pub mod jobs;
pub mod mcp;
pub mod models;
pub mod oauth;
pub mod rooms;
pub mod tools;
pub mod translate;
pub mod workflows;

use crate::state::AppState;
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Default, Clone)]
pub struct TurnStats {
    pub tool_success: Vec<String>,
    pub tool_errors: Vec<String>,
}

/// Runs a turn and collects tool evidence for the goal harness.
pub async fn run_quiet_with_stats(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<(Option<Value>, TurnStats), String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let collector = tokio::spawn(async move {
        let mut stats = TurnStats::default();
        while let Some(event) = receiver.recv().await {
            let name = event
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            match event.get("type").and_then(|value| value.as_str()) {
                Some("tool_end") if !name.is_empty() => {
                    stats.tool_success.push(name.to_string())
                }
                Some("tool_error") if !name.is_empty() => {
                    stats.tool_errors.push(name.to_string())
                }
                _ => {}
            }
        }
        stats
    });
    let result = chat::run_turn(state, request, &sender).await;
    drop(sender);
    let stats = collector.await.unwrap_or_default();
    Ok((result?, stats))
}

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    run_quiet_with_stats(state, request).await.map(|(message, _)| message)
}
