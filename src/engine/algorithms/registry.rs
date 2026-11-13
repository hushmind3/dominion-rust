//! Algorithm persistence, activation and per-kind resolution.
use super::*;

pub fn seed(state: &AppState) -> Vec<Value> {
    let mut saved = Vec::new();
    let mut active_kinds = state
        .store
        .all("algorithm")
        .into_iter()
        .filter(|item| item["status"].as_str() == Some("active"))
        .filter_map(|item| item["kind"].as_str().map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>();
    for mut algorithm in builtins() {
        // The first builtin of each kind is the explicit default. Later
        // activation never silently promotes another kind or another tool.
        let kind = algorithm["kind"].as_str().unwrap_or("").to_string();
        if !active_kinds.contains(&kind) {
            algorithm["status"] = json!("active");
            algorithm["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            active_kinds.insert(kind);
        }
        let id = algorithm["id"].as_str().unwrap_or("").to_string();
        if state.store.get("algorithm", &id).is_none() {
            if let Some(record) = state.store.put("algorithm", &algorithm) {
                state.bus.record("algorithm", &record);
                saved.push(record);
            }
        } else if let Some(mut existing) = state.store.get("algorithm", &id) {
            // Upgrade records created before executable-kernel metadata.
            let mut changed = false;
            if existing["implementation"].is_null() {
                existing["implementation"] = existing["variant"].clone();
                changed = true;
            }
            if existing["affects"].is_null() {
                existing["affects"] = existing["kind"].clone();
                changed = true;
            }
            if changed {
                if let Some(record) = state.store.put("algorithm", &existing) {
                    state.bus.record("algorithm", &record);
                }
            }
        }
    }
    saved
}

pub fn list(state: &AppState) -> Value {
    seed(state);
    let mut items = state.store.all("algorithm");
    items.sort_by(|a, b| {
        a["kind"]
            .as_str()
            .unwrap_or("")
            .cmp(b["kind"].as_str().unwrap_or(""))
            .then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
            })
    });
    // Observation counters live in `algorithm_stat`. `global` already mirrors
    // the per-agent totals, so prefer it and fall back to summing agent rows.
    let mut globals = std::collections::BTreeMap::new();
    let mut agent_totals: std::collections::BTreeMap<
        String,
        (u64, u64, u64, Option<String>),
    > = std::collections::BTreeMap::new();
    let mut observed_agents: std::collections::BTreeMap<
        String,
        std::collections::BTreeSet<String>,
    > = std::collections::BTreeMap::new();
    for stat in state.store.all("algorithm_stat") {
        let Some(revision) = stat["revision"].as_str() else {
            continue;
        };
        let agent = stat["agentId"].as_str().unwrap_or("");
        if agent == "global" {
            globals.insert(revision.to_string(), stat);
            continue;
        }
        observed_agents
            .entry(revision.to_string())
            .or_default()
            .insert(agent.to_string());
        let entry = agent_totals
            .entry(revision.to_string())
            .or_insert((0, 0, 0, None));
        entry.0 += stat["uses"].as_u64().unwrap_or(0);
        entry.1 += stat["successes"].as_u64().unwrap_or(0);
        entry.2 += stat["failures"].as_u64().unwrap_or(0);
        if let Some(last) = stat["lastUsedAt"].as_str() {
            if entry.3.as_deref().is_none_or(|current| last > current) {
                entry.3 = Some(last.to_string());
            }
        }
    }
    let mut observations: std::collections::BTreeMap<String, Value> =
        std::collections::BTreeMap::new();
    for (revision, stat) in &globals {
        observations.insert(
            revision.clone(),
            json!({
                "uses": stat["uses"],
                "successes": stat["successes"],
                "failures": stat["failures"],
                "successRate": stat["successRate"],
                "lastUsedAt": stat["lastUsedAt"],
                "agents": observed_agents.get(revision).map_or(0, |agents| agents.len()),
            }),
        );
    }
    for (revision, (uses, successes, failures, last_used)) in agent_totals {
        let agents = observed_agents.get(&revision).map_or(0, |agents| agents.len());
        observations.entry(revision).or_insert_with(|| {
            json!({
                "uses": uses,
                "successes": successes,
                "failures": failures,
                "successRate": if uses > 0 { successes as f64 / uses as f64 } else { 0.0 },
                "lastUsedAt": last_used,
                "agents": agents,
            })
        });
    }
    let kinds = KINDS
        .iter()
        .map(|kind| {
            let rows = items
                .iter()
                .filter(|item| item["kind"].as_str() == Some(*kind))
                .map(|item| {
                    let mut row = item.clone();
                    let revision = row["revision"].as_str().unwrap_or("").to_string();
                    if let Some(observed) = observations.get(&revision) {
                        row["stats"] = observed.clone();
                    }
                    row
                })
                .collect::<Vec<_>>();
            json!({
                "kind": kind,
                "label": kind_label(kind),
                "kernels": executable_kernels(kind),
                "active": rows.iter().find(|item| item["status"].as_str() == Some("active")).cloned(),
                "algorithms": rows,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "kinds": kinds,
        // Algorithm activation is per kind. Self-improvement and the tool
        // gateway are harness capabilities, not algorithm kinds, so changing
        // one algorithm cannot disable them or leak into another tool.
        "isolation": {
            "activationScope": "kind",
            "selfImprovementIndependent": true,
            "toolGatewayIndependent": true,
        },
    })
}

pub fn active(state: &AppState, kind: &str) -> Option<Value> {
    seed(state);
    state
        .store
        .all("algorithm")
        .into_iter()
        .find(|item| {
            item["kind"].as_str() == Some(kind)
                && item["status"].as_str() == Some("active")
        })
}

/// The executable kernel chosen for a kind. `variant` is display metadata and
/// must never diverge from the kernel the harness actually runs.
pub fn variant(state: &AppState, kind: &str) -> String {
    active(state, kind)
        .and_then(|item| {
            item["implementation"]
                .as_str()
                .or_else(|| item["variant"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| match kind {
            "strategy.selection" => "estimated-softmax".into(),
            "program.repair" => "completed-preserving".into(),
            "tool.selection" => "read-before-write".into(),
            "verification.predicate" => "three-valued".into(),
            "learning.applicability" => "conservative".into(),
            _ => "balanced".into(),
        })
}

