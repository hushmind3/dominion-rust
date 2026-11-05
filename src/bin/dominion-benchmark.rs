//! S7 isolated paired experiment runner.
//!
//! Starts a disposable DOMINION engine with its own data/workspace/port,
//! freezes the fixture state, seeds the same procedure for two agents
//! (model-guided vs harness-assisted), and runs balanced AB/BA trials.
//!
//! The runner deliberately does not report "improvement" from a smoke run.
//! Every trial is persisted before the next one starts, and `summary.json` can
//! be recomputed from `trials.jsonl` without starting an engine:
//!
//! ```text
//! BENCH_RECALCULATE=1 BENCH_ROOT=/path/to/experiment target/release/dominion-benchmark
//! ```
#[allow(dead_code)]
#[path = "../engine/plan_quality.rs"]
mod plan_quality;

use futures_util::StreamExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SCHEMA_VERSION: u32 = 1;
const ORACLE_VERSION: &str = "s7-harness-audit-v1";
const TASK: &str = "bench/harness_report.txt 파일을 만들고 내용을 REPORT-OK로 저장해.";
const AUDIT_PATH: &str = "bench/harness_audit.txt";
const REPORT_PATH: &str = "bench/harness_report.txt";
const AUDIT_EXPECTED: &str = "audit-harness";
const REPORT_EXPECTED: &str = "REPORT-OK";
// The read-only input deliberately uses a non-source extension so automatic
// acceptance inference does not demand that an input file change.
const UPDATE_TASK: &str = "bench/config_input.cfg를 읽고 bench/config.txt에 mode=final을 저장해.";

#[derive(Clone, Debug)]
struct Fixture {
    id: &'static str,
    task: &'static str,
    audit_path: &'static str,
    report_path: &'static str,
    audit_expected: &'static str,
    report_expected: &'static str,
    seeds: &'static [(&'static str, &'static str)],
    /// When non-empty, every listed file must match for oracle success.
    checks: &'static [(&'static str, &'static str)],
}

fn fixture(id: &str) -> Result<Fixture, String> {
    match id {
        "audit_report" => Ok(Fixture {
            id: "audit_report",
            task: TASK,
            audit_path: AUDIT_PATH,
            report_path: REPORT_PATH,
            audit_expected: AUDIT_EXPECTED,
            report_expected: REPORT_EXPECTED,
            seeds: &[],
            checks: &[],
        }),
        "planning_multi" => Ok(Fixture {
            id: "planning_multi",
            task: "bench/in1.txt부터 bench/in5.txt까지 각 파일의 내용을 읽고, 같은 내용을 bench/out1.txt부터 bench/out5.txt에 각각 복사해 저장해. 다섯 파일 모두 정확해야 한다.",
            audit_path: "bench/out1.txt",
            report_path: "bench/out5.txt",
            audit_expected: "TOKEN-ONE",
            report_expected: "TOKEN-FIVE",
            seeds: &[
                ("bench/in1.txt", "TOKEN-ONE"),
                ("bench/in2.txt", "TOKEN-TWO"),
                ("bench/in3.txt", "TOKEN-THREE"),
                ("bench/in4.txt", "TOKEN-FOUR"),
                ("bench/in5.txt", "TOKEN-FIVE"),
            ],
            checks: &[
                ("bench/out1.txt", "TOKEN-ONE"),
                ("bench/out2.txt", "TOKEN-TWO"),
                ("bench/out3.txt", "TOKEN-THREE"),
                ("bench/out4.txt", "TOKEN-FOUR"),
                ("bench/out5.txt", "TOKEN-FIVE"),
            ],
        }),
        "planning_pair" => Ok(Fixture {
            id: "planning_pair",
            task: "bench/harness_audit.txt 파일을 만들고 audit-harness를 저장한 뒤, bench/harness_report.txt 파일을 만들고 REPORT-OK를 저장해.",
            audit_path: AUDIT_PATH,
            report_path: REPORT_PATH,
            audit_expected: AUDIT_EXPECTED,
            report_expected: REPORT_EXPECTED,
            seeds: &[],
            checks: &[],
        }),
        "planning_evaluation" => Ok(Fixture {
            id: "planning_evaluation",
            task: TASK,
            audit_path: AUDIT_PATH,
            report_path: REPORT_PATH,
            audit_expected: AUDIT_EXPECTED,
            report_expected: REPORT_EXPECTED,
            seeds: &[],
            checks: &[],
        }),
        "update_config" => Ok(Fixture {
            id: "update_config",
            task: UPDATE_TASK,
            audit_path: "bench/config.txt",
            report_path: "bench/config_input.cfg",
            audit_expected: "mode=final",
            report_expected: "mode=draft",
            seeds: &[("bench/config_input.cfg", "mode=draft")],
            checks: &[],
        }),
        other => Err(format!(
            "BENCH_FIXTURE must be audit_report or update_config (got {other})"
        )),
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Clone, Debug)]
struct BenchConfig {
    mode: String,
    model: String,
    pairs: usize,
    seed: u64,
    root: PathBuf,
    fresh: bool,
    recalculate: bool,
    fixture: Fixture,
    holdout: bool,
    /// When set, claim one queued experiment request from this production engine.
    claim_url: Option<String>,
    provider_base: String,
    trial_timeout_secs: u64,
}

fn env_string(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn config() -> Result<BenchConfig, String> {
    let mode = env_string("BENCH_MODE", "smoke");
    if !matches!(mode.as_str(), "smoke" | "real") {
        return Err("BENCH_MODE must be smoke or real".into());
    }
    let pairs = env_string("BENCH_PAIRS", if mode == "smoke" { "1" } else { "3" })
        .parse::<usize>()
        .map_err(|_| "BENCH_PAIRS must be a positive integer".to_string())?;
    if pairs == 0 {
        return Err("BENCH_PAIRS must be at least 1".into());
    }
    let seed = env_string("BENCH_SEED", "94371")
        .parse::<u64>()
        .map_err(|_| "BENCH_SEED must be an unsigned integer".to_string())?;
    let recalculate = std::env::var("BENCH_RECALCULATE").as_deref() == Ok("1");
    let explicit_root = std::env::var("BENCH_ROOT")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let root = explicit_root
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("dominion-s7-{}", uuid::Uuid::new_v4())));
    // An explicitly named, non-existent root is a new experiment; an existing
    // root is resumed. Without BENCH_ROOT every run is fresh.
    let fresh = !recalculate
        && explicit_root
            .as_ref()
            .map(|path| !path.exists())
            .unwrap_or(true);
    let fixture = fixture(&env_string("BENCH_FIXTURE", "audit_report"))?;
    Ok(BenchConfig {
        mode,
        model: env_string("BENCH_MODEL", "qwen3:4b"),
        pairs,
        seed,
        root,
        fresh,
        recalculate,
        fixture,
        holdout: std::env::var("BENCH_HOLDOUT").as_deref() == Ok("1"),
        claim_url: std::env::var("BENCH_CLAIM_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty()),
        provider_base: env_string("BENCH_PROVIDER_BASE", "http://127.0.0.1:11434/v1"),
        trial_timeout_secs: env_string("BENCH_TRIAL_TIMEOUT_SECS", "300")
            .parse::<u64>()
            .unwrap_or(300)
            .clamp(30, 900),
    })
}

fn engine_binary() -> PathBuf {
    std::env::var("DOMINION_BIN")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("dominion-rust")))
        })
        .unwrap_or_else(|| PathBuf::from("target/release/dominion-rust"))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(4417)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> String {
    std::fs::read(path).map(|bytes| sha256(&bytes)).unwrap_or_default()
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn repo_digest() -> Value {
    let head = command_output("git", &["rev-parse", "HEAD"]);
    let dirty = Command::new("git")
        .args(["diff", "--binary"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .map(|output| sha256(&output.stdout))
        .unwrap_or_default();
    let status = command_output("git", &["status", "--porcelain"]);
    json!({
        "commit": head,
        "dirtyDiffSha256": dirty,
        "statusSha256": sha256(status.as_bytes()),
    })
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn under(path: &Path, root: &Path) -> bool {
    let path = canonical(path);
    let root = canonical(root);
    path.starts_with(root)
}

/// The runner only accepts a disposable root below the OS temp directory. This
/// is not an OS sandbox; it only prevents accidentally pointing the experiment
/// at the live repository or the user's home.
fn validate_root(root: &Path, port: u16) -> Result<(), String> {
    if port == 4317 {
        return Err("refusing the live 4317 port".into());
    }
    let root = canonical(root);
    let temp = canonical(&std::env::temp_dir());
    if !root.starts_with(&temp) {
        return Err(format!(
            "benchmark root must be under {} (got {})",
            temp.display(),
            root.display()
        ));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if under(&root, &home.join("dominion-rust")) || root == canonical(&home) {
            return Err("refusing the live repository or home as benchmark root".into());
        }
    }
    Ok(())
}

fn ensure_root(cfg: &BenchConfig, experiment_id: &str) -> Result<PathBuf, String> {
    let root = canonical(&cfg.root);
    if !cfg.fresh && !root.exists() {
        return Err(format!(
            "BENCH_ROOT does not exist for resume: {}",
            root.display()
        ));
    }
    if cfg.fresh {
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    }
    let sentinel = root.join(".dominion-benchmark-sentinel");
    if cfg.fresh {
        std::fs::write(&sentinel, format!("benchmark\n{experiment_id}\n"))
            .map_err(|error| error.to_string())?;
    } else if !sentinel.is_file() {
        return Err(format!(
            "missing benchmark sentinel: {}",
            sentinel.display()
        ));
    }
    for directory in ["data", "workspace", "workspace/bench", "artifacts", "snapshots"] {
        std::fs::create_dir_all(root.join(directory)).map_err(|error| error.to_string())?;
    }
    Ok(root)
}

fn reset_fixture(root: &Path, fixture: &Fixture) -> Result<(), String> {
    let bench = root.join("workspace/bench");
    if bench.exists() {
        std::fs::remove_dir_all(&bench).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&bench).map_err(|error| error.to_string())?;
    for (path, content) in fixture.seeds {
        let full = root.join("workspace").join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(full, content).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn fixture_digest(root: &Path, fixture: &Fixture) -> String {
    let mut parts = Vec::new();
    for path in [fixture.audit_path, fixture.report_path] {
        let full = root.join("workspace").join(path);
        let text = std::fs::read_to_string(&full).unwrap_or_default();
        parts.push(format!("{path}\n{text}"));
    }
    sha256(parts.join("\n---\n").as_bytes())
}

/// Balanced AB/BA order. The seed is recorded before execution so the
/// assignment can be replayed without rerunning the model.
fn assignment_order(seed: u64, pair: usize) -> [&'static str; 2] {
    // Strict AB/BA balancing. The seed only decides which arm starts first.
    let offset = (seed & 1) as usize;
    if (pair + offset) % 2 == 0 {
        ["baseline", "treatment"]
    } else {
        ["treatment", "baseline"]
    }
}

fn trial_id(experiment_id: &str, pair: usize, arm: &str, replicate: usize) -> String {
    format!("{experiment_id}:p{pair}:{arm}:r{replicate}")
}

fn arm_agent_name(arm: &str) -> &'static str {
    match arm {
        "baseline" => "S7 baseline",
        _ => "S7 treatment",
    }
}

fn arm_assistance_mode(arm: &str) -> &'static str {
    match arm {
        "baseline" => "model_guided",
        _ => "harness_assisted",
    }
}

fn classify_error(text: &str, elapsed_ms: u128, limit_ms: u128) -> &'static str {
    if elapsed_ms >= limit_ms {
        return "timeout";
    }
    if text.contains("status\":\"error") || text.contains("\"type\":\"error") {
        return "stream_error";
    }
    if text.starts_with("HTTP") || text.contains(": error sending request") {
        return "transport_error";
    }
    if text.contains("revision") || text.contains("계약") {
        return "contract_error";
    }
    "engine_error"
}

fn strict_success(trial: &Value) -> bool {
    let requires_compliance = trial["requiresCompliance"].as_bool().unwrap_or(true);
    trial["status"] == "completed"
        && trial["auditOk"] == true
        && trial["reportOk"] == true
        && trial["taskOutcome"] == "succeeded"
        && (!requires_compliance || trial["compliance"] == true)
        && trial["errorClass"] == ""
}

fn percentile(values: &mut [u128], fraction: f64) -> u128 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values[index]
}

fn aggregate(trials: &[Value], arm: &str, pairs: usize) -> Value {
    let rows = trials
        .iter()
        .filter(|trial| trial["arm"] == arm)
        .collect::<Vec<_>>();
    let success = rows.iter().filter(|trial| strict_success(trial)).count();
    let invalid = rows.iter().filter(|trial| trial["status"] == "invalid").count();
    let unknown = rows
        .iter()
        .filter(|trial| matches!(trial["taskOutcome"].as_str(), Some("unknown")))
        .count();
    let failure = rows
        .len()
        .saturating_sub(success)
        .saturating_sub(invalid)
        .saturating_sub(unknown);
    let mut latencies = rows
        .iter()
        .filter_map(|trial| trial["elapsedMs"].as_u64().map(|value| value as u128))
        .collect::<Vec<_>>();
    let tokens = rows
        .iter()
        .filter_map(|trial| trial["tokens"].as_u64())
        .sum::<u64>();
    let compliant = rows
        .iter()
        .filter(|trial| trial["compliance"] == true)
        .count();
    let applicable = rows
        .iter()
        .filter(|trial| trial["applicable"] == true)
        .count();
    let plan_quality = if rows.is_empty() {
        0.0
    } else {
        rows.iter()
            .map(|trial| trial["planQuality"].as_f64().unwrap_or(0.0))
            .sum::<f64>()
            / rows.len() as f64
    };
    let independent_plan_quality = if rows.is_empty() {
        0.0
    } else {
        rows.iter()
            .map(|trial| {
                trial["planQualityIndependent"]
                    .as_f64()
                    .unwrap_or(0.0)
            })
            .sum::<f64>()
            / rows.len() as f64
    };
    json!({
        "assigned": pairs,
        "runs": rows.len(),
        "success": success,
        "failure": failure,
        "unknown": unknown,
        "invalid": invalid,
        "strictSuccessRate": if rows.is_empty() { Value::Null } else { json!(success as f64 / rows.len() as f64) },
        "ittSuccessRate": if pairs == 0 { Value::Null } else { json!(success as f64 / pairs as f64) },
        "compliance": compliant,
        "triggerCoverage": if applicable == 0 { Value::Null } else { json!(compliant as f64 / applicable as f64) },
        "tokens": tokens,
        "planQuality": round4(plan_quality),
        "planQualityIndependent": round4(independent_plan_quality),
        "p50Ms": percentile(&mut latencies.clone(), 0.50),
        "p95Ms": percentile(&mut latencies, 0.95),
    })
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn pair_effect(trials: &[Value], planned_pairs: usize, seed: u64) -> Value {
    // ITT: every planned pair counts once. A missing arm or a timeout is a
    // failure for that arm, not a silently dropped observation.
    let pairs = if planned_pairs > 0 {
        planned_pairs
    } else {
        trials
            .iter()
            .filter_map(|trial| trial["pair"].as_u64())
            .map(|pair| pair as usize)
            .max()
            .map(|max| max + 1)
            .unwrap_or(0)
    };
    let mut deltas = Vec::new();
    let mut semantic_deltas = Vec::new();
    let mut independent_semantic_deltas = Vec::new();
    let mut missing_arms = 0usize;
    let mut timeout_trials = 0usize;
    let success = |trial: Option<&Value>| {
        trial.is_some_and(|trial| {
            trial["status"].as_str() == Some("completed") && strict_success(trial)
        })
    };
    for pair in 0..pairs {
        let baseline = trials
            .iter()
            .find(|trial| trial["pair"].as_u64() == Some(pair as u64) && trial["arm"] == "baseline");
        let treatment = trials
            .iter()
            .find(|trial| trial["pair"].as_u64() == Some(pair as u64) && trial["arm"] == "treatment");
        if baseline.is_none() && treatment.is_none() {
            continue;
        }
        if baseline.is_none() {
            missing_arms += 1;
        }
        if treatment.is_none() {
            missing_arms += 1;
        }
        for trial in [baseline, treatment].into_iter().flatten() {
            if trial["errorClass"].as_str() == Some("timeout") {
                timeout_trials += 1;
            }
        }
        let b = if success(baseline) { 1.0 } else { 0.0 };
        let t = if success(treatment) { 1.0 } else { 0.0 };
        deltas.push(t - b);
        let bq = baseline
            .and_then(|trial| trial["planQuality"].as_f64())
            .unwrap_or(0.0);
        let tq = treatment
            .and_then(|trial| trial["planQuality"].as_f64())
            .unwrap_or(0.0);
        semantic_deltas.push(tq - bq);
        let bi = baseline
            .and_then(|trial| trial["planQualityIndependent"].as_f64())
            .unwrap_or(0.0);
        let ti = treatment
            .and_then(|trial| trial["planQualityIndependent"].as_f64())
            .unwrap_or(0.0);
        independent_semantic_deltas.push(ti - bi);
    }
    let n = deltas.len();
    let mean = if n == 0 {
        0.0
    } else {
        deltas.iter().sum::<f64>() / n as f64
    };
    let alpha = 0.05;
    let halfwidth = if n == 0 {
        1.0
    } else {
        (2.0 * (2.0f64 / alpha).ln() / n as f64).sqrt()
    };
    let lower = (mean - halfwidth).max(-1.0);
    let upper = (mean + halfwidth).min(1.0);
    let semantic_mean = if semantic_deltas.is_empty() {
        0.0
    } else {
        semantic_deltas.iter().sum::<f64>() / semantic_deltas.len() as f64
    };
    let independent_semantic_mean = if independent_semantic_deltas.is_empty() {
        0.0
    } else {
        independent_semantic_deltas.iter().sum::<f64>()
            / independent_semantic_deltas.len() as f64
    };
    let semantic_halfwidth = if semantic_deltas.is_empty() {
        1.0
    } else {
        (2.0 * (2.0f64 / alpha).ln() / semantic_deltas.len() as f64).sqrt()
    };
    let verdict = if n == 0 {
        "not_evaluated"
    } else if n < 10 || lower <= 0.0 {
        "inconclusive"
    } else {
        "preliminary_support"
    };
    json!({
        "plannedUnits": pairs,
        "independentUnits": n,
        "missingArms": missing_arms,
        "timeoutTrials": timeout_trials,
        "seed": seed,
        "delta": mean,
        "halfwidth": halfwidth,
        "ci": [lower, upper],
        "verdict": verdict,
        "semanticMetric": "plan_quality_v1",
        "semanticDelta": round4(semantic_mean),
        "independentSemanticDelta": round4(independent_semantic_mean),
        "semanticCi": [
            round4((semantic_mean - semantic_halfwidth).max(-1.0)),
            round4((semantic_mean + semantic_halfwidth).min(1.0))
        ],
        "semanticUnits": semantic_deltas.len(),
        "note": "ITT per planned pair; timeouts and missing arms count as failures.",
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap_or_default())
        .map_err(|error| error.to_string())
}

fn append_jsonl(path: &Path, value: &Value) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{}", serde_json::to_string(value).unwrap_or_default())
        .map_err(|error| error.to_string())
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

#[allow(dead_code)] // retained for immutable result replay/deduplication
fn trial_signature(trial: &Value) -> String {
    let mut value = trial.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("finishedAt");
        object.remove("elapsedMs");
        object.remove("streamChars");
        object.remove("artifacts");
        object.remove("operations");
    }
    sha256(value.to_string().as_bytes())
}

fn existing_trial(trials: &[Value], id: &str) -> Option<Value> {
    trials
        .iter()
        .find(|trial| trial["trialId"].as_str() == Some(id))
        .cloned()
}

async fn get(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {text}"));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn post(client: &reqwest::Client, url: &str, body: Value) -> Result<Value, String> {
    let response = client
        .post(url)
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {text}"));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn chat(
    client: &reqwest::Client,
    base: &str,
    session: &str,
    agent: &str,
    provider: &str,
    input: &str,
    timeout_secs: u64,
) -> (String, u128) {
    let started = Instant::now();
    let response = client
        .post(format!("{base}/api/chat"))
        .json(&json!({
            "sessionId": session,
            "agentId": agent,
            "providerId": provider,
            "input": input,
            "toolsEnabled": true,
            "autoLearning": false,
        }))
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => return (format!("transport: {error}"), started.elapsed().as_millis()),
    };
    let status = response.status();
    let mut stream = response.bytes_stream();
    let mut text = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => text.push_str(&String::from_utf8_lossy(&chunk)),
            Err(error) => {
                text.push_str(&format!("\nstream: {error}"));
                break;
            }
        }
    }
    if !status.is_success() {
        text = format!("HTTP {status}: {text}");
    }
    (text, started.elapsed().as_millis())
}

fn file_artifact(root: &Path, relative: &str) -> Value {
    let path = root.join("workspace").join(relative);
    match std::fs::read(&path) {
        Ok(bytes) => json!({
            "relativePath": relative,
            "exists": true,
            "sha256": sha256(&bytes),
            "bytes": bytes.len(),
        }),
        Err(_) => json!({
            "relativePath": relative,
            "exists": false,
            "sha256": "",
            "bytes": 0,
        }),
    }
}

fn write_report(root: &Path, manifest: &Value, summary: &Value) -> Result<(), String> {
    let baseline = &summary["arms"]["baseline"];
    let treatment = &summary["arms"]["treatment"];
    let effect = &summary["effect"];
    let table = format!(
        "| arm | assigned | success | failure | unknown | invalid | compliance / coverage | tokens | p95 |\n\
         |---|---:|---:|---:|---:|---:|---|---:|---:|\n\
         | baseline | {} | {} | {} | {} | {} | {} / {} | {} | {}ms |\n\
         | treatment | {} | {} | {} | {} | {} | {} / {} | {} | {}ms |",
        baseline["assigned"], baseline["success"], baseline["failure"], baseline["unknown"], baseline["invalid"],
        baseline["compliance"], baseline["triggerCoverage"], baseline["tokens"], baseline["p95Ms"],
        treatment["assigned"], treatment["success"], treatment["failure"], treatment["unknown"], treatment["invalid"],
        treatment["compliance"], treatment["triggerCoverage"], treatment["tokens"], treatment["p95Ms"],
    );
    let text = format!(
        "# DOMINION S7 paired experiment\n\n\
         - experiment: `{}`\n\
         - mode: `{}`\n\
         - model: `{}`\n\
         - engine commit: `{}`\n\
         - dirty diff: `{}`\n\
         - oracle: `{}`\n\
         - pairs: {}\n\
         - verdict: `{}`\n\n\
         {}\n\n\
         ## Effect\n\n\
         - independent units: {}\n\
         - delta: {}\n\
         - conservative CI: {}\n\n\
         > {}\n\n\
         ## Limits\n\n\
         - This is a small smoke/paired run unless the manifest says `mode=real`.\n\
         - Deterministic file oracle + harness compliance are used; semantic quality is not judged.\n\
         - A zero or negative result must be reported as-is.\n\
         - Raw trials: `trials.jsonl`; per-trial evidence: `artifacts/<trial-id>/`.\n",
        manifest["experimentId"].as_str().unwrap_or(""),
        manifest["mode"].as_str().unwrap_or(""),
        manifest["model"]["model"].as_str().unwrap_or(""),
        manifest["engine"]["commit"].as_str().unwrap_or(""),
        manifest["engine"]["dirtyDiffSha256"].as_str().unwrap_or(""),
        manifest["oracleVersion"].as_str().unwrap_or(""),
        manifest["pairs"],
        effect["verdict"].as_str().unwrap_or(""),
        table,
        effect["independentUnits"],
        effect["delta"],
        effect["ci"],
        effect["note"].as_str().unwrap_or(""),
    );
    std::fs::write(root.join("REPORT.md"), text).map_err(|error| error.to_string())
}

fn write_outputs(root: &Path, manifest: &Value, trials: &[Value]) -> Result<Value, String> {
    if trials.is_empty() {
        return Err("no trials to aggregate".into());
    }
    let summary = json!({
        "schemaVersion": SCHEMA_VERSION,
        "experimentId": manifest["experimentId"],
        "mode": manifest["mode"],
        "fixture": manifest["fixture"],
        "evaluationRole": manifest["evaluationRole"],
        "model": manifest["model"],
        "oracleVersion": ORACLE_VERSION,
        "arms": {
            "baseline": aggregate(trials, "baseline", manifest["pairs"].as_u64().unwrap_or(0) as usize),
            "treatment": aggregate(trials, "treatment", manifest["pairs"].as_u64().unwrap_or(0) as usize),
        },
        "effect": pair_effect(
            trials,
            manifest["pairs"].as_u64().unwrap_or(0) as usize,
            manifest["randomizationSeed"].as_u64().unwrap_or(0),
        ),
        "trialCount": trials.len(),
    });
    write_json(&root.join("summary.json"), &summary)?;
    write_report(root, manifest, &summary)?;
    Ok(summary)
}

fn recalculate(cfg: &BenchConfig) -> Result<(), String> {
    let root = canonical(&cfg.root);
    validate_root(&root, 0)?;
    let manifest = serde_json::from_str::<Value>(
        &std::fs::read_to_string(root.join("manifest.json"))
            .map_err(|error| format!("manifest.json: {error}"))?,
    )
    .map_err(|error| error.to_string())?;
    let trials = read_jsonl(&root.join("trials.jsonl"));
    if trials.is_empty() {
        return Err("trials.jsonl is empty".into());
    }
    let summary = write_outputs(&root, &manifest, &trials)?;
    println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    Ok(())
}

fn seed_verification(fixture: &Fixture) -> Value {
    match fixture.id {
        "update_config" => json!({
            "type":"procedure",
            "appliesTo":{"paths":[fixture.audit_path]},
            "bindings":{},
            "nodes":[
                {"id":"read","tool":"workspace_read","arguments":{"path":fixture.report_path}},
                {"id":"write","tool":"workspace_write","arguments":{"path":fixture.audit_path,"content":fixture.audit_expected}}
            ],
            "edges":[{"from":"read","to":"write"}],
            "after":[{"kind":"file_contains","path":fixture.audit_path,"text":fixture.audit_expected}]
        }),
        _ => json!({
            "type":"procedure",
            "appliesTo":{"paths":[fixture.report_path]},
            "bindings":{},
            "nodes":[
                {"id":"audit","tool":"workspace_write","arguments":{"path":fixture.audit_path,"content":fixture.audit_expected}},
                {"id":"report","tool":"workspace_write","arguments":{"path":fixture.report_path,"content":fixture.report_expected}}
            ],
            "edges":[{"from":"audit","to":"report"}],
            "after":[{"kind":"file_contains","path":fixture.audit_path,"text":fixture.audit_expected}]
        }),
    }
}

async fn wait_ready(
    client: &reqwest::Client,
    base: &str,
    instance: &str,
    root: &Path,
    child: &Child,
) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match get(client, &format!("{base}/api/benchmark/identity")).await {
            Ok(identity)
                if identity["benchmark"] == true
                    && identity["instanceId"].as_str() == Some(instance)
                    && identity["pid"].as_u64() == Some(child.id() as u64)
                    && identity["dataRoot"].as_str() == Some(canonical(&root.join("data")).to_string_lossy().as_ref())
                    && identity["workspace"].as_str() == Some(canonical(&root.join("workspace")).to_string_lossy().as_ref()) =>
            {
                return Ok(identity);
            }
            _ if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            _ => return Err("isolated engine identity did not match the benchmark root".into()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut cfg = config()?;
    if cfg.recalculate {
        return recalculate(&cfg);
    }
    let client = reqwest::Client::new();
    let claim = if let Some(url) = &cfg.claim_url {
        let response = post(
            &client,
            &format!("{url}/api/experiment-requests/claim"),
            json!({
                "runnerId": std::env::var("BENCH_RUNNER_ID")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| format!("benchmark-{}", uuid::Uuid::new_v4())),
                "ttlSeconds": 1800,
            }),
        )
        .await?;
        if response["request"]["id"].is_null() {
            println!("no queued experiment requests");
            return Ok(());
        }
        if let Some(fixture_id) = response["request"]["fixture"].as_str() {
            cfg.fixture = fixture(fixture_id)?;
        }
        Some(response)
    } else {
        None
    };
    let policy_claim = claim
        .as_ref()
        .filter(|claim| claim["request"]["kind"].as_str() == Some("policy"))
        .cloned();
    let experiment_id = if cfg.fresh {
        uuid::Uuid::new_v4().to_string()
    } else {
        std::fs::read_to_string(cfg.root.join("manifest.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|manifest| manifest["experimentId"].as_str().map(str::to_string))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    };
    let root = ensure_root(&cfg, &experiment_id)?;
    let port = free_port();
    validate_root(&root, port)?;
    let base = format!("http://127.0.0.1:{port}");
    let binary = engine_binary();
    if !binary.is_file() {
        return Err(format!("engine binary not found: {}", binary.display()));
    }
    let instance = uuid::Uuid::new_v4().to_string();
    let child = Command::new(&binary)
        .env("DOMINION_PORT", port.to_string())
        .env("DOMINION_DATA", root.join("data"))
        .env("DOMINION_WORKSPACE", root.join("workspace"))
        .env("DOMINION_HOME", root.join("home"))
        .env("DOMINION_WEB", root.join("web"))
        .env("DOMINION_BENCHMARK", "1")
        // The isolated experiment engine must never start its own production
        // scheduler or remote backup loop; those are inherited env otherwise.
        .env("DOMINION_EXPERIMENT_SCHEDULER", "0")
        .env("DOMINION_BACKUP_INTERVAL_SECS", "0")
        .env("DOMINION_ENGINE_INSTANCE", &instance)
        .env("DOMINION_JOB_WORKERS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let child = child;
    let identity = wait_ready(&client, &base, &instance, &root, &child).await?;
    let _guard = ChildGuard(child);

    // Scoped fixture approval only; never enable global auto-approval.
    post(
        &client,
        &format!("{base}/api/policy"),
        json!({
            "autoApproveAll": false,
            "autoApproveMcp": false,
            "autoApprove": [
                "workspace_write",
                "workspace_read",
                "workspace_list",
                "file_read",
                "file_write",
                // The engine runs in a disposable temp workspace and the model
                // legitimately verifies files with shell commands. Without this
                // scoped approval every verification waits 120s and times out.
                "shell_exec"
            ]
        }),
    )
    .await?;

    let provider = post(
        &client,
        &format!("{base}/api/providers"),
        json!({
            "name":"s7-ollama",
            "baseUrl":cfg.provider_base,
            "model":cfg.model,
            "kind":"ollama",
            "local":true,
            "enabled":true,
        }),
    )
    .await?;
    let provider_id = provider["id"].as_str().unwrap_or("").to_string();
    let mut agents = std::collections::HashMap::new();
    for arm in ["baseline", "treatment"] {
        let agent = post(
            &client,
            &format!("{base}/api/agents"),
            json!({
                "name":arm_agent_name(arm),
                "providerId":provider_id,
                "model":cfg.model,
                "systemPrompt":"You are a precise file-operation agent.",
            }),
        )
        .await?;
        agents.insert(
            arm.to_string(),
            agent["id"].as_str().unwrap_or("").to_string(),
        );
    }
    let verification = claim
        .as_ref()
        .and_then(|claim| claim["strategy"]["verification"].as_object().map(|_| {
            claim["strategy"]["verification"].clone()
        }))
        .unwrap_or_else(|| seed_verification(&cfg.fixture));
    let strategy_text = claim
        .as_ref()
        .and_then(|claim| claim["strategy"]["text"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "harness 절차로 audit 후 report를 실행한다".to_string());
    let mut treatment_strategy_id: Option<String> = None;
    for arm in ["baseline", "treatment"] {
        if policy_claim.is_some() {
            break;
        }
        let seeded = post(
            &client,
            &format!("{base}/api/benchmark/strategies"),
            json!({
                "agentId":agents[arm],
                "assistanceMode":arm_assistance_mode(arm),
                "text":strategy_text,
                "rationale":"S7 paired experiment procedure",
                "verification":verification,
            }),
        )
        .await?;
        if seeded["assistanceMode"].as_str() != Some(arm_assistance_mode(arm)) {
            return Err(format!("could not seed {arm} strategy: {seeded}"));
        }
        if arm == "treatment" {
            treatment_strategy_id = seeded["id"].as_str().map(str::to_string);
        }
    }

    let improvements = get(&client, &format!("{base}/api/improvements")).await?;
    let memory = get(&client, &format!("{base}/api/memory/overview")).await?;
    let jobs = get(&client, &format!("{base}/api/jobs")).await?;
    let active_jobs = jobs
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|job| matches!(job["status"].as_str(), Some("queued" | "running")))
                .filter(|job| matches!(job["kind"].as_str(), Some("learn" | "reflect")))
                .count()
        })
        .unwrap_or(0);
    if active_jobs > 0 {
        return Err("learning/reflect jobs are active; evaluation must be frozen".into());
    }

    let existing_manifest = if cfg.fresh {
        None
    } else {
        std::fs::read_to_string(root.join("manifest.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    };
    let manifest = if let Some(mut manifest) = existing_manifest {
        // A resumed run may plan more pairs than the original manifest.
        manifest["pairs"] = json!(cfg.pairs);
        manifest
    } else {
        json!({
        "schemaVersion": SCHEMA_VERSION,
        "experimentId": experiment_id,
        "purpose": "controller_guard_effect",
        "mode": cfg.mode,
        "engine": repo_digest(),
        "binary": {"path": binary.to_string_lossy(), "sha256": sha256_file(&binary)},
        "model": {
            "provider": "ollama",
            "model": cfg.model,
            "temperature": 0,
            "thinking": false,
            "seedSupport": "unknown-recorded",
        },
        "knowledgeSnapshotHash": sha256(format!("{improvements}{memory}").as_bytes()),
        "toolCatalogHash": sha256(b"workspace_write,workspace_read,workspace_list,file_read,file_write"),
        "fixture": cfg.fixture.id,
        "evaluationRole": if cfg.holdout { "holdout" } else { "development" },
        "taskSplitManifestHash": sha256(format!("{}\n{ORACLE_VERSION}", cfg.fixture.task).as_bytes()),
        "oracleVersion": ORACLE_VERSION,
        "arms": ["baseline", "treatment"],
        "randomizationSeed": cfg.seed,
        "pairs": cfg.pairs,
        "order": "balanced_ab_ba",
        "learningDuringEvaluation": false,
        "budget": {"maxRounds": 8, "maxTools": 40, "maxSeconds": 600},
        "primaryMetric": "strict_task_success_delta",
        "minimumUsefulDelta": 0.05,
        "alpha": 0.05,
        "plannedComparisons": 1,
        "promotionRule": "conservative_bound",
        "maxCostRatio": 1.25,
        "environment": {
            "benchmarkOnly": true,
            "instanceId": instance,
            "identity": identity,
            "root": root.to_string_lossy(),
            "dataRoot": canonical(&root.join("data")).to_string_lossy(),
            "workspace": canonical(&root.join("workspace")).to_string_lossy(),
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    };
    write_json(&root.join("manifest.json"), &manifest)?;
    write_json(
        &root.join("baseline-fingerprint.json"),
        &json!({
            "engine": manifest["engine"],
            "binary": manifest["binary"],
            "model": manifest["model"],
            "toolCatalogHash": manifest["toolCatalogHash"],
        }),
    )?;
    write_json(
        &root.join("snapshots/fixture.json"),
        &json!({
            "task": cfg.fixture.task,
            "fixture": cfg.fixture.id,
            "oracleVersion": ORACLE_VERSION,
            "files": {
                cfg.fixture.audit_path: {"expected": cfg.fixture.audit_expected},
                cfg.fixture.report_path: {"expected": cfg.fixture.report_expected},
            },
            "procedure": verification,
        }),
    )?;

    let mut assignments = Vec::new();
    for pair in 0..cfg.pairs {
        assignments.push(json!({
            "pair": pair,
            "order": assignment_order(cfg.seed, pair),
            "trialIds": [
                trial_id(&experiment_id, pair, "baseline", 0),
                trial_id(&experiment_id, pair, "treatment", 0),
            ],
        }));
    }
    if read_jsonl(&root.join("assignments.jsonl")).is_empty() {
        for assignment in &assignments {
            append_jsonl(&root.join("assignments.jsonl"), assignment)?;
        }
    }

    let trials_path = root.join("trials.jsonl");
    let mut trials = read_jsonl(&trials_path);
    for pair in 0..cfg.pairs {
        for arm in assignment_order(cfg.seed, pair) {
            let id = trial_id(&experiment_id, pair, arm, 0);
            if existing_trial(&trials, &id).is_some() {
                println!("resume {id}: already recorded");
                continue;
            }
            let started_at = chrono::Utc::now().to_rfc3339();
            if let Some(claim) = &policy_claim {
                let policy = &claim["policy"];
                let payload = if arm == "treatment" {
                    json!({
                        "slot": policy["slot"],
                        "name": policy["name"],
                        "parameters": policy["parameters"],
                    })
                } else {
                    // Baseline must be an explicit no-plan policy; clearing the
                    // slot would fall back to the built-in heuristic, which can
                    // also plan and destroy the policy contrast.
                    json!({
                        "slot": policy["slot"],
                        "name": "baseline-no-plan",
                        "parameters": {"outline": false},
                    })
                };
                post(
                    &client,
                    &format!("{base}/api/benchmark/policies/state"),
                    payload,
                )
                .await?;
            }
            if let Err(error) = reset_fixture(&root, &cfg.fixture) {
                let trial = json!({
                    "schemaVersion": SCHEMA_VERSION,
                    "experimentId": experiment_id,
                    "trialId": id,
                    "pair": pair,
                    "arm": arm,
                    "status": "invalid",
                    "errorClass": "invalid_environment",
                    "error": error,
                    "startedAt": started_at,
                    "finishedAt": chrono::Utc::now().to_rfc3339(),
                    "elapsedMs": 0,
                    "auditOk": false,
                    "reportOk": false,
                    "compliance": false,
                    "applicable": true,
                    "taskOutcome": "unknown",
                    "requiresCompliance": policy_claim.is_none(),
                    "planQuality": 0.0,
                    "planQualityIndependent": 0.0,
                    "planText": "",
                    "planSteps": 0,
                });
                append_jsonl(&trials_path, &trial)?;
                trials.push(trial);
                continue;
            }
            let pre_hash = fixture_digest(&root, &cfg.fixture);
            let session = post(
                &client,
                &format!("{base}/api/sessions"),
                json!({"title":format!("S7 {arm} {pair}")}),
            )
            .await?;
            let session_id = session["id"].as_str().unwrap_or("").to_string();
            let (stream, elapsed) = chat(
                &client,
                &base,
                &session_id,
                &agents[arm],
                &provider_id,
                cfg.fixture.task,
                cfg.trial_timeout_secs,
            )
            .await;
            let error_class =
                classify_error(&stream, elapsed, (cfg.trial_timeout_secs as u128) * 1000);
            let audit = file_artifact(&root, cfg.fixture.audit_path);
            let report = file_artifact(&root, cfg.fixture.report_path);
            let (audit_ok, report_ok) = if cfg.fixture.checks.is_empty() {
                (
                    std::fs::read_to_string(
                        root.join("workspace").join(cfg.fixture.audit_path),
                    )
                    .map(|text| text.trim() == cfg.fixture.audit_expected)
                    .unwrap_or(false),
                    std::fs::read_to_string(
                        root.join("workspace").join(cfg.fixture.report_path),
                    )
                    .map(|text| text.trim() == cfg.fixture.report_expected)
                    .unwrap_or(false),
                )
            } else {
                let mut all = true;
                for (path, expected) in cfg.fixture.checks {
                    let matches = std::fs::read_to_string(root.join("workspace").join(path))
                        .map(|text| text.trim() == *expected)
                        .unwrap_or(false);
                    all &= matches;
                }
                let last = cfg.fixture.checks.last().unwrap();
                let last_ok = std::fs::read_to_string(root.join("workspace").join(last.0))
                    .map(|text| text.trim() == last.1)
                    .unwrap_or(false);
                (all, last_ok)
            };
            let executions = get(&client, &format!("{base}/api/executions"))
                .await
                .unwrap_or(json!([]));
            let execution = executions
                .as_array()
                .and_then(|items| {
                    items.iter().find(|item| item["sessionId"] == session_id)
                })
                .cloned()
                .unwrap_or(Value::Null);
            let operations = if let Some(id) = execution["id"].as_str() {
                get(&client, &format!("{base}/api/executions/{id}/operations"))
                    .await
                    .unwrap_or(json!([]))
            } else {
                json!([])
            };
            let task_id = execution["taskId"].as_str().unwrap_or("").to_string();
            let task = if task_id.is_empty() {
                Value::Null
            } else {
                get(&client, &format!("{base}/api/tasks/{task_id}"))
                    .await
                    .unwrap_or(Value::Null)
            };
            let compliance = execution["strategyCompliance"]
                .as_array()
                .map(|items| {
                    items.iter().any(|item| {
                        item["authority"] == "standing"
                            && item["applicable"] == true
                            && item["result"]["verified"] == true
                            && item["result"]["triggered"] == true
                            && item["result"]["followed"] == true
                    })
                })
                .unwrap_or(false);
            let applicable = execution["strategyCompliance"]
                .as_array()
                .map(|items| items.iter().any(|item| item["applicable"] == true))
                .unwrap_or(false);
            let task_outcome = execution["taskOutcome"].as_str().unwrap_or("unknown");
            let operation_rows = operations.as_array().cloned().unwrap_or_default();
            let verified = audit_ok
                && report_ok
                && task_outcome == "succeeded"
                && (policy_claim.is_some() || compliance);
            let recorded_error = if error_class == "engine_error" && verified {
                ""
            } else {
                error_class
            };
            let status = if execution["id"].is_null()
                || matches!(recorded_error, "timeout" | "transport_error")
            {
                "error"
            } else {
                "completed"
            };
            let trial = json!({
                "schemaVersion": SCHEMA_VERSION,
                "experimentId": experiment_id,
                "trialId": id,
                "pair": pair,
                "arm": arm,
                "replicate": 0,
                "status": status,
                "errorClass": recorded_error,
                "error": if recorded_error.is_empty() { "" } else { recorded_error },
                "startedAt": started_at,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
                "elapsedMs": elapsed,
                "sessionId": session_id,
                "taskId": task_id,
                "executionId": execution["id"],
                "taskOutcome": task_outcome,
                "compliance": compliance,
                "applicable": applicable,
                "auditOk": audit_ok,
                "reportOk": report_ok,
                "requiresCompliance": policy_claim.is_none(),
                "planQuality": execution["planQuality"]["total"]
                    .as_f64()
                    .unwrap_or(0.0),
                "planQualityIndependent": execution["planQualityIndependent"]["total"]
                    .as_f64()
                    .unwrap_or(0.0),
                "planText": execution["plan"]
                    .as_str()
                    .map(|text| text.chars().take(4_000).collect::<String>())
                    .unwrap_or_default(),
                "planSteps": execution["planQuality"]["steps"]
                    .as_u64()
                    .unwrap_or(0),
                "preStateHash": pre_hash,
                "postStateHash": fixture_digest(&root, &cfg.fixture),
                "tokens": execution["tokens"],
                "toolCalls": execution["toolCalls"],
                "modelCalls": execution["modelCalls"],
                "streamChars": stream.chars().count(),
                "artifacts": {"audit": audit, "report": report},
                "operations": operation_rows.iter().map(|operation| json!({
                    "tool": operation["tool"],
                    "status": operation["status"],
                    "origin": operation["origin"],
                    "opSeq": operation["opSeq"],
                })).collect::<Vec<_>>(),
                "taskState": {
                    "rootOutcome": task["rootOutcome"],
                    "attempts": task["attempts"].as_array().map(Vec::len),
                },
            });
            let directory = root.join("artifacts").join(id.replace(':', "_"));
            std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            write_json(&directory.join("trial.json"), &trial)?;
            write_json(&directory.join("execution.json"), &execution)?;
            write_json(&directory.join("operations.json"), &operations)?;
            write_json(&directory.join("task.json"), &task)?;
            std::fs::write(directory.join("stream.txt"), &stream).map_err(|error| error.to_string())?;
            append_jsonl(&trials_path, &trial)?;
            trials.push(trial.clone());
            println!(
                "pair {pair} {arm}: success={} audit={audit_ok} report={report_ok} outcome={task_outcome} compliance={compliance} {elapsed}ms",
                strict_success(&trial)
            );
        }
    }

    let summary = write_outputs(&root, &manifest, &trials)?;

    // A claimed production request must be completed even when the result is
    // inconclusive: the request lifecycle is durable, promotion is separate.
    if let (Some(url), Some(claim)) = (&cfg.claim_url, &claim) {
        let request_id = claim["request"]["id"].as_str().unwrap_or("");
        let token = claim["request"]["claimToken"].as_str().unwrap_or("");
        let effect = &summary["effect"];
        let plans = trials
            .iter()
            .filter(|trial| trial["arm"].as_str().is_some())
            .map(|trial| {
                json!({
                    "pair": trial["pair"],
                    "arm": trial["arm"],
                    "plan": trial["planText"],
                })
            })
            .collect::<Vec<_>>();
        let evidence_trials = trials
            .iter()
            .map(|trial| {
                json!({
                    "pair": trial["pair"],
                    "arm": trial["arm"],
                    "status": trial["status"],
                    "auditOk": trial["auditOk"],
                    "reportOk": trial["reportOk"],
                    "taskOutcome": trial["taskOutcome"],
                    "compliance": trial["compliance"],
                    "requiresCompliance": trial["requiresCompliance"],
                    "errorClass": trial["errorClass"],
                    "planQuality": trial["planQuality"],
                    "planQualityIndependent": trial["planQualityIndependent"],
                    "elapsedMs": trial["elapsedMs"],
                })
            })
            .collect::<Vec<_>>();
        let frozen_manifest = json!({
            "pairs": cfg.pairs,
            "seed": cfg.seed,
            "oracleVersion": manifest["oracleVersion"],
            "fixture": cfg.fixture.id,
            "mode": cfg.mode,
            "model": manifest["model"],
            "taskSplitManifestHash": manifest["taskSplitManifestHash"],
        });
        let manifest_hash = sha256(serde_json::to_string(&frozen_manifest).unwrap_or_default().as_bytes());
        let completed = post(
            &client,
            &format!("{url}/api/experiment-requests/{request_id}/complete"),
            json!({
                "claimToken": token,
                "experimentId": summary["experimentId"],
                "verdict": effect["verdict"],
                "delta": effect["delta"],
                "independentUnits": effect["independentUnits"],
                "semanticDelta": effect["semanticDelta"],
                "independentSemanticDelta": effect["independentSemanticDelta"],
                "semanticMetric": effect["semanticMetric"],
                "semanticEvaluator": {
                    "id": crate::plan_quality::EVALUATOR_ID,
                    "version": crate::plan_quality::EVALUATOR_VERSION,
                },
                "independentEvaluator": {
                    "id": crate::plan_quality::INDEPENDENT_EVALUATOR_ID,
                    "version": crate::plan_quality::INDEPENDENT_EVALUATOR_VERSION,
                },
                "task": cfg.fixture.task,
                "plans": plans,
                "trials": evidence_trials,
                "manifest": frozen_manifest,
                "manifestHash": manifest_hash,
                "oracleVersion": manifest["oracleVersion"],
            }),
        )
        .await;
        match completed {
            Ok(value) => println!(
                "request completed: {} promotion={}",
                value["request"]["status"].as_str().unwrap_or(""),
                value["promotion"]["evidenceGrade"].as_str().unwrap_or("none")
            ),
            Err(error) => println!("request completion failed: {error}"),
        }
    } else if cfg.mode == "real" {
        if let Some(strategy_id) = treatment_strategy_id.as_deref() {
            let effect = &summary["effect"];
            match post(
                &client,
                &format!("{base}/api/benchmark/experiments"),
                json!({
                    "strategyId": strategy_id,
                    "experimentId": summary["experimentId"],
                    "verdict": effect["verdict"],
                    "delta": effect["delta"],
                    "independentUnits": effect["independentUnits"],
                    "oracleVersion": manifest["oracleVersion"],
                }),
            )
            .await
            {
                Ok(record) => println!(
                    "promotion: status={} evidence={}",
                    record["status"].as_str().unwrap_or(""),
                    record["evidenceGrade"].as_str().unwrap_or("")
                ),
                Err(error) => println!("promotion skipped: {error}"),
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeouts_count_as_itt_failures() {
        let trials = vec![
            json!({"pair":0,"arm":"baseline","status":"completed","auditOk":true,"reportOk":true,"taskOutcome":"succeeded","compliance":true,"errorClass":"","planQuality":0.0}),
            json!({"pair":0,"arm":"treatment","status":"error","auditOk":false,"reportOk":false,"taskOutcome":"unknown","compliance":false,"errorClass":"timeout","planQuality":0.4}),
            json!({"pair":1,"arm":"baseline","status":"completed","auditOk":true,"reportOk":true,"taskOutcome":"succeeded","compliance":true,"errorClass":"","planQuality":0.0}),
            json!({"pair":1,"arm":"treatment","status":"completed","auditOk":true,"reportOk":true,"taskOutcome":"succeeded","compliance":true,"errorClass":"","planQuality":0.6}),
        ];
        let effect = pair_effect(&trials, 2, 7);
        assert_eq!(effect["plannedUnits"], 2);
        assert_eq!(effect["independentUnits"], 2);
        assert_eq!(effect["timeoutTrials"], 1);
        assert_eq!(effect["delta"], -0.5);
        assert_eq!(effect["verdict"], "inconclusive");
        assert_eq!(effect["semanticDelta"], 0.5);
    }

    #[test]
    fn missing_arm_counts_as_failure_instead_of_being_dropped() {
        let trials = vec![
            json!({"pair":0,"arm":"baseline","status":"completed","auditOk":false,"reportOk":false,"taskOutcome":"failed","compliance":false,"errorClass":"","planQuality":0.0}),
        ];
        let effect = pair_effect(&trials, 1, 1);
        assert_eq!(effect["independentUnits"], 1);
        assert_eq!(effect["missingArms"], 1);
        assert_eq!(effect["delta"], 0.0);
    }

    #[test]
    fn policy_effect_reports_semantic_plan_delta() {
        let base = |pair: u64, arm: &str, plan: f64, success: bool| {
            json!({
                "pair":pair,"arm":arm,"status":"completed",
                "auditOk":success,"reportOk":success,
                "taskOutcome":if success {"succeeded"} else {"failed"},
                "compliance":false,"requiresCompliance":false,"errorClass":"",
                "elapsedMs":10,"planQuality":plan
            })
        };
        let trials = vec![
            base(0, "baseline", 0.1, false),
            base(0, "treatment", 0.8, true),
            base(1, "baseline", 0.2, false),
            base(1, "treatment", 0.9, true),
        ];
        let effect = pair_effect(&trials, 2, 1);
        assert_eq!(effect["independentUnits"], 2);
        assert_eq!(effect["semanticUnits"], 2);
        assert!((effect["semanticDelta"].as_f64().unwrap() - 0.7).abs() < 0.0001);
        assert_eq!(effect["semanticMetric"], "plan_quality_v1");
        let aggregate = aggregate(&trials, "treatment", 2);
        assert!((aggregate["planQuality"].as_f64().unwrap() - 0.85).abs() < 0.0001);
    }

    #[test]
    fn policy_trials_do_not_require_procedure_compliance() {
        let mut trial = json!({
            "status":"completed","auditOk":true,"reportOk":true,
            "taskOutcome":"succeeded","compliance":false,"errorClass":"",
            "requiresCompliance":false
        });
        assert!(strict_success(&trial));
        trial["requiresCompliance"] = json!(true);
        assert!(!strict_success(&trial));
    }

    #[test]
    fn fixtures_are_declared_explicitly_and_tasks_differ() {
        let audit = fixture("audit_report").unwrap();
        let update = fixture("update_config").unwrap();
        assert_ne!(audit.task, update.task);
        assert_eq!(update.report_expected, "mode=draft");
        assert_eq!(update.seeds.len(), 1);
        assert!(fixture("unknown").is_err());
    }

    #[test]
    fn assignment_is_balanced_and_reproducible() {
        let first = (0..20)
            .map(|pair| assignment_order(7, pair))
            .collect::<Vec<_>>();
        let second = (0..20)
            .map(|pair| assignment_order(7, pair))
            .collect::<Vec<_>>();
        assert_eq!(first, second);
        let baseline_first = first.iter().filter(|order| order[0] == "baseline").count();
        assert_eq!(baseline_first, 10);
    }

    #[test]
    fn trial_ids_are_stable_and_arm_specific() {
        assert_eq!(trial_id("e", 2, "baseline", 0), "e:p2:baseline:r0");
        assert_ne!(
            trial_id("e", 2, "baseline", 0),
            trial_id("e", 2, "treatment", 0)
        );
    }

    #[test]
    fn strict_success_requires_oracle_and_compliance() {
        let mut trial = json!({
            "status":"completed","auditOk":true,"reportOk":true,
            "taskOutcome":"succeeded","compliance":true,"errorClass":""
        });
        assert!(strict_success(&trial));
        trial["auditOk"] = json!(false);
        assert!(!strict_success(&trial));
        trial["auditOk"] = json!(true);
        trial["compliance"] = json!(false);
        assert!(!strict_success(&trial));
    }

    #[test]
    fn error_classification_is_explicit() {
        assert_eq!(classify_error("", 300_000, 300_000), "timeout");
        assert_eq!(
            classify_error("data: {\"type\":\"error\"}\n\n", 10, 300_000),
            "stream_error"
        );
        assert_eq!(
            classify_error("HTTP 500: boom", 10, 300_000),
            "transport_error"
        );
        assert_eq!(classify_error("engine failed", 10, 300_000), "engine_error");
    }

    #[test]
    fn aggregate_does_not_turn_zero_runs_into_success() {
        let trials = vec![json!({
            "arm":"baseline","status":"completed","auditOk":false,"reportOk":true,
            "taskOutcome":"failed","compliance":false,"errorClass":""
        })];
        let aggregate = aggregate(&trials, "baseline", 1);
        assert_eq!(aggregate["success"], 0);
        assert_eq!(aggregate["failure"], 1);
        assert_eq!(aggregate["strictSuccessRate"], 0.0);
    }

    #[test]
    fn empty_effect_is_not_evaluated() {
        let effect = pair_effect(&[], 0, 94371);
        assert_eq!(effect["verdict"], "not_evaluated");
        assert_eq!(effect["independentUnits"], 0);
    }

    #[test]
    fn root_validation_rejects_production_locations() {
        let root = std::env::temp_dir().join(format!("dominion-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(validate_root(&root, 5000).is_ok());
        assert!(validate_root(&root, 4317).is_err());
        assert!(validate_root(Path::new("/tmp/../Users"), 5000).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
