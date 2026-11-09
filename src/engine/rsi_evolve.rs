//! Opt-in experimental evolution. The coordinator and its oracle stay frozen;
//! each accepted source revision is rebuilt and actually generates the next patch.
//! Only private detached worktrees are committed. Production refs never move.
use crate::{
    engine::{
        experience,
        jobs::{self, JobContext},
        memory::MemoryScope,
        rsi_worktree, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::process::Command;

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "ui", "improver"];

#[cfg(test)]
#[path = "rsi_oracle_fixture.rs"]
mod oracle_tests;

pub fn limits(body: &Value) -> Result<Value, String> {
    let bound = |key: &str, default: u64, min: u64, max: u64| -> Result<u64, String> {
        let value = body
            .get(key)
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("{key}는 정수여야 합니다."))
            })
            .unwrap_or(Ok(default))?;
        if (min..=max).contains(&value) {
            Ok(value)
        } else {
            Err(format!("{key} 범위는 {min}~{max}입니다."))
        }
    };
    let token_budget = match body.get("tokenBudget") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value.as_u64().ok_or("tokenBudget은 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (1_000..=2_000_000).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "tokenBudget 범위는 1,000~2,000,000입니다. 0 또는 생략은 상한 없음입니다."
                        .into(),
                );
            }
        }
    };
    let time_budget = match body.get("timeBudgetSeconds") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or("timeBudgetSeconds는 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (60..=7200).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "timeBudgetSeconds 범위는 60~7200입니다. 0 또는 생략은 상한 없음입니다.".into(),
                );
            }
        }
    };
    Ok(json!({"maxAttempts":bound("maxAttempts",1,1,100)?,
        "tokenBudget":token_budget,
        "timeBudgetSeconds":time_budget}))
}

/// Explore every part of the product, then prefer strategies with measured
/// gains per unit cost. Failed and inconclusive attempts remain in the denominator.
pub fn choose_domain(history: &[Value]) -> String {
    let mut best = (f64::NEG_INFINITY, DOMAINS[0]);
    for domain in DOMAINS {
        let rows = history
            .iter()
            .filter(|r| r["domain"] == *domain)
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return domain.to_string();
        }
        let successes = rows.iter().filter(|r| r["accepted"] == true).count() as f64;
        let cost = rows
            .iter()
            .map(|r| r["tokens"].as_u64().unwrap_or(0))
            .sum::<u64>() as f64;
        let utility = successes / (1.0 + cost / 10_000.0) / rows.len() as f64;
        let exploration = (2.0 * ((history.len() + 1) as f64).ln() / rows.len() as f64).sqrt();
        if utility + exploration > best.0 {
            best = (utility + exploration, domain);
        }
    }
    best.1.into()
}

pub fn auto_write_allowed(state: &AppState, session_id: &str, tool: &str, args: &Value) -> bool {
    if !matches!(tool, "workspace_write" | "workspace_replace")
        || !args["path"]
            .as_str()
            .is_some_and(crate::engine::rsi::safe_path)
    {
        return false;
    }
    let Some(session) = state.store.get("session", session_id) else {
        return false;
    };
    let Some(id) = session["rsiCampaignId"].as_str() else {
        return false;
    };
    let Some(parent) = state.store.get("job", id) else {
        return false;
    };
    if parent["kind"] != "rsi_evolve"
        || parent["status"] != "running"
        || parent["cancelRequested"] == true
        || parent["input"]["rsiNativeApproved"] != true
    {
        return false;
    }
    let Some(child) = session["rsiGenerationJobId"]
        .as_str()
        .and_then(|id| state.store.get("job", id))
    else {
        return false;
    };
    child["kind"] == "rsi_generation"
        && child["input"]["rsiCampaignId"] == id
        && child["status"] == "running"
        && child["cancelRequested"] != true
        && child["rsiWorkspace"]
            .as_str()
            .is_some_and(|path| Path::new(path) == state.config.workspace)
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
        return Err("자동 개선의 토큰 예산이 부족해 다음 모델 호출을 중단했습니다.".into());
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

async fn git(context: &JobContext, dir: &Path, args: &[&str]) -> Result<String, String> {
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

fn save(context: &JobContext, record: &Value) -> Result<(), String> {
    let saved = context
        .state
        .store
        .put("rsi_campaign", record)
        .ok_or("자동 개선 상태를 저장할 수 없습니다.")?;
    context.state.bus.record("rsi_campaign", &saved);
    context.checkpoint(json!({"evolution":saved}))
}

fn elapsed(record: &mut Value, started: Instant) {
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
async fn snapshot_source(
    context: &JobContext,
    source: &Path,
    lab: &Path,
) -> Result<(bool, String), String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(source)
        .args(["diff", "--binary", "--no-ext-diff", "HEAD"]);
    let output = command(context, cmd, Duration::from_secs(30)).await?;
    if !output.status.success() || output.stdout.len() >= 79_000 {
        return Err("원본 변경을 안전하게 복사하지 못했습니다.".into());
    }
    let mut fingerprint = Sha256::new();
    fingerprint.update(&output.stdout);
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
        fingerprint.update(name.as_bytes());
        fingerprint.update([0]);
        fingerprint.update(&contents);
        std::fs::write(destination, contents).map_err(|e| e.to_string())?;
    }
    let fingerprint = format!("{:x}", fingerprint.finalize());
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

pub fn choose(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    let record = state
        .store
        .get("rsi_campaign", id)
        .ok_or("진행 중인 개선을 찾을 수 없습니다.")?;
    if record["status"] != "running"
        || record["phase"] != "waiting_for_choice"
        || state
            .store
            .get("job", id)
            .is_none_or(|job| job["status"] != "running" || job["cancelRequested"] == true)
    {
        return Err("현재 선택을 기다리는 비교가 없습니다.".into());
    }
    let comparison = &record["comparison"];
    let choice = body["choice"]
        .as_str()
        .filter(|v| matches!(*v, "baseline" | "candidate"))
        .ok_or("기존 버전 또는 개선 버전을 선택하세요.")?;
    if body["attempt"] != comparison["attempt"] {
        return Err("새 비교 결과가 도착했습니다. 최신 결과에서 선택하세요.".into());
    }
    if choice == "candidate" && comparison["canSelectCandidate"] != true {
        return Err("개선 효과와 회귀 검사를 통과한 버전만 선택할 수 있습니다.".into());
    }
    let decision = json!({"id":format!("{id}:{}",comparison["attempt"]),"campaignId":id,
        "attempt":comparison["attempt"],"evaluationId":comparison["evaluationId"],
        "choice":choice,"at":chrono::Utc::now().to_rfc3339()});
    if let Some(saved) = state.store.put_if_absent("rsi_choice", &decision) {
        return Ok(saved);
    }
    state
        .store
        .get("rsi_choice", decision["id"].as_str().unwrap())
        .filter(|saved| {
            saved["choice"] == choice && saved["evaluationId"] == comparison["evaluationId"]
        })
        .ok_or("이미 다른 버전을 선택했습니다.".into())
}

async fn select(context: &JobContext, record: &mut Value) -> Result<String, String> {
    if record["selectionPolicy"] == "automatic" {
        let choice = if record["comparison"]["canSelectCandidate"] == true {
            "candidate"
        } else {
            "baseline"
        };
        record["comparison"]["selected"] = json!(choice);
        return Ok(choice.into());
    }
    record["phase"] = json!("waiting_for_choice");
    save(context, record)?;
    let waiting = Instant::now();
    let key = format!("{}:{}", context.job_id, record["comparison"]["attempt"]);
    loop {
        if context
            .state
            .store
            .get("job", &context.job_id)
            .is_none_or(|j| j["status"] != "running" || j["cancelRequested"] == true)
        {
            return Err("사용자 요청으로 개선이 중단되었습니다.".into());
        }
        if let Some(decision) = context.state.store.get("rsi_choice", &key) {
            if decision["evaluationId"] != record["comparison"]["evaluationId"] {
                return Err("선택한 비교 기준이 다릅니다.".into());
            }
            record["waitingMs"] = json!(
                record["waitingMs"].as_u64().unwrap_or(0) + waiting.elapsed().as_millis() as u64
            );
            record["comparison"]["selected"] = decision["choice"].clone();
            record["phase"] = json!("selecting");
            save(context, record)?;
            return decision["choice"]
                .as_str()
                .map(str::to_string)
                .ok_or("선택 기록 손상".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    if job["input"]["rsiNativeApproved"] != true {
        return Err("자동 실험 실행 선택이 없습니다.".into());
    }
    if job["rsiEvolutionInFlight"] == true {
        return Err("needs_review: 중단된 실험 계보를 확인하세요.".into());
    }
    let input = &job["input"];
    let source = std::fs::canonicalize(input["rsiSourceDir"].as_str().ok_or("소스 경로 없음")?)
        .map_err(|e| e.to_string())?;
    let base = input["rsiBaseCommit"].as_str().ok_or("기준 커밋 없음")?;
    if git(context, &source, &["rev-parse", "HEAD"]).await? != base {
        return Err("원본 소스가 시작 시점과 다릅니다. 다시 시작하세요.".into());
    }
    let limits = limits(&input["limits"])?;
    let parent = context.state.config.data_dir.join("rsi-labs");
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    let lab = parent.join(format!("lab-{}", uuid::Uuid::new_v4()));
    context.checkpoint(json!({"rsiEvolutionInFlight":true,"rsiPhase":"preparing_lab"}))?;
    git(
        context,
        &source,
        &[
            "worktree",
            "add",
            "--detach",
            lab.to_str().ok_or("실험 경로 오류")?,
            base,
        ],
    )
    .await?;
    let (snapshot, fingerprint) = snapshot_source(context, &source, &lab).await?;
    if let Some(selected) = context.state.store.get("rsi_current", "selected-engine") {
        if selected["sourceDir"].as_str() == source.to_str()
            && selected["sourceBaseCommit"] == base
            && selected["sourceFingerprint"] == fingerprint
        {
            if let Some(commit) = selected["commit"]
                .as_str()
                .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                git(context, &lab, &["reset", "--hard", commit]).await?;
            }
        }
    }
    let head = git(context, &lab, &["rev-parse", "HEAD"]).await?;
    #[cfg(unix)]
    if source.join("web/node_modules").is_dir()
        && lab.join("web").is_dir()
        && !lab.join("web/node_modules").exists()
    {
        std::os::unix::fs::symlink(
            source.join("web/node_modules"),
            lab.join("web/node_modules"),
        )
        .map_err(|e| e.to_string())?;
    }
    let mut record = json!({"id":context.job_id,"status":"running","phase":"building_generator",
        "sourceDir":source,"labDir":lab,"baseCommit":base,"headCommit":head,"sourceSnapshot":snapshot,"sourceFingerprint":fingerprint,
        "nativeApproved":true,"limits":limits,"attempts":0,"acceptedGenerations":0,"tokens":0,
        "history":[],"activeGenerationJobId":null,"createdAt":chrono::Utc::now().to_rfc3339(),
        "selectionPolicy":input["selectionPolicy"],"waitingMs":0,
        "productionChanged":false,"accelerationProven":false});
    save(context, &record)?;
    let started = Instant::now();
    let result = run_loop(context, job, &mut record, started).await;
    if let Some(id) = record["activeGenerationJobId"].as_str().map(str::to_string) {
        if let Some(child) = context.state.store.get("job", &id) {
            if let Some(session) = child["sessionId"].as_str() {
                let usage = context.state.store.session_usage(session);
                record["tokens"] = json!(record["activeBaseTokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(usage["totalTokens"].as_u64().unwrap_or(0)));
                if child["status"] == "running" {
                    context.state.cancel_session(session);
                }
            }
            if child["status"] == "running" {
                context.state.store.patch_if("job",&id,"status",&json!("running"),
                    &json!({"status":"needs_review","cancelRequested":true,"error":"자동 개선 종료 시 중단된 세대 작업"}));
            }
        }
    }
    elapsed(&mut record, started);
    match result {
        Ok(reason) => {
            record["status"] = json!("completed");
            record["stopReason"] = json!(reason);
        }
        Err(error) if error == "time_budget" || error == "token_budget" => {
            record["status"] = json!("completed");
            record["stopReason"] = json!(error);
        }
        Err(error) => {
            let cancelled = context
                .state
                .store
                .get("job", &context.job_id)
                .is_none_or(|j| j["cancelRequested"] == true || j["status"] == "cancelled");
            record["status"] = json!(if cancelled {
                "cancelled"
            } else {
                "needs_review"
            });
            record["error"] = json!(error);
        }
    }
    record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    context
        .state
        .store
        .put("rsi_campaign", &record)
        .ok_or("최종 실험 기록 저장 실패")?;
    context.state.bus.record("rsi_campaign", &record);
    // Cancellation deliberately forbids resurrecting the parent job.
    if record["status"] != "cancelled" {
        context.checkpoint(json!({"rsiEvolutionInFlight":false,"evolution":record}))?;
    }
    Ok(json!({"status":record["status"],"evolution":record}))
}

fn generation_context(state: &AppState, input: &Value, record: &Value) -> String {
    let objective = input["rsiObjective"].as_str().unwrap_or("");
    let scope = MemoryScope {
        session_id: "rsi-evolution".into(),
        agent_id: None,
        partition_key: "rsi".into(),
    };
    let (prior_experience, _) =
        experience::context_block_with_manifest(state, &scope, objective);
    let mut text = String::new();
    text.push_str("\n\n## 이번 세대에서 계승된 실행 능력\n");
    text.push_str("- `code_search(query)`: 저장된 코드 인덱스에서 파일·심볼·줄 번호를 찾습니다. 파일을 읽기 전에 사용하세요.\n");
    text.push_str("- `workspace_read_range(path, startLine, endLine)`: 필요한 줄만 읽습니다.\n");
    text.push_str("- `workspace_replace(path, oldText, newText)`: 한 구간만 수정합니다.\n");
    text.push_str("- 위 능력은 이전 세대에서 계승되었습니다. 적용 가능하면 실제로 호출하고, 적용할 수 없으면 이유를 쓰세요.\n");
    if let Some(block) = prior_experience.filter(|block| !block.trim().is_empty()) {
        text.push_str("\n### 기존 RSI 경험·지식\n");
        text.push_str(&block);
        text.push('\n');
    }
    if let Some(history) = record["history"].as_array() {
        let recent = history.iter().rev().take(2).collect::<Vec<_>>();
        if !recent.is_empty() {
            text.push_str("\n### 이전 세대의 실제 능력 사용 기록\n");
            for generation in recent.into_iter().rev() {
                let used = generation["toolsUsed"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                text.push_str(&format!(
                    "- {}세대: {}\n",
                    generation["attempt"],
                    serde_json::to_string(&used).unwrap_or_else(|_| "[]".into())
                ));
            }
        }
    }
    text
}

fn session_tool_usage(state: &AppState, session_id: &str) -> Vec<Value> {
    let mut counts = BTreeMap::<String, u64>::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(tool).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(tool, calls)| json!({"tool": tool, "calls": calls}))
        .collect()
}

async fn run_loop(
    context: &JobContext,
    job: &Value,
    record: &mut Value,
    started: Instant,
) -> Result<String, String> {
    let input = &job["input"];
    let lab = PathBuf::from(record["labDir"].as_str().ok_or("실험 경로 없음")?);
    let target = context
        .state
        .config
        .data_dir
        .join("rsi-build")
        .join(&context.job_id);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(target).map_err(|e| e.to_string())?;
    let binary = target
        .join("release")
        .join(format!("dominion-rust{}", std::env::consts::EXE_SUFFIX));
    let mut built_commit = String::new();
    loop {
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
        if remaining == 0 {
            return Ok("time_budget".into());
        }
        if record["attempts"].as_u64().unwrap_or(0)
            >= record["limits"]["maxAttempts"].as_u64().unwrap_or(10)
        {
            return Ok("attempt_limit".into());
        }
        if record["limits"]["tokenBudget"]
            .as_u64()
            .is_some_and(|budget| record["tokens"].as_u64().unwrap_or(0) >= budget)
        {
            return Ok("token_budget".into());
        }
        let head = record["headCommit"]
            .as_str()
            .ok_or("세대 커밋 없음")?
            .to_string();
        if built_commit != head {
            record["phase"] = json!("building_generator");
            save(context, record)?;
            let mut build = Command::new(rsi_worktree::cargo_executable());
            build
                .current_dir(&lab)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "--bin",
                    "dominion-rust",
                    "--target-dir",
                ])
                .arg(&target)
                .env("DOMINION_RSI_BUILD_COMMIT", &head);
            let build_timeout = if remaining == u64::MAX {
                3600
            } else {
                remaining.min(600)
            };
            let output = command(context, build, Duration::from_secs(build_timeout)).await?;
            if !output.status.success() {
                return Err(format!(
                    "차세대 개선기 빌드 실패: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                ));
            }
            let mut identity = Command::new(&binary);
            identity.arg("--rsi-worker-identity");
            let output = command(context, identity, Duration::from_secs(10)).await?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != head {
                return Err("실제 개선기 바이너리와 세대 커밋이 다릅니다.".into());
            }
            record["generatorBinarySha256"] = json!(format!(
                "{:x}",
                Sha256::digest(std::fs::read(&binary).map_err(|e| e.to_string())?)
            ));
            record["actualGeneratorCommit"] = json!(head);
            built_commit = head.clone();
        }
        let attempt = record["attempts"].as_u64().unwrap_or(0) + 1;
        let mut child_input = input.clone();
        child_input["rsiSourceDir"] = json!(lab);
        child_input["rsiBaseCommit"] = json!(head);
        child_input["rsiCampaignId"] = json!(context.job_id);
        child_input["rsiGeneratorCommit"] = json!(head);
        child_input["rsiGenerationCapabilities"] =
            json!(["code_search", "workspace_read_range", "workspace_replace"]);
        let inherited = generation_context(&context.state, input, record);
        child_input["prompt"] = json!(format!(
            "{}\n\n{}",
            child_input["prompt"].as_str().unwrap_or(""),
            inherited
        ));
        let child = context
            .state
            .store
            .put(
                "job",
                &json!({"kind":"rsi_generation","status":"running",
            "input":child_input,"queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1}),
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
            remaining.clamp(1, 900)
        };
        let output = command(context, worker, Duration::from_secs(worker_timeout)).await;
        let current = context
            .state
            .store
            .get("job", child_id)
            .ok_or("세대 작업 사라짐")?;
        let session = current["sessionId"].as_str();
        let tools_used = session
            .map(|session_id| session_tool_usage(&context.state, session_id))
            .unwrap_or_default();
        let capabilities_used = tools_used
            .iter()
            .filter(|row| {
                matches!(
                    row["tool"].as_str(),
                    Some("code_search" | "workspace_read_range" | "workspace_replace")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let usage = session
            .map(|s| context.state.store.session_usage(s))
            .unwrap_or(Value::Null);
        let tokens = usage["totalTokens"].as_u64().unwrap_or(0);
        record["tokensEstimated"] = json!(
            record["tokensEstimated"] == true || usage["estimatedCalls"].as_u64().unwrap_or(0) > 0
        );
        record["tokens"] = json!(record["tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(tokens));
        let mut generation = json!({"id":format!("{}-{attempt}",context.job_id),"campaignId":context.job_id,
            "attempt":attempt,"generatorCommit":head,"jobId":child_id,"tokens":tokens,
            "generatorBinarySha256":record["generatorBinarySha256"],"generatorModel":input["model"],"generatorProviderId":input["providerId"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"capabilitiesUsed":capabilities_used,
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});
        let worker_ok = output.as_ref().is_ok_and(|out| out.status.success());
        let candidate_id = current["result"]["rsi"]["candidateId"]
            .as_str()
            .filter(|_| {
                worker_ok
                    && current["result"]["status"] == "completed"
                    && current["result"]["rsi"]["checksPassed"] == true
            });
        let waiting_before = record["waitingMs"].as_u64().unwrap_or(0);
        if let Some(id) = candidate_id {
            record["phase"] = json!("measuring");
            save(context, record)?;
            generation["candidateId"] = json!(id);
            let compared = rsi_worktree::compare_at(
                context,
                id,
                &lab,
                Path::new(
                    record["sourceDir"]
                        .as_str()
                        .ok_or("고정 평가기 경로 없음")?,
                ),
            )
            .await;
            match compared {
                Ok(evaluation) => {
                    generation["evaluation"] = evaluation;
                    let candidate = context
                        .state
                        .store
                        .get("rsi_candidate", id)
                        .ok_or("후보 소실")?;
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true;
                    let winner = if eligible {
                        "candidate"
                    } else if candidate["status"] == "rejected" {
                        "baseline"
                    } else {
                        "inconclusive"
                    };
                    record["comparison"] = json!({"attempt":attempt,"candidateId":id,"winner":winner,
                        "metrics":candidate["evaluation"]["metrics"],"baselineCommit":head,
                        "evaluationId":candidate["evaluation"]["id"],"canSelectCandidate":eligible});
                    let choice = select(context, record).await?;
                    generation["selection"] = json!(choice);
                    if eligible && choice == "candidate" {
                        let commit = adopt(context, &lab, &candidate, &head).await?;
                        generation["accepted"] = json!(true);
                        generation["acceptedCommit"] = json!(commit);
                        record["headCommit"] = json!(commit);
                        record["acceptedGenerations"] =
                            json!(record["acceptedGenerations"].as_u64().unwrap_or(0) + 1);
                        let mut saved = candidate;
                        saved["experimentAdoption"] = json!({"campaignId":context.job_id,
                            "commit":commit,"generatorCommit":head,"productionDeployed":false});
                        context
                            .state
                            .store
                            .put("rsi_candidate", &saved)
                            .ok_or("채택 기록 저장 실패")?;
                        context.state.store.put("rsi_current",&json!({"id":"selected-engine","commit":commit,
                            "sourceDir":record["sourceDir"],"sourceBaseCommit":record["baseCommit"],
                            "sourceFingerprint":record["sourceFingerprint"],"campaignId":context.job_id,
                            "candidateId":id,"selectedAt":chrono::Utc::now().to_rfc3339()})).ok_or("선택한 개선기 저장 실패")?;
                    }
                }
                Err(error) => generation["error"] = json!(error),
            }
        } else {
            let worker_error = output.as_ref().err().map(String::as_str);
            let child = &current["result"]["rsi"];
            generation["error"] = json!(worker_error
                .or_else(|| child["modelError"].as_str())
                .or_else(|| child["captureError"].as_str())
                .or_else(|| current["error"].as_str())
                .unwrap_or("유효한 변경이나 검사 통과 없음"));
        }
        generation["elapsedMs"] = json!((worker_started.elapsed().as_millis() as u64)
            .saturating_sub(
                record["waitingMs"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(waiting_before)
            ));
        context
            .state
            .store
            .put("rsi_generation", &generation)
            .ok_or("세대 실험 증거 저장 실패")?;
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // Do not spend the full attempt/token budget when the model keeps
        // returning without an actual source change. Three consecutive
        // no-edit attempts are enough evidence that this route is stuck.
        let no_edit = candidate_id.is_none()
            && current["result"]["rsi"]["diffBytes"].as_u64().unwrap_or(0) == 0;
        let streak = if no_edit {
            record["noProgressStreak"].as_u64().unwrap_or(0) + 1
        } else {
            0
        };
        record["noProgressStreak"] = json!(streak);
        record["activeGenerationJobId"] = Value::Null;
        elapsed(record, started);
        save(context, record)?;
        if streak >= 3 {
            return Ok("no_progress".into());
        }
    }
}

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["baseCommit"] != base
    {
        return Err("독립 평가를 통과한 현재 세대 후보만 실험 계보에 채택합니다.".into());
    }
    if git(context, lab, &["rev-parse", "HEAD"]).await? != base
        || !git(context, lab, &["status", "--porcelain"])
            .await?
            .is_empty()
    {
        return Err("실험 기준 소스가 변경됐습니다.".into());
    }
    let patch = candidate["patch"].as_str().ok_or("패치 없음")?;
    if format!("{:x}", Sha256::digest(patch.as_bytes()))
        != candidate["patchSha256"].as_str().unwrap_or("")
    {
        return Err("채택 패치 해시 불일치".into());
    }
    let file = lab
        .parent()
        .ok_or("실험 경로 오류")?
        .join(format!("{}.patch", uuid::Uuid::new_v4()));
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let applied = git(
        context,
        lab,
        &["apply", file.to_str().ok_or("패치 경로 오류")?],
    )
    .await;
    let _ = std::fs::remove_file(file);
    applied?;
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
            "RSI: adopt independently measured experimental candidate",
        ],
    )
    .await?;
    git(context, lab, &["rev-parse", "HEAD"]).await
}

/// Executes only one durable generation, without starting a server, scheduler,
/// migrations or recovering another process's jobs in the shared ledger.
pub async fn worker(job_id: &str) -> Result<(), String> {
    let config = crate::config::Config::from_env();
    let store = crate::db::Store::open(&config.db_path()).map_err(|e| e.to_string())?;
    let vault = crate::vault::Vault::open(&config.key_path());
    let state = AppState::new(config, store, vault);
    let mut job = state
        .store
        .get("job", job_id)
        .ok_or("세대 작업을 찾을 수 없습니다.")?;
    if job["kind"] != "rsi_generation"
        || job["status"] != "running"
        || job["cancelRequested"] == true
        || job["input"]["rsiGeneratorCommit"].as_str() != option_env!("DOMINION_BUILD_COMMIT")
    {
        return Err("세대 작업 또는 개선기 소스 revision이 일치하지 않습니다.".into());
    }
    let campaign = job["input"]["rsiCampaignId"]
        .as_str()
        .ok_or("자동 개선 ID 없음")?;
    if state.store.get("job", campaign).is_none_or(|p| {
        p["kind"] != "rsi_evolve" || p["status"] != "running" || p["cancelRequested"] == true
    }) {
        return Err("자동 개선이 중단되었습니다.".into());
    }
    let history = state
        .store
        .all("rsi_generation")
        .into_iter()
        .filter(|g| g["campaignId"] == campaign)
        .collect::<Vec<_>>();
    let domain = choose_domain(&history);
    let feedback = history
        .iter()
        .take(8)
        .map(|row| {
            json!({"domain":row["domain"],"accepted":row["accepted"],
        "tokens":row["tokens"],"elapsedMs":row["elapsedMs"],"verdict":row["evaluation"]["verdict"],
        "measurements":row["evaluation"]["metrics"]["measurements"],"error":row["error"]})
        })
        .collect::<Vec<_>>();
    let feedback = json!(feedback)
        .to_string()
        .chars()
        .take(12_000)
        .collect::<String>();
    job["rsiDomain"] = json!(domain);
    job["input"]["prompt"]=json!(format!("{}\n이번 탐색 분야: {domain}. 전체 제품 기능·화면·외부 연결도 개선 대상입니다. 과거 성공·실패와 측정값을 참고하되 관측되지 않은 성능 향상을 주장하지 마세요.\n이 실험 계보의 과거 결과: {}",
        job["input"]["prompt"].as_str().unwrap_or(""),feedback));
    state.store.put("job", &job).ok_or("세대 지시 저장 실패")?;
    let context = JobContext::new(state.clone(), job_id.into());
    let result = jobs::run_prompt(&context, &job).await;
    match result {
        Ok(value) => {
            context.checkpoint(json!({"status":"completed","result":value}))?;
            Ok(())
        }
        Err(error) => {
            context.checkpoint(json!({"status":"failed","error":error}))?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-evolution-test-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root.join("work"),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, root)
    }
    fn parent(state: &AppState) -> JobContext {
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"running","input":{"rsiNativeApproved":true}})).unwrap();
        JobContext::new(state.clone(), "campaign".into())
    }
    #[test]
    fn capability_usage_is_read_from_real_operation_records() {
        let (state, root) = state();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op1","sessionId":"s1","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op2","sessionId":"s1","tool":"workspace_read_range","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op3","sessionId":"s2","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op4","sessionId":"s1","tool":"workspace_read","status":"failed"}),
            )
            .unwrap();
        let usage = session_tool_usage(&state, "s1");
        assert_eq!(usage.len(), 2, "{usage:?}");
        assert!(usage.iter().any(|row| row["tool"] == "code_search"));
        assert!(usage
            .iter()
            .any(|row| row["tool"] == "workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn inherited_capability_context_includes_prior_usage() {
        let (state, root) = state();
        let input = json!({"rsiObjective":"코드 탐색 능력을 다음 세대에 계승"});
        let record = json!({"history":[{
            "attempt":1,
            "toolsUsed":[{"tool":"code_search","calls":2},{"tool":"workspace_read_range","calls":1}]
        }]});
        let text = generation_context(&state, &input, &record);
        assert!(text.contains("code_search"), "{text}");
        assert!(text.contains("workspace_read_range"), "{text}");
        assert!(text.contains("1세대"), "{text}");
        assert!(text.contains("이전 세대의 실제 능력 사용 기록"), "{text}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resource_limits_reject_unbounded_or_malformed_requests() {
        assert!(limits(&json!({})).unwrap()["tokenBudget"].is_null());
        assert!(limits(&json!({})).unwrap()["timeBudgetSeconds"].is_null());
        assert!(limits(&json!({"tokenBudget":0,"timeBudgetSeconds":0})).unwrap()["tokenBudget"].is_null());
        for body in [
            json!({"maxAttempts":0}),
            json!({"timeBudgetSeconds":999999}),
            json!({"tokenBudget":"unlimited"}),
            json!({"tokenBudget":999}),
        ] {
            assert!(limits(&body).is_err());
        }
    }
    #[tokio::test]
    async fn manual_selection_waits_for_owner_and_is_revision_bound() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record = json!({"id":"campaign","status":"running","selectionPolicy":"manual","waitingMs":0,
            "comparison":{"attempt":2,"evaluationId":"evaluation-2","canSelectCandidate":true}});
        let owner = async {
            loop {
                if state
                    .store
                    .get("rsi_campaign", "campaign")
                    .is_some_and(|r| r["phase"] == "waiting_for_choice")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":1})
            )
            .is_err());
            let chosen = choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":2}),
            )
            .unwrap();
            assert_eq!(
                choose(
                    &state,
                    "campaign",
                    &json!({"choice":"candidate","attempt":2})
                )
                .unwrap(),
                chosen
            );
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"baseline","attempt":2})
            )
            .is_err());
        };
        let (selected, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(select(&context, &mut record), owner)
        })
        .await
        .unwrap();
        assert_eq!(selected.unwrap(), "candidate");
        assert_eq!(record["comparison"]["selected"], "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn automatic_choice_keeps_the_baseline_when_superiority_is_unproven() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record =
            json!({"selectionPolicy":"automatic","comparison":{"canSelectCandidate":false}});
        assert_eq!(select(&context, &mut record).await.unwrap(), "baseline");
        record["comparison"]["canSelectCandidate"] = json!(true);
        assert_eq!(select(&context, &mut record).await.unwrap(), "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn automatic_write_permission_is_scoped_to_the_active_private_worktree() {
        let (state, root) = state();
        let _context = parent(&state);
        state.store.put(
            "job",
            &json!({"id":"generation","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"campaign"},"rsiWorkspace":state.config.workspace}),
        );
        state.store.put("session",&json!({"id":"session","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"generation"}));
        let args = json!({"path":"src/engine/algorithm.rs"});
        assert!(auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        assert!(!auto_write_allowed(&state, "session", "shell_exec", &args));
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &json!({"path":".git/config"})
        ));
        let mut outside = state.clone();
        outside.config.workspace = root.join("outside");
        assert!(!auto_write_allowed(
            &outside,
            "session",
            "workspace_write",
            &args
        ));
        state.store.patch_if(
            "job",
            "campaign",
            "status",
            &json!("running"),
            &json!({"cancelRequested":true}),
        );
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn restart_never_blindly_replays_a_partial_evolution() {
        let (state, root) = state();
        let context = parent(&state);
        context
            .checkpoint(json!({"rsiEvolutionInFlight":true}))
            .unwrap();
        jobs::recover(&state);
        assert_eq!(
            state.store.get("job", "campaign").unwrap()["status"],
            "needs_review"
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn exhausted_usage_prevents_a_new_model_call_and_caps_the_remaining_output() {
        let (state, root) = state();
        let context = parent(&state);
        context.checkpoint(json!({"input":{"limits":{"tokenBudget":2000}},"evolution":{"activeBaseTokens":500}})).unwrap();
        state
            .store
            .put("session", &json!({"id":"s","rsiCampaignId":"campaign"}));
        assert_eq!(
            remaining_output_tokens(&state, "s", 1000).unwrap(),
            Some(500)
        );
        assert!(remaining_output_tokens(&state, "s", 1400).is_err());
        assert_eq!(
            remaining_output_tokens(&state, "ordinary", 99999).unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_paired_runner_finds_a_quality_gain_when_every_guard_already_passes() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(
            source.join("Cargo.toml"),
            "[package]\nname=\"rsi-test-subject\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let before = "pub fn accepts(n:i32)->bool { n >= 0 }\nfn main(){}\n";
        let after = "pub fn accepts(n:i32)->bool { n > 0 }\nfn main(){}\n";
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "baseline",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), after).unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&source).args(["diff", "HEAD"]);
        let patch = command(&context, cmd, Duration::from_secs(5))
            .await
            .unwrap()
            .stdout;
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        let fixtures = root.join("frozen");
        std::fs::create_dir_all(&fixtures).unwrap();
        let oracle = fixtures.join("oracle.rs");
        let mut code = String::new();
        for (index, value) in [-4, -3, -2, -1, 1, 2, 3].into_iter().enumerate() {
            code.push_str(&format!(
                "#[test] fn case_{index}(){{assert_eq!(crate::accepts({value}),{value}>0);}}\n"
            ));
        }
        code.push_str("#[test] fn case_7(){let quality=(-4..=4).filter(|n|crate::accepts(*n)==(*n>0)).count() as f64/9.0;println!(\"DOMINION_RSI_METRIC {{\\\"quality\\\":{quality}}}\");}\n");
        std::fs::write(&oracle, &code).unwrap();
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i==7{"quality"}else{"guard"},"holdout":i==0||i==7,
            "filter":format!("rsi_frozen_oracle::case_{i}"),"metrics":if i==7{json!([{"key":"quality","direction":"higher","minRelativeGain":0.05}])}else{json!([])}})).collect::<Vec<_>>();
        let plan = json!({"oracleVersion":"independent-boundary-v1","cases":cases,"oracleFixture":oracle,
            "oracleSha256":format!("{:x}",Sha256::digest(code.as_bytes()))});
        let patch_file = fixtures.join("candidate.patch");
        std::fs::write(&patch_file, &patch).unwrap();
        let plan_file = fixtures.join("plan.json");
        std::fs::write(&plan_file, plan.to_string()).unwrap();
        std::fs::write(
            fixtures.join("rsi-lab.sh"),
            include_str!("../../scripts/rsi-lab.sh"),
        )
        .unwrap();
        std::fs::write(
            fixtures.join("rsi-measure.mjs"),
            include_str!("../../scripts/rsi-measure.mjs"),
        )
        .unwrap();
        let report_file = fixtures.join("report.json");
        let mut cmd = Command::new("bash");
        cmd.current_dir(&source)
            .arg(fixtures.join("rsi-lab.sh"))
            .arg(&base)
            .arg(&patch_file)
            .arg(&plan_file)
            .arg(&report_file);
        let output = command(&context, cmd, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(&report_file).unwrap()).unwrap();
        assert_eq!(report["baselineBuildOk"], true);
        assert_eq!(report["candidateBuildOk"], true);
        assert!(
            report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["baselinePass"] == true && r["candidatePass"] == true),
            "{report}"
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["candidate"], 1.0);
        assert!(
            report["cases"][7]["metrics"]["quality"]["baseline"]
                .as_f64()
                .unwrap()
                < 1.0
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["samples"], 6);
        assert_eq!(std::fs::read_to_string(&oracle).unwrap(), code);
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_and_adoption_preserve_the_users_checkout_and_commit() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/main.rs"), "base\n").unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "base",
            ],
        )
        .await
        .unwrap();
        let original = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), "owner edit\n").unwrap();
        std::fs::write(source.join("src/new.rs"), "new code\n").unwrap();
        std::fs::write(source.join("notes.txt"), "owner research\n").unwrap();
        std::fs::create_dir_all(source.join("scripts/research")).unwrap();
        std::fs::write(
            source.join("scripts/research/probe.py"),
            "owner research script\n",
        )
        .unwrap();
        std::fs::write(
            source.join("scripts/new-build.mjs"),
            "// new build helper\n",
        )
        .unwrap();
        let lab = root.join("lab");
        git(
            &context,
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                lab.to_str().unwrap(),
                &original,
            ],
        )
        .await
        .unwrap();
        let (snapshot, fingerprint) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(snapshot);
        assert_eq!(fingerprint.len(), 64);
        assert!(!lab.join("notes.txt").exists());
        assert!(lab.join("src/new.rs").exists());
        assert!(!lab.join("scripts/research/probe.py").exists());
        assert!(lab.join("scripts/new-build.mjs").exists());
        let base = git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap();
        std::fs::write(lab.join("src/main.rs"), "measured improvement\n").unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&lab).args(["diff", "HEAD"]);
        let patch = String::from_utf8(
            command(&context, cmd, Duration::from_secs(5))
                .await
                .unwrap()
                .stdout,
        )
        .unwrap();
        std::fs::write(lab.join("src/main.rs"), "owner edit\n").unwrap();
        let mut candidate = json!({"status":"verified","baseCommit":base,"patch":patch,
            "patchSha256":format!("{:x}",Sha256::digest(patch.as_bytes())),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":false}}});
        assert!(adopt(&context, &lab, &candidate, &base).await.is_err());
        candidate["evaluation"]["metrics"]["eligible"] = json!(true);
        let accepted = adopt(&context, &lab, &candidate, &base).await.unwrap();
        assert_ne!(accepted, base);
        assert_eq!(
            git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap(),
            accepted
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            "owner edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "owner research\n"
        );
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn accepted_source_is_rebuilt_and_really_executes_the_next_generation() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(
            source.join("src/main.rs"),
            include_str!("../../tests/fixtures/rsi-generation-worker.rs"),
        )
        .unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source)
            .args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "controlled worker",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context
            .checkpoint(
                json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "limits":{"maxAttempts":2,"tokenBudget":1000,"timeBudgetSeconds":240}}}),
            )
            .unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(240), run(&context, &job))
            .await
            .unwrap()
            .unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 2);
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        assert_eq!(evolution["history"][0]["domain"], "original_executable");
        assert_eq!(
            evolution["history"][1]["domain"], "improved_executable",
            "a label change alone must not count as recursion"
        );
        assert_eq!(
            evolution["history"][1]["generatorCommit"],
            evolution["history"][0]["acceptedCommit"]
        );
        assert_ne!(
            evolution["history"][1]["generatorBinarySha256"],
            evolution["history"][0]["generatorBinarySha256"]
        );
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            evolution["headCommit"]
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert!(std::fs::read_to_string(source.join("src/main.rs"))
            .unwrap()
            .contains("n >= 0"));
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
