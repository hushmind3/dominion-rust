use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus { Queued, Running, AwaitingApproval, Paused, Completed, Failed, Cancelled }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub seq: u64,
    pub kind: String,
    pub message: String,
    pub detail: Option<Value>,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: Uuid,
    pub instruction: String,
    pub status: RunStatus,
    pub plan: Option<String>,
    pub answer: Option<String>,
    pub messages: Vec<Value>,
    pub events: Vec<TimelineEvent>,
    pub queued_calls: Vec<ToolCall>,
    pub pending_approval: Option<ToolCall>,
    pub approval_decision: Option<bool>,
    pub steps: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_krw: f64,
    pub budget_krw: f64,
    pub max_steps: usize,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
#[derive(Debug, Deserialize)]
pub struct CreateRun {
    pub instruction: String,
    pub budget_krw: Option<f64>,
    pub max_steps: Option<usize>,
}
#[derive(Debug, Deserialize)]
pub struct Approval { pub allow: bool }
#[derive(Debug, Serialize)]
pub struct PublicSettings {
    pub model: String,
    pub economy_model: String,
    pub configured: bool,
    pub shell_enabled: bool,
    pub max_steps: usize,
}
pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
impl Run {
    pub fn new(req: CreateRun, default_steps: usize) -> Result<Self, String> {
        let instruction = req.instruction.trim().to_string();
        if instruction.is_empty() || instruction.len() > 20_000 { return Err("instruction must be 1..20000 characters".into()); }
        let budget = req.budget_krw.unwrap_or(10_000.0);
        if !budget.is_finite() || !(0.0..=1_000_000.0).contains(&budget) { return Err("invalid budget_krw".into()); }
        let id = Uuid::new_v4();
        let time = now_ms();
        let mut run = Self {
            id, instruction, status: RunStatus::Queued, plan: None, answer: None,
            messages: Vec::new(), events: Vec::new(), queued_calls: Vec::new(),
            pending_approval: None, approval_decision: None, steps: 0,
            input_tokens: 0, output_tokens: 0, cost_krw: 0.0, budget_krw: budget,
            max_steps: req.max_steps.unwrap_or(default_steps).clamp(1,100),
            created_at_ms: time, updated_at_ms: time,
        };
        run.event("created", "New execution created", None);
        Ok(run)
    }
    pub fn event(&mut self, kind: &str, message: impl Into<String>, detail: Option<Value>) {
        let at = now_ms();
        self.updated_at_ms = at;
        self.events.push(TimelineEvent {
            seq: self.events.last().map_or(1, |e| e.seq + 1),
            kind: kind.into(), message: message.into(), detail, at_ms: at,
        });
    }
    pub fn terminal(&self) -> bool {
        matches!(self.status, RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled)
    }
}
