//! Native Rust chat engine: shared request contract and stable public entry points.
mod approval;
mod budget;
mod context;
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
                Some(effort)
                    if matches!(
                        effort.as_str(),
                        "none" | "minimal" | "low" | "medium" | "high"
                    ) =>
                {
                    Some(effort)
                }
                Some(_) => {
                    return Err(
                        "추론 강도는 none/minimal/low/medium/high 중 하나여야 합니다.".into(),
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

#[derive(Clone, Default)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct Completion {
    content: String,
    reasoning: String,
    tool_calls: Vec<ToolCall>,
    usage: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    usage_exact: bool,
    first_delta_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct ProviderRef {
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
