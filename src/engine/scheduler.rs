//! H8 production scheduler: a durable periodic loop that wakes the isolated
//! experiment runner when a request is queued.
//!
//! The production engine never runs experiments in-process. It only invokes the
//! runner binary, which claims one request from `claim_url`, runs paired trials
//! in its own data root, and completes the claim. If the process fails, this
//! loop releases its claim so the request is retried later.
use crate::state::AppState;
use serde_json::json;
use std::process::Stdio;
use std::time::Duration;

fn env_string(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn enabled() -> bool {
    std::env::var("DOMINION_EXPERIMENT_SCHEDULER").as_deref() == Ok("1")
}

fn has_queued(state: &AppState) -> bool {
    state
        .store
        .all("experiment_request")
        .iter()
        .any(|request| request["status"].as_str() == Some("queued"))
}

pub fn start(state: AppState) {
    if !enabled() {
        return;
    }
    let Some(runner) = std::env::var("DOMINION_EXPERIMENT_RUNNER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        state.store.audit(
            "scheduler.disabled",
            json!({"reason":"DOMINION_EXPERIMENT_RUNNER is not set"}),
        );
        return;
    };
    let interval = env_string("DOMINION_EXPERIMENT_INTERVAL_SECS", "60")
        .parse::<u64>()
        .unwrap_or(60)
        .max(1);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        loop {
            ticker.tick().await;
            if has_queued(&state) {
                run_once(&state, &runner).await;
            }
        }
    });
}

async fn run_once(state: &AppState, runner: &str) {
    let runner_id = format!("scheduler-{}", state.instance_id);
    let run_id = format!("scheduler-run-{}", uuid::Uuid::new_v4());
    let started_at = chrono::Utc::now().to_rfc3339();
    let engine_binary = std::env::current_exe()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| "target/release/dominion-rust".into());
    let mut command = tokio::process::Command::new(runner);
    command
        .env(
            "BENCH_CLAIM_URL",
            format!("http://127.0.0.1:{}", state.config.port),
        )
        .env("BENCH_RUNNER_ID", &runner_id)
        .env("DOMINION_BIN", engine_binary)
        .env("BENCH_MODE", "real")
        .env(
            "BENCH_PAIRS",
            env_string("DOMINION_EXPERIMENT_PAIRS", "10"),
        )
        .env(
            "BENCH_MODEL",
            env_string("DOMINION_EXPERIMENT_MODEL", "qwen3:4b"),
        )
        .env(
            "BENCH_TRIAL_TIMEOUT_SECS",
            env_string("DOMINION_EXPERIMENT_TRIAL_TIMEOUT_SECS", "300"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Ok(base) = std::env::var("DOMINION_EXPERIMENT_PROVIDER_BASE") {
        if !base.trim().is_empty() {
            command.env("BENCH_PROVIDER_BASE", base);
        }
    }
    let output = command.output().await;
    let (status, exit_code, stdout, stderr) = match output {
        Ok(output) => (
            if output.status.success() {
                "success"
            } else {
                "failed"
            },
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ),
        Err(error) => ("failed", -1, String::new(), error.to_string()),
    };
    let released = if status == "failed" {
        crate::api::experiments::release_runner_claims(state, &runner_id)
    } else {
        0
    };
    let record = json!({
        "id": run_id,
        "runnerId": runner_id,
        "runner": runner,
        "status": status,
        "exitCode": exit_code,
        "releasedClaims": released,
        "stdoutTail": stdout.chars().rev().take(2_000).collect::<String>().chars().rev().collect::<String>(),
        "stderrTail": stderr.chars().rev().take(1_000).collect::<String>().chars().rev().collect::<String>(),
        "startedAt": started_at,
        "finishedAt": chrono::Utc::now().to_rfc3339(),
    });
    if let Some(saved) = state.store.put("scheduler_run", &record) {
        state.bus.record("scheduler_run", &saved);
    }
    state.store.audit(
        "scheduler.run",
        json!({"runnerId": state.instance_id, "status": status, "released": released}),
    );
}
