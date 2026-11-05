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
    let root = match std::env::var("BENCH_ROOT").ok().filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => std::env::temp_dir().join(format!("dominion-s7-{}", uuid::Uuid::new_v4())),
    };
    Ok(BenchConfig {
        mode,
        model: env_string("BENCH_MODEL", "qwen3:4b"),
        pairs,
        seed,
        root,
        fresh: !recalculate && std::env::var("BENCH_ROOT").is_err(),
        recalculate,
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

fn reset_fixture(root: &Path) -> Result<(), String> {
    let bench = root.join("workspace/bench");
    if bench.exists() {
        std::fs::remove_dir_all(&bench).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&bench).map_err(|error| error.to_string())?;
    Ok(())
}

fn fixture_digest(root: &Path) -> String {
    let mut parts = Vec::new();
    for path in [AUDIT_PATH, REPORT_PATH] {
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
    trial["status"] == "completed"
        && trial["auditOk"] == true
        && trial["reportOk"] == true
        && trial["taskOutcome"] == "succeeded"
        && trial["compliance"] == true
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
    json!({
        "assigned": pairs,
        "runs": rows.len(),
        "success": success,
        "failure": failure,
        "unknown": unknown,
        "invalid": invalid,
        "strictSuccessRate": if rows.is_empty() { Value::Null } else { json!(success as f64 / rows.len() as f64) },
        "compliance": compliant,
        "triggerCoverage": if applicable == 0 { Value::Null } else { json!(compliant as f64 / applicable as f64) },
        "tokens": tokens,
        "p50Ms": percentile(&mut latencies.clone(), 0.50),
        "p95Ms": percentile(&mut latencies, 0.95),
    })
}

fn pair_effect(trials: &[Value], seed: u64) -> Value {
    let pairs = trials
        .iter()
        .filter_map(|trial| trial["pair"].as_u64())
        .map(|pair| pair as usize)
        .max()
        .map(|max| max + 1)
        .unwrap_or(0);
    let mut deltas = Vec::new();
    let mut units = 0usize;
    for pair in 0..pairs {
        let baseline = trials
            .iter()
            .find(|trial| trial["pair"].as_u64() == Some(pair as u64) && trial["arm"] == "baseline");
        let treatment = trials
            .iter()
            .find(|trial| trial["pair"].as_u64() == Some(pair as u64) && trial["arm"] == "treatment");
        let (Some(baseline), Some(treatment)) = (baseline, treatment) else {
            continue;
        };
        if baseline["status"] != "completed" || treatment["status"] != "completed" {
            continue;
        }
        units += 1;
        let b = if strict_success(baseline) { 1.0 } else { 0.0 };
        let t = if strict_success(treatment) { 1.0 } else { 0.0 };
        deltas.push(t - b);
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
        (2.0 * (2.0f64 / alpha).ln() / n as f64).sqrt().min(1.0)
    };
    let lower = (mean - halfwidth).max(-1.0);
    let upper = (mean + halfwidth).min(1.0);
    let verdict = if units == 0 {
        "not_evaluated"
    } else if units < 10 || lower <= 0.0 {
        "inconclusive"
    } else {
        "preliminary_support"
    };
    json!({
        "independentUnits": units,
        "seed": seed,
        "delta": mean,
        "halfwidth": halfwidth,
        "ci": [lower, upper],
        "verdict": verdict,
        "note": "Small N is inconclusive; this is not E4 evidence.",
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
        .timeout(Duration::from_secs(300))
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
        "model": manifest["model"],
        "oracleVersion": ORACLE_VERSION,
        "arms": {
            "baseline": aggregate(trials, "baseline", manifest["pairs"].as_u64().unwrap_or(0) as usize),
            "treatment": aggregate(trials, "treatment", manifest["pairs"].as_u64().unwrap_or(0) as usize),
        },
        "effect": pair_effect(trials, manifest["randomizationSeed"].as_u64().unwrap_or(0)),
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

fn seed_verification() -> Value {
    json!({
        "type":"procedure",
        "appliesTo":{"paths":[REPORT_PATH]},
        "bindings":{},
        "nodes":[
            {"id":"audit","tool":"workspace_write","arguments":{"path":AUDIT_PATH,"content":AUDIT_EXPECTED}},
            {"id":"report","tool":"workspace_write","arguments":{"path":REPORT_PATH,"content":REPORT_EXPECTED}}
        ],
        "edges":[{"from":"audit","to":"report"}],
        "after":[{"kind":"file_contains","path":AUDIT_PATH,"text":AUDIT_EXPECTED}]
    })
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
    let cfg = config()?;
    if cfg.recalculate {
        return recalculate(&cfg);
    }
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
        .env("DOMINION_ENGINE_INSTANCE", &instance)
        .env("DOMINION_JOB_WORKERS", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let child = child;
    let client = reqwest::Client::new();
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
                "file_write"
            ]
        }),
    )
    .await?;

    let provider = post(
        &client,
        &format!("{base}/api/providers"),
        json!({
            "name":"s7-ollama",
            "baseUrl":"http://127.0.0.1:11434/v1",
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
    let verification = seed_verification();
    let mut treatment_strategy_id: Option<String> = None;
    for arm in ["baseline", "treatment"] {
        let seeded = post(
            &client,
            &format!("{base}/api/benchmark/strategies"),
            json!({
                "agentId":agents[arm],
                "assistanceMode":arm_assistance_mode(arm),
                "text":"harness 절차로 audit 후 report를 실행한다",
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
    let manifest = if let Some(manifest) = existing_manifest {
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
        "taskSplitManifestHash": sha256(format!("{TASK}\n{ORACLE_VERSION}").as_bytes()),
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
            "task": TASK,
            "oracleVersion": ORACLE_VERSION,
            "files": {
                AUDIT_PATH: {"expected": AUDIT_EXPECTED},
                REPORT_PATH: {"expected": REPORT_EXPECTED},
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
            if let Err(error) = reset_fixture(&root) {
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
                });
                append_jsonl(&trials_path, &trial)?;
                trials.push(trial);
                continue;
            }
            let pre_hash = fixture_digest(&root);
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
                TASK,
            )
            .await;
            let error_class = classify_error(&stream, elapsed, 300_000);
            let audit = file_artifact(&root, AUDIT_PATH);
            let report = file_artifact(&root, REPORT_PATH);
            let audit_ok = std::fs::read_to_string(root.join("workspace").join(AUDIT_PATH))
                .map(|text| text.trim() == AUDIT_EXPECTED)
                .unwrap_or(false);
            let report_ok = std::fs::read_to_string(root.join("workspace").join(REPORT_PATH))
                .map(|text| text.trim() == REPORT_EXPECTED)
                .unwrap_or(false);
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
            let recorded_error = if error_class == "engine_error"
                && compliance
                && task_outcome == "succeeded"
                && audit_ok
                && report_ok
            {
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
                "preStateHash": pre_hash,
                "postStateHash": fixture_digest(&root),
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

    // Closed loop: only a real experiment with enough independent units may
    // promote or roll back the treatment strategy. Smoke runs are recorded in
    // the summary but never change production policy.
    if cfg.mode == "real" {
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
        let effect = pair_effect(&[], 94371);
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
