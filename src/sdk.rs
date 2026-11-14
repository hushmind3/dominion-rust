//! First-party, **in-process Rust** SDK for the actual DOMINION agent engine.
//!
//! This is not an HTTP wrapper, JavaScript runtime, or third-party agent SDK.
//! The library and the app both execute the same Rust model/tool/harness paths.
//!
//! A standalone SDK owns its data directory. Never open a second SDK process
//! against the data directory of a running DOMINION server.
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

/// Completed native engine turn: persisted assistant message plus witnessed
/// tool operations, including partial operations on a provider failure.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub message: Option<Value>,
    pub successful_tools: Vec<String>,
    pub failed_tools: Vec<String>,
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
        let callback: crate::engine::custom_tools::Callback =
            std::sync::Arc::new(move |args| Box::pin(handler(args)));
        self.state.custom_tools.register(
            name, description, input_schema, needs_approval, callback,
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
        sdk.register_tool(
            "echo", "Echo a value",
            json!({"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}),
            false,
            |args| async move { Ok(json!({"echo":args["text"]})) },
        ).unwrap();
        let session = sdk.create_session("owner-steering").unwrap();
        let session_id = session["id"].as_str().unwrap().to_owned();
        let options = TurnOptions::new("Run the native echo tool")
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
}
