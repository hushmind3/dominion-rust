//! Bounded source commands, worktree snapshots, and immutable lineage identities.
use super::{
    Digest, AppState, Value, JobContext, Duration,
    Instant, Path, Command, Sha256, json,
    rsi, tools, limits, select,
};

fn isolated_write_check(
    state: &AppState, session_id: &str, tool: &str, args: &Value,
) -> Result<(), &'static str> {
    if !matches!(tool, "workspace_write" | "workspace_replace" | "workspace_replace_function") {
        return Err("tool_is_not_rsi_source_edit");
    }
    if !args["path"].as_str().is_some_and(crate::engine::rsi::safe_path) {
        return Err("source_path_not_allowed: use a relative src/ or tests/ path");
    }
    let session=state.store.get("session",session_id)
        .ok_or("source_edit_session_missing")?;
    if session["rsiRun"]!=true {
        return Err("source_edit_session_is_not_isolated_rsi");
    }
    let campaign_id=session["rsiCampaignId"].as_str()
        .ok_or("source_edit_campaign_missing")?;
    let parent=state.store.get("job",campaign_id)
        .ok_or("source_edit_campaign_job_missing")?;
    if parent["kind"]!="rsi_evolve"
        || parent["status"]!="running"
        || parent["cancelRequested"]==true
        || parent["input"]["rsiNativeApproved"]!=true {
        return Err("source_edit_campaign_not_active_or_not_approved");
    }
    let child_id=session["rsiGenerationJobId"].as_str()
        .ok_or("source_edit_generation_missing")?;
    let child=state.store.get("job",child_id)
        .ok_or("source_edit_generation_job_missing")?;
    if child["kind"]!="rsi_generation"
        || child["input"]["rsiCampaignId"]!=campaign_id
        || child["status"]!="running"
        || child["cancelRequested"]==true {
        return Err("source_edit_generation_not_active_or_not_owned");
    }
    if !child["rsiWorkspace"].as_str()
        .is_some_and(|path|Path::new(path)==state.config.workspace) {
        return Err("source_edit_worktree_mismatch");
    }
    Ok(())
}

pub fn auto_write_allowed(state: &AppState, session_id: &str, tool: &str, args: &Value) -> bool {
    isolated_write_check(state,session_id,tool,args).is_ok()
}

/// None means the normal interactive approval path applies. Some(reason)
/// means an *already started*, isolated RSI generation tried an ineligible
/// edit and must receive a repairable tool error rather than a fake user deny.
pub fn auto_write_denial_reason(
    state:&AppState,session_id:&str,tool:&str,args:&Value,
)->Option<&'static str> {
    if !matches!(tool,"workspace_write"|"workspace_replace"|"workspace_replace_function") {
        return None;
    }
    let session=state.store.get("session",session_id)?;
    if session["rsiRun"]!=true || !session["rsiCampaignId"].is_string() {
        return None;
    }
    isolated_write_check(state,session_id,tool,args).err()
}

pub fn campaign_source(state: &AppState, input: &Value, source: &Path, base: &str) -> bool {
    let Some(id) = input["rsiCampaignId"].as_str() else {
        return false;
    };
    state.store.get("rsi_campaign", id).is_some_and(|record| {
        record["labDir"]
            .as_str()
            .is_some_and(|p| Path::new(p) == source)
            && record["headCommit"] == base
            && record["status"] == "running"
            && record["nativeApproved"] == true
    })
}

pub fn remaining_output_tokens(
    state: &AppState,
    session_id: &str,
    prompt_estimate: u64,
) -> Result<Option<u64>, String> {
    let Some(id) = state
        .store
        .get("session", session_id)
        .and_then(|s| s["rsiCampaignId"].as_str().map(str::to_string))
    else {
        return Ok(None);
    };
    let parent = state
        .store
        .get("job", &id)
        .ok_or("자동 개선 작업이 없습니다.")?;
    if parent["kind"] != "rsi_evolve"
        || parent["status"] != "running"
        || parent["cancelRequested"] == true
    {
        return Err("자동 개선이 중단되었습니다.".into());
    }
    let Some(budget) = parent["input"]["limits"]["tokenBudget"]
        .as_u64()
        .filter(|budget| *budget > 0)
    else {
        // The owner explicitly asked not to cap self-improvement tokens. We
        // still return None so the provider's own context/output limits apply.
        return Ok(None);
    };
    let prior = parent["evolution"]["activeBaseTokens"]
        .as_u64()
        .unwrap_or(0);
    let spent = state.store.session_usage(session_id)["totalTokens"]
        .as_u64()
        .unwrap_or(0);
    let available =
        budget.saturating_sub(prior.saturating_add(spent).saturating_add(prompt_estimate));
    if available < 256 {
        return Err("rsi_token_budget: 자동 개선의 토큰 예산이 부족해 다음 모델 호출을 중단했습니다.".into());
    }
    Ok(Some(available.min(8192)))
}

pub(crate) async fn command(
    context: &JobContext,
    mut command: Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    if context
        .state
        .store
        .get("job", &context.job_id)
        .is_none_or(|job| job["status"] != "running" || job["cancelRequested"] == true)
    {
        return Err("사용자 요청으로 자동 개선이 중단되었습니다.".into());
    }
    let operation = tools::run_command(&mut command, limit);
    tokio::pin!(operation);
    loop {
        tokio::select! {
            result=&mut operation=>return result,
            _=tokio::time::sleep(Duration::from_millis(100))=>{
                if context.state.store.get("job",&context.job_id)
                    .is_none_or(|j|j["status"]!="running"||j["cancelRequested"]==true){
                    return Err("사용자 요청으로 자동 개선이 중단되었습니다.".into());
                }
                if let Some(record)=context.state.store.get("rsi_campaign",&context.job_id){
                    let active=chrono::Utc::now().signed_duration_since(
                        record["createdAt"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())
                            .map(|v|v.with_timezone(&chrono::Utc)).unwrap_or_else(chrono::Utc::now)
                    ).num_milliseconds().max(0) as u64;
                    if record["phase"]!="waiting_for_choice"
                        && record["limits"]["timeBudgetSeconds"].as_u64().is_some_and(|budget|
                            active.saturating_sub(record["waitingMs"].as_u64().unwrap_or(0)) >= budget.saturating_mul(1000))
                    {
                        return Err("time_budget".into());
                    }
                    let active_tokens=record["activeGenerationJobId"].as_str()
                        .and_then(|id|context.state.store.get("job",id))
                        .and_then(|j|j["sessionId"].as_str().map(str::to_string))
                        .map(|s|context.state.store.session_usage(&s)["totalTokens"].as_u64().unwrap_or(0)).unwrap_or(0);
                    if record["phase"]=="diagnosing_and_editing"
                        && record["limits"]["tokenBudget"].as_u64().is_some_and(|budget|
                            record["activeBaseTokens"].as_u64().unwrap_or(record["tokens"].as_u64().unwrap_or(0)).saturating_add(active_tokens) >= budget)
                    {
                        return Err("token_budget".into());
                    }
                }
            }
        }
    }
}

pub(super) async fn git(context: &JobContext, dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    let output = command(context, cmd, Duration::from_secs(30)).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1200)
            .collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn save(context: &JobContext, record: &Value) -> Result<(), String> {
    let saved = context
        .state
        .store
        .put("rsi_campaign", record)
        .ok_or("자동 개선 상태를 저장할 수 없습니다.")?;
    context.state.bus.record("rsi_campaign", &saved);
    context.checkpoint(json!({"evolution":saved}))
}

pub(super) fn elapsed(record: &mut Value, started: Instant) {
    let active_ms = (started.elapsed().as_millis() as u64)
        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0));
    record["elapsedMs"] = json!(active_ms);
    let accepted = record["acceptedGenerations"].as_u64().unwrap_or(0) as f64;
    let tokens = record["tokens"].as_u64().unwrap_or(0) as f64;
    record["efficiency"] = json!({
        "verifiedGainsPerHour":accepted/(active_ms as f64/1000.0).max(1.0)*3600.0,
        "tokensPerAcceptedGeneration":if accepted>0.0{json!(tokens/accepted)}else{Value::Null},
        "accelerationProven":false
    });
}

/// Read-only snapshot of the user's checkout. Unrelated untracked research,
/// runtime data and secrets are excluded; users need not learn Git first.
pub(super) fn lineage_fingerprint(commit: &str, tree: &str, diff: &[u8], untracked: &[(String, Vec<u8>)]) -> String {
    let mut fingerprint = Sha256::new();
    fingerprint.update(commit.trim().as_bytes());
    fingerprint.update([0]);
    fingerprint.update(tree.trim().as_bytes());
    fingerprint.update([0]);
    fingerprint.update(diff);
    for (name, contents) in untracked {
        fingerprint.update(name.as_bytes());
        fingerprint.update([0]);
        fingerprint.update(contents);
    }
    format!("{:x}", fingerprint.finalize())
}

/// A real lineage fingerprint hashes the source commit and tree. Older
/// campaigns hashed only `git diff HEAD`, which is empty for a clean checkout
/// and produced the SHA-256 of the empty string.
pub fn lineage_fingerprint_valid(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
}

pub(super) async fn snapshot_source(
    context: &JobContext,
    source: &Path,
    lab: &Path,
) -> Result<(bool, String), String> {
    let commit = git(context, source, &["rev-parse", "HEAD"]).await?;
    let tree = git(context, source, &["rev-parse", "HEAD^{tree}"]).await?;
    let mut cmd = Command::new("git");
    cmd.current_dir(source)
        .args(["diff", "--binary", "--no-ext-diff", "HEAD"]);
    let output = command(context, cmd, Duration::from_secs(30)).await?;
    if !output.status.success() || output.stdout.len() >= 79_000 {
        return Err("원본 변경을 안전하게 복사하지 못했습니다.".into());
    }
    if !output.stdout.is_empty() {
        let file = lab
            .parent()
            .ok_or("실험 경로 없음")?
            .join(format!("snapshot-{}.patch", uuid::Uuid::new_v4()));
        std::fs::write(&file, &output.stdout).map_err(|e| e.to_string())?;
        let applied = git(
            context,
            lab,
            &["apply", file.to_str().ok_or("원본 복사 경로 오류")?],
        )
        .await;
        let _ = std::fs::remove_file(file);
        applied?;
    }
    let files = git(
        context,
        source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    let mut bytes = 0u64;
    let mut untracked: Vec<(String, Vec<u8>)> = Vec::new();
    for name in files.split('\0').filter(|name| !name.is_empty()) {
        let code = crate::engine::rsi::safe_path(name)
            || name
                .strip_prefix("scripts/")
                .is_some_and(|part| !part.contains('/') && !part.starts_with('.'));
        if !code || name.chars().any(char::is_control) {
            continue;
        }
        let path = source.join(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        bytes = bytes.saturating_add(metadata.len());
        if bytes > 8 * 1024 * 1024 {
            return Err("새 소스 파일의 복사 상한을 초과했습니다.".into());
        }
        let destination = lab.join(name);
        std::fs::create_dir_all(destination.parent().ok_or("원본 파일 경로 오류")?)
            .map_err(|e| e.to_string())?;
        let contents = std::fs::read(&path).map_err(|e| e.to_string())?;
        untracked.push((name.to_string(), contents.clone()));
        std::fs::write(destination, contents).map_err(|e| e.to_string())?;
    }
    let fingerprint = lineage_fingerprint(&commit, &tree, &output.stdout, &untracked);
    if git(context, lab, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return Ok((false, fingerprint));
    }
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: snapshot existing source without changing the original checkout",
        ],
    )
    .await?;
    Ok((true, fingerprint))
}
