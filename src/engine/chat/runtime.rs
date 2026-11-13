//! Turn lifecycle, cancellation, limits, tool orchestration and durable evidence.
use super::{
    approval::{close_pending_approvals, request_approval},
    context::history,
    provider::{select_provider, stream_completion},
};
use super::{
    parse_text_tool_calls, Metrics, ToolCall, TurnRequest, MAX_ELAPSED, MAX_ROUNDS, MAX_TOKENS, MAX_TOOL_CALLS,
};
use super::evidence::{evidence_for_source, prepare_owner_action};
use crate::engine::adaptation::types::TaskOutcome;
use crate::engine::goals;
use crate::{
    engine::{
        llm, mcp,
        tools::{self, ToolContext},
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

#[path = "runtime/turn.rs"]
mod turn;
use turn::run_turn_body;

#[path = "runtime/rsi_guard.rs"]
mod rsi_guard;
pub(crate) use rsi_guard::*;

#[path = "runtime/entry.rs"]
mod entry;
pub use entry::{run_turn,run_turn_background,run_turn_resume,run_room_turn};
pub(crate) use entry::*;

#[path = "runtime/adaptation.rs"]
mod adaptation;
pub(crate) use adaptation::*;

#[path = "runtime/tool_selection.rs"]
mod tool_selection;
pub(crate) use tool_selection::*;

#[path = "runtime/program.rs"]
mod program;
pub(crate) use program::*;

#[path = "runtime/tool_dispatch.rs"]
mod tool_dispatch;
pub(crate) use tool_dispatch::*;

#[path = "runtime/evaluation.rs"]
mod evaluation;
pub(crate) use evaluation::*;
pub(super) use evaluation::tool_succeeded;

#[path = "runtime/finalization.rs"]
mod finalization;
pub(crate) use finalization::*;
pub(super) use finalization::{finish_attempt,finalize_execution};

#[path = "runtime/session.rs"]
mod session;
pub(crate) use session::*;


/// Task contracts bound intent to 4096 characters. Keep the full model prompt
/// intact but derive a bounded, content-unique value for the contract.
#[cfg(test)]
include!("../../../tests/support/engine/chat_runtime.tests.rs");
