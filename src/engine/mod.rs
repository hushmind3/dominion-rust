pub mod chat;
pub mod dev;
pub mod goals;
pub mod jobs;
pub mod mcp;
pub mod rooms;
pub mod tools;
pub mod workflows;

use crate::state::AppState;
use serde_json::Value;
use tokio::sync::mpsc;

/// Runs a turn with nobody listening on the event stream (background jobs).
/// The channel stays open so tool execution is not treated as cancelled.
pub async fn run_quiet(
    state: &AppState,
    request: &chat::TurnRequest,
) -> Result<Option<Value>, String> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<Value>();
    let drain = tokio::spawn(async move { while receiver.recv().await.is_some() {} });
    let result = chat::run_turn(state, request, &sender).await;
    drop(sender);
    let _ = drain.await;
    result
}
