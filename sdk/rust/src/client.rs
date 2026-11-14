use crate::error::{DominionError, Result};
use crate::streaming::{ChatStream, EventStream};
use crate::types::{
    ChatEvent, ChatRequest, Goal, GoalCreate, Job, Message, ModelSelection,
    Session, SessionCreate, WaitOptions, WatchOptions,
};
use futures_util::StreamExt;
use reqwest::{header::HeaderValue, Method, Response, Url};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};

/// Authenticated native Rust API handle. Clone cheaply: reqwest pools connections.
/// The SDK delegates execution to Dominion's *own* Rust runtime, not another
/// vendor's policy or agent service.
#[derive(Clone, Debug)]
pub struct DominionClient {
    base: Url,
    regular_http: reqwest::Client,
    stream_http: reqwest::Client,
    token: Option<HeaderValue>,
}

impl DominionClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let url = Url::parse(base_url)
            .map_err(|e| DominionError::Configuration(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty() || url.password().is_some()
            || url.query().is_some() || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(DominionError::Configuration(
                "expected a root http(s) URL without credentials, query, or fragment".into(),
            ));
        }
        let regular_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        // A model may stream for minutes. The streaming transport intentionally
        // has no *total* timeout, only connection setup timeout.
        let stream_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { base: url, regular_http, stream_http, token: None })
    }

    /// DOMINION_ADMIN_TOKEN never appears in URLs or log text.
    pub fn with_admin_token(mut self, value: impl AsRef<str>) -> Result<Self> {
        let mut token = HeaderValue::from_str(value.as_ref())
            .map_err(|_| DominionError::Configuration("invalid admin token header".into()))?;
        token.set_sensitive(true);
        self.token = Some(token);
        Ok(self)
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| {
                DominionError::Configuration("Dominion host has no path segments".into())
            })?;
            path.clear().push("api").extend(segments.iter().copied());
        }
        Ok(url)
    }

    fn request(&self, streaming: bool, method: Method, url: Url) -> reqwest::RequestBuilder {
        let http = if streaming { &self.stream_http } else { &self.regular_http };
        let mut req = http.request(method, url).header("Accept",
            if streaming { "text/event-stream" } else { "application/json" });
        if let Some(token) = &self.token { req = req.header("X-Dominion-Token", token.clone()); }
        req
    }

    async fn checked_response(response: Response) -> Result<Response> {
        if response.status().is_success() { return Ok(response); }
        let status = response.status();
        let data = response.bytes().await?;
        let message = serde_json::from_slice::<Value>(&data)
            .ok().and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&data[..data.len().min(4096)]).into_owned());
        Err(DominionError::Http { status, message })
    }

    async fn json<T: DeserializeOwned>(
        &self, method: Method, segments: &[&str], body: Option<Value>,
    ) -> Result<T> {
        let mut req = self.request(false, method, self.api_url(segments)?);
        if let Some(body) = body { req = req.json(&body); }
        let response = Self::checked_response(req.send().await?).await?;
        Ok(response.json::<T>().await?)
    }

    pub(crate) async fn open_chat_stream(&self, body: ChatRequest) -> Result<Response> {
        let req = self.request(true, Method::POST, self.api_url(&["chat"])?).json(&body);
        let response = Self::checked_response(req.send().await?).await?;
        Self::require_sse(&response)?;
        Ok(response)
    }

    pub(crate) async fn open_event_stream(&self, cursor: u64) -> Result<Response> {
        let mut url = self.api_url(&["events"])?;
        url.query_pairs_mut().append_pair("after", &cursor.to_string());
        let response = Self::checked_response(
            self.request(true, Method::GET, url).send().await?
        ).await?;
        Self::require_sse(&response)?;
        Ok(response)
    }

    fn require_sse(response: &Response) -> Result<()> {
        let is_sse = response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("text/event-stream"));
        if !is_sse {
            return Err(DominionError::Protocol("expected text/event-stream response".into()));
        }
        Ok(())
    }

    pub async fn health(&self) -> Result<Value> {
        self.json(Method::GET, &["health"], None).await
    }
    pub async fn capabilities(&self) -> Result<Value> {
        self.json(Method::GET, &["capabilities"], None).await
    }
    pub async fn agents(&self) -> Result<Vec<Value>> {
        self.json(Method::GET, &["agents"], None).await
    }
    pub async fn providers(&self) -> Result<Vec<Value>> {
        self.json(Method::GET, &["providers"], None).await
    }
    pub async fn tools(&self) -> Result<Value> {
        self.json(Method::GET, &["tools"], None).await
    }

    pub async fn create_goal(&self, input: GoalCreate) -> Result<Goal> {
        self.json(Method::POST, &["goals"], Some(serde_json::to_value(input)?)).await
    }
    pub async fn goals(&self) -> Result<Vec<Goal>> {
        self.json(Method::GET, &["goals"], None).await
    }
    pub async fn goal(&self, id: &str) -> Result<Goal> {
        self.json(Method::GET, &["goals", id], None).await
    }
    pub async fn cancel_goal(&self, id: &str) -> Result<Goal> {
        self.json(Method::POST, &["goals", id, "cancel"], Some(json!({}))).await
    }
    pub async fn steer_goal(&self, id: &str, plan: &str) -> Result<Goal> {
        self.json(Method::POST, &["goals", id, "plan"], Some(json!({"plan":plan}))).await
    }
    /// The server requires the owner's explicit acknowledgement of unknown
    /// side effects, and only resumes a review-pending/failed goal.
    pub async fn resume_goal(&self, id: &str, acknowledge_unknown_effects: bool) -> Result<Goal> {
        self.json(Method::POST, &["goals", id, "resume"],
            Some(json!({"acknowledgeUnknownEffects":acknowledge_unknown_effects}))).await
    }
    pub async fn goal_changes(&self, id: &str) -> Result<Value> {
        self.json(Method::GET, &["goals", id, "changes"], None).await
    }
    pub async fn goal_metrics(&self) -> Result<Value> {
        self.json(Method::GET, &["goals", "metrics"], None).await
    }
    pub fn goal_handle(&self, id: impl Into<String>) -> GoalHandle {
        GoalHandle { client: self.clone(), id: id.into() }
    }
    pub async fn start_goal(&self, input: GoalCreate) -> Result<GoalHandle> {
        let goal = self.create_goal(input).await?;
        Ok(self.goal_handle(goal.id))
    }
    pub async fn wait_goal(&self, id: &str, options: WaitOptions) -> Result<Goal> {
        let started = tokio::time::Instant::now();
        loop {
            let goal = self.goal(id).await?;
            if goal.is_terminal() { return Ok(goal); }
            let elapsed = started.elapsed();
            if elapsed >= options.timeout {
                return Err(DominionError::WaitTimeout { resource: format!("goal {id}") });
            }
            tokio::time::sleep(options.poll_interval.max(Duration::from_millis(50))
                .min(options.timeout - elapsed)).await;
        }
    }

    pub async fn create_prompt_job(&self, prompt: &str, model: ModelSelection) -> Result<Job> {
        let mut input = serde_json::to_value(model)?;
        input["prompt"] = json!(prompt);
        input["toolsEnabled"] = json!(true);
        self.json(Method::POST, &["jobs"], Some(json!({
            "kind": "prompt", "input": input,
        }))).await
    }
    pub async fn jobs(&self) -> Result<Vec<Job>> {
        self.json(Method::GET, &["jobs"], None).await
    }
    pub async fn job(&self, id: &str) -> Result<Job> {
        self.json(Method::GET, &["jobs", id], None).await
    }
    pub async fn cancel_job(&self, id: &str) -> Result<Job> {
        self.json(Method::POST, &["jobs", id, "cancel"], Some(json!({}))).await
    }
    pub async fn wait_job(&self, id: &str, options: WaitOptions) -> Result<Job> {
        let started = tokio::time::Instant::now();
        loop {
            let job = self.job(id).await?;
            if job.is_terminal() { return Ok(job); }
            let elapsed = started.elapsed();
            if elapsed >= options.timeout {
                return Err(DominionError::WaitTimeout { resource: format!("job {id}") });
            }
            tokio::time::sleep(options.poll_interval.max(Duration::from_millis(50))
                .min(options.timeout - elapsed)).await;
        }
    }

    pub async fn create_session(&self, input: SessionCreate) -> Result<Session> {
        self.json(Method::POST, &["sessions"], Some(serde_json::to_value(input)?)).await
    }
    pub async fn sessions(&self) -> Result<Vec<Session>> {
        self.json(Method::GET, &["sessions"], None).await
    }
    pub async fn session(&self, id: &str) -> Result<Session> {
        self.json(Method::GET, &["sessions", id], None).await
    }
    pub async fn messages(&self, id: &str) -> Result<Vec<Message>> {
        self.json(Method::GET, &["sessions", id, "messages"], None).await
    }
    pub async fn fork_session(&self, id: &str) -> Result<Session> {
        self.json(Method::POST, &["sessions", id, "fork"], Some(json!({}))).await
    }
    pub async fn cancel_session(&self, id: &str) -> Result<Value> {
        self.json(Method::POST, &["sessions", id, "cancel"], Some(json!({}))).await
    }

    pub async fn pending_approvals(&self) -> Result<Vec<Value>> {
        self.json(Method::GET, &["approvals"], None).await
    }
    pub async fn resolve_approval(&self, id: &str, allow: bool) -> Result<Value> {
        self.json(Method::POST, &["approvals"], Some(json!({"id":id,"allow":allow}))).await
    }

    pub async fn executions(&self) -> Result<Vec<Value>> {
        self.json(Method::GET, &["executions"], None).await
    }
    pub async fn execution(&self, id: &str) -> Result<Value> {
        self.json(Method::GET, &["executions", id], None).await
    }
    pub async fn execution_operations(&self, id: &str) -> Result<Vec<Value>> {
        self.json(Method::GET, &["executions", id, "operations"], None).await
    }
    pub async fn cancel_execution(&self, id: &str) -> Result<Value> {
        self.json(Method::POST, &["executions", id, "cancel"], Some(json!({}))).await
    }

    pub async fn stream_chat(&self, request: ChatRequest) -> Result<ChatStream> {
        crate::streaming::chat_stream(self.open_chat_stream(request).await?)
    }
    pub fn watch_events(&self, options: WatchOptions) -> EventStream {
        crate::streaming::events_stream(self.clone(), options)
    }
    pub async fn start_thread(&self, input: SessionCreate) -> Result<DominionThread> {
        let selection = input.selection.clone();
        let session = self.create_session(input).await?;
        Ok(DominionThread { client: self.clone(), id: session.id, selection })
    }
    pub fn resume_thread(&self, id: impl Into<String>, selection: ModelSelection) -> DominionThread {
        DominionThread { client: self.clone(), id: id.into(), selection }
    }
}

#[derive(Clone, Debug)]
pub struct GoalHandle { client: DominionClient, pub id: String }
impl GoalHandle {
    pub async fn get(&self) -> Result<Goal> { self.client.goal(&self.id).await }
    pub async fn wait(&self, options: WaitOptions) -> Result<Goal> {
        self.client.wait_goal(&self.id, options).await
    }
    pub async fn steer(&self, plan: &str) -> Result<Goal> {
        self.client.steer_goal(&self.id, plan).await
    }
    pub async fn cancel(&self) -> Result<Goal> {
        self.client.cancel_goal(&self.id).await
    }
    pub async fn resume(&self, acknowledge_unknown_effects: bool) -> Result<Goal> {
        self.client.resume_goal(&self.id, acknowledge_unknown_effects).await
    }
}

#[derive(Clone, Debug)]
pub struct DominionThread {
    client: DominionClient,
    pub id: String,
    selection: ModelSelection,
}

#[derive(Debug)]
pub struct ThreadResult {
    pub final_response: String,
    pub message: Option<Message>,
    pub events: Vec<ChatEvent>,
}

impl DominionThread {
    pub async fn messages(&self) -> Result<Vec<Message>> {
        self.client.messages(&self.id).await
    }
    pub async fn cancel(&self) -> Result<Value> {
        self.client.cancel_session(&self.id).await
    }
    pub async fn fork(&self) -> Result<Self> {
        let fork = self.client.fork_session(&self.id).await?;
        Ok(Self {client:self.client.clone(), id:fork.id, selection:self.selection.clone()})
    }
    pub async fn run_streamed(&self, input: impl Into<String>) -> Result<ChatStream> {
        self.client.stream_chat(
            ChatRequest::new(self.id.clone(), input).selection(self.selection.clone())
        ).await
    }
    pub async fn run(&self, input: impl Into<String>) -> Result<ThreadResult> {
        let seen: HashSet<String> = self.messages().await?.into_iter().map(|msg| msg.id).collect();
        let mut stream = self.run_streamed(input).await?;
        let mut events = Vec::new();
        while let Some(event) = stream.next().await { events.push(event?); }
        let message = self.messages().await?.into_iter()
            .filter(|m| m.role == "assistant" && !seen.contains(&m.id))
            .last();
        let final_response = message.as_ref().map_or_else(String::new, |m| m.content.clone());
        Ok(ThreadResult { final_response, message, events })
    }
}
