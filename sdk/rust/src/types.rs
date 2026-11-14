use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCreate {
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<ModelSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<ModelSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner: Option<ModelSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolate_worktree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_replans: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replan_after: Option<u64>,
}

impl GoalCreate {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            success_criteria: None, executor: None, reviewer: None, planner: None,
            isolate_worktree: None, token_budget: None, max_iterations: None,
            max_replans: None, replan_after: None,
        }
    }
    pub fn max_iterations(mut self, limit: u64) -> Self {
        self.max_iterations = Some(limit);
        self
    }
    pub fn isolate_worktree(mut self, isolate: bool) -> Self {
        self.isolate_worktree = Some(isolate);
        self
    }
    pub fn executor(mut self, selection: ModelSelection) -> Self {
        self.executor = Some(selection);
        self
    }
    pub fn success_criteria(mut self, checks: Vec<Value>) -> Self {
        self.success_criteria = Some(checks);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub goal: String,
    pub status: String,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub iterations: Option<u64>,
    #[serde(default)]
    pub evidence: Option<Vec<Value>>,
    #[serde(default)]
    pub gaps: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Goal {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled" | "needs_review")
    }
    pub fn is_success(&self) -> bool { self.status == "completed" }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub input: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Job {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled" | "needs_review")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreate {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub selection: ModelSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub session_id: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl ChatEvent {
    pub fn field(&self, name: &str) -> Option<&Value> { self.extra.get(name) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub seq: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub session_id: String,
    pub input: String,
    #[serde(flatten)]
    pub selection: ModelSelection,
    pub tools_enabled: bool,
    pub tools_execution_enabled: bool,
    pub auto_learning: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_language: Option<String>,
}
impl ChatRequest {
    pub fn new(session_id: impl Into<String>, input: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(), input: input.into(),
            selection: ModelSelection::default(), tools_enabled: true,
            tools_execution_enabled: true, auto_learning: false,
            thinking: None, reasoning_effort: None, reasoning_language: None,
        }
    }
    pub fn selection(mut self, model: ModelSelection) -> Self {
        self.selection = model;
        self
    }
}

#[derive(Debug, Clone)]
pub struct WaitOptions {
    pub poll_interval: Duration,
    pub timeout: Duration,
}
impl Default for WaitOptions {
    fn default() -> Self {
        Self { poll_interval: Duration::from_millis(750), timeout: Duration::from_secs(600) }
    }
}

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub after: u64,
    pub reconnect: bool,
    /// None = reconnect until the returned stream is dropped.
    pub max_reconnects: Option<usize>,
    pub retry_delay: Duration,
}
impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            after: 0, reconnect: true, max_reconnects: None,
            retry_delay: Duration::from_millis(750),
        }
    }
}


/// Runs Dominion's own Rust worktree/compiled-descendant RSI coordinator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RsiSelectionPolicy { Manual, Automatic }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RsiEvolutionRequest {
    pub request: String,
    pub provider_id: String,
    pub model: String,
    pub selection_policy: RsiSelectionPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_budget: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_budget_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus_files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continue_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method_ablation: Option<bool>,
}
impl RsiEvolutionRequest {
    pub fn new(provider_id: impl Into<String>, model: impl Into<String>,
        request: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(), model: model.into(),
            request: request.into(), selection_policy: RsiSelectionPolicy::Manual,
            max_attempts: None, token_budget: None, time_budget_seconds: None,
            focus_files: None, continue_from: None, method_ablation: None,
        }
    }
    pub fn selection_policy(mut self, policy: RsiSelectionPolicy) -> Self {
        self.selection_policy = policy;
        self
    }
    pub fn max_attempts(mut self, attempts: u64) -> Self {
        self.max_attempts = Some(attempts);
        self
    }
    pub fn token_budget(mut self, tokens: u64) -> Self {
        self.token_budget = Some(tokens);
        self
    }
    pub fn time_budget_seconds(mut self, seconds: u64) -> Self {
        self.time_budget_seconds = Some(seconds);
        self
    }
    pub fn focus_files(mut self, paths: Vec<String>) -> Self {
        self.focus_files = Some(paths);
        self
    }
    pub fn continue_from(mut self, campaign: impl Into<String>) -> Self {
        self.continue_from = Some(campaign.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RsiCampaign {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub head_commit: Option<String>,
    #[serde(default)]
    pub recursive_proof: Option<Value>,
    #[serde(default)]
    pub capability_demonstrated: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl RsiCampaign {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "cancelled" | "needs_review")
    }
}
