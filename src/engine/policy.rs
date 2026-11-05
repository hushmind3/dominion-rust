//! H9 versioned policy slots.
//!
//! A policy slot names a replaceable algorithm/parameter set (for example the
//! planning strategy). New revisions are candidates; only an isolated paired
//! experiment can activate them, and activation supersedes the previous active
//! revision. This is deliberately separate from one-off strategy text: the
//! artifact is versioned and rollbackable.
use crate::state::AppState;
use serde_json::{json, Value};

pub const SLOTS: &[&str] = &["planning", "recovery", "execution"];
pub const MIN_EXPERIMENT_UNITS: u64 = 8;

/// Minimum distinct recorded failures before a candidate is proposed.
pub const PROPOSER_MIN_EVIDENCE: usize = 3;

/// Derives planning-policy candidates from failure clusters. Proposals are
/// candidates only: the outer gate in `apply_experiment_result` requires an
/// independent >=8-unit experiment before activation.
/// The optimizer is itself a versioned policy. These fields decide how
/// failures become candidates and how much exploration each search spends.
pub fn default_optimizer() -> Value {
    json!({
        "id": "optimizer-active",
        "revision": "optimizer-v1",
        "status": "active",
        "evidenceSelection": "recent_failures",
        "failureClustering": "by_failure_class",
        "candidateGeneration": "settings_and_probe",
        "candidateDeduplication": "by_failure_class_and_mechanism",
        "explorationPolicy": "balanced",
        "experimentAllocation": {"pairs": MIN_EXPERIMENT_UNITS},
        "stoppingPolicy": "after_min_units",
        "createdAt": chrono::Utc::now().to_rfc3339(),
    })
}

pub fn active_optimizer(state: &AppState) -> Value {
    state
        .store
        .get("optimizer_policy", "optimizer-active")
        .filter(|policy| policy["status"].as_str() == Some("active"))
        .unwrap_or_else(default_optimizer)
}

/// Comparison data for the optimizer meta-loop. Every number comes from
/// recorded search episodes, not from the revision string itself.
pub fn optimizer_report(state: &AppState) -> Value {
    let active = active_optimizer(state);
    let episodes = state.store.all("search_episode");
    let policies = state.store.all("adaptation_policy");
    let optimizer_revisions = state
        .store
        .all("optimizer_policy")
        .into_iter()
        .map(|policy| policy["revision"].as_str().unwrap_or("optimizer-v1").to_string())
        .chain(std::iter::once(
            active["revision"].as_str().unwrap_or("optimizer-v1").to_string(),
        ))
        .collect::<std::collections::BTreeSet<_>>();
    let mut rows = Vec::new();
    for revision in optimizer_revisions {
        let runs = episodes
            .iter()
            .filter(|episode| episode["optimizerRevision"].as_str() == Some(revision.as_str()))
            .collect::<Vec<_>>();
        let wins = runs
            .iter()
            .filter(|episode| episode["selectedPolicy"].as_str().is_some())
            .count() as f64;
        let cost = runs
            .iter()
            .map(|episode| episode["cost"].as_f64().unwrap_or(0.0))
            .sum::<f64>();
        let generated = policies
            .iter()
            .filter(|policy| policy["optimizerRevision"].as_str() == Some(revision.as_str()))
            .count();
        rows.push(json!({
            "revision": revision,
            "searches": runs.len(),
            "selected": wins,
            "generatedCandidates": generated,
            "cost": cost,
            "score": (wins + 1.0) / (runs.len() as f64 + 2.0) - cost * 0.001,
            "active": active["revision"].as_str() == Some(revision.as_str()),
        }));
    }
    json!({
        "active": active["revision"],
        "optimizers": rows,
        "episodes": episodes.len(),
    })
}

pub fn register_optimizer(
    state: &AppState,
    name: &str,
    parameters: &Value,
    rationale: &str,
) -> Option<Value> {
    let id = format!(
        "optimizer-{}",
        crate::engine::memory::stable_id(&[name, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("optimizer_policy", &id) {
        return Some(existing);
    }
    let mut policy = default_optimizer();
    policy["id"] = json!(id);
    policy["revision"] = json!(crate::engine::memory::stable_id(&[&id, &parameters.to_string()]));
    policy["name"] = json!(name);
    policy["status"] = json!("candidate");
    policy["rationale"] = json!(rationale);
    if let Some(object) = parameters.as_object() {
        for (key, value) in object {
            policy[key] = value.clone();
        }
    }
    state.store.put("optimizer_policy", &policy)
}

/// Chooses the optimizer with the best observed candidate yield per cost.
/// Only real search episodes count; an optimizerRevision string alone is not
/// evidence.
pub fn meta_improve(state: &AppState) -> Option<Value> {
    use std::collections::BTreeMap;
    // Keep one structurally different optimizer available so selection is a
    // real comparison, not a single incumbent with no challenger.
    let mut optimizers = state.store.all("optimizer_policy");
    if optimizers.len() < 2 {
        register_optimizer(
            state,
            "counterexample-first",
            &json!({
                "failureClustering": "by_failure_class_and_recovery",
                "candidateGeneration": "counterexample_first",
                "explorationPolicy": "wide",
                "stoppingPolicy": "after_min_units",
            }),
            "반례를 먼저 찾고 더 넓게 탐색하는 개선 정책",
        );
        optimizers = state.store.all("optimizer_policy");
    }
    let episodes = state.store.all("search_episode");
    let active = active_optimizer(state);
    let active_revision = active["revision"].as_str().unwrap_or("optimizer-v1").to_string();
    let active_episodes = episodes.iter().filter(|episode| {
        episode["optimizerRevision"].as_str() == Some(active_revision.as_str())
    }).count();
    let has_untried = optimizers.iter().any(|optimizer| {
        optimizer["revision"].as_str() != Some(active_revision.as_str())
            && !episodes.iter().any(|episode| {
                episode["optimizerRevision"] == optimizer["revision"]
            })
    });
    if active_episodes > 0 && has_untried {
        if let Some(challenger) = optimizers.iter().find(|optimizer| {
            optimizer["revision"].as_str() != Some(active_revision.as_str())
                && !episodes.iter().any(|episode| {
                    episode["optimizerRevision"] == optimizer["revision"]
                })
        }) {
            for mut policy in state.store.all("optimizer_policy") {
                if policy["status"].as_str() == Some("active") {
                    policy["status"] = json!("superseded");
                    policy["supersededBy"] = challenger["id"].clone();
                    state.store.put("optimizer_policy", &policy);
                }
            }
            let mut activated = challenger.clone();
            activated["status"] = json!("active");
            activated["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            activated["activationReason"] = json!("online exploration round");
            let saved = state.store.put("optimizer_policy", &activated)?;
            state.store.audit(
                "optimizer.exploration",
                json!({"from": active_revision, "to": activated["revision"]}),
            );
            return Some(saved);
        }
    }
    let mut stats: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new();
    for episode in state.store.all("search_episode") {
        let revision = episode["optimizerRevision"]
            .as_str()
            .unwrap_or("optimizer-v1")
            .to_string();
        let entry = stats.entry(revision).or_insert((0.0, 0.0, 0.0));
        entry.0 += 1.0;
        if episode["selectedPolicy"].as_str().is_some() {
            entry.1 += 1.0;
        }
        entry.2 += episode["cost"].as_f64().unwrap_or(0.0);
    }
    let incumbent = active_optimizer(state);
    let incumbent_revision = incumbent["revision"].as_str().unwrap_or("optimizer-v1");
    let mut best = (incumbent_revision.to_string(), -1.0);
    for (revision, (runs, wins, cost)) in &stats {
        if *runs < 1.0 {
            continue;
        }
        // Laplace smoothing keeps a single lucky search from dominating.
        let score = (wins + 1.0) / (runs + 2.0) - cost * 0.001;
        if score > best.1 {
            best = (revision.clone(), score);
        }
    }
    if best.0 == incumbent_revision {
        return None;
    }
    let candidate = state.store.all("optimizer_policy").into_iter().find(|policy| {
        policy["revision"].as_str() == Some(best.0.as_str())
    })?;
    for mut policy in state.store.all("optimizer_policy") {
        if policy["status"].as_str() == Some("active") {
            policy["status"] = json!("superseded");
            policy["supersededBy"] = candidate["id"].clone();
            state.store.put("optimizer_policy", &policy);
        }
    }
    let mut activated = candidate;
    activated["status"] = json!("active");
    activated["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("optimizer_policy", &activated)?;
    state.store.audit(
        "optimizer.meta_improved",
        json!({
            "from": incumbent_revision,
            "to": best.0,
            "score": best.1,
            "episodes": stats,
        }),
    );
    state.bus.record("optimizer_policy", &saved);
    Some(saved)
}

pub fn propose_candidates(state: &AppState) -> Vec<Value> {
    use std::collections::BTreeMap;
    let mut clusters: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    let mut executions = state.store.all("execution");
    executions.sort_by(|a, b| {
        b["startedAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["startedAt"].as_str().unwrap_or(""))
    });
    for execution in executions.iter().take(500) {
        let Some(class) = execution["failureClass"].as_str() else {
            continue;
        };
        if matches!(class, "cancelled") {
            continue;
        }
        clusters.entry(class).or_default().push(json!({
            "executionId": execution["id"],
            "taskOutcome": execution["taskOutcome"],
            "recoveryHint": execution["recoveryHint"],
        }));
    }
    let optimizer = active_optimizer(state);
    let optimizer_revision = optimizer["revision"].as_str().unwrap_or("optimizer-v1").to_string();
    let variant_limit = 3;
    let mut created = Vec::new();
    for (class, evidence) in clusters.iter() {
        let class = *class;
        if evidence.len() < PROPOSER_MIN_EVIDENCE {
            continue;
        }
        // Search space: the same failure cluster can be addressed by planning
        // always, planning only long tasks, or not planning at all. Each
        // revision gets its own experiment; the search then selects the best.
        let variants: Vec<(&str, Value, &str)> = match class {
            "verification_failed" => vec![
                (
                    "plan-before-acting",
                    json!({"outline": true}),
                    "검증 실패가 반복되어 행동 전 계획을 세우는 정책을 제안합니다.",
                ),
                (
                    "plan-only-long-tasks",
                    json!({"outline": true, "minChars": 200}),
                    "짧은 작업은 건너뛰고 긴 작업만 계획하는 정책을 제안합니다.",
                ),
                (
                    "skip-extra-planning",
                    json!({"outline": false}),
                    "계획 호출 자체를 줄이는 정책을 제안합니다.",
                ),
            ],
            "limit_exceeded" | "model_error" => vec![
                (
                    "skip-extra-planning",
                    json!({"outline": false}),
                    "예산/모델 실패가 반복되어 추가 계획 호출을 생략하는 정책을 제안합니다.",
                ),
                (
                    "plan-only-long-tasks",
                    json!({"outline": true, "minChars": 400}),
                    "긴 작업에만 계획을 사용해 호출 비용을 줄이는 정책을 제안합니다.",
                ),
            ],
            _ => continue,
        };
        let evidence_ids = evidence
            .iter()
            .filter_map(|item| item["executionId"].as_str())
            .take(8)
            .collect::<Vec<_>>();
        let search_id = format!(
            "policy-search-{}",
            crate::engine::memory::stable_id(&[
                "planning",
                class,
                &evidence_ids.join(",")
            ])
        );
        for (name, parameters, base_rationale) in variants.into_iter().take(variant_limit) {
            let rationale = format!(
                "{base_rationale} (근거 {}건: {})",
                evidence.len(),
                evidence_ids.join(", ")
            );
            let Ok(candidate) = register(state, "planning", name, &parameters, &rationale) else {
                continue;
            };
            let policy_id = candidate["id"].as_str().unwrap_or("");
            let mut tagged = candidate.clone();
            tagged["searchId"] = json!(search_id);
            tagged["optimizerRevision"] = json!(optimizer_revision);
            if let Some(saved) = state.store.put("adaptation_policy", &tagged) {
                state.bus.record("adaptation_policy", &saved);
            }
            if let Some((request, policy, is_new)) =
                enqueue_policy_request(state, policy_id, "planning_evaluation", Some(&search_id))
            {
                if is_new {
                    created.push(json!({"candidate": policy, "request": request}));
                }
            }
        }

        // R5: generate actual executable control-flow candidates from the
        // same evidence. They differ structurally, not by a settings toggle.
        let base = format!(
            "opt-{}-{}",
            class,
            crate::engine::memory::stable_id(&[class, &evidence_ids.join(",")])
        );
        let templates = vec![
            (
                "probe-before-acting",
                "observe workspace before the next decision",
                json!([
                    {"type":"call_tool","id":"probe_workspace","tool":"workspace_list",
                     "inputs":{"path":""},"saveAs":"probe"},
                    {"type":"observe","id":"probe_observation"},
                    {"type":"model_decision","id":"probe_decision"}
                ]),
            ),
            (
                "verify-before-acting",
                "probe then verify the observed workspace before deciding",
                json!([
                    {"type":"call_tool","id":"probe_workspace","tool":"workspace_list",
                     "inputs":{"path":""},"saveAs":"probe"},
                    {"type":"verify","id":"probe_verify","checks":[
                        {"predicate":{"type":"exists","from":"probe","path":"/entries"}}
                    ]},
                    {"type":"model_decision","id":"probe_decision"}
                ]),
            ),
            (
                "parallel-probe",
                "run independent workspace probes concurrently before deciding",
                json!([
                    {"type":"parallel","id":"probe_parallel","nodes":[
                        {"type":"call_tool","id":"probe_root","tool":"workspace_list",
                         "inputs":{"path":""},"saveAs":"probe_root"},
                        {"type":"call_tool","id":"probe_dot","tool":"workspace_list",
                         "inputs":{"path":"."},"saveAs":"probe_dot"}
                    ]},
                    {"type":"observe","id":"probe_observation"},
                    {"type":"model_decision","id":"probe_decision"}
                ]),
            ),
        ];
        for (suffix, mechanism, nodes) in templates {
            let program = json!({
                "revision": format!("{base}-{suffix}"),
                "nodes": nodes,
            });
            if crate::engine::program::validate(&program).is_err() {
                continue;
            }
            let parameters = json!({"program": program});
            let name = format!("{suffix}-{class}");
            let Ok(mut candidate) = register(
                state,
                "execution",
                &name,
                &parameters,
                &format!(
                    "{} 실패 {}건에서 {mechanism} 프로그램을 생성했습니다.",
                    class,
                    evidence.len()
                ),
            ) else {
                continue;
            };
            let optimization_id = format!(
                "optimization-{}",
                crate::engine::memory::stable_id(&[class, suffix, &evidence_ids.join(",")])
            );
            let optimization = json!({
                "id": optimization_id,
                "parentRevision": "baseline",
                "targetComponent": "execution_program",
                "problemEvidence": evidence_ids,
                "mechanism": mechanism,
                "changedPolicy": parameters,
                "predictedBenefit": "fewer blind retries and earlier observation",
                "applicability": {"failureClass": class},
                "testRequirements": ["same model", "same fixture", "paired budget"],
                "estimatedCost": 0.1,
                "status": "proposed",
                "executable": true,
                "candidateId": candidate["id"],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            });
            state.store.put("optimization_candidate", &optimization);
            let search_id = format!(
                "execution-search-{}",
                crate::engine::memory::stable_id(&[class, suffix, &evidence_ids.join(",")])
            );
            candidate["searchId"] = json!(search_id);
            candidate["optimizerRevision"] = json!(optimizer_revision);
            let policy_id = candidate["id"].as_str().unwrap_or("").to_string();
            if let Some(saved) = state.store.put("adaptation_policy", &candidate) {
                state.bus.record("adaptation_policy", &saved);
            }
            if let Some((request, policy, is_new)) = enqueue_policy_request(
                state, &policy_id, "planning_evaluation", Some(&search_id),
            ) {
                if is_new {
                    created.push(json!({"candidate": policy, "request": request}));
                }
            }
        }
        // Record non-Program ideas separately. They are not activated until a
        // typed representation exists.
        let context_candidate = json!({
            "id": format!(
                "optimization-{}",
                crate::engine::memory::stable_id(&[class, "context-selection", &evidence_ids.join(",")])
            ),
            "parentRevision": "current",
            "targetComponent": "context_selection",
            "problemEvidence": evidence_ids,
            "mechanism": "prioritize failure cases and conditional methods for this failure class",
            "changedPolicy": {"preferFailureClass": class},
            "predictedBenefit": "less irrelevant context at the next decision",
            "applicability": {"failureClass": class},
            "testRequirements": ["requires typed context policy"],
            "estimatedCost": 0.0,
            "status": "proposed",
            "executable": false,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        });
        state.store.put("optimization_candidate", &context_candidate);
        for (component, mechanism) in [
            (
                "tool_selection",
                "prefer read/list probes over retrying the failed write",
            ),
            (
                "memory_retrieval",
                "retrieve counterexamples only when their environment differs",
            ),
            (
                "verification",
                "verify the produced artifact before claiming completion",
            ),
        ] {
            let record = json!({
                "id": format!(
                    "optimization-{}",
                    crate::engine::memory::stable_id(&[class, component, &evidence_ids.join(",")])
                ),
                "parentRevision": "current",
                "targetComponent": component,
                "problemEvidence": evidence_ids,
                "mechanism": mechanism,
                "changedPolicy": {"failureClass": class},
                "predictedBenefit": "fewer repeated failures on this class",
                "applicability": {"failureClass": class},
                "testRequirements": [format!("typed {component} policy")],
                "estimatedCost": 0.0,
                "status": "proposed",
                "executable": false,
                "createdAt": chrono::Utc::now().to_rfc3339(),
            });
            state.store.put("optimization_candidate", &record);
        }
    }
    created
}

/// Freezes the experiment contract before any trial runs. Runners can report
/// their own manifest, but completion is compared against this server copy.
pub fn frozen_manifest(
    request_id: &str,
    fixture: &str,
    candidate_revision: &str,
    units: u64,
) -> (Value, String) {
    use sha2::Digest;
    let manifest = json!({
        "experimentId": request_id,
        "candidateRevision": candidate_revision,
        "baselineRevision": "baseline",
        "modelRevision": "runner-reported",
        "toolRevisions": [],
        "fixtures": [fixture],
        "environmentSeeds": [],
        "pairAssignments": (0..units).collect::<Vec<_>>(),
        "evaluationRules": {
            "primary": "task success (ITT)",
            "secondary": ["plan quality", "cost", "latency"],
            "missingOutcome": "failure",
        },
        "totalBudget": {"pairs": units},
        "stoppingRules": {"minIndependentUnits": units},
    });
    let hash = format!("{:x}", sha2::Sha256::digest(manifest.to_string().as_bytes()));
    (manifest, hash)
}

/// Creates one durable experiment request per policy revision. Idempotent.
pub fn enqueue_policy_request(
    state: &AppState,
    policy_id: &str,
    fixture: &str,
    search_id: Option<&str>,
) -> Option<(Value, Value, bool)> {
    let policy = state.store.get("adaptation_policy", policy_id)?;
    let revision = policy["revision"].as_str().unwrap_or("");
    let request_id = format!(
        "experiment-request-policy-{}",
        crate::engine::memory::stable_id(&[policy_id, revision])
    );
    if let Some(existing) = state.store.get("experiment_request", &request_id) {
        return Some((existing, policy, false));
    }
    let (manifest, manifest_hash) = frozen_manifest(
        &request_id, fixture, revision, MIN_EXPERIMENT_UNITS,
    );
    let request = json!({
        "id": request_id,
        "kind": "policy",
        "policyId": policy_id,
        "slot": policy["slot"],
        "revision": revision,
        "status": "queued",
        "fixture": fixture,
        "searchId": search_id,
        "minIndependentUnits": MIN_EXPERIMENT_UNITS,
        "frozenManifest": manifest,
        "frozenManifestHash": manifest_hash,
        "externalRunner": true,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state.store.put("experiment_request", &request)?;
    state.bus.record("experiment_request", &saved);
    state.store.audit(
        "policy.candidate_queued",
        json!({"policyId": policy_id, "requestId": saved["id"], "searchId": search_id}),
    );
    Some((saved, policy, true))
}

/// Recovery retry budgets from the `recovery` slot. Values are clamped so a
/// policy can tune, but not remove, execution safety limits.
pub fn recovery_limits(parameters: &Value) -> (usize, usize) {
    let acceptance = parameters["maxAcceptanceRetries"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    let repairs = parameters["maxProcedureRepairs"]
        .as_u64()
        .unwrap_or(2)
        .min(4) as usize;
    (acceptance, repairs)
}

/// Decides whether a planning policy asks for a plan. `None` means the policy
/// does not override the built-in heuristic.
pub fn outline_decision(parameters: &Value, input_chars: usize) -> Option<bool> {
    let outline = parameters["outline"].as_bool()?;
    if !outline {
        return Some(false);
    }
    match parameters["minChars"].as_u64() {
        Some(minimum) => Some(input_chars as u64 >= minimum),
        None => Some(true),
    }
}

/// Selects the best evaluated revision in a search group using the more
/// conservative of the two evaluator deltas. Ineligible or inferior revisions
/// cannot stay active.
pub fn select_best(state: &AppState, search_id: &str) -> Option<Value> {
    use crate::engine::plan_quality::MIN_SEMANTIC_DELTA;
    let policies = state
        .store
        .all("adaptation_policy")
        .into_iter()
        .filter(|policy| policy["searchId"].as_str() == Some(search_id))
        .collect::<Vec<_>>();
    if policies.is_empty() {
        return None;
    }
    let score = |policy: &Value| -> Option<f64> {
        let experiment = &policy["lastExperiment"];
        let units = experiment["independentUnits"].as_u64().unwrap_or(0);
        let verdict = experiment["verdict"].as_str().unwrap_or("");
        let main = experiment["semanticDelta"].as_f64().unwrap_or(0.0);
        let independent = experiment["independentSemanticDelta"]
            .as_f64()
            .unwrap_or(0.0);
        (units >= MIN_EXPERIMENT_UNITS
            && matches!(verdict, "improved" | "preliminary_support")
            && main >= MIN_SEMANTIC_DELTA
            && independent >= MIN_SEMANTIC_DELTA)
            .then(|| main.min(independent))
    };
    let best = policies
        .iter()
        .filter_map(|policy| {
            score(policy).map(|score| (policy["id"].as_str().unwrap_or("").to_string(), score))
        })
        .max_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.cmp(&a.0))
        });
    let policy_count = policies.len();
    let optimizer_revision = policies
        .iter()
        .find_map(|policy| policy["optimizerRevision"].as_str())
        .unwrap_or("optimizer-v1")
        .to_string();
    let selected_policy_id = best.as_ref().map(|(id, _)| id.clone());
    let mut candidates = Vec::new();
    for mut policy in policies {
        let id = policy["id"].as_str().unwrap_or("").to_string();
        let candidate_score = score(&policy);
        if best.as_ref().is_some_and(|(best_id, _)| best_id == &id) {
            policy["status"] = json!("active");
            policy["selectedBySearch"] = json!(search_id);
        } else if policy["status"].as_str() == Some("active") {
            policy["status"] = json!("superseded");
            policy["supersededBy"] = json!(best.as_ref().map(|(id, _)| id.clone()));
        }
        let status = policy["status"].clone();
        if let Some(saved) = state.store.put("adaptation_policy", &policy) {
            state.bus.record("adaptation_policy", &saved);
        }
        candidates.push(json!({
            "policyId": id,
            "score": candidate_score,
            "status": status,
        }));
    }
    let record = json!({
        "id": search_id,
        "slot": "planning",
        "selectedPolicyId": best.as_ref().map(|(id, _)| id.clone()),
        "selectedScore": best.as_ref().map(|(_, score)| score),
        "candidates": candidates,
        "selectedAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state.store.put("policy_search", &record);
    if let Some(saved) = &saved {
        state.bus.record("policy_search", saved);
        state.store.audit(
            "policy.search_selected",
            json!({"searchId": search_id, "selected": record["selectedPolicyId"]}),
        );
    }
    state.store.put(
        "search_episode",
        &json!({
            "id": format!("search-{}", crate::engine::memory::stable_id(&[search_id, &optimizer_revision])),
            "searchId": search_id,
            "optimizerRevision": optimizer_revision,
            "generatedCandidates": policy_count,
            "selectedPolicy": selected_policy_id,
            "cost": policy_count as f64 * 0.1,
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    saved
}

pub fn list(state: &AppState) -> Vec<Value> {
    let mut items = state.store.all("adaptation_policy");
    items.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .unwrap_or("")
            .cmp(a["createdAt"].as_str().unwrap_or(""))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    items
}

pub fn active(state: &AppState, slot: &str) -> Option<Value> {
    state
        .store
        .all("adaptation_policy")
        .into_iter()
        .find(|policy| {
            policy["slot"].as_str() == Some(slot) && policy["status"].as_str() == Some("active")
        })
}

#[allow(dead_code)] // used by tests and by future slots that only need parameters
pub fn active_parameters(state: &AppState, slot: &str) -> Value {
    active(state, slot)
        .map(|policy| policy["parameters"].clone())
        .unwrap_or_else(|| json!({}))
}

/// Registers a candidate revision. Identical name+parameters is idempotent.
pub fn register(
    state: &AppState,
    slot: &str,
    name: &str,
    parameters: &Value,
    rationale: &str,
) -> Result<Value, String> {
    if !SLOTS.contains(&slot) {
        return Err(format!("지원하지 않는 정책 슬롯입니다: {slot}"));
    }
    if name.trim().is_empty() {
        return Err("정책 이름이 필요합니다.".into());
    }
    if !parameters.is_object() {
        return Err("정책 parameters는 객체여야 합니다.".into());
    }
    let id = format!(
        "policy-{}",
        crate::engine::memory::stable_id(&[slot, name, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("adaptation_policy", &id) {
        return Ok(existing);
    }
    let revision = crate::engine::memory::stable_id(&[&id, &parameters.to_string()]);
    let record = json!({
        "id": id,
        "slot": slot,
        "name": name,
        "revision": revision,
        "status": "candidate",
        "evidenceGrade": "E0",
        "parameters": parameters,
        "rationale": rationale,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let saved = state
        .store
        .put("adaptation_policy", &record)
        .ok_or("정책 후보를 저장할 수 없습니다.")?;
    state.bus.record("adaptation_policy", &saved);
    state.store.audit(
        "policy.candidate",
        json!({"id": saved["id"], "slot": slot, "name": name}),
    );
    Ok(saved)
}

/// Benchmark-sandbox only: force a policy active after an isolated engine is
/// disposable. Production activation still goes through the experiment gate.
pub fn force_activate(state: &AppState, policy_id: &str) -> Option<Value> {
    let mut policy = state.store.get("adaptation_policy", policy_id)?;
    let slot = policy["slot"].as_str().unwrap_or("").to_string();
    for mut other in state.store.all("adaptation_policy") {
        if other["id"].as_str() != Some(policy_id)
            && other["slot"].as_str() == Some(slot.as_str())
            && other["status"].as_str() == Some("active")
        {
            other["status"] = json!("superseded");
            other["supersededBy"] = policy["id"].clone();
            if let Some(saved) = state.store.put("adaptation_policy", &other) {
                state.bus.record("adaptation_policy", &saved);
            }
        }
    }
    policy["status"] = json!("active");
    policy["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("adaptation_policy", &policy)?;
    state.bus.record("adaptation_policy", &saved);
    Some(saved)
}

pub fn deactivate_slot(state: &AppState, slot: &str) -> usize {
    let mut count = 0;
    for mut policy in state.store.all("adaptation_policy") {
        if policy["slot"].as_str() == Some(slot)
            && policy["status"].as_str() == Some("active")
        {
            policy["status"] = json!("inactive");
            if let Some(saved) = state.store.put("adaptation_policy", &policy) {
                state.bus.record("adaptation_policy", &saved);
                count += 1;
            }
        }
    }
    count
}

/// Activates or rolls back a candidate based on an isolated experiment.
pub fn experiment_result_records(
    state: &AppState,
    policy_id: &str,
    experiment: &Value,
) -> Result<Vec<(String, Value)>, String> {
    let Some(mut policy) = state.store.get("adaptation_policy", policy_id) else {
        return Err("정책 후보를 찾을 수 없습니다.".into());
    };
    let units = experiment["independentUnits"].as_u64().unwrap_or(0);
    if units == 0 {
        return Err("독립 실험 표본이 없습니다.".into());
    }
    let verdict = experiment["verdict"].as_str().unwrap_or("inconclusive");
    let delta = experiment["delta"].as_f64().unwrap_or(0.0);
    let semantic_delta = experiment["semanticDelta"].as_f64();
    let independent_semantic_delta = experiment["independentSemanticDelta"].as_f64();
    let semantic_metric = experiment["semanticMetric"]
        .as_str()
        .unwrap_or(crate::engine::plan_quality::METRIC_VERSION);
    let experiment_id = experiment["experimentId"].as_str().unwrap_or("");
    let revision = crate::engine::memory::stable_id(&[
        experiment_id,
        experiment["oracleVersion"].as_str().unwrap_or(""),
        &units.to_string(),
        &format!("{delta:.6}"),
    ]);
    policy["lastExperiment"] = json!({
        "experimentId": experiment_id,
        "verdict": verdict,
        "delta": delta,
        "independentUnits": units,
        "semanticDelta": semantic_delta,
        "independentSemanticDelta": independent_semantic_delta,
        "semanticMetric": semantic_metric,
        "oracleVersion": experiment["oracleVersion"],
        "evaluatedAt": chrono::Utc::now().to_rfc3339(),
    });

    let mut records: Vec<(String, Value)> = Vec::new();
    if units >= MIN_EXPERIMENT_UNITS && verdict == "regressed" {
        policy["status"] = json!("rolled_back");
        policy["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
        policy["experimentRevision"] = json!(revision);
    } else if units >= MIN_EXPERIMENT_UNITS
        && matches!(verdict, "improved" | "preliminary_support")
        && delta > 0.0
    {
        let slot = policy["slot"].as_str().unwrap_or("").to_string();
        // Planning policies must also improve the semantic plan metric. A
        // file-success win with no better plan is not enough for activation.
        // Every slot uses the same independent-experiment gate: both semantic
        // evaluators must agree that the revision is not worse.
        let main_ok = semantic_delta
            .is_some_and(|value| value >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA);
        let independent_ok = independent_semantic_delta.is_some_and(|value| {
            value >= crate::engine::plan_quality::MIN_SEMANTIC_DELTA
        });
        let semantic_ok = main_ok && independent_ok;
        if !semantic_ok {
            policy["requiresExperiment"] = json!(true);
            policy["semanticVerified"] = json!(false);
            policy["reason"] = json!(format!(
                "의미 계획 개선폭이 기준({})보다 낮아 활성화하지 않았습니다.",
                crate::engine::plan_quality::MIN_SEMANTIC_DELTA
            ));
            return Ok(vec![("adaptation_policy".to_string(), policy)]);
        }
        policy["semanticVerified"] = json!(true);
        for mut other in state.store.all("adaptation_policy") {
            if other["id"].as_str() != Some(policy_id)
                && other["slot"].as_str() == Some(slot.as_str())
                && other["status"].as_str() == Some("active")
            {
                other["status"] = json!("superseded");
                other["supersededBy"] = policy["id"].clone();
                other["supersededAt"] = json!(chrono::Utc::now().to_rfc3339());
                records.push(("adaptation_policy".to_string(), other));
            }
        }
        policy["status"] = json!("active");
        policy["evidenceGrade"] = json!("E2");
        policy["experimentRevision"] = json!(revision);
        policy["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    }
    records.push(("adaptation_policy".to_string(), policy));
    Ok(records)
}
pub fn apply_experiment_result(
    state: &AppState,
    policy_id: &str,
    experiment: &Value,
) -> Result<Value, String> {
    let records = experiment_result_records(state, policy_id, experiment)?;
    let saved = state.store.transaction(&records)?;
    for record in &saved {
        state.bus.record("adaptation_policy", record);
    }
    let policy = saved
        .iter()
        .find(|record| record["id"].as_str() == Some(policy_id))
        .cloned()
        .ok_or("정책 결과를 저장할 수 없습니다.")?;
    state.store.audit(
        "policy.experiment_result",
        json!({
            "id": policy_id,
            "experimentId": experiment["experimentId"],
            "verdict": experiment["verdict"],
            "units": experiment["independentUnits"],
            "status": policy["status"],
        }),
    );
    Ok(policy)
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn failure(state: &AppState, id: &str, class: &str) {
        state.store.put(
            "execution",
            &json!({
                "id":id,"status":"failed","taskOutcome":"failed",
                "failureClass":class,"recoveryHint":"hint",
                "sessionId":"s","startedAt":format!("2026-11-05T10:00:0{id}Z")
            }),
        );
    }

    #[test]
    fn proposer_rejects_clusters_below_the_evidence_threshold() {
        let (state, dir) = state();
        failure(&state, "1", "verification_failed");
        failure(&state, "2", "verification_failed");
        assert!(propose_candidates(&state).is_empty());
        assert!(list(&state).is_empty());
        assert!(state.store.all("experiment_request").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn proposer_creates_one_candidate_and_policy_experiment_request() {
        let (state, dir) = state();
        for id in 1..=3 {
            failure(&state, &id.to_string(), "verification_failed");
        }
        let created = propose_candidates(&state);
        assert!(created.len() >= 3, "search should propose multiple revisions");
        let planning = created
            .iter()
            .filter(|item| {
                item["request"]["searchId"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("policy-search"))
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(planning.len(), 3, "three planning revisions");
        let search_ids = planning
            .iter()
            .filter_map(|item| item["request"]["searchId"].as_str().map(str::to_string))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(search_ids.len(), 1, "one search group per failure cluster");
        let mut variants = planning
            .iter()
            .map(|item| item["candidate"]["parameters"].clone())
            .collect::<Vec<_>>();
        variants.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        assert!(variants.iter().any(|value| value["outline"] == false));
        assert!(
            variants
                .iter()
                .any(|value| value["outline"] == true && value["minChars"].is_u64()),
            "min-length threshold variant missing: {variants:?}"
        );
        for item in &planning {
            assert_eq!(item["candidate"]["status"], "candidate");
            assert_eq!(item["request"]["kind"], "policy");
            assert_eq!(item["request"]["status"], "queued");
            assert_eq!(item["request"]["minIndependentUnits"], MIN_EXPERIMENT_UNITS);
            assert!(item["candidate"]["rationale"]
                .as_str()
                .is_some_and(|text| text.contains("근거 3건")));
        }
        // Idempotent: a second pass creates nothing new.
        assert!(propose_candidates(&state).is_empty());
        assert!(state.store.all("adaptation_policy").len() >= 6);
        assert!(state.store.all("experiment_request").len() >= 6);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn proposed_candidate_stays_inactive_until_eight_units_and_rolls_back() {
        let (state, dir) = state();
        for id in 1..=3 {
            failure(&state, &id.to_string(), "limit_exceeded");
        }
        let created = propose_candidates(&state);
        let policy_id = created[0]["candidate"]["id"].as_str().unwrap().to_string();
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"small","verdict":"improved","delta":0.5,"independentUnits":4,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &small).unwrap();
        assert_eq!(record["status"], "candidate", "underpowered promotion");

        let powered = json!({"experimentId":"powered","verdict":"preliminary_support","delta":0.4,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, &policy_id, &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let other = register(&state, "planning", "other", &json!({"outline": true}), "other").unwrap();
        let regressed = json!({"experimentId":"bad","verdict":"regressed","delta":-0.3,"independentUnits":8,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, other["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert_eq!(active(&state, "planning").unwrap()["id"], policy_id.as_str());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn candidates_need_a_powered_experiment_before_activation() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "outline-off",
            &json!({"outline": false}),
            "skip planning for short tasks",
        )
        .unwrap();
        assert_eq!(candidate["status"], "candidate");
        assert!(active(&state, "planning").is_none());

        let small = json!({"experimentId":"e1","verdict":"improved","delta":0.5,"independentUnits":2,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &small).unwrap();
        assert_eq!(record["status"], "candidate");

        let powered = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.2,"independentUnits":10,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, candidate["id"].as_str().unwrap(), &powered).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["evidenceGrade"], "E2");
        assert_eq!(active_parameters(&state, "planning")["outline"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn activation_supersedes_the_previous_revision_and_regression_rolls_back() {
        let (state, dir) = state();
        let first = register(&state, "planning", "a", &json!({"outline": true}), "a").unwrap();
        let powered = json!({"experimentId":"e1","verdict":"improved","delta":0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, first["id"].as_str().unwrap(), &powered).unwrap();
        let second = register(&state, "planning", "b", &json!({"outline": false}), "b").unwrap();
        let powered2 = json!({"experimentId":"e2","verdict":"preliminary_support","delta":0.4,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        apply_experiment_result(&state, second["id"].as_str().unwrap(), &powered2).unwrap();

        let first = state.store.get("adaptation_policy", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(first["status"], "superseded");
        assert_eq!(active(&state, "planning").unwrap()["id"], second["id"]);
        assert_eq!(active_parameters(&state, "planning")["outline"], false);

        let regressed = json!({"experimentId":"e3","verdict":"regressed","delta":-0.3,"independentUnits":9,"semanticDelta":0.25,"independentSemanticDelta":0.2,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, second["id"].as_str().unwrap(), &regressed).unwrap();
        assert_eq!(record["status"], "rolled_back");
        assert!(active(&state, "planning").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn planning_activation_requires_both_units_and_semantic_improvement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "semantic-gate",
            &json!({"outline": true}),
            "semantic gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();

        // Enough units, file success, but no semantic plan improvement.
        let no_semantic = json!({"experimentId":"no-semantic","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.0,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &no_semantic).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        // Semantic improvement alone without enough units is not enough.
        let few_units = json!({"experimentId":"few","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &few_units).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");

        // Both gates pass.
        let both = json!({"experimentId":"both","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &both).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["semanticVerified"], true);
        assert_eq!(record["lastExperiment"]["semanticDelta"], 0.2);
        assert_eq!(
            record["lastExperiment"]["semanticMetric"],
            crate::engine::plan_quality::METRIC_VERSION
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn planning_activation_rejects_an_independent_evaluator_disagreement() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "planning",
            "independent-gate",
            &json!({"outline": true}),
            "independent gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        // Primary evaluator improves but the independent evaluator does not.
        let disagree = json!({"experimentId":"disagree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.0,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &disagree).unwrap();
        assert_eq!(record["status"], "candidate", "{record}");
        assert_eq!(record["semanticVerified"], false);

        let agree = json!({"experimentId":"agree","verdict":"preliminary_support","delta":0.5,"semanticDelta":0.4,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &agree).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(record["lastExperiment"]["independentSemanticDelta"], 0.2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn outline_thresholds_control_planning() {
        assert_eq!(
            outline_decision(&json!({"outline": true}), 10),
            Some(true)
        );
        assert_eq!(
            outline_decision(&json!({"outline":false}), 10_000),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 50),
            Some(false)
        );
        assert_eq!(
            outline_decision(&json!({"outline":true,"minChars":100}), 150),
            Some(true)
        );
        assert_eq!(outline_decision(&json!({}), 10), None);
    }

    #[test]
    fn search_selection_keeps_the_best_independent_score_only() {
        let (state, dir) = state();
        let search = "search-1";
        let make = |id: &str, main: f64, independent: f64, status: &str| {
            let mut policy = register(
                &state,
                "planning",
                id,
                &json!({"outline":true}),
                "selection test",
            )
            .unwrap();
            policy["searchId"] = json!(search);
            policy["status"] = json!(status);
            policy["lastExperiment"] = json!({
                "independentUnits":10,
                "verdict":"preliminary_support",
                "semanticDelta":main,
                "independentSemanticDelta":independent,
            });
            state.store.put("adaptation_policy", &policy);
            policy
        };
        let best = make("best", 0.4, 0.35, "active");
        let weaker = make("weaker", 0.4, 0.10, "active");
        let rejected = make("rejected", 0.4, 0.0, "candidate");
        let selected = select_best(&state, search).unwrap();
        assert_eq!(selected["selectedPolicyId"], best["id"]);
        assert_eq!(selected["selectedScore"], 0.35);
        assert_eq!(
            state.store.get("adaptation_policy", best["id"].as_str().unwrap()).unwrap()["status"],
            "active"
        );
        assert_eq!(
            state.store.get("adaptation_policy", weaker["id"].as_str().unwrap()).unwrap()["status"],
            "superseded"
        );
        assert_eq!(
            state.store.get("adaptation_policy", rejected["id"].as_str().unwrap()).unwrap()["status"],
            "candidate"
        );
        let searches = state.store.all("policy_search");
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0]["selectedPolicyId"], best["id"]);
        assert_eq!(searches[0]["candidates"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recovery_limits_are_bounded_and_configurable() {
        assert_eq!(recovery_limits(&json!({})), (2, 2));
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":1,"maxProcedureRepairs":0})),
            (1, 0)
        );
        // A policy can tune but not remove safety bounds entirely.
        assert_eq!(
            recovery_limits(&json!({"maxAcceptanceRetries":99,"maxProcedureRepairs":99})),
            (4, 4)
        );
    }

    #[test]
    fn a_recovery_policy_uses_the_same_experiment_gate() {
        let (state, dir) = state();
        let candidate = register(
            &state,
            "recovery",
            "fewer-retries",
            &json!({"maxAcceptanceRetries":0,"maxProcedureRepairs":0}),
            "recovery gate",
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        let underpowered = json!({"experimentId":"r1","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":4,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &underpowered).unwrap();
        assert_eq!(record["status"], "candidate");

        let staged = json!({"experimentId":"r2","verdict":"preliminary_support","delta":0.2,"semanticDelta":0.2,"independentSemanticDelta":0.2,"independentUnits":8,"oracleVersion":"v1"});
        let record = apply_experiment_result(&state, id, &staged).unwrap();
        assert_eq!(record["status"], "active");
        assert_eq!(
            recovery_limits(&record["parameters"]),
            (0, 0)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_slots_and_non_object_parameters_are_rejected() {
        let (state, dir) = state();
        assert!(register(&state, "shell", "x", &json!({}), "").is_err());
        assert!(register(&state, "planning", "x", &json!([]), "").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
