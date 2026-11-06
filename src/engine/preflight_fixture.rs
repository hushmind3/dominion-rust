//! Frozen file-mutation holdout. The expected bytes and evidence checks are
//! shared by the isolated runner and the experiment settlement endpoint.
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const ID: &str = "preflight_holdout";
pub const ORACLE: &str = "preflight-file-v3";
pub const EXACT_KERNEL: &str = "exact-file-replacement-v3";
pub const GUARD_KERNEL: &str = "same-file-revision-v2";
pub const GUARD_MODE: &str = "guard_only";
pub const GUARD_WORKLOAD: &str = "model-mediated-exact-workspace-file-v1";
pub const MAX_COST_RATIO: f64 = 1.25;
pub const MAX_ROUNDS: u64 = 8;
pub const MAX_TOOLS: u64 = 40;
pub const MAX_SECONDS: u64 = 600;
pub const MAX_OUTPUT_TOKENS: u64 = 1024;

pub fn budget(timeout_secs: u64) -> Value {
    serde_json::json!({"maxRounds":MAX_ROUNDS,"maxTools":MAX_TOOLS,
        "maxSeconds":MAX_SECONDS,"clientTimeoutSecs":timeout_secs})
}

/// Only paired guard trials are eligible for adoption. Missing metering
/// makes the comparison ineligible without implying a cost regression.
pub fn guard_cost(trials: &[Value]) -> (Option<f64>, bool) {
    let mut tokens = [0u64; 2];
    let mut timeouts = [0u64; 2];
    for trial in trials {
        let arm = usize::from(trial["arm"] == "treatment");
        tokens[arm] = tokens[arm].saturating_add(trial["tokens"].as_u64().unwrap_or(0));
        timeouts[arm] += u64::from(trial["errorClass"] == "timeout");
    }
    let ratio = (!tokens.contains(&0)).then(|| tokens[1] as f64 / tokens[0] as f64);
    (ratio, timeouts[1] > timeouts[0])
}

/// The upstream model matters for independent evidence, never the runtime.
pub fn evaluation_cohort(model: &Value) -> Option<String> {
    let name = match model["provider"].as_str()? {
        "ollama" => model["model"].as_str()?,
        "openai" => model["expectedRoute"].as_str()?,
        _ => return None,
    }.rsplit('/').next()?.trim().to_ascii_lowercase();
    let name = name.strip_suffix(":free").or_else(|| name.strip_suffix(":latest")).unwrap_or(&name);
    (!name.is_empty()).then(|| name.to_owned())
}
pub const TOOLS: &[&str] = &["workspace_read", "workspace_write"];

pub fn scope(base_url: &str, model: &str, tool_mode: &str) -> String {
    digest(serde_json::json!([base_url.trim_end_matches('/'), model, tool_mode]).to_string().as_bytes())
}

pub fn evaluating() -> bool {
    std::env::var("DOMINION_BENCHMARK").as_deref() == Ok("1")
        && std::env::var("DOMINION_BENCHMARK_PREFLIGHT").as_deref() == Ok("1")
}
pub const CASES: &[(&str, &str, &str, &str)] = &[
    (
        "routing.txt",
        "mode=shadow\nowner=R6P9W",
        "shadow",
        "active",
    ),
    ("workers.txt", "workers=2\nqueue_tag=F4Q7X", "2", "6"),
    ("retention.txt", "bucket=K8C4V\ndays=7", "7", "30"),
    (
        "release.txt",
        "ring: canary\nnonce: A2T5Z",
        "canary",
        "stable",
    ),
    ("traffic.txt", "label=J5M8R\nweight=10", "10", "40"),
    (
        "refresh.txt",
        "interval=15s\nregion_code=L4V7P",
        "15s",
        "60s",
    ),
    ("feature.txt", "owner_id=W8K3N\nsearch=off", "off", "on"),
    (
        "logging.txt",
        "level: debug\nstream: Y6M4X",
        "debug",
        "warn",
    ),
    (
        "access.txt",
        "group=Z8B2K\naccess=review",
        "review",
        "audit",
    ),
    ("storage.txt", "tier=cold\nmount_id=P7D3T", "cold", "warm"),
    ("cache.txt", "shard: G4N8K\nttl: 10m", "10m", "25m"),
    ("polling.txt", "poll=slow\nclient=V2P8F", "slow", "fast"),
];

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_ok(pair: usize, hash: &str) -> bool {
    CASES.get(pair).is_some_and(|(_, seed, old, new)| {
        let expected = seed.replacen(old, new, 1);
        hash == digest(format!("{expected}\n").as_bytes())
    })
}

/// Compare successful reads and writes of the target in recorded operation
/// order. A write consumes the evidence, so subsequent writes need a new read.
pub fn write_evidence(pair: usize, operations: &[Value]) -> (Option<String>, bool) {
    let Some((name, seed, _, _)) = CASES.get(pair) else {
        return (None, false);
    };
    let path = format!("bench/{name}");
    let mut current = digest(format!("{seed}\n").as_bytes());
    let mut observed = false;
    let mut wrote = false;
    let mut checked = true;
    let mut ordered = operations.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|operation| operation["opSeq"].as_u64());
    for operation in ordered {
        if operation["target"] != path {
            continue;
        }
        if operation["status"] != "completed" {
            if operation["tool"] == "workspace_write"
                && operation["preflightBlocked"] == true
                && operation["preflightObserved"] == true
            {
                observed = operation["observedSha256"] == current;
            }
            continue;
        }
        match operation["tool"].as_str() {
            Some("workspace_read" | "file_read") => {
                observed = operation["observedSha256"] == current;
            }
            Some("workspace_write" | "file_write") => {
                wrote = true;
                checked &= observed
                    && operation["preflightChecked"] == true
                    && operation["observedSha256"] == current;
                observed = false;
                current = operation["writtenSha256"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
            }
            _ => {}
        }
    }
    (wrote.then_some(current), wrote && checked)
}
