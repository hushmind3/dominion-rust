//! Per-agent strategy selection, observed outcome updates and statistics rebuilding.
use super::*;

pub(super) fn agent_of(_state: &AppState, agent_id: Option<&str>) -> Option<String> {
    // A turn without an agent still needs a stable scope. Use the shared
    // "global" statistics instead of silently picking another bot.
    Some(
        agent_id
            .filter(|value| !value.is_empty())
            .unwrap_or("global")
            .to_string(),
    )
}

pub(super) fn stat_id(agent: &str, kind: &str, revision: &str) -> String {
    format!(
        "algorithm-stat-{}",
        crate::engine::memory::stable_id(&[agent, kind, revision])
    )
}

/// UCB1 over the agent's own outcomes. The model is irrelevant; what improves
/// is which judgment kernel this bot has actually had success with.
pub fn choose_for_agent(
    state: &AppState,
    kind: &str,
    agent_id: Option<&str>,
    execution_id: Option<&str>,
) -> Option<Value> {
    seed(state);
    let agent = agent_of(state, agent_id)?;
    let candidates = state
        .store
        .all("algorithm")
        .into_iter()
        .filter(|algorithm| {
            algorithm["kind"].as_str() == Some(kind)
                // Builtins are trusted executable baselines. Generated/user
                // candidates cannot enter live UCB exploration merely by
                // being registered as "available": they require validation
                // or explicit activation by the owner.
                && (matches!(algorithm["status"].as_str(),
                        Some("active" | "validated"))
                    || (algorithm["status"].as_str() == Some("available")
                        && algorithm["source"].as_str() == Some("builtin")))
                && algorithm["implementation"]
                    .as_str()
                    .is_some_and(|implementation| kernel_exists(kind, implementation))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    let all_stats = state.store.all("algorithm_stat");
    let stats = all_stats
        .iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some(agent.as_str())
                && stat["kind"].as_str() == Some(kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let global_stats = all_stats
        .iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some("global")
                && stat["kind"].as_str() == Some(kind)
        })
        .cloned()
        .collect::<Vec<_>>();
    let total = stats
        .iter()
        .map(|stat| stat["uses"].as_u64().unwrap_or(0))
        .sum::<u64>() as f64;
    let active_revision = active(state, kind)
        .and_then(|algorithm| algorithm["revision"].as_str().map(str::to_string));
    let chosen = candidates
        .into_iter()
        .max_by(|a, b| {
            let score = |algorithm: &Value| {
                let revision = algorithm["revision"].as_str().unwrap_or("");
                let stat = stats
                    .iter()
                    .find(|stat| stat["revision"].as_str() == Some(revision))
                    .or_else(|| {
                        global_stats
                            .iter()
                            .find(|stat| stat["revision"].as_str() == Some(revision))
                    });
                let uses = stat
                    .and_then(|stat| stat["uses"].as_u64())
                    .unwrap_or(0) as f64;
                let successes = stat
                    .and_then(|stat| stat["successes"].as_u64())
                    .unwrap_or(0) as f64;
                let rate = if uses > 0.0 { successes / uses } else { 0.0 };
                let exploration = if uses > 0.0 {
                    (2.0 * (total + 1.0).ln() / uses).sqrt()
                } else {
                    1.5
                };
                let active_bonus = if active_revision
                    .as_deref()
                    == algorithm["revision"].as_str()
                {
                    0.05
                } else {
                    0.0
                };
                rate + exploration + active_bonus
            };
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    if let Some(execution_id) = execution_id {
        let usage_id = format!(
            "algorithm-usage-{}",
            crate::engine::memory::stable_id(&[execution_id, kind])
        );
        if state.store.get("algorithm_usage", &usage_id).is_none() {
            if let Some(saved) = state.store.put(
                "algorithm_usage",
                &json!({
                    "id": usage_id,
                    "executionId": execution_id,
                    "agentId": agent,
                    "kind": kind,
                    "revision": chosen["revision"],
                    "status": "pending",
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                }),
            ) {
                state.bus.record("algorithm_usage", &saved);
            }
        }
    }
    Some(chosen)
}

pub fn variant_for_agent(
    state: &AppState,
    kind: &str,
    agent_id: Option<&str>,
) -> String {
    choose_for_agent(state, kind, agent_id, None)
        .and_then(|algorithm| {
            algorithm["implementation"]
                .as_str()
                .or_else(|| algorithm["variant"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| variant(state, kind))
}

/// Records the observable outcome of a specific algorithm decision. The final
/// task outcome is only a fallback; this signal distinguishes which domain
/// actually succeeded or failed.
pub fn mark_signal(state: &AppState, execution_id: &str, kind: &str, ok: bool) {
    let usage_id = format!(
        "algorithm-usage-{}",
        crate::engine::memory::stable_id(&[execution_id, kind])
    );
    let Some(mut usage) = state.store.get("algorithm_usage", &usage_id) else {
        return;
    };
    if !usage["signal"].is_object() {
        usage["signal"] = json!({"kind": kind, "successes": 0, "failures": 0});
    }
    if ok {
        usage["signal"]["successes"] =
            json!(usage["signal"]["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        usage["signal"]["failures"] =
            json!(usage["signal"]["failures"].as_u64().unwrap_or(0) + 1);
    }
    usage["signalUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_usage", &usage) {
        state.bus.record("algorithm_usage", &saved);
    }
}

/// Marks a decision as observable but not gradeable. Used when the harness can
/// see that an algorithm ran but cannot independently judge whether it helped.
pub fn mark_unrated(state: &AppState, execution_id: &str, kind: &str, reason: &str) {
    let usage_id = format!(
        "algorithm-usage-{}",
        crate::engine::memory::stable_id(&[execution_id, kind])
    );
    let Some(mut usage) = state.store.get("algorithm_usage", &usage_id) else {
        return;
    };
    usage["forceUnrated"] = json!(true);
    usage["unratedReason"] = json!(reason);
    usage["signalUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_usage", &usage) {
        state.bus.record("algorithm_usage", &saved);
    }
}

/// Records one domain outcome for a concrete kernel. Used by harness paths that
/// do not have a normal execution record (for example a room-level debate
/// resolution) but still need the same per-agent and global statistics.
pub fn record_domain_outcome(
    state: &AppState,
    agent_id: Option<&str>,
    kind: &str,
    implementation: &str,
    ok: bool,
) {
    seed(state);
    let agent = agent_of(state, agent_id).unwrap_or_else(|| "global".into());
    let Some(revision) = state
        .store
        .all("algorithm")
        .into_iter()
        .find(|algorithm| {
            algorithm["kind"].as_str() == Some(kind)
                && algorithm["implementation"].as_str() == Some(implementation)
                && algorithm["status"].as_str() != Some("disabled")
        })
        .and_then(|algorithm| algorithm["revision"].as_str().map(str::to_string))
    else {
        return;
    };
    apply_stat(state, &agent, kind, &revision, ok);
}

pub(super) fn apply_stat(state: &AppState, agent: &str, kind: &str, revision: &str, success: bool) {
    let id = stat_id(agent, kind, revision);
    let mut stat = state.store.get("algorithm_stat", &id).unwrap_or_else(|| {
        json!({
            "id": id,
            "agentId": agent,
            "kind": kind,
            "revision": revision,
            "uses": 0,
            "successes": 0,
            "failures": 0,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    });
    stat["uses"] = json!(stat["uses"].as_u64().unwrap_or(0) + 1);
    if success {
        stat["successes"] = json!(stat["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        stat["failures"] = json!(stat["failures"].as_u64().unwrap_or(0) + 1);
    }
    stat["successRate"] = json!(
        stat["successes"].as_u64().unwrap_or(0) as f64
            / stat["uses"].as_u64().unwrap_or(1).max(1) as f64
    );
    stat["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_stat", &stat) {
        state.bus.record("algorithm_stat", &saved);
    }
    // Collective statistics: a new bot starts from what the group has already
    // learned instead of a blank slate. A global agent already updated this
    // record above; do not count it twice.
    if agent == "global" {
        return;
    }
    let global_id = stat_id("global", kind, revision);
    let mut global = state.store.get("algorithm_stat", &global_id).unwrap_or_else(|| {
        json!({
            "id": global_id,
            "agentId": "global",
            "kind": kind,
            "revision": revision,
            "uses": 0,
            "successes": 0,
            "failures": 0,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        })
    });
    global["uses"] = json!(global["uses"].as_u64().unwrap_or(0) + 1);
    if success {
        global["successes"] = json!(global["successes"].as_u64().unwrap_or(0) + 1);
    } else {
        global["failures"] = json!(global["failures"].as_u64().unwrap_or(0) + 1);
    }
    global["successRate"] = json!(
        global["successes"].as_u64().unwrap_or(0) as f64
            / global["uses"].as_u64().unwrap_or(1).max(1) as f64
    );
    global["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(saved) = state.store.put("algorithm_stat", &global) {
        state.bus.record("algorithm_stat", &saved);
    }
}

/// One durable usage can be scored only by an independent domain signal or by
/// an explicit task outcome. An unverified conversation answer is neither a
/// success nor a failure; treating it as a failure poisoned the counters and
/// then fed a fake "0% success" back into the prompt.
pub(super) fn usage_rating(usage: &Value, task_outcome: &str) -> Option<(bool, &'static str)> {
    // Some paths only observe exposure, not whether the decision helped. They
    // force `unrated` even when the surrounding task succeeded or failed.
    if usage["forceUnrated"].as_bool() == Some(true)
        || usage["kind"].as_str() == Some("memory.retrieval")
    {
        return None;
    }
    if usage["signal"].is_object() {
        let successes = usage["signal"]["successes"].as_u64().unwrap_or(0);
        let failures = usage["signal"]["failures"].as_u64().unwrap_or(0);
        if successes > 0 || failures > 0 {
            return Some((
                (successes > 0 && failures == 0) || successes > failures,
                "domain_signal",
            ));
        }
    }
    match task_outcome {
        "succeeded" => Some((true, "task_outcome")),
        "failed" => Some((false, "task_outcome")),
        _ => None,
    }
}

/// Applies the execution outcome to every algorithm used during that run.
pub fn finalize_execution(state: &AppState, execution: &Value) {
    let Some(execution_id) = execution["id"].as_str() else {
        return;
    };
    // A deterministic shortcut made no model decision, so it carries no
    // evidence for which judgment algorithm works better.
    if execution["deterministicShortcut"].is_object() {
        for mut usage in state.store.all("algorithm_usage") {
            if usage["executionId"].as_str() == Some(execution_id)
                && usage["status"].as_str() == Some("pending")
            {
                usage["status"] = json!("excluded");
                usage["reason"] = json!("deterministic shortcut: no model decision");
                state.store.put("algorithm_usage", &usage);
            }
        }
        return;
    }
    let task_outcome = execution["taskOutcome"].as_str().unwrap_or("");
    for mut usage in state.store.all("algorithm_usage") {
        if usage["executionId"].as_str() != Some(execution_id)
            || usage["status"].as_str() != Some("pending")
        {
            continue;
        }
        let (Some(agent), Some(kind), Some(revision)) = (
            usage["agentId"].as_str(),
            usage["kind"].as_str(),
            usage["revision"].as_str(),
        ) else {
            continue;
        };
        match usage_rating(&usage, task_outcome) {
            Some((success, source)) => {
                // Domain signals win over the whole-task fallback. Multiple
                // events in one run are aggregated, not overwritten by the
                // last one.
                apply_stat(state, agent, kind, revision, success);
                usage["status"] = json!("completed");
                usage["rated"] = json!(true);
                usage["ratingSource"] = json!(source);
                usage["success"] = json!(success);
            }
            None => {
                usage["status"] = json!("unrated");
                usage["rated"] = json!(false);
                usage["ratingSource"] = json!("unrated");
                usage["success"] = Value::Null;
                usage["unratedReason"] =
                    json!("task outcome is not a verified success or failure");
            }
        }
        state.store.put("algorithm_usage", &usage);
    }
}

/// Recomputes statistics from durable usage evidence. A conversation that was
/// never scored must not appear as an algorithm failure, so the rebuild also
/// rewrites the usage status to `unrated` before aggregating.
pub fn rebuild_stats_from_usage(state: &AppState) -> Result<Value, String> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Aggregate {
        uses: u64,
        successes: u64,
        failures: u64,
        first_used: Option<String>,
        last_used: Option<String>,
    }

    impl Aggregate {
        fn add(&mut self, success: bool, at: Option<&str>) {
            self.uses += 1;
            if success {
                self.successes += 1;
            } else {
                self.failures += 1;
            }
            if let Some(at) = at.filter(|value| !value.is_empty()) {
                if self
                    .first_used
                    .as_deref()
                    .is_none_or(|current| at < current)
                {
                    self.first_used = Some(at.to_string());
                }
                if self
                    .last_used
                    .as_deref()
                    .is_none_or(|current| at > current)
                {
                    self.last_used = Some(at.to_string());
                }
            }
        }
    }

    let outcomes = state
        .store
        .all("execution")
        .into_iter()
        .filter_map(|execution| {
            Some((
                execution["id"].as_str()?.to_string(),
                execution["taskOutcome"].as_str().unwrap_or("").to_string(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut per_agent: BTreeMap<(String, String, String), Aggregate> = BTreeMap::new();
    let mut global: BTreeMap<(String, String), Aggregate> = BTreeMap::new();
    let (mut rated, mut unrated, mut skipped) = (0u64, 0u64, 0u64);
    for mut usage in state.store.all("algorithm_usage") {
        match usage["status"].as_str() {
            Some("excluded" | "pending") => {
                skipped += 1;
                continue;
            }
            _ => {}
        }
        let (Some(agent), Some(kind), Some(revision)) = (
            usage["agentId"].as_str().map(str::to_string),
            usage["kind"].as_str().map(str::to_string),
            usage["revision"].as_str().map(str::to_string),
        ) else {
            skipped += 1;
            continue;
        };
        let outcome = usage["executionId"]
            .as_str()
            .and_then(|id| outcomes.get(id))
            .map(String::as_str)
            .unwrap_or("");
        // Legacy `memory.retrieval` signals only checked whether a tool named
        // in a past case happened to be called. That is an exposure proxy, not
        // evidence that retrieval helped, so it is not a gradeable outcome.
        if kind == "memory.retrieval" {
            usage["forceUnrated"] = json!(true);
            usage["unratedReason"] =
                json!("retrieval exposure is not an independently measured outcome");
        }
        let at = usage["signalUpdatedAt"]
            .as_str()
            .or_else(|| usage["updatedAt"].as_str())
            .or_else(|| usage["createdAt"].as_str())
            .map(str::to_string);
        match usage_rating(&usage, outcome) {
            Some((success, source)) => {
                rated += 1;
                usage["status"] = json!("completed");
                usage["rated"] = json!(true);
                usage["ratingSource"] = json!(source);
                usage["success"] = json!(success);
                if agent != "global" {
                    per_agent
                        .entry((agent.clone(), kind.clone(), revision.clone()))
                        .or_default()
                        .add(success, at.as_deref());
                }
                global
                    .entry((kind, revision))
                    .or_default()
                    .add(success, at.as_deref());
            }
            None => {
                unrated += 1;
                usage["status"] = json!("unrated");
                usage["rated"] = json!(false);
                usage["ratingSource"] = json!("unrated");
                usage["success"] = Value::Null;
                usage["unratedReason"] =
                    json!("task outcome is not a verified success or failure");
            }
        }
        state
            .store
            .put("algorithm_usage", &usage)
            .ok_or("could not persist rebuilt algorithm usage")?;
    }
    // The old counters cannot be repaired in place because they contain an
    // unknown mix of rated and unrated runs. Replace them with aggregates.
    for stat in state.store.all("algorithm_stat") {
        if let Some(id) = stat["id"].as_str() {
            state.store.remove("algorithm_stat", id);
        }
    }
    let mut written = 0usize;
    let write = |agent: &str, kind: &str, revision: &str, aggregate: &Aggregate| -> Result<(), String> {
        if aggregate.uses == 0 {
            return Ok(());
        }
        let record = json!({
            "id": stat_id(agent, kind, revision),
            "agentId": agent,
            "kind": kind,
            "revision": revision,
            "uses": aggregate.uses,
            "successes": aggregate.successes,
            "failures": aggregate.failures,
            "successRate": aggregate.successes as f64 / aggregate.uses as f64,
            "statVersion": 2,
            "source": "usage_rebuild",
            "createdAt": aggregate.first_used.clone().unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
            "lastUsedAt": aggregate.last_used.clone(),
        });
        state
            .store
            .put("algorithm_stat", &record)
            .ok_or("could not persist rebuilt algorithm statistics")?;
        Ok(())
    };
    for ((agent, kind, revision), aggregate) in &per_agent {
        write(agent, kind, revision, aggregate)?;
        written += 1;
    }
    for ((kind, revision), aggregate) in &global {
        write("global", kind, revision, aggregate)?;
        written += 1;
    }
    let report = json!({
        "usages": rated + unrated,
        "rated": rated,
        "unrated": unrated,
        "skipped": skipped,
        "statistics": written,
    });
    state
        .store
        .audit("algorithm.statistics_rebuilt", report.clone());
    Ok(report)
}

