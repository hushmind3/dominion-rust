//! Native Rust chat engine: shared request contract and stable public entry points.
mod approval;
mod budget;
mod codex_ws;
mod context;
mod continuity;
mod room_context;

pub(crate) fn estimate_text_tokens(text:&str)->u64 {room_context::estimate(text) as u64}
mod evidence;
pub(crate) use evidence::prepare_owner_action;
mod protocol;
mod provider;
mod runtime;
#[cfg(test)]
mod tests;
mod transport;

pub use protocol::parse_text_tool_calls;
pub use runtime::{run_room_turn, run_turn, run_turn_background, run_turn_resume};

/// The coordinator owns the fixture and assertions. Only aliases to the real
/// runtime functions cross this boundary; candidate-owned test helpers cannot
/// manufacture the oracle's observations.
#[cfg(test)]
pub(crate) mod context_oracle {
    pub(crate) use super::room_context::compile;
    pub(crate) use super::provider::select_provider;
    pub(crate) use super::continuity::{dependencies,prepare,project,commit,commit_projection};
}

/// Cancel approvals bound to a session that is being archived/reset.
pub(crate) fn close_session_approvals(state: &crate::state::AppState, session_id: &str) {
    approval::close_pending_approvals(state, session_id);
}

/// Static prompt preview for the room settings UI.
pub(crate) fn system_prompt_preview(
    state: &crate::state::AppState,
    room_id: &str,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let tools = state
        .store
        .get("room", room_id)
        .and_then(|room| room["toolsEnabled"].as_bool())
        .unwrap_or(true);
    room_context::preview(state, room_id, agent_id, tools)
}

/// Clean up old durable approvals at startup and in the maintenance loop.
pub(crate) fn expire_stale_approvals(state: &crate::state::AppState) -> usize {
    approval::expire_stale_approvals(state)
}
use serde_json::Value;
use std::time::Duration;
pub use transport::run;

const MAX_ROUNDS: usize = 8;

const MAX_TOOL_CALLS: usize = 40;

/// Hard token budget per turn. Usage is read from the provider stream.
const MAX_TOKENS: u64 = 250_000;

const MAX_ELAPSED: Duration = Duration::from_secs(600);

const HISTORY_LIMIT: usize = 40;

// One logical turn: a user prompt, optional agent/provider/model overrides.
// Workflows, rooms and the chat UI all go through this.

#[derive(Clone, Debug)]
pub struct TurnRequest {
    pub session_id: Option<String>,
    pub input: String,
    pub attachment_ids: Vec<String>,
    pub agent_id: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tools_enabled: bool,
    /// Explicit overall tool execution permission. `tools_enabled` only
    /// controls whether the model may call tools directly.
    pub tools_execution_enabled: bool,
    pub thinking: Option<bool>,
    pub reasoning_effort: Option<String>,
    pub reasoning_language: Option<String>,
    pub auto_learning: bool,
    pub system_override: Option<String>,
    /// Root task episode this turn attempts. Derived prompts (goal iterations,
    /// room speakers) must attach to an existing root instead of creating a
    /// new owner instruction.
    pub task_id: Option<String>,
    pub parent_task_id: Option<String>,
    /// Frozen contract revision the caller believes it is executing.
    pub task_revision: Option<String>,
    /// `parent` means the parent lifecycle owns final verification (a goal
    /// reviewer). The child still runs deterministic checks and records
    /// evidence, but does not call a second semantic reviewer.
    pub verification_owner: Option<String>,
}

impl TurnRequest {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        crate::engine::reasoning::validate(value)?;
        let text = value
            .get("input")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let attachment_ids = crate::engine::attachments::ids(&value["attachmentIds"])?;
        if text.is_empty() && attachment_ids.is_empty() {
            return Err("메시지가 비어 있습니다.".into());
        }
        let string = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .filter(|item| !item.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            session_id: string("sessionId"),
            input: text,
            attachment_ids,
            agent_id: string("agentId"),
            provider_id: string("providerId"),
            model: string("model"),
            tools_enabled: value
                .get("toolsEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            tools_execution_enabled: value
                .get("toolsExecutionEnabled")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            thinking: value.get("thinking").and_then(|item| item.as_bool()),
            reasoning_effort: match string("reasoningEffort") {
                Some(effort) if crate::engine::reasoning::valid_effort(&effort) =>
                {
                    Some(effort)
                }
                Some(_) => {
                    return Err(
                        "추론 강도 값이 올바르지 않습니다.".into(),
                    )
                }
                None => None,
            },
            reasoning_language: string("reasoningLanguage"),
            auto_learning: value
                .get("autoLearning")
                .and_then(|item| item.as_bool())
                .unwrap_or(true),
            system_override: string("systemOverride"),
            task_id: string("taskId"),
            parent_task_id: string("parentTaskId"),
            task_revision: string("taskRevision"),
            verification_owner: string("verificationOwner"),
        })
    }
}

#[derive(Clone, Debug, Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct Completion {
    continuity: Option<Value>,
    // Internal working history; never serialized into client message metadata.
    projection: Option<continuity::Projection>,
    content: String,
    reasoning: String,
    tool_calls: Vec<ToolCall>,
    usage: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    // None means the provider did not report cache usage, not a cache miss.
    cached_prompt_tokens: Option<u64>,
    usage_exact: bool,
    first_delta_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProviderRef {
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    model: String,
    tool_mode: String,
    reasoning_protocol: String,
    local: bool,
    context_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    requests_per_minute: Option<u64>,
    kind: String,
    account_id: Option<String>,
}

// SSE entry point used by `POST /api/chat`.

#[derive(Default)]
struct Metrics {
    model_calls: u32,
    tool_calls: u32,
    iterations: u32,
    tokens: u64,
    first_token_ms: Option<u64>,
    model_elapsed_ms: u64,
    tool_elapsed_ms: u64,
    context_elapsed_ms: u64,
}
