//! First-party, **in-process Rust** SDK for the actual DOMINION agent engine.
//!
//! This is not an HTTP wrapper, JavaScript runtime, or third-party agent SDK.
//! The library and the app both execute the same Rust model/tool/harness paths.
//!
//! A standalone SDK owns its data directory. Never open a second SDK process
//! against the data directory of a running DOMINION server.
pub use crate::engine::custom_tools::ToolLimits;

use crate::{
    config::Config,
    db::Store,
    engine::{self, chat::TurnRequest},
    migrations,
    state::AppState,
    vault::Vault,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
};
use tokio::{sync::mpsc, task::JoinHandle};

/// Storage and workspace used by a standalone, embedded Rust agent.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
}

impl EngineConfig {
    pub fn new(workspace: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self { workspace: workspace.into(), data_dir: data_dir.into() }
    }
}

/// User-supplied turn options; this is passed to the native Rust harness with
/// no additional SDK-level content classification or substituted instructions.
#[derive(Debug, Clone)]
pub struct TurnOptions {
    pub input: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub tools_enabled: bool,
    pub tools_execution_enabled: bool,
    pub auto_learning: bool,
    pub thinking: Option<bool>,
    pub reasoning_effort: Option<String>,
}

impl TurnOptions {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            session_id: None,
            agent_id: None,
            provider_id: None,
            model: None,
            tools_enabled: true,
            tools_execution_enabled: true,
            auto_learning: true,
            thinking: None,
            reasoning_effort: None,
        }
    }

    pub fn with_session(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    pub fn with_provider(mut self, id: impl Into<String>) -> Self {
        self.provider_id = Some(id.into());
        self
    }

    fn into_value(self) -> Value {
        json!({
            "input": self.input,
            "sessionId": self.session_id,
            "agentId": self.agent_id,
            "providerId": self.provider_id,
            "model": self.model,
            "toolsEnabled": self.tools_enabled,
            "toolsExecutionEnabled": self.tools_execution_enabled,
            "autoLearning": self.auto_learning,
            "thinking": self.thinking,
            "reasoningEffort": self.reasoning_effort,
        })
    }

    fn into_request(self) -> Result<TurnRequest, String> {
        TurnRequest::from_value(&self.into_value())
    }
}

/// Configuration of a durable, self-verifying multi-step goal.
/// The goal is executed by DOMINION's native planner, tool loop, reviewer and
/// frozen acceptance checks, never by a second SDK-side agent.
#[derive(Debug, Clone)]
pub struct GoalOptions {
    pub objective: String,
    pub success_criteria: Value,
    pub executor: Value,
    pub planner: Value,
    pub reviewer: Value,
    pub max_iterations: u64,
    pub max_replans: u64,
    pub replan_after: u64,
    pub max_executor_tokens: u64,
    pub isolate_worktree: bool,
}

impl GoalOptions {
    pub fn new(objective: impl Into<String>) -> Self {
        Self {
            objective: objective.into(),
            success_criteria: json!([]),
            executor: json!({}),
            planner: json!({}),
            reviewer: json!({}),
            max_iterations: 16,
            max_replans: 6,
            replan_after: 2,
            max_executor_tokens: 80_000,
            isolate_worktree: true,
        }
    }

    fn into_value(self) -> Value {
        json!({
            "goal": self.objective,
            "successCriteria": self.success_criteria,
            "executor": self.executor,
            "planner": self.planner,
            "reviewer": self.reviewer,
            "maxIterations": self.max_iterations,
            "maxReplans": self.max_replans,
            "replanAfter": self.replan_after,
            "maxExecutorTokens": self.max_executor_tokens,
            "isolateWorktree": self.isolate_worktree,
        })
    }
}

/// Owner decision required to resume a paused or interrupted autonomous goal.
/// Unknown external effects are not replayed unless the owner has inspected
/// them and explicitly records how they were reconciled.
#[derive(Debug, Clone, Default)]
pub struct GoalResumeOptions {
    pub resolution_note: Option<String>,
    pub acknowledge_external_effects: bool,
    pub max_iterations: Option<u64>,
    pub max_executor_tokens: Option<u64>,
}

impl GoalResumeOptions {
    fn into_value(self) -> Value {
        let mut body = json!({
            "acknowledgeExternalEffects": self.acknowledge_external_effects,
            "resolutionNote": self.resolution_note.unwrap_or_default()
        });
        if let Some(value) = self.max_iterations {
            body["maxIterations"] = json!(value);
        }
        if let Some(value) = self.max_executor_tokens {
            body["maxExecutorTokens"] = json!(value);
        }
        body
    }
}

/// Completed native engine turn: persisted assistant message plus witnessed
/// tool operations, including partial operations on a provider failure.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub message: Option<Value>,
    pub successful_tools: Vec<String>,
    pub failed_tools: Vec<String>,
    /// Real bounded tool stderr, exit status, and failure categories.
    /// SDK consumers can drive their own replanners without re-running
    /// failed side effects or parsing an LLM's natural language.
    pub failure_details: Vec<Value>,
    pub tool_evidence: Vec<Value>,
}

/// A real native turn's event receiver. Dropping the receiver does not
/// secretly cancel a durable operation; call cancel_session explicitly.
pub struct TurnStream {
    receiver: mpsc::UnboundedReceiver<Value>,
    task: JoinHandle<()>,
}

impl TurnStream {
    pub async fn recv(&mut self) -> Option<Value> {
        self.receiver.recv().await
    }

    pub async fn finish(self) -> Result<(), String> {
        self.task.await.map_err(|error| format!("native turn task failed: {error}"))
    }
}

/// Standalone and embedded paths both use the same engine state and all of its
/// native Rust tools, provider routing, verification and persistence.
#[derive(Clone)]
pub struct DominionSdk {
    state: AppState,
    workers_started: Arc<AtomicBool>,
}

impl DominionSdk {
    /// Open a separate agent data directory and apply the same database
    /// migrations as the desktop binary. This SDK instance owns that store.
    pub fn open(config: EngineConfig) -> Result<Self, String> {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|error| format!("cannot create SDK data directory: {error}"))?;
        let runtime_config = Config {
            port: 0,
            web_dir: config.data_dir.join("web"),
            data_dir: config.data_dir,
            workspace: config.workspace,
        };
        let store = Store::open(&runtime_config.db_path())
            .map_err(|error| format!("cannot open SDK store: {error}"))?;
        let vault = Vault::open(&runtime_config.key_path());
        let state = AppState::new(runtime_config, store, vault);
        migrations::run(&state)?;
        engine::jobs::recover(&state);
        Ok(Self::from_state(state))
    }

    /// The production HTTP adapter uses the identical in-process SDK path.
    pub(crate) fn from_state(state: AppState) -> Self {
        Self { state, workers_started: Arc::new(AtomicBool::new(false)) }
    }

    /// Start the existing durable Rust job worker pool once per SDK instance.
    /// Must be called from inside a Tokio runtime before queued jobs execute.
    /// The embedded desktop binary starts its own workers in bootstrap.
    pub fn start_job_workers(&self) -> bool {
        if self.workers_started.swap(true, Ordering::SeqCst) {
            return false;
        }
        engine::jobs::start(self.state.clone());
        true
    }

    /// Create a durable session without any HTTP/JavaScript hop.
    pub fn create_session(&self, title: &str) -> Result<Value, String> {
        let title = if title.trim().is_empty() { "새 대화" } else { title.trim() };
        let candidate = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "title": title.chars().take(200).collect::<String>(),
            "archived": false,
        });
        let saved = self.state.store.put_if_absent("session", &candidate)
            .ok_or("could not persist SDK session")?;
        self.state.bus.record("session", &saved);
        Ok(saved)
    }

    /// Register a first-party, in-process asynchronous Rust function tool.
    ///
    /// The native engine advertises `sdk__{name}` to the model and dispatches
    /// it through the same approval, operation evidence and audit path as
    /// built-in tools. The callback is allowed to perform I/O; choose
    /// `needs_approval = true` when it can mutate files/external services.
    /// No external agent SDK or content-filtering policy is involved.
    pub fn register_tool<F, Fut>(
        &self,
        name: &str,
        description: &str,
        input_schema: Value,
        needs_approval: bool,
        handler: F,
    ) -> Result<String, String>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, String>> + Send + 'static,
    {
        self.register_tool_with_limits(
            name,description,input_schema,needs_approval,ToolLimits::default(),handler,
        )
    }

    /// Per-tool execution envelope. A callback timeout or callback-level
    /// failure is treated as an unknown external outcome, never as safe to
    /// retry automatically. This affects all worker types using the same
    /// Rust tool registry, not merely SDK callers.
    pub fn register_tool_with_limits<F,Fut>(
        &self,name:&str,description:&str,input_schema:Value,
        needs_approval:bool,limits:ToolLimits,handler:F,
    )->Result<String,String>
    where
        F:Fn(Value)->Fut+Send+Sync+'static,
        Fut:std::future::Future<Output=Result<Value,String>>+Send+'static,
    {
        let callback:crate::engine::custom_tools::Callback =
            std::sync::Arc::new(move |args|Box::pin(handler(args)));
        self.state.custom_tools.register_with_limits(
            name,description,input_schema,needs_approval,callback,None,limits,
        )
    }

    /// Register an effectful Rust callback with an *independent* asynchronous
    /// postcondition. The verifier receives the original arguments and tool
    /// output, re-checks actual host state (e.g. file readback), and returns
    /// true only when the requested effect was observed. Merely returning
    /// {"success":true} from a tool is never enough to mark an action verified.
    ///
    /// Verified callbacks participate in ordinary goals, tool evidence,
    /// checkpoint recovery and the frozen acceptance verifier.
    pub fn register_verified_tool<F, Fut, V, VFut>(
        &self,
        name: &str,
        description: &str,
        input_schema: Value,
        needs_approval: bool,
        handler: F,
        verifier: V,
    ) -> Result<String, String>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Value, String>> + Send + 'static,
        V: Fn(Value, Value) -> VFut + Send + Sync + 'static,
        VFut: std::future::Future<Output = Result<bool, String>> + Send + 'static,
    {
        self.register_verified_tool_with_limits(
            name,description,input_schema,needs_approval,ToolLimits::default(),
            handler,verifier,
        )
    }

    /// Bounded verified Rust tool callback and separately bounded verifier.
    /// Verification failure or timeout preserves unknown side-effect state.
    pub fn register_verified_tool_with_limits<F,Fut,V,VFut>(
        &self,name:&str,description:&str,input_schema:Value,
        needs_approval:bool,limits:ToolLimits,handler:F,verifier:V,
    )->Result<String,String>
    where
        F:Fn(Value)->Fut+Send+Sync+'static,
        Fut:std::future::Future<Output=Result<Value,String>>+Send+'static,
        V:Fn(Value,Value)->VFut+Send+Sync+'static,
        VFut:std::future::Future<Output=Result<bool,String>>+Send+'static,
    {
        let callback:crate::engine::custom_tools::Callback =
            std::sync::Arc::new(move |args|Box::pin(handler(args)));
        let verifier:crate::engine::custom_tools::Verifier =
            std::sync::Arc::new(move |args,result|Box::pin(verifier(args,result)));
        self.state.custom_tools.register_with_limits(
            name,description,input_schema,needs_approval,callback,Some(verifier),limits,
        )
    }

    pub fn unregister_tool(&self, qualified_name: &str) -> bool {
        self.state.custom_tools.remove(qualified_name)
    }

    pub fn registered_tools(&self) -> Vec<Value> {
        self.state.custom_tools.definitions()
    }

    pub fn session(&self, id: &str) -> Option<Value> {
        self.state.store.get("session", id)
    }

    pub fn messages(&self, session_id: &str) -> Vec<Value> {
        self.state.store.messages(session_id)
    }

    pub fn session_usage(&self, session_id: &str) -> Value {
        self.state.store.session_usage(session_id)
    }

    pub fn cancel_session(&self, session_id: &str) {
        self.state.cancel_session(session_id);
    }

    /// Steer a running agent **at its next model-step boundary**, or queue the
    /// original instruction until the next turn if the agent is idle.
    /// The native Rust engine owns delivery and durable recovery.
    pub fn steer(&self, session_id: &str, instruction: &str) -> Result<Value, String> {
        engine::steering::enqueue(&self.state, session_id, instruction)
    }

    /// Change the live autonomous goal, not just the generated plan.
    /// Its new checks and independent baselines become authoritative at the
    /// next Rust executor turn boundary.
    pub fn redirect_goal(
        &self, goal_id: &str, expected_revision:u64, objective:&str, criteria:&Value,
    ) -> Result<Value, String> {
        engine::owner_objective::submit(
            &self.state, goal_id, expected_revision, objective, criteria,
        ).map_err(|error|format!("goal objective revision: {error:?}"))
    }

    /// Register a provider using the same encrypted secret store as the app.
    /// The API key is not persisted in the provider JSON record.
    pub fn register_provider(
        &self, id: &str, base_url: &str, model: &str, api_key: Option<&str>,
    ) -> Result<Value, String> {
        if id.trim().is_empty() || model.trim().is_empty() {
            return Err("provider id and model are required".into());
        }
        let url = reqwest::Url::parse(base_url).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("provider endpoint must be HTTP(S)".into());
        }
        if let Some(key) = api_key.filter(|key| !key.is_empty()) {
            self.state.save_secret(&format!("provider:{id}"), key);
            if self.state.read_secret(&format!("provider:{id}")).as_deref() != Some(key) {
                return Err("could not seal the provider API key".into());
            }
        }
        let provider = json!({
            "id": id,
            "name": id,
            "baseUrl": base_url,
            "model": model,
            "enabled": true,
        });
        let saved = self.state.store.put("provider", &provider)
            .ok_or("could not persist SDK provider")?;
        self.state.bus.record("provider", &saved);
        Ok(saved)
    }

    /// Run a full model -> native tools -> verify -> persist cycle directly
    /// inside the Rust process. Completed tool evidence survives model errors.
    pub async fn run(&self, options: TurnOptions) -> Result<TurnOutcome, String> {
        let request = options.into_request()?;
        let (outcome, stats) = engine::run_quiet_outcome(&self.state, &request).await;
        outcome.map(|message| TurnOutcome {
            message,
            successful_tools: stats.tool_success,
            failed_tools: stats.tool_errors,
            failure_details: stats.tool_failure_details,
            tool_evidence: stats.evidence,
        })
    }

    /// Start the same native Rust turn as the desktop app, yielding live
    /// model/tool/error/done events via a Tokio channel.
    pub fn stream(&self, options: TurnOptions) -> Result<TurnStream, String> {
        let input = options.into_value();
        TurnRequest::from_value(&input)?;
        let (tx, receiver) = mpsc::unbounded_channel();
        let sdk = self.clone();
        let task = tokio::spawn(async move { sdk.run_transport(input, tx).await });
        Ok(TurnStream { receiver, task })
    }

    /// Durable prompt jobs use the native queue; they survive a process
    /// restart and can be resumed by the existing job recovery worker.
    pub fn enqueue_prompt(&self, options: TurnOptions) -> Result<Value, String> {
        let input = options.into_value();
        let prompt = input["input"].as_str().unwrap_or("");
        if prompt.trim().is_empty() || prompt.len() > 64_000 {
            return Err("prompt must contain 1..64000 bytes".into());
        }
        let mut queued = json!({
            "prompt": prompt,
            "toolsEnabled": input["toolsEnabled"],
        });
        for key in ["agentId", "providerId", "model", "reasoningEffort"] {
            if let Some(value) = input[key].as_str().filter(|value| !value.is_empty()) {
                queued[key] = json!(value);
            }
        }
        if let Some(thinking) = input["thinking"].as_bool() {
            queued["thinking"] = json!(thinking);
        }
        engine::jobs::enqueue(&self.state, "prompt", queued)
            .ok_or_else(|| "could not persist SDK job".into())
    }

    /// Atomically persist a complete autonomous goal and its runnable job.
    /// Call start_job_workers() in a standalone SDK runtime to execute queued
    /// goals. The returned record contains the durable jobId.
    pub fn enqueue_goal(&self, options: GoalOptions) -> Result<Value, String> {
        engine::goals::enqueue_goal(&self.state, &options.into_value())
            .map_err(|error| error.to_string())
    }

    /// Resume a paused native autonomous goal through the same atomic
    /// owner-reviewed state transition used by the HTTP adapter.
    pub fn resume_goal(
        &self, id: &str, options: GoalResumeOptions,
    ) -> Result<Value, String> {
        engine::goals::resume_goal(&self.state, id, &options.into_value())
            .map_err(|error| error.to_string())
    }

    pub fn goal(&self, id: &str) -> Option<Value> {
        self.state.store.get("goal", id)
    }

    /// Only the receipt of this goal's *current* job is authoritative.
    /// Never resurrect the receipt of an earlier attempt after resume.
    pub fn goal_receipt(&self, id: &str) -> Option<Value> {
        let goal = self.goal(id)?;
        let job_id = goal["jobId"].as_str()?;
        self.state.store.get("goal_receipt", &format!("goal-receipt:{id}:{job_id}"))
    }

    /// Observed model spend of this goal and all delegated descendants.
    /// This is native SQLite-audited usage, never a model's own cost claim.
    /// The ceiling is checked between calls; provider billing may differ.
    pub fn goal_usage(&self, id: &str) -> Result<Value, String> {
        let goal = self.goal(id).ok_or("목표를 찾을 수 없습니다.")?;
        let session_id = goal["sessionId"].as_str()
            .filter(|id| !id.is_empty())
            .ok_or("목표 실행 세션이 아직 생성되지 않았습니다.")?;
        self.state.store.session_tree_usage(session_id)
    }

    /// Inspect actual source changes recorded by the native worktree engine.
    pub async fn goal_diff(&self, id: &str) -> Result<Value, String> {
        engine::goals::diff_for_goal(&self.state, id).await
    }

    /// Apply only the full, reviewed SHA-256 worktree patch, through the
    /// existing Rust source/goal verifier. This does not auto-approve,
    /// publish, merge or bypass any original checkout conflict checks.
    pub async fn apply_goal_patch(
        &self, goal_id: &str, expected_patch_sha256: &str,
    ) -> Result<Value, String> {
        let digest = expected_patch_sha256.trim();
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("reviewed full patch SHA-256 must be 64 hexadecimal bytes".into());
        }
        engine::goal_workspace::apply_reviewed(&self.state, goal_id, digest).await
    }

    /// Dispose a goal's isolated worktree only after an explicit owner
    /// confirmation. Never deletes the original repository checkout.
    pub async fn discard_goal_worktree(
        &self, goal_id: &str, confirmation: &str,
    ) -> Result<Value, String> {
        if confirmation != "DISCARD_GOAL_WORKTREE" {
            return Err("explicit DISCARD_GOAL_WORKTREE confirmation required".into());
        }
        engine::goal_workspace::discard(&self.state, goal_id).await
    }

    /// Cancel a job using the same atomic transition as the HTTP endpoint.
    /// A stale cancellation cannot stop a completed job's active session.
    pub fn cancel_job(&self, id: &str) -> Result<Value, String> {
        engine::jobs::cancel_job(&self.state, id).map_err(|error| error.to_string())
    }

    pub fn cancel_goal(&self, id: &str) -> Result<Value, String> {
        let goal = self.goal(id).ok_or("goal not found")?;
        let job_id = goal["jobId"].as_str().ok_or("goal has no job")?;
        self.cancel_job(job_id)
    }

    /// Await persisted terminal state, not a guessed model completion.
    /// Duration bounds waiting even when no worker has been started.
    pub async fn wait_job(
        &self, id: &str, max_wait: std::time::Duration,
    ) -> Result<Value, String> {
        if max_wait.is_zero() { return Err("wait duration must be positive".into()); }
        let start = std::time::Instant::now();
        loop {
            let job = self.job(id).ok_or("job not found")?;
            if matches!(job["status"].as_str(),
                Some("completed" | "failed" | "needs_review" | "cancelled")) {
                return Ok(job);
            }
            let left = max_wait.saturating_sub(start.elapsed());
            if left.is_zero() { return Err(format!("job {id} wait timed out")); }
            tokio::time::sleep(left.min(std::time::Duration::from_millis(100))).await;
        }
    }

    pub fn job(&self, id: &str) -> Option<Value> {
        self.state.store.get("job", id)
    }

    /// This is the *production* HTTP adapter entrypoint. The adapter owns
    /// HTTP/SSE formatting only; all execution remains in the native engine.
    pub(crate) async fn run_transport(
        &self, input: Value, sender: mpsc::UnboundedSender<Value>,
    ) {
        engine::chat::run(self.state.clone(), input, sender).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};

    fn fresh() -> (DominionSdk, PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-native-sdk-{}", uuid::Uuid::new_v4()));
        let sdk = DominionSdk::open(EngineConfig::new(root.clone(), root.join("data"))).unwrap();
        (sdk, root)
    }

    #[tokio::test]
    async fn native_sdk_goal_admission_cancellation_and_receipt_follow_engine_contract() {
        let (sdk, root) = fresh();
        let mut invalid = GoalOptions::new("Fix the repository");
        invalid.max_iterations = 0;
        assert!(sdk.enqueue_goal(invalid).is_err());
        assert!(sdk.state.store.all("goal").is_empty());
        assert!(sdk.state.store.all("job").is_empty());

        let goal = sdk.enqueue_goal(GoalOptions::new("Fix the repository")).unwrap();
        let id = goal["id"].as_str().unwrap();
        let job_id = goal["jobId"].as_str().unwrap();
        assert_eq!(goal["status"], "queued");
        assert_eq!(goal["isolateWorktree"], true);
        let job = sdk.job(job_id).unwrap();
        assert_eq!(job["kind"], "goal");
        assert_eq!(job["input"]["goalId"], id);
        assert_eq!(sdk.goal(id).unwrap()["jobId"], job_id);
        assert!(sdk.goal_receipt(id).is_none(),
            "a queued goal must never expose a speculative success receipt");

        let cancelled = sdk.cancel_goal(id).unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        let settled = sdk.wait_job(job_id, std::time::Duration::from_millis(200))
            .await.unwrap();
        assert_eq!(settled["status"], "cancelled");
        assert_eq!(sdk.goal(id).unwrap()["status"], "cancelled");
        assert!(sdk.cancel_goal(id).is_err(),
            "a stale owner cancellation cannot replay against a settled job");
        assert!(sdk.goal_receipt(id).is_none());
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_sdk_patch_apply_and_worktree_discard_require_owner_proof() {
        let (sdk, root) = fresh();
        let goal = sdk.enqueue_goal(GoalOptions::new("Write a verified source patch")).unwrap();
        let id = goal["id"].as_str().unwrap();
        assert!(sdk.apply_goal_patch(id, "not-a-hash").await.is_err());
        assert!(sdk.discard_goal_worktree(id, "").await.is_err());
        // An unstarted goal has no source patch; an arbitrary correct-format
        // hash is still not sufficient to claim an applied source change.
        assert!(sdk.apply_goal_patch(id, &"a".repeat(64)).await.is_err());
        assert_eq!(sdk.goal(id).unwrap()["status"], "queued");
        sdk.cancel_goal(id).unwrap();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_goal_resume_creates_new_job_without_reusing_old_receipt() {
        let (sdk, root) = fresh();
        // The first attempt can be cancelled before any worktree was
        // provisioned; this is a legitimate clean resume.
        let goal = sdk.enqueue_goal(GoalOptions::new("Repair then verify")).unwrap();
        let id = goal["id"].as_str().unwrap();
        let old_job_id = goal["jobId"].as_str().unwrap();
        sdk.cancel_goal(id).unwrap();
        let resumed = sdk.resume_goal(id, GoalResumeOptions::default()).unwrap();
        let next_job = resumed["jobId"].as_str().unwrap();
        assert_ne!(old_job_id, next_job);
        assert_eq!(sdk.goal(id).unwrap()["jobId"], next_job);
        assert_eq!(sdk.job(old_job_id).unwrap()["status"], "cancelled");
        assert_eq!(sdk.job(next_job).unwrap()["status"], "queued");
        assert!(sdk.goal_receipt(id).is_none());
        assert_eq!(resumed["reviewAcknowledged"], false);
        assert!(sdk.resume_goal(id, GoalResumeOptions::default()).is_err(),
            "a second resume cannot enqueue a duplicate job");
        sdk.cancel_goal(id).unwrap();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_goal_resume_requires_review_for_unknown_tool_outcome() {
        let (sdk, root) = fresh();
        let mut options = GoalOptions::new("Reconcile external operation");
        options.isolate_worktree = false;
        let goal = sdk.enqueue_goal(options).unwrap();
        let id = goal["id"].as_str().unwrap();
        let old_job_id = goal["jobId"].as_str().unwrap();
        sdk.cancel_goal(id).unwrap();
        sdk.state.store.patch_if("job", old_job_id, "status", &json!("cancelled"),
            &json!({"goalState":{"inFlight":true,"nextIteration":1,
                "transcript":"external operation may have executed"}})).unwrap();
        assert!(sdk.resume_goal(id, GoalResumeOptions::default()).is_err());
        assert_eq!(sdk.goal(id).unwrap()["status"], "cancelled");
        let options = GoalResumeOptions {
            acknowledge_external_effects: true,
            resolution_note: Some(
                "Owner inspected the external operation and reconciled actual state".into()
            ),
            ..GoalResumeOptions::default()
        };
        let resumed = sdk.resume_goal(id, options).unwrap();
        assert_eq!(resumed["nextIteration"], 2);
        assert_eq!(resumed["reviewAcknowledged"], true);
        assert_eq!(sdk.goal(id).unwrap()["resumeHistory"][0]["unknownEffectsAcknowledged"],true);
        let new_job_id = resumed["jobId"].as_str().unwrap();
        assert_ne!(new_job_id, old_job_id);
        assert_eq!(sdk.goal(id).unwrap()["resumeState"]["inFlight"], false);
        assert_eq!(sdk.goal(id).unwrap()["resumeState"]["nextIteration"], 2);
        sdk.cancel_goal(id).unwrap();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_sdk_job_wait_is_bounded_without_workers() {
        let (sdk, root) = fresh();
        let goal = sdk.enqueue_goal(GoalOptions::new("Wait for external worker")).unwrap();
        let job_id = goal["jobId"].as_str().unwrap();
        assert!(sdk.wait_job(job_id, std::time::Duration::from_millis(20))
            .await.unwrap_err().contains("timed out"));
        assert_eq!(sdk.job(job_id).unwrap()["status"], "queued");
        sdk.cancel_goal(goal["id"].as_str().unwrap()).unwrap();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn native_sdk_owns_durable_session_and_provider_secrets() {
        let (sdk, root) = fresh();
        let session = sdk.create_session("Rust SDK").unwrap();
        let id = session["id"].as_str().unwrap();
        assert_eq!(sdk.session(id).unwrap()["title"], "Rust SDK");
        let provider = sdk.register_provider("demo", "http://127.0.0.1:9988", "example", Some("private-key")).unwrap();
        assert!(provider.get("apiKey").is_none());
        assert_eq!(sdk.state.read_secret("provider:demo").as_deref(), Some("private-key"));
        assert!(!root.join("data").join("dominion.sqlite").as_os_str().is_empty());
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_sdk_runs_a_real_mock_provider_turn_without_http_to_dominion() {
        async fn fixture(Json(_body): Json<Value>) -> axum::response::Response {
            use axum::response::IntoResponse;
            let chunk = json!({"choices":[{"delta":{"content":"Rust native success"},"finish_reason":"stop"}]});
            (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                format!("data: {chunk}\n\ndata: [DONE]\n\n"),
            ).into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/chat/completions", post(fixture)))
                .await.unwrap();
        });
        let (sdk, root) = fresh();
        sdk.register_provider("p", &format!("http://{address}"), "fixture", None).unwrap();
        let session = sdk.create_session("fixture").unwrap();
        let mut turn = TurnOptions::new("Use the real Rust harness");
        turn.session_id = Some(session["id"].as_str().unwrap().to_string());
        turn.provider_id = Some("p".into());
        turn.tools_enabled = false;
        turn.auto_learning = false;
        let result = sdk.run(turn).await.unwrap();
        assert!(result.message.is_some());
        assert!(sdk.messages(session["id"].as_str().unwrap())
            .iter().any(|message| message["content"].as_str().unwrap_or("").contains("Rust native success")));
        server.abort();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stream_exposes_native_errors_and_terminal_done_without_js() {
        let (sdk, root) = fresh();
        let session = sdk.create_session("stream").unwrap();
        let mut turn = TurnOptions::new("Run without a configured provider");
        turn.session_id = Some(session["id"].as_str().unwrap().to_string());
        turn.auto_learning = false;
        let mut stream = sdk.stream(turn).unwrap();
        let mut events = Vec::new();
        while let Some(event) = stream.recv().await {
            let is_done = event["type"] == "done";
            events.push(event);
            if is_done { break; }
        }
        stream.finish().await.unwrap();
        assert!(events.iter().any(|event| event["type"] == "error"));
        assert!(events.iter().any(|event| event["type"] == "done"));
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn sdk_tool_timeout_is_a_durable_unknown_effect_not_a_retryable_failure(){
        let (sdk,root)=fresh();
        let name=sdk.register_tool_with_limits(
            "maybe_changed","May have changed an external service",
            json!({"type":"object","properties":{"id":{"type":"string"}}}),
            false,
            ToolLimits{
                execution_timeout:std::time::Duration::from_millis(5),
                verification_timeout:std::time::Duration::from_secs(1),
                max_result_bytes:4096,
            },
            |_args|async{
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                Ok(json!({"changed":true}))
            },
        ).unwrap();
        let args=json!({"id":"external-1"});
        let returned=sdk.state.custom_tools.execute(&name,&args).await.unwrap();
        assert_eq!(returned["outcomeUnknown"],true);
        let prepared=engine::effect::prepare("owner-session",&name,&args,&[]);
        let sealed=engine::effect::seal(
            &root,&name,&args,&returned,false,false,false,&prepared,
        ).await;
        assert_eq!(sealed["effectState"],"effect_unknown");
        let attempts=vec![json!({
            "effectId":prepared["effectId"],
            "effectState":"effect_unknown",
        })];
        assert!(engine::effect::unresolved(&prepared,&attempts).is_some(),
            "unknown external outcomes must block an identical retry");
        drop(sdk);let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn native_sdk_registers_tool_and_executes_callback_without_node() {
        let (sdk, root) = fresh();
        let called = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = called.clone();
        let qualified = sdk.register_tool("hello", "Respond to user text",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}),
            false,
            move |args| {
                let count = count.clone();
                async move {
                    count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
                    Ok(json!({"echo":args["text"]}))
                }
            }
        ).unwrap();
        assert_eq!(qualified, "sdk__hello");
        assert_eq!(sdk.registered_tools()[0]["function"]["name"], "sdk__hello");
        let reply = sdk.state.custom_tools.execute(&qualified,
            &json!({"text":"시킨 그대로"})).await.unwrap();
        assert_eq!(reply["echo"], "시킨 그대로");
        assert_eq!(called.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(sdk.unregister_tool(&qualified));
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    fn stateful_sdk_proof(sdk: &DominionSdk, session_id: &str) -> bool {
        sdk.state.store.all("execution_operation").into_iter().any(|operation| {
            operation["sessionId"].as_str()==Some(session_id)
                && operation["tool"]=="sdk__echo"
                && operation["toolReceipt"]["verified"]==true
                && operation["toolReceipt"]["verifier"]["kind"]=="native_sdk_postcondition"
        })
    }

    #[tokio::test]
    async fn owner_steering_is_seen_by_the_next_real_provider_step_after_tool_execution() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use tokio::sync::Notify;

        #[derive(Default)]
        struct Fixture {
            first_request: Notify,
            resume: Notify,
            saw_steer: AtomicBool,
            model_calls: AtomicUsize,
        }

        let fixture = std::sync::Arc::new(Fixture::default());
        let handler_fixture = fixture.clone();
        let app = Router::new().route("/chat/completions", post(
            move |Json(body): Json<Value>| {
                let state = handler_fixture.clone();
                async move {
                    use axum::response::IntoResponse;
                    let calls = state.model_calls.fetch_add(1, Ordering::SeqCst);
                    let chunk = if calls == 0 {
                        // Deliberately pause the provider so the owner can
                        // replace direction during the first model request.
                        state.first_request.notify_one();
                        state.resume.notified().await;
                        json!({"choices":[{"delta":{"tool_calls":[{
                            "index":0,"id":"native-sdk-call-1",
                            "function":{
                                "name":"sdk__echo",
                                "arguments":"{\"text\":\"first model call\"}"
                            }
                        }]},"finish_reason":"tool_calls"}]})
                    } else {
                        let seen = body["messages"].as_array().is_some_and(|messages| {
                            messages.iter().any(|message| {
                                message["role"] == "user"
                                    && message["content"] == "원래 계획 대신 새 지시를 우선하세요"
                            })
                        });
                        state.saw_steer.store(seen, Ordering::SeqCst);
                        json!({"choices":[{"delta":{"content":"Completed with owner correction"},"finish_reason":"stop"}]})
                    };
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                        format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                    ).into_response()
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener,app).await.unwrap() });

        let (sdk, root) = fresh();
        sdk.register_provider("p", &format!("http://{address}"), "fixture", None).unwrap();
        let written = root.join("sdk-native-proof.txt");
        let write_path = written.clone();
        let verify_path = written.clone();
        sdk.register_verified_tool(
            "echo", "Write exact input with an independent file readback",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            false,
            move |args| {
                let path = write_path.clone();
                async move {
                    let text = args["text"].as_str().ok_or("text required")?;
                    tokio::fs::write(path, text).await.map_err(|e|e.to_string())?;
                    Ok(json!({"written":true}))
                }
            },
            move |args, output| {
                let path = verify_path.clone();
                async move {
                    let text=tokio::fs::read_to_string(path).await.map_err(|e|e.to_string())?;
                    Ok(output["written"]==true && args["text"].as_str()==Some(text.as_str()))
                }
            },
        ).unwrap();
        let session = sdk.create_session("owner-steering").unwrap();
        let session_id = session["id"].as_str().unwrap().to_owned();
        let options = TurnOptions::new("What exact value does the registered native echo tool return?")
            .with_session(&session_id)
            .with_provider("p");

        let running_sdk = sdk.clone();
        let turn_task = tokio::spawn(async move { running_sdk.run(options).await });
        tokio::time::timeout(std::time::Duration::from_secs(15), fixture.first_request.notified())
            .await.expect("first native model request should start");
        let queued = sdk.steer(&session_id,"원래 계획 대신 새 지시를 우선하세요").unwrap();
        assert_eq!(queued["status"],"queued");
        fixture.resume.notify_one();

        let result = tokio::time::timeout(std::time::Duration::from_secs(40), turn_task)
            .await.expect("native turn timed out").unwrap().unwrap();
        assert!(result.message.is_some());
        assert_eq!(fixture.model_calls.load(Ordering::SeqCst),2);
        assert!(fixture.saw_steer.load(Ordering::SeqCst),
            "the real model must see the owner's correction, not only the SQLite queue");
        assert!(result.successful_tools.iter().any(|tool|tool=="sdk__echo"));
        let verified=stateful_sdk_proof(&sdk,&session_id);
        assert!(verified, "native SDK action must produce a verified operation receipt");
        assert_eq!(sdk.state.store.get("chat_steer", queued["id"].as_str().unwrap()).unwrap()["status"],"delivered");
        assert_eq!(sdk.messages(&session_id).iter()
            .filter(|message|message["metadata"]["source"]=="sdk_chat_steer")
            .count(), 1);
        server.abort();
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_user_request_returns_an_error_but_is_not_rewritten() {
        let (sdk, root) = fresh();
        assert!(sdk.enqueue_prompt(TurnOptions::new("")).is_err());
        let instruction = "원문을 고치지 말고 그대로 처리해 주세요";
        let queued = sdk.enqueue_prompt(TurnOptions::new(instruction)).unwrap();
        assert_eq!(queued["input"]["prompt"], instruction);
        drop(sdk);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn native_sdk_goal_usage_counts_descendant_tokens_without_other_sessions() {
        let (sdk, root) = fresh();
        let goal = sdk.enqueue_goal(GoalOptions::new("Show native goal usage")).unwrap();
        let id=goal["id"].as_str().unwrap();
        assert!(sdk.goal_usage(id).is_err(),
            "queued goal without an execution session must not claim zero usage");
        let mut saved=goal.clone();
        saved["sessionId"]=json!("goal-root");
        sdk.state.store.put("goal",&saved).unwrap();
        sdk.state.store.put("session",&json!({"id":"goal-root","goalId":id})).unwrap();
        sdk.state.store.put("session",&json!({"id":"agent-child",
            "parentSessionId":"goal-root","goalId":id,"delegated":true})).unwrap();
        sdk.state.store.put("session",&json!({"id":"not-ours"})).unwrap();
        sdk.state.store.audit("model.usage",json!({"sessionId":"goal-root",
            "promptTokens":14,"completionTokens":6,"totalTokens":20}));
        sdk.state.store.audit("model.usage",json!({"sessionId":"agent-child",
            "promptTokens":29,"completionTokens":11,"totalTokens":40}));
        sdk.state.store.audit("model.usage",json!({"sessionId":"not-ours",
            "promptTokens":100,"completionTokens":100,"totalTokens":200}));
        let usage=sdk.goal_usage(id).unwrap();
        assert_eq!(usage["totalTokens"],60);
        assert_eq!(usage["sessions"],2);
        assert_eq!(usage["calls"],2);
        assert_eq!(sdk.state.store.enclosing_goal("agent-child").unwrap().0,
            "goal-root");
        drop(sdk);
        let _=std::fs::remove_dir_all(root);
    }
}
