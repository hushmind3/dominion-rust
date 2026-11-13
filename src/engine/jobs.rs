use crate::engine::chat::TurnRequest;
use crate::engine::run_quiet;
use crate::engine::{goals, learning, reflect, rooms, workflows};
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
        if let Err(error) = self.checkpoint(patch) {
            self.state.store.audit(
                "job.checkpoint_failed",
                json!({"jobId":self.job_id,"error":error}),
            );
        }
    }

    pub fn checkpoint(&self, patch: Value) -> Result<(), String> {
        let job = self
            .state
            .store
            .get("job", &self.job_id)
            .ok_or("작업 체크포인트를 찾을 수 없습니다.")?;
        if job["cancelRequested"].as_bool() == Some(true) {
            return Err("사용자 요청으로 실행이 중단되었습니다.".into());
        }
        let saved = self
            .state
            .store
            .patch_if(
                "job",
                &self.job_id,
                "cancelRequested",
                job.get("cancelRequested").unwrap_or(&Value::Null),
                &patch,
            )
            .ok_or("작업 체크포인트가 변경되었거나 실행이 중단되었습니다.")?;
        self.state.bus.record("job", &saved);
        Ok(())
    }
}

pub fn enqueue(state: &AppState, kind: &str, input: Value) -> Option<Value> {
    let job = state.store.put(
        "job",
        &json!({
            "kind": kind,
            "status": "queued",
            "queuedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            "input": input,
            "attempts": 0,
            "stepIndex": 0,
            "stepTotal": 0,
            "events": [],
        }),
    );
    if let Some(job) = &job {
        state.bus.record("job", job);
        state.store.audit(
            "job.queued",
            json!({ "kind": kind, "jobId": job.get("id") }),
        );
    }
    job
}

/// Atomically deduplicate an immutable operation across HTTP requests and
/// workers. The supplied ID belongs to the engine, never a client record.
pub fn enqueue_once(state: &AppState, id: &str, kind: &str, input: Value) -> Option<Value> {
    let candidate = json!({"id":id,"kind":kind,"input":input,"status":"queued",
        "queuedAt":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros,true),
        "attempts":0,"stepIndex":0,"stepTotal":0,"events":[]});
    if let Some(saved) = state.store.put_if_absent("job", &candidate) {
        state.bus.record("job", &saved);
        state
            .store
            .audit("job.queued", json!({"kind":kind,"jobId":id}));
        return Some(saved);
    }
    state
        .store
        .get("job", id)
        .filter(|existing| existing["kind"] == kind && existing["input"] == candidate["input"])
}

/// Recovers work left behind by a crash: running jobs are re-queued (unless
/// they already exhausted retries) and running executions become interrupted.
pub fn recover(state: &AppState) {
    for job in state.store.all("job") {
        if job.get("status").and_then(|value| value.as_str()) != Some("running") {
            continue;
        }
        let attempts = job
            .get("attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let mut updated = job.clone();
        updated["recovered"] = json!(true);
        let review_replayable = job["kind"] == "goal"
            && crate::engine::goal_review::replayable(&job);
        // When the final goal decision was saved before the worker's terminal
        // job update, reconcile its immutable outcome without another LLM call.
        let terminal_goal = job.pointer("/input/goalId").and_then(Value::as_str)
            .filter(|_| job["kind"] == "goal")
            .and_then(|id| state.store.get("goal", id))
            .filter(|goal| matches!(goal["status"].as_str(),
                Some("completed" | "cancelled" | "failed" | "needs_review")));
        let flagged = [
            "/inFlight",
            "/promptState/inFlight",
            "/rsiCompareInFlight",
            "/rsiEvolutionInFlight",
            "/goalState/inFlight",
            "/goalState/reviewPending",
            "/roomState/inFlight",
            "/workflowState/inFlight",
            "/discordState/inFlight",
            "/videoState/inFlight",
        ]
        .iter()
        .any(|path| {
            if *path == "/goalState/reviewPending" && review_replayable {
                return false;
            }
            job.pointer(path)
                .is_some_and(|value| !value.is_null() && value != &json!(false))
        });
        // A tool may have reached an external system before the crash. Its
        // result is unknown, so replaying the job could repeat a side effect.
        let job_session = job["sessionId"]
            .as_str()
            .or_else(|| job.pointer("/input/sessionId").and_then(Value::as_str));
        let unknown_external_result = job_session.is_some_and(|session| {
            state.store.all("execution_operation").iter().any(|operation| {
                operation["outcomeUnknown"] == true
                    && operation["runId"]
                        .as_str()
                        .and_then(|run_id| state.store.get("execution", run_id))
                        .is_some_and(|run| run["sessionId"].as_str() == Some(session))
            })
        });
        let uncertain = flagged || unknown_external_result;
        // A generation whose campaign is already finished can never become
        // runnable again. Re-queueing it wedges the self-improvement start
        // guard forever, so retire it during recovery.
        let orphan_child = job["kind"].as_str() == Some("rsi_generation")
            && job.pointer("/input/rsiCampaignId").and_then(Value::as_str).is_some_and(|id| {
                state.store.get("job", id).is_none_or(|parent| {
                    !matches!(parent["status"].as_str(), Some("running" | "queued"))
                })
            });
        updated["status"] = if let Some(ref completed) = terminal_goal {
            completed["status"].clone()
        } else if orphan_child || attempts >= 3 || uncertain {
            json!("needs_review")
        } else {
            json!("queued")
        };
        if let Some(ref completed) = terminal_goal {
            updated["result"] = json!({
                "goalId":completed["id"],"status":completed["status"],
                "sessionId":completed["sessionId"],"iterations":completed["iterations"],
                "evidence":completed["evidence"],"gaps":completed["gaps"],
            });
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        }
        if terminal_goal.is_some() {
            // The committed goal result is authoritative; don't manufacture
            // an interruption error after a successful completion.
            updated["error"] = Value::Null;
        } else if orphan_child {
            updated["error"] =
                json!("부모 캠페인이 종료되어 고아 세대 작업을 정리했습니다.");
        } else if uncertain {
            updated["error"] =
                json!("재시작 전 작업 결과를 확정할 수 없습니다. 실행 기록을 확인하세요.");
        }
        state.store.put("job", &updated);
        if matches!(job["kind"].as_str(), Some("room" | "meeting")) {
            if let Some(id) = job.pointer("/input/roomId").and_then(Value::as_str) {
                let status = updated["status"].as_str().unwrap_or("needs_review");
                if let Some(saved)=state.store.patch_if("room",id,"id",&json!(id),&json!({
                    "status":status,"activeAgentId":null,"pauseReason":if status=="needs_review" {"재시작 전 발언이나 도구 결과를 확인해야 합니다."} else {""}
                })) { state.bus.record("room",&saved); }
            }
        }
        if uncertain || attempts >= 3 {
            if let Some(id) = job.pointer("/input/goalId").and_then(Value::as_str) {
                if let Some(mut goal) = state.store.get("goal", id) {
                    if matches!(
                        goal["status"].as_str(),
                        Some("completed" | "cancelled" | "failed" | "needs_review")
                    ) {
                        continue;
                    }
                    goal["status"] = json!("needs_review");
                    goal["phase"] = json!("needs_review");
                    goal["gaps"] = json!([
                        "재시작 전 실행 결과가 확정되지 않았습니다. 실행 증거를 확인하세요."
                    ]);
                    state.store.put("goal", &goal);
                }
            }
        }
        state.store.audit(
            "job.recovered",
            json!({ "jobId": job.get("id"), "attempts": attempts }),
        );
    }
    for execution in state.store.all("execution") {
        if execution.get("status").and_then(|value| value.as_str()) == Some("running") {
            let mut updated = execution.clone();
            updated["status"] = json!("interrupted");
            updated["phase"] = json!("interrupted");
            updated["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            updated["taskOutcome"] = json!("cancelled");
            updated["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
            updated["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution", &updated);
        }
    }
    for mut operation in state.store.all("execution_operation") {
        if operation["status"] == "running" {
            operation["status"] = json!("interrupted");
            operation["outcomeUnknown"] = json!(true);
            operation["phase"] = json!("interrupted");
            operation["reason"] = json!("백엔드가 재시작되어 도구 결과를 확정할 수 없습니다.");
            operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("execution_operation", &operation);
        }
    }
    // Align campaign records with their job. A campaign stuck on "running"
    // after its job finished also blocks the UI and later starts.
    for mut campaign in state.store.all("rsi_campaign") {
        if campaign["status"].as_str() != Some("running") {
            continue;
        }
        let id = campaign["id"].as_str().unwrap_or("").to_string();
        let job = state.store.get("job", &id);
        let live = job
            .as_ref()
            .is_some_and(|job| matches!(job["status"].as_str(), Some("running" | "queued")));
        if live {
            continue;
        }
        campaign["status"] = job
            .as_ref()
            .and_then(|job| job["status"].as_str())
            .map(|status| json!(status))
            .unwrap_or_else(|| json!("needs_review"));
        campaign["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("rsi_campaign", &campaign);
    }
    // After interrupted executions have been recorded, expire durable approvals
    // that outlived their live waiter before wake decisions are made.
    crate::engine::chat::expire_stale_approvals(state);
}

/// Starts the durable job worker plus the automation scheduler.
/// Requeues learning jobs that failed because the learning model was
/// unavailable. The work is kept, not lost, and a later worker picks it up.
pub fn retry_failed_learning(state: &AppState) -> usize {
    let mut retried = 0;
    for job in state.store.all("job") {
        if job["kind"].as_str() != Some("learn")
            || job["status"].as_str() != Some("failed")
        {
            continue;
        }
        let attempts = job["attempts"].as_u64().unwrap_or(0);
        if attempts >= 3 {
            continue;
        }
        let Some(id) = job["id"].as_str() else {
            continue;
        };
        let delay = 300i64.saturating_mul(attempts.max(1) as i64);
        let available = chrono::Utc::now() + chrono::Duration::seconds(delay);
        if let Some(saved) = state.store.patch_if(
            "job",
            id,
            "status",
            &json!("failed"),
            &json!({
                "status": "queued",
                "availableAt": available.to_rfc3339(),
                "retriedAt": chrono::Utc::now().to_rfc3339(),
                "lastError": job["error"],
                "error": Value::Null,
            }),
        ) {
            state.bus.record("job", &saved);
            state.store.audit("job.learning_retry", json!({"jobId": id, "attempts": attempts}));
            retried += 1;
        }
    }
    retried
}

pub fn start(state: AppState) {
    crate::engine::channel_adapter::start(state.clone());
    let workers = std::env::var("DOMINION_JOB_WORKERS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 16);
    for _ in 0..workers {
        let worker_state = state.clone();
        tokio::spawn(async move {
            loop {
                match next_queued(&worker_state) {
                    Some(job) => run_job(worker_state.clone(), job).await,
                    None => tokio::time::sleep(Duration::from_millis(400)).await,
                }
            }
        });
    }

    let maintenance_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            schedule_automations(&state);
            // A process restart removes in-memory approval timers. The persisted
            // deadline still expires automatically during normal operation.
            crate::engine::chat::expire_stale_approvals(&state);
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            retry_failed_learning(&maintenance_state);
            crate::engine::agent_growth::sweep(&maintenance_state);
            crate::engine::distill::sweep(&maintenance_state);
            crate::engine::memory::learning_sweep(&maintenance_state);
        }
    });
}

fn next_queued(state: &AppState) -> Option<Value> {
    let jobs = state.store.all("job");
    let learning_running = jobs
        .iter()
        .any(|job| job["status"] == "running" && job["kind"] == "learn");
    let comparison_running = jobs.iter().any(|job|
        job["status"]=="running"&&job["kind"]=="rsi_compare");
    let mut queued = jobs
        .iter()
        .cloned()
        .filter(|job| job.get("status").and_then(|value| value.as_str()) == Some("queued"))
        // Learning is background work: at most one at a time, so it can never
        // occupy every worker while a prompt/room/workflow job waits.
        .filter(|job| !(job["kind"] == "learn" && learning_running))
        .filter(|job| !(job["kind"] == "rsi_compare" && comparison_running))
        .filter(|job| {
            if !matches!(job["kind"].as_str(), Some("room" | "meeting")) {
                return true;
            }
            let Some(room_id) = job.pointer("/input/roomId").and_then(Value::as_str) else {
                return true;
            };
            !state
                .active
                .lock()
                .unwrap()
                .contains(&format!("room-run:{room_id}"))
                && !jobs.iter().any(|other| {
                    other["status"] == "running"
                        && matches!(other["kind"].as_str(), Some("room" | "meeting"))
                        && other.pointer("/input/roomId").and_then(Value::as_str) == Some(room_id)
                })
        })
        .filter(|job| {
            job.get("availableAt")
                .and_then(Value::as_str)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_none_or(|at| at <= chrono::Utc::now())
        })
        .collect::<Vec<_>>();
    queued.sort_by(|a, b| {
        let priority = |job: &Value| u8::from(matches!(job["kind"].as_str(),Some("learn"|"rsi_compare")));
        let by_priority = priority(a).cmp(&priority(b));
        if by_priority != std::cmp::Ordering::Equal {
            return by_priority;
        }
        let left = a
            .get("queuedAt")
            .or_else(|| a.get("createdAt"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let right = b
            .get("queuedAt")
            .or_else(|| b.get("createdAt"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        left.cmp(right)
    });
    queued.into_iter().find_map(|job| {
        let id = job.get("id")?.as_str()?;
        state.store.patch_if(
            "job",
            id,
            "status",
            &json!("queued"),
            &json!({
                "status": "running", "startedAt": chrono::Utc::now().to_rfc3339(),
                "attempts": job["attempts"].as_u64().unwrap_or(0) + 1,
            }),
        )
    })
}

/// Runs a delegated job immediately instead of waiting for a worker slot, so a
/// parent waiting on a child can never deadlock the worker pool.
pub fn start_now(state: AppState, job_id: String) {
    tokio::spawn(async move {
        let Some(job) = state.store.get("job", &job_id) else {
            return;
        };
        if job["status"] != "queued" {
            return;
        }
        let Some(running) = state.store.patch_if(
            "job",
            &job_id,
            "status",
            &json!("queued"),
            &json!({"status":"running","startedAt":chrono::Utc::now().to_rfc3339(),"attempts":job["attempts"].as_u64().unwrap_or(0)+1}),
        ) else {
            return;
        };
        run_job(state, running).await;
    });
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

    state.bus.record("job", &job);

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
        "rsi_compare" => crate::engine::rsi_worktree::compare(&context,&job).await,
        "rsi_evolve" => crate::engine::rsi_evolve::run(&context,&job).await,
        "resume" => run_resume(&context, &job).await,
        "learn" => learning::run(&context, &job).await,
        "reflect" => reflect::run(&context, &job).await,
        "discord" => crate::engine::discord::run(&context, &job).await,
        "video" => crate::engine::video::run(&context, &job).await,
        "youtube_upload" => crate::engine::video::upload(&context, &job).await,
        other => Err(format!("지원하지 않는 작업 종류입니다: {other}")),
    };

    match result {
        Ok(result) => {
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "needs_review" | "cancelled" | "failed"))
                .unwrap_or("completed");
            context.patch(json!({
                "status": status,
                "result": result,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state.store.audit(
                &format!("job.{status}"),
                json!({ "jobId": job_id, "kind": kind }),
            );
        }
        Err(error) => {
            if error.starts_with("deferred:") {
                context.patch(json!({"status":"queued","availableAt":(chrono::Utc::now()+chrono::Duration::seconds(1)).to_rfc3339(),
                    "attempts":job["attempts"].as_u64().unwrap_or(1).saturating_sub(1),"error":error}));
                return;
            }
            let status = if error.starts_with("needs_review:") {
                "needs_review"
            } else if error.contains("중단") || error.contains("cancelled") {
                "cancelled"
            } else {
                "failed"
            };
            context.patch(json!({
                "status": status,
                "error": error,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "events": context.events.lock().unwrap().clone(),
            }));
            state.store.audit(
                "job.failed",
                json!({ "jobId": job_id, "kind": kind, "error": error }),
            );
        }
    }
}

/// An implementation-stage generation must carry a real edit contract,
/// including autonomous descendants. Previously autonomous=true disabled the
/// edit-only continuation even after a successful source read: three live
/// generations read the target but made zero write calls. Discovery-stage
/// workers still retain the broad tool palette.
fn rsi_focused_edit_goal(input:&Value)->bool {
    let has_file=input["rsiFocusFiles"].as_array().is_some_and(|paths|
        paths.iter().any(|path|path.as_str().is_some_and(|path|!path.is_empty())));
    let patch_mode=matches!(input["rsiImprovementDirective"]["mode"].as_str(),
        Some("implement_research"|"change_approach"|"revise_candidate"|"repair_counterexample"));
    has_file && patch_mode
}

/// A patch-oriented generation that made zero tool-backed edits is an
/// unfinished attempt, not an opportunity to record imaginary progress.
/// Retry once with the prior conversation and the same detached worktree.
fn rsi_no_edit_retry_needed(input:&Value, outcome:&Result<Value,String>)->bool {
    let Some(result)=outcome.as_ref().ok() else {return false};
    matches!(input["rsiImprovementDirective"]["mode"].as_str(),
        Some("implement_research"|"change_approach"|"repair_counterexample"|"revise_candidate"))
        && result["rsi"]["candidateId"].is_null()
        && result["rsi"]["sourceClean"]==true
        && ((result["rsi"]["diffBytes"]==0
            && result["rsi"]["captureError"]=="모델이 실제 소스 파일을 변경하지 않았습니다.")
            || (input["rsiRepairCandidateId"].is_string()
                && result["rsi"]["captureError"]==
                    "복원한 개선 후보를 이번 세대에서 실제로 수정하지 않았습니다."))
        && result["rsi"]["modelError"].as_str().is_some_and(str::is_empty)
        && input["toolsEnabled"]!=false
        && input["toolsExecutionEnabled"]!=false
}

/// Opt-in source-driven induction runs inside a clean detached worktree.
/// Explicit owner edits and inherited compiler failures remain model-driven.
fn rsi_native_preflight(input:&Value, restored:bool)->bool {
    input["rsiNativeInduction"]==true
        && input["rsiAutoFocus"]==true
        && !restored
        && !input["rsiRepairCandidateId"].is_string()
        && !input["rsiInheritedCandidateId"].is_string()
}

pub(crate) async fn run_prompt(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    let prompt = input
        .get("prompt")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if prompt.trim().is_empty() {
        return Err("작업 입력(prompt)이 비어 있습니다.".into());
    }
    if let Some(result) = job.pointer("/promptState/result") {
        return Ok(result.clone());
    }
    if job
        .pointer("/promptState/inFlight")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(
            json!({"status":"needs_review","reason":"중단된 프롬프트 작업 결과를 확인해야 합니다."}),
        );
    }
    let rsi_workspace = if input["rsiSourceDir"].is_string() {
        Some(crate::engine::rsi_worktree::prepare(context,&input).await?)
    } else { None };
    // Recover the prior compiler-rejected model patch in the new detached
    // worktree BEFORE the worker starts. The baseline HEAD is unchanged;
    // normal candidate capture later includes the repaired original patch.
    let restored=if let Some(worktree)=&rsi_workspace {
        crate::engine::rsi_worktree::restore_failed_candidate(context,&input,worktree).await?
    }else{None};
    let repair_instruction=restored.as_ref().map(|restored|{
        let paths=restored["paths"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).take(3).collect::<Vec<_>>().join(", ");
        let errors=restored["compilerErrors"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).collect::<Vec<_>>().join("\n");
        if restored["repairKind"]=="quality_plateau" {
            let metrics=&restored["qualityMetrics"];
            format!("\n\n[독립 평가된 미달 후보 계승] 이전 모델이 만든 Rust 패치가 별도 작업공간 ({paths})에 실제로 복원돼 있습니다. 컴파일은 통과했지만 품질 이득이 검증 기준에 부족했습니다. 실제 영수증: {metrics}. 먼저 workspace_read_range로 복원된 파일의 최신 원문과 해시를 확인한 뒤 기존 구현의 누락 동작을 고쳐 workspace_replace_function 또는 workspace_replace_range로 변경하세요. 이전 코드를 처음부터 다시 만들거나 분석문만 출력하지 마세요. 새 후보의 동결 평가가 다시 실행됩니다.")
        } else {
            format!("\n\n[실제 컴파일 실패 패치 복구] 이전 모델의 실패한 소스 변경이 지금 별도 작업공간에 실제로 복원돼 있습니다. 새 파일이나 조사 계획을 만들지 말고, 복구된 파일 ({paths})을 workspace_read_range로 읽으세요. 아래 컴파일 오류를 해결하도록 파일을 직접 수정하세요. 기존 패치를 다시 적용할 필요가 없습니다. 고친 코드만 workspace_replace_function 또는 workspace_replace_range로 쓰세요. 컴파일과 독립 평가는 엔진이 진행합니다.\n{errors}")
        }
    }).unwrap_or_default();
    // The dedicated source checkout is validated and detached before a model
    // call, never modified directly by the RSI execution.
    let session_id = job
        .get("sessionId")
        .or_else(|| input.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let session_id = match session_id {
        Some(id) => id,
        None => context.state.store.put("session",&json!({"title":prompt.chars().take(40).collect::<String>(),"agentId":input.get("agentId")}))
            .and_then(|session|session["id"].as_str().map(str::to_string)).ok_or("작업 세션을 저장할 수 없습니다.")?,
    };
    if rsi_workspace.is_some() {
        if let Some(mut session)=context.state.store.get("session",&session_id) {
            session["rsiRun"]=json!(true);
            session["rsiControlExperiment"]=json!(job["rsiControlExperiment"]==true);
            session["rsiSystemEvolution"]=json!(input["rsiSystemEvolution"]==true);
            // The actual provider call enforces this local allowance across
            // all model and compiler-repair turns sharing this session.
            session["rsiGenerationTokenLimit"]=input["rsiGenerationTokenLimit"].clone();
            if restored.is_some() {
                session["rsiCompilerRepair"]=json!(restored.as_ref()
                    .is_some_and(|row|row["repairKind"]=="compiler_failure"));
                session["rsiRestoredCandidateId"]=input["rsiRepairCandidateId"].clone();
                session["rsiQualityRepair"]=json!(restored.as_ref()
                    .is_some_and(|row|row["repairKind"]=="quality_plateau"));
            }
            session["rsiObjective"]=input["rsiObjective"].clone();
            // Whole-system generation must edit coupled task/meta modules.
            // A source-specific ancestor scope may not freeze sibling edits.
            session["rsiExplicitFocus"]=json!(input["rsiSystemEvolution"]!=true
                && rsi_focused_edit_goal(&input));
            session["rsiFocusContextReady"]=json!(
                restored.is_none()
                && input["rsiFocusContextChars"].as_u64().unwrap_or(0)>0
                && input["rsiParentContextFit"]["sourceExcerptDeferred"]!=true);
            crate::engine::rsi_evolve::configure_method_session(&context.state, &input, &mut session);
            session["rsiCodeIndexId"]=input["rsiIndexId"].clone();
            // The coordinator learned this native execution policy from
            // prior real source/evaluation receipts. The model is a worker,
            // never the authority for upgrading the policy.
            session["rsiExecutionMethod"]=input["rsiExecutionMethod"].clone();
            session["rsiEditPolicy"]=input["rsiEditPolicy"].clone();
            session["rsiSourceDir"]=input["rsiSourceDir"].clone();
            if let Some(id)=input["rsiCampaignId"].as_str(){
                session["rsiCampaignId"]=json!(id);
                session["rsiGenerationJobId"]=json!(context.job_id);
            }
            context.state.store.put("session",&session);
        }
    }
    let reused_research=if let Some(worktree)=&rsi_workspace {
        crate::engine::rsi_evolve::reuse_research(&context.state,&input,&worktree.path,&session_id)
    }else{String::new()};
    context.checkpoint(json!({"sessionId":session_id,"promptState":{"inFlight":true}}))?;
    let request = TurnRequest {
        session_id: Some(session_id.clone()),
        input: if rsi_workspace.is_some() { format!("{prompt}{reused_research}{repair_instruction}\n\n현재 대상은 별도의 Git 작업공간입니다. 제공된 검증 원문과 인덱스는 바로 활용하고, 빠진 정보만 도구로 조회하세요. 개선 근거가 있을 때 workspace_replace 또는 workspace_replace_range로 수정하세요. workspace_replace_range는 workspace_read_range에서 읽은 파일의 수정 범위와 newText를 사용합니다. 모델이 SHA를 누락해도 엔진이 같은 파일의 실제 읽기 영수증에서 자동 연결하고, 쓰기 직전에 파일 버전을 다시 검사합니다. 변경이 불가능하면 실제 도구 오류를 설명하세요. 절대경로 파일 도구, 셸, Git API, 다른 에이전트는 사용하지 마세요. 빌드·타입 검사와 패치 수집은 실행 엔진이 종료 후 별도로 처리합니다.") } else { prompt },
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
        attachment_ids: Vec::new(),
        tools_enabled: input["toolsEnabled"].as_bool().unwrap_or(true),tools_execution_enabled: input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
        thinking: input["thinking"].as_bool(),
        auto_learning: false,
        reasoning_effort: input["reasoningEffort"].as_str().map(str::to_string),
        reasoning_language: None,
        system_override: None,
        task_id: None,
        parent_task_id: None,
        task_revision: None,
        verification_owner: None,
    };

    // A detached worktree protects the running DOMINION checkout. The worker
    // captures the real diff and verifier evidence before removing that copy.
    let mut scoped_state = context.state.clone();
    if let Some(worktree)=&rsi_workspace {scoped_state.config.workspace=worktree.path.clone();}
    // Source-driven induction is an independently identified generator,
    // not a synthetic model tool call. Its real patch follows the SAME
    // verifier, paired frozen holdout and descendant adoption pipeline.
    let native_preflight=if rsi_native_preflight(&input,restored.is_some()) {
        if let Some(worktree)=&rsi_workspace {
            match crate::engine::rsi_mutation::induce(
                &context.state,&context.job_id,&input,&worktree.source,&worktree.path
            ) {
                Ok(receipt)=>receipt,
                Err(error)=>{
                    context.state.store.audit("rsi.native_induction_error",
                        json!({"jobId":context.job_id,"error":error}));
                    None
                }
            }
        }else{None}
    }else{None};
    let mut message=if native_preflight.is_some() {
        Ok(None)
    }else{
        run_quiet(&scoped_state,&request).await
    };
    let result = if let Some(worktree)=&rsi_workspace {
        // Spend one focused compiler repair here, then hand the failed
        // *real source diff* to a rebuilt descendant. Spending the entire
        // campaign on repeated reads starved generation two in real Actions.
        const MAX_REPAIRS:usize=1;
        let mut attempts=Vec::<Value>::new();
        let mut finished=crate::engine::rsi_worktree::finish(
            context,job,worktree,message,&session_id
        ).await;
        // Separate a generation that never edited from one whose edit failed
        // verification. No source mutations are fabricated by this fallback:
        // the same model must make a real workspace tool call to produce a diff.
        let mut no_edit_retry_attempts=0usize;
        if rsi_no_edit_retry_needed(&input,&finished)
            && context.checkpoint(json!({"rsiPhase":"retry_no_edit",
                "rsiNoEditRetryAttempt":1})).is_ok()
        {
            let focus=input["rsiFocusFiles"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).take(4).collect::<Vec<_>>().join(", ");
            let mut retry=request.clone();
            retry.input=format!(
                "이전 실제 실행은 소스 변경 0건으로 종료되었습니다. 지금 같은 격리 작업공간에서 사용자 원래 수정 목표를 반드시 이어가세요. 지정된 파일: {focus}.                  코드 근거가 부족하면 workspace_read_range로 해당 부분을 읽고, 검토한 원문의 oldText로 workspace_replace를 쓰거나, 읽은 범위의 startLine/endLine/sourceSha256를 이용해 workspace_replace_range로 수정하세요.                  기존 응답이나 조사 요약만 반복하지 말고 실행 도구를 실제 호출하세요. 편집이 불가능하면 구체적인 코드·도구 오류를 설명하세요.                  사용자 목표: {}",
                input["rsiObjective"].as_str().unwrap_or("")
            );
            let correction=run_quiet(&scoped_state,&retry).await;
            finished=crate::engine::rsi_worktree::finish(
                context,job,worktree,correction,&session_id
            ).await;
            no_edit_retry_attempts=1;
        }
        // Borrow Sub3's bounded rustc-guided source repair: a rejected
        // model edit gets machine-applicable compiler suggestions before
        // spending another model turn. All amended bytes are re-captured
        // and independently evaluated; an unsuccessful repair rolls back.
        let mut compiler_repairs=Vec::<Value>::new();
        for revision in 0..=MAX_REPAIRS {
            let compiler_failed=finished.as_ref().ok().is_some_and(|prior|
                prior["reason"]=="build_or_typecheck_failed"
                && prior["rsi"]["editEvidence"]["verifiedWrites"].as_u64().unwrap_or(0)>0
                && prior["rsi"]["candidateOrigin"]!="native_inductive_source_mutation");
            if compiler_failed {
                match crate::engine::rsi_compiler_repair::recover(worktree).await {
                    Ok(Some(receipt)) => {
                        // The original candidate hash is stale after any
                        // compiler amendment; always capture the FULL diff.
                        match crate::engine::rsi_worktree::finish(
                            context,job,worktree,Ok(None),&session_id
                        ).await {
                            Ok(rechecked) => {
                                context.state.store.audit("rsi.compiler_repair_verified",
                                    json!({"jobId":context.job_id,"receipt":receipt,
                                        "candidateId":rechecked["rsi"]["candidateId"]}));
                                compiler_repairs.push(receipt);
                                finished=Ok(rechecked);
                            }
                            Err(error) => context.state.store.audit(
                                "rsi.compiler_repair_capture_failed",
                                json!({"jobId":context.job_id,"error":error})),
                        }
                    }
                    Ok(None) => {}
                    Err(error) => context.state.store.audit(
                        "rsi.compiler_repair_unavailable",
                        json!({"jobId":context.job_id,"error":error})),
                }
            }
            if revision==MAX_REPAIRS {break;}
            let Some(feedback)=finished.as_ref().ok()
                .and_then(crate::engine::rsi_worktree::repair_feedback) else {break;};
            // Cancellation or process restart is never a reason to replay a write.
            if context.checkpoint(json!({
                "rsiPhase":"repairing","rsiRepairAttempt":revision+1
            })).is_err(){break;}
            let prior=finished.as_ref().unwrap();
            let iteration=json!({
                "id":format!("{}-{}",context.job_id,revision),
                "jobId":context.job_id,
                "attempt":revision+1,"result":prior["rsi"],
                "status":prior["status"],"reason":prior["reason"]
            });
            context.state.store.put("rsi_iteration",&iteration);
            attempts.push(iteration);
            let mut retry=request.clone();
            // Supply the current compiler-failing source and its full-file
            // digest; this is an executable edit target, not a vague model
            // instruction or an old excerpt from a previous generation.
            let source=crate::engine::rsi_worktree::compiler_repair_context(
                &worktree.path,prior);
            retry.input=match source {
                Some(context)=>format!("{feedback}\n\n{context}"),
                None=>feedback,
            };
            // This is a correction attempt, not a new research generation.
            // The native runtime must not restart its inherited discovery
            // procedure ahead of the compiler-failing source edit.
            if let Some(mut session)=scoped_state.store.get("session",&session_id) {
                session["rsiCompilerRepair"]=json!(true);
                scoped_state.store.put("session",&session);
            }
            message=run_quiet(&scoped_state,&retry).await;
            finished=crate::engine::rsi_worktree::finish(
                context,job,worktree,message,&session_id
            ).await;
        }
        if let Some(mut session)=scoped_state.store.get("session",&session_id) {
            session["rsiCompilerRepair"]=json!(false);
            scoped_state.store.put("session",&session);
        }
        let candidate_id=finished.as_ref().ok().map(|result|result["rsi"]["candidateId"].clone()).unwrap_or(Value::Null);
        let hypothesis=finished.as_ref().ok().map(|result|result["rsi"]["modelMessageId"].clone()).unwrap_or(Value::Null);
        let generation=json!({"jobId":context.job_id,"candidateId":candidate_id,"accepted":false,"domain":job["rsiDomain"],"hypothesisMessageId":hypothesis});
        // OFF experiments are counterfactual observations, not new
        // production experience or executable-method promotions. Only ON
        // generations may add knowledge or method execution feedback.
        if job["rsiControlExperiment"]!=true {
            crate::engine::rsi_evolve::remember_research(
                &context.state,&input,&generation,&session_id);
            crate::engine::rsi_evolve::collect_method_executions(
                &context.state,&session_id,&worktree.path);
        }
        if let Err(error)=crate::engine::rsi_worktree::cleanup(worktree).await {
            context.state.store.audit("rsi.worktree_cleanup_failed",
                json!({"jobId":context.job_id,"error":error}));
        }
        let mut output=finished?;
        output["noEditRetryAttempts"]=json!(no_edit_retry_attempts);
        output["repairAttempts"]=json!(attempts.len());
        output["compilerRepairReceipts"]=json!(compiler_repairs);
        output["previousAttempts"]=json!(attempts);
        output
    } else {
        let message=message?;
        json!({
            "sessionId": message.as_ref().and_then(|item| item.get("session_id")),
            "messageId": message.as_ref().and_then(|item| item.get("id")),
        })
    };
    context.checkpoint(json!({"promptState":{"inFlight":false,"result":result}}))?;
    Ok(result)
}

/// Resumes a cleanly interrupted run after a crash. The owner message already
/// exists, so this path uses the no-record-input turn instead of run_prompt.
async fn run_resume(context: &JobContext, job: &Value) -> Result<Value, String> {
    let input = job.get("input").cloned().unwrap_or_else(|| json!({}));
    if let Some(result) = job.pointer("/promptState/result") {
        return Ok(result.clone());
    }
    if job
        .pointer("/promptState/inFlight")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(json!({
            "status":"needs_review",
            "reason":"재개 작업의 이전 시도 결과를 확인해야 합니다."
        }));
    }
    let prompt = input["prompt"].as_str().unwrap_or("").to_string();
    let session_id = input["sessionId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("재개 작업에 sessionId가 필요합니다.")?
        .to_string();
    if prompt.trim().is_empty() {
        return Err("재개 작업의 원래 요청이 비어 있습니다.".into());
    }
    if context.state.store.get("session", &session_id).is_none() {
        return Err("재개 작업의 세션을 찾을 수 없습니다.".into());
    }
    context.checkpoint(json!({"sessionId":session_id,"promptState":{"inFlight":true}}))?;
    let request = TurnRequest {
        session_id: Some(session_id.clone()),
        input: prompt,
        attachment_ids: Vec::new(),
        agent_id: input["agentId"].as_str().map(str::to_string),
        provider_id: input["providerId"].as_str().map(str::to_string),
        model: input["model"].as_str().map(str::to_string),
        tools_enabled: input["toolsEnabled"].as_bool().unwrap_or(true),
        tools_execution_enabled: input["toolsExecutionEnabled"].as_bool().unwrap_or(true),
        thinking: None,
        auto_learning: false,
        reasoning_effort: None,
        reasoning_language: None,
        system_override: None,
        task_id: input["taskId"].as_str().map(str::to_string),
        parent_task_id: None,
        task_revision: input["taskRevision"].as_str().map(str::to_string),
        verification_owner: None,
    };
    let message = crate::engine::run_quiet_resume(&context.state, &request).await?;
    let result = json!({
        "sessionId": session_id,
        "messageId": message.as_ref().and_then(|item| item.get("id")),
        "resumedExecutionId": input["resumedExecutionId"],
    });
    context.checkpoint(json!({"promptState":{"inFlight":false,"result":result}}))?;
    Ok(result)
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
        let input = automation
            .get("input")
            .cloned()
            .unwrap_or_else(|| json!({}));
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
#[path = "jobs/tests.rs"]
mod tests;
