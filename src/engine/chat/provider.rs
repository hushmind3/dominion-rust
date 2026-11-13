//! OpenAI-compatible provider resolution, native thinking controls and HTTP streaming.
use super::{protocol::SseBuffer, Completion, ProviderRef, ToolCall, TurnRequest, MAX_TOOL_CALLS};
use crate::state::AppState;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

const TEXT_TOOL_OPEN: &str = "<tool_call>";
const TEXT_TOOL_CLOSE: &str = "</tool_call>";

/// Streams visible text while hiding text-protocol tool calls. Small local
/// models emit `<tool_call>...` as ordinary content; without this buffer the
/// user sees the raw JSON before the tool actually runs.

#[path = "provider/resolve.rs"]
mod resolve;
pub(crate) use resolve::select_provider;
pub(super) use resolve::{apply_reasoning_options,cached_prompt_tokens};
pub(crate) use resolve::*;

#[path = "provider/stream.rs"]
mod stream;
pub(super) use stream::stream_completion;
pub(crate) use stream::*;

#[path = "provider/protocols.rs"]
mod protocols;
pub(super) use protocols::{codex_function_call_item,codex_tool_calls,codex_terminal_error};
pub(crate) use protocols::*;

#[path = "provider/codex.rs"]
mod codex;
pub(crate) use codex::*;

#[path = "provider/anthropic.rs"]
mod anthropic;
pub(crate) use anthropic::*;

#[cfg(test)]
#[path = "provider/tests.rs"]
mod tests;
