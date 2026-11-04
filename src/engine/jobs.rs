use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::{goals, learning, rooms, workflows};
use crate::state::AppState;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_EVENTS: usize = 200;

/// Shared handle a running job uses to report progress and persist its state.
#[derive(Clone)]
pub struct JobContext {
    pub state: AppState,
    pub job_id: String,
    events: Arc<Mutex<Vec<Value>>>,
}

impl JobContext {
    pub fn new(state: AppState, job_id: String) -> Self {
        Self {
            state,
            job_id,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn emit(&self, event: Value) {
        let mut payload = event.clone();
        payload["jobId"] = json!(self.job_id);
        self.state.bus.emit(json!({
            "type": "job_event",
            "jobId": self.job_id,
            "event": payload,
        }));
        let mut events = self.events.lock().unwrap();
        if events.len() < MAX_EVENTS {
            events.push(json!({
                "type": event.get("type").cloned().unwrap_or(json!("event")),
                "at": chrono::Utc::now().to_rfc3339(),
            }));
        }
    }

    /// Merge a patch into the persisted job record and notify listeners.
    pub fn patch(&self, patch: Value) {
        let Some(mut job) = self.state.store.get("job", &self.job_id) else {
            return;
        };
        if let (Value::Object(target), Value::Object(source)) = (&mut job, patch) {
            for (key, value) in source {
                target.insert(key, value);
            }
        }
        if let Some(saved) = self.state.store.put("job", &job) {
            self.state.bus.record("job", &saved);
        }
    }
}

pub fn enqueue(state: &AppState, kind: &str, input: Value) -> Option<Value> {
    let job = state.store.put(
        "job",
        &json!({
            "kind": kind,
            "status": "queued",
            "input": input,
            "attempts": 0,
            "stepIndex": 0,
            "stepTotal": 0,
            "events": [],
        }),
    );
    if let Some(job) = &job {
        state.bus.record("job", job);
        state
            .store
            .audit("job.queued", json!({ "kind": kind, "jobId": job.get("id") }));
    }
    job
}

/// Recovers work left behind by a crash: running jobs are re-queued (unless
/// they already exhausted retries) and running executions become interrupted.
pub fn recover(state: &AppState) {
    for job in state.store.all("job") {
        if job.get("status").and_then(|value| value.as_str()) != Some("running") {
            continue;
        }
        let attempts = job.get("attempts").and_then(|value| value.as_u64()).unwrap_or(0);
        let mut updated = job.clone();
        updated["recovered"] = json!(true);
        updated["status"] = json!(if attempts >= 3 { "needs_review" } else { "queued" });
        state.store.put("job", &updated);
        state
            .store
            .audit("job.recovered", json!({ "jobId": job.get("id"), "attempts": attempts }));
    }
    for execution in state.store.all("execution") {
        if execution.get("status").and_then(|value| value.as_str()) == Some("running") {
            let mut updated = execution.clone();
            updated["status"] = json!("interrupted");
            updated["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution", &updated);
        }
    }
}

/// Starts the durable job worker plus the automation scheduler.
pub fn start(state: AppState) {
    let worker_state = state.clone();
    tokio::spawn(async move {
        loop {
            match next_queued(&worker_state) {
                Some(job) => run_job(worker_state.clone(), job).await,
                None => tokio::time::sleep(Duration::from_millis(400)).await,
            }
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            schedule_automations(&state);
        }
    });
}

fn next_queued(state: &AppState) -> Option<Value> {
    let mut queued = state
        .store
        .all("job")
        .into_iter()
        .filter(|job| job.get("status").and_then(|value| value.as_str()) == Some("queued"))
        .collect::<Vec<_>>();
    queued.sort_by(|a, b| {
        let left = a.get("createdAt").and_then(|value| value.as_str()).unwrap_or("");
        let right = b.get("createdAt").and_then(|value| value.as_str()).unwrap_or("");
        left.cmp(right)
    });
    queued.into_iter().next()
}

async fn run_job(state: AppState, job: Value) {
    let job_id = job
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if job_id.is_empty() {
        return;
    }
    let context = JobContext::new(state.clone(), job_id.clone());

    let mut running = job.clone();
    running["status"] = json!("running");
    running["startedAt"] = json!(chrono::Utc::now().to_rfc3339());
    running["attempts"] = json!(job.get("attempts").and_then(|value| value.as_u64()).unwrap_or(0) + 1);
    state.store.put("job", &running);
    state.bus.record("job", &running);

    let kind = job
        .get("kind")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let result = match kind.as_str() {
        "workflow" => workflows::run(&context, &job).await,
        "room" | "meeting" => rooms::run(&context, &job).await,
        "goal" => goals::run(&context, &job).await,
        "prompt" => run_prompt(&context, &job).await,
        "learn" => learning::run(&context, &job).await,
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    match result {
        Ok(result) => {
            context.patch(json!({
                "status": "completed",
                "result": result,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state
                .store
                .audit("job.completed", json!({ "jobId": job_id, "kind": kind }));
        }
        Err(error) => {
            context.patch(json!({
                "status": "failed",
                "error": error,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state
                .store
                .audit("job.failed", json!({ "jobId": job_id, "kind": kind, "error": error }));
        }
    }
}

async fn run_prompt(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let prompt = input
        .get("prompt")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if prompt.trim().is_empty() {
        return Err("작업 입력(prompt)이 비어 있습니다.".into());
    }
    let request = TurnRequest {
        session_id: input
            .get("sessionId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        input: prompt,
        agent_id: input
            .get("agentId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        provider_id: input
            .get("providerId")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        model: input
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        tools_enabled: true,
        thinking: None,
        auto_learning: false,
        reasoning_effort: None,
        reasoning_language: None,
        system_override: None,
    };

    let message = run_quiet(&context.state, &request).await?;

    Ok(json!({
        "sessionId": message.as_ref().and_then(|item| item.get("session_id")),
        "messageId": message.as_ref().and_then(|item| item.get("id")),
    }))
}

fn schedule_automations(state: &AppState) {
    let now = chrono::Utc::now();
    for automation in state.store.all("automation") {
        if automation.get("enabled").and_then(|value| value.as_bool()) != Some(true) {
            continue;
        }
        let Some(next_at) = automation.get("nextAt").and_then(|value| value.as_str()) else {
            continue;
        };
        let Ok(next) = chrono::DateTime::parse_from_rfc3339(next_at) else {
            continue;
        };
        if next.with_timezone(&chrono::Utc) > now {
            continue;
        }
        let kind = automation
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("prompt");
        let input = automation.get("input").cloned().unwrap_or_else(|| json!({}));
        let job_kind = if kind == "meeting" { "room" } else { kind };
        if !matches!(job_kind, "prompt" | "workflow" | "room" | "learn" | "goal") {
            continue;
        }
        enqueue(state, job_kind, input);
        let mut updated = automation.clone();
        let interval = automation
            .get("intervalMinutes")
            .and_then(|value| value.as_i64())
            .unwrap_or(1440)
            .max(1);
        updated["lastAt"] = json!(now.to_rfc3339());
        updated["nextAt"] = json!((now + chrono::Duration::minutes(interval)).to_rfc3339());
        state.store.put("automation", &updated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn temp_state() -> AppState {
        let dir = std::env::temp_dir().join(format!("dominion-jobs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }

    #[test]
    fn enqueue_creates_a_queued_job() {
        let state = temp_state();
        let job = enqueue(&state, "workflow", json!({ "workflowId": "w1", "input": "hi" }))
            .expect("job");
        assert_eq!(job["status"], "queued");
        assert_eq!(job["kind"], "workflow");
        assert_eq!(job["stepTotal"], 0);

        let queued = next_queued(&state).expect("worker sees it");
        assert_eq!(queued["id"], job["id"]);

        let mut running = queued.clone();
        running["status"] = json!("running");
        state.store.put("job", &running);
        assert!(next_queued(&state).is_none());
    }

    #[test]
    fn recover_requeues_running_jobs() {
        let state = temp_state();
        let job = enqueue(&state, "workflow", json!({ "workflowId": "w", "input": "" })).unwrap();
        let mut running = job.clone();
        running["status"] = json!("running");
        running["attempts"] = json!(1);
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "queued");
        assert_eq!(stored["recovered"], true);

        running["attempts"] = json!(3);
        running["status"] = json!("running");
        state.store.put("job", &running);
        recover(&state);
        let stored = state.store.get("job", job["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["status"], "needs_review");
    }

    #[test]
    fn job_errors_are_recorded_as_failed() {
        let state = temp_state();
        let job = enqueue(&state, "room", json!({ "roomId": "missing", "goal": "x" })).unwrap();
        let id = job["id"].as_str().unwrap().to_string();
        let _ = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run_job(state.clone(), job));
        let stored = state.store.get("job", &id).unwrap();
        assert_eq!(stored["status"], "failed");
        assert!(stored["error"].as_str().unwrap().contains("회의실"));
    }
}
