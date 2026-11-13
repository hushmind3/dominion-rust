//! Explicit job/worker lifecycle boundary for a single compiled descendant.
//! Approval, limits, cancellation and job persistence remain coordinated by
//! the parent campaign; this module only launches an attributed worker.
use super::*;

pub(super) async fn launch_generation_worker(
    context:&JobContext,record:&mut Value,child_input:&Value,
    lab:&PathBuf,binary:&PathBuf,attempt:u64,started:Instant,
)->Result<(Value,Result<std::process::Output,String>,Instant),String>{
        let child = context
            .state
            .store
            .put(
                "job",
                &json!({"kind":"rsi_generation","status":"running",
            "input":child_input,"queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1,
            "rsiMethodAblationInputSha256":child_input["rsiMethodAblationInputSha256"]}),
            )
            .ok_or("세대 작업 저장 실패")?;
        let child_id = child["id"].as_str().ok_or("세대 작업 ID 없음")?;
        record["attempts"] = json!(attempt);
        record["activeGenerationJobId"] = json!(child_id);
        record["activeBaseTokens"] = record["tokens"].clone();
        record["phase"] = json!("diagnosing_and_editing");
        save(context, record)?;
        let mut worker = Command::new(&binary);
        worker
            .arg("--rsi-worker-job")
            .arg(child_id)
            .current_dir(&lab)
            .env(
                "DOMINION_DATA",
                std::fs::canonicalize(&context.state.config.data_dir).map_err(|e| e.to_string())?,
            )
            .env("DOMINION_WORKSPACE", &lab)
            .env_remove("DOMINION_DESKTOP_PARENT");
        let worker_started = Instant::now();
        let remaining = record["limits"]["timeBudgetSeconds"]
            .as_u64()
            .map(|budget| {
                budget.saturating_sub(
                    started
                        .elapsed()
                        .as_secs()
                        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0) / 1000),
                )
            })
            .unwrap_or(u64::MAX);
        let worker_timeout = if remaining == u64::MAX {
            7 * 24 * 3600
        } else {
            remaining.max(1)
        };
        let output = command(context, worker, Duration::from_secs(worker_timeout)).await;
        Ok((child,output,worker_started))
}
