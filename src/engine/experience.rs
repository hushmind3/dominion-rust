//! Experience bank: concrete task cases with outcomes, lessons and usage.
//! Unlike free-form memories, these are retrieved by task similarity and fed
//! into planning so failures are avoided and successful procedures reused.
use crate::engine::memory::{self, MemoryScope};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_string)
        .collect()
}

fn signature(request: &str) -> String {
    let mut seen = HashSet::new();
    tokens(request)
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .take(10)
        .collect::<Vec<_>>()
        .join(" ")
}

fn visible(record: &Value, scope: &MemoryScope) -> bool {
    if record["scope"].as_str() == Some("global") {
        return true;
    }
    if record["partitionKey"].as_str() != Some(scope.partition_key.as_str()) {
        return false;
    }
    record["scope"].as_str() == Some("shared")
        || record["agentId"].as_str() == scope.agent_id.as_deref()
}

fn rule_visible(rule: &Value, scope: &MemoryScope) -> bool {
    if rule["scope"].as_str() == Some("global") {
        return true;
    }
    if rule["partitionKey"].as_str() != Some(scope.partition_key.as_str()) {
        return false;
    }
    match rule["scope"].as_str() {
        Some("shared") => true,
        Some("agent") => rule["agentId"].as_str() == scope.agent_id.as_deref(),
        _ => false,
    }
}

/// Observable features used by LearnedMethod applicability predicates.
pub fn method_observation(
    state: &AppState,
    failed_tool: &str,
    failed_inputs: &Value,
    error: &str,
) -> Value {
    let mut path_state = serde_json::Map::new();
    for key in ["path", "cwd"] {
        let Some(value) = failed_inputs.get(key).and_then(Value::as_str) else {
            continue;
        };
        let path = if std::path::Path::new(value).is_absolute() {
            std::path::PathBuf::from(value)
        } else {
            state.config.workspace.join(value)
        };
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                path_state.insert(key.to_string(), json!({
                    "exists": true,
                    "isDir": metadata.is_dir(),
                    "len": metadata.len(),
                }));
            }
            Err(_) => {
                path_state.insert(key.to_string(), json!({"exists": false}));
            }
        }
    }
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&state.config.workspace) {
        for entry in read_dir.flatten().take(64) {
            entries.push((
                entry.file_name().to_string_lossy().to_string(),
                entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            ));
        }
    }
    entries.sort();
    json!({
        "tool": failed_tool,
        "arguments": failed_inputs,
        "error": error.chars().take(600).collect::<String>(),
        "failureClass": failure_class(error),
        "inputShape": input_shape(failed_inputs),
        "pathState": path_state,
        "environmentFingerprint": memory::stable_id(&[
            &json!(entries).to_string(),
            &json!(path_state).to_string(),
        ]),
    })
}

/// Three-valued applicability. Unknown is not applied as if it were True.
pub fn method_applicability(method: &Value, observation: &Value) -> &'static str {
    use crate::engine::program::{predicate_verdict, PredicateVerdict};
    let predicates = method["applicabilityPredicates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if predicates.is_empty() {
        // Legacy rules match on their structured fields.
        let matches = method["applicability"]["failedTool"].as_str()
            == observation["tool"].as_str()
            && method["applicability"]["failureClass"].as_str()
                == observation["failureClass"].as_str()
            && method["applicability"]["failedInputShape"].as_str()
                == observation["inputShape"].as_str();
        return if matches { "true" } else { "false" };
    }
    let mut unknown = false;
    let resolved = json!({"observation": observation});
    for predicate in predicates {
        match predicate_verdict(&predicate, &resolved) {
            Ok(PredicateVerdict::True) => {}
            Ok(PredicateVerdict::False) => return "false",
            Ok(PredicateVerdict::Unknown) => unknown = true,
            Err(_) => unknown = true,
        }
    }
    if unknown { "unknown" } else { "true" }
}

#[allow(dead_code)] // retained for agent-private cases and direct tests
pub fn record(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
) -> Option<Value> {
    record_with_scope(
        state, scope, request, outcome, lesson, avoid, evidence_ids, tools, gaps, "agent",
    )
}

/// Shared cases are visible to every agent in the same partition.
pub fn record_shared(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
) -> Option<Value> {
    record_with_scope(
        state, scope, request, outcome, lesson, avoid, evidence_ids, tools, gaps, "shared",
    )
}

/// Global cases are visible in every partition (chat, room, channel).
pub fn record_global(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
) -> Option<Value> {
    record_with_scope(
        state, scope, request, outcome, lesson, avoid, evidence_ids, tools, gaps, "global",
    )
}

#[allow(clippy::too_many_arguments)]
fn record_with_scope(
    state: &AppState,
    scope: &MemoryScope,
    request: &str,
    outcome: &str,
    lesson: &str,
    avoid: &str,
    evidence_ids: &[String],
    tools: &[String],
    gaps: &[String],
    scope_kind: &str,
) -> Option<Value> {
    let effective_scope = if scope_kind == "agent" && scope.agent_id.is_none() {
        "shared"
    } else {
        scope_kind
    };
    let request = request.trim().chars().take(2000).collect::<String>();
    let lesson = lesson.trim().chars().take(2000).collect::<String>();
    if request.is_empty() || lesson.is_empty() || !matches!(outcome, "success" | "failure") {
        return None;
    }
    let sig = signature(&request);
    let id = format!(
        "experience-{}",
        memory::stable_id(&[
            if effective_scope == "global" {
                "global"
            } else {
                scope.partition_key.as_str()
            },
            if effective_scope == "agent" {
                scope.agent_id.as_deref().unwrap_or("default")
            } else {
                effective_scope
            },
            &sig,
            outcome,
            &normalize(&lesson),
        ])
    );
    if let Some(mut existing) = state.store.get("experience", &id) {
        existing["occurrences"] = json!(existing["occurrences"].as_u64().unwrap_or(1) + 1);
        existing["lastSeenAt"] = json!(chrono::Utc::now().to_rfc3339());
        let merged = existing["evidenceMessageIds"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .chain(evidence_ids.iter().cloned().map(Value::String))
            .collect::<Vec<_>>();
        existing["evidenceMessageIds"] = json!(merged.into_iter().take(12).collect::<Vec<_>>());
        let saved = state.store.put("experience", &existing)?;
        state.bus.record("experience", &saved);
        return Some(saved);
    }
    let saved = state.store.put(
        "experience",
        &json!({
            "id": id,
            "scope": effective_scope,
            "agentId": scope.agent_id,
            "partitionKey": scope.partition_key,
            "sessionId": scope.session_id,
            "signature": sig,
            "request": if effective_scope == "global" { sig.clone() } else { request },
            "outcome": outcome,
            "lesson": lesson,
            "avoid": avoid.chars().take(1000).collect::<String>(),
            "gaps": gaps.iter().take(8).cloned().collect::<Vec<_>>(),
            "tools": tools.iter().take(20).cloned().collect::<Vec<_>>(),
            "evidenceMessageIds": evidence_ids.iter().take(12).cloned().collect::<Vec<_>>(),
            "occurrences": 1,
            "uses": 0,
            "status": "active",
            "createdAt": chrono::Utc::now().to_rfc3339(),
            "lastSeenAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    if let Some(saved) = &saved {
        state.bus.record("experience", saved);
        state
            .store
            .audit("experience.recorded", json!({"id": saved["id"], "outcome": outcome}));
    }
    saved
}

/// Retrieves similar cases and bumps their usage counters.
#[allow(dead_code)]
pub fn retrieve(state: &AppState, scope: &MemoryScope, query: &str, limit: usize) -> Vec<Value> {
    retrieve_with_execution(state, scope, query, limit, None)
}


/* Hybrid reranker uses corpus-weighted words plus Korean/spacing-resistant
 * character n-grams. It runs entirely on stored experience records and makes
 * no model or embedding request. This is a fuzzy lexical kernel, not a
 * substitute for a semantic embedding index. */
fn hybrid_terms(text: &str) -> HashSet<String> {
    let mut result = HashSet::new();
    let lower = text.to_lowercase();
    for token in lower.split(|ch: char| !ch.is_alphanumeric()) {
        let chars = token.chars().collect::<Vec<_>>();
        if chars.len() >= 2 {
            result.insert(format!("w:{token}"));
            if chars.iter().any(|ch| ('가'..='힣').contains(ch)) {
                for pair in chars.windows(2) {
                    result.insert(format!("g:{}", pair.iter().collect::<String>()));
                }
            }
        }
    }
    // Collapse whitespace/punctuation so "데이터베이스" also finds
    // "데이터 베이스"; unlike substring matching, record both boundaries.
    let compact = normalize(text);
    let chars = compact.chars().collect::<Vec<_>>();
    for window in chars.windows(3) {
        result.insert(format!("c:{}", window.iter().collect::<String>()));
    }
    result
}

fn balanced_rerank(query: &str, cases: &[Value], failure_boost: f64) -> Vec<(f64, Value)> {
    let query_terms = hybrid_terms(query);
    if query_terms.is_empty() {
        return Vec::new();
    }
    let documents = cases.iter().map(|record| {
        let body = format!(
            "{} {} {}",
            record["signature"].as_str().unwrap_or(""),
            record["request"].as_str().unwrap_or(""),
            record["lesson"].as_str().unwrap_or("")
        );
        hybrid_terms(&body)
    }).collect::<Vec<_>>();
    let mut document_frequency: HashMap<&str, usize> = HashMap::new();
    for terms in &documents {
        for term in terms {
            if query_terms.contains(term) {
                *document_frequency.entry(term.as_str()).or_default() += 1;
            }
        }
    }
    let corpus = documents.len() as f64;
    cases.iter().zip(documents.iter()).filter_map(|(record, terms)| {
        let shared = query_terms.intersection(terms).collect::<Vec<_>>();
        if shared.is_empty() {
            return None;
        }
        let evidence = shared.iter().map(|term| {
            let df = *document_frequency.get(term.as_str()).unwrap_or(&0) as f64;
            let idf = (1.0 + (corpus - df + 0.5) / (df + 0.5)).ln();
            let weight = if term.starts_with("w:") { 2.0 }
                else if term.starts_with("g:") { 0.7 } else { 0.5 };
            idf * weight
        }).sum::<f64>();
        // Normalization prevents huge stored lessons from dominating a query.
        let coverage = shared.len() as f64 / query_terms.len().max(1) as f64;
        let size_penalty = (terms.len().max(1) as f64).sqrt();
        let frequency = (1.0 + record["occurrences"].as_u64().unwrap_or(1) as f64).ln();
        let failure = if record["outcome"].as_str() == Some("failure") {
            failure_boost.clamp(0.0, 4.0) * 0.15
        } else {
            0.15
        };
        Some((evidence / size_penalty + coverage * 4.0 + frequency * 0.10 + failure,
              record.clone()))
    }).collect()
}

pub fn retrieve_with_execution(
    state: &AppState,
    scope: &MemoryScope,
    query: &str,
    limit: usize,
    execution_id: Option<&str>,
) -> Vec<Value> {
    let retrieval = crate::engine::algorithms::choose_for_agent(
        state,
        "memory.retrieval",
        scope.agent_id.as_deref(),
        execution_id,
    );
    let failure_boost = retrieval
        .as_ref()
        .and_then(|algorithm| algorithm["parameters"]["failureBoost"].as_f64())
        .unwrap_or(2.0);
    let query_tokens = tokens(query);
    if query_tokens.is_empty() {
        return Vec::new();
    }
    let kernel = retrieval.as_ref()
        .and_then(|algorithm| algorithm["implementation"].as_str())
        .unwrap_or("lexical");
    let candidates = state.store.all("experience").into_iter()
        .filter(|record| record["status"].as_str() == Some("active") && visible(record, scope))
        .collect::<Vec<_>>();
    let mut scored = if kernel == "balanced" {
        balanced_rerank(query, &candidates, failure_boost)
    } else {
        let mut legacy = Vec::new();
        for record in candidates {
            let haystack = format!(
                "{} {} {}",
                record["signature"].as_str().unwrap_or(""),
                record["request"].as_str().unwrap_or(""),
                record["lesson"].as_str().unwrap_or("")
            ).to_lowercase();
            let overlap = query_tokens.iter()
                .filter(|token| haystack.contains(token.as_str())).count();
            if overlap == 0 {
                continue;
            }
            let boost = if record["outcome"] == "failure" { failure_boost } else { 1.0 };
            let occurrences = record["occurrences"].as_u64().unwrap_or(1) as f64;
            legacy.push((overlap as f64 * 3.0 + boost + occurrences * 0.1, record));
        }
        legacy
    };
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let selected = scored
        .into_iter()
        .take(limit.clamp(1, 8))
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    for record in &selected {
        if let Some(mut stored) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
            stored["uses"] = json!(stored["uses"].as_u64().unwrap_or(0) + 1);
            stored["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("experience", &stored);
        }
    }
    selected
}

#[allow(dead_code)] // compatibility wrapper; snapshots use the manifest form
pub fn context_block(state: &AppState, scope: &MemoryScope, query: &str) -> Option<String> {
    context_block_with_manifest(state, scope, query).0
}

/// Same retrieval as `context_block`, plus the exact cases delivered to the
/// model so the selection snapshot can freeze their ids and revisions.
pub fn context_block_with_manifest(
    state: &AppState,
    scope: &MemoryScope,
    query: &str,
) -> (Option<String>, Vec<Value>) {
    context_block_with_manifest_at(state, scope, query, None)
}

pub fn context_block_with_manifest_at(
    state: &AppState,
    scope: &MemoryScope,
    query: &str,
    execution_id: Option<&str>,
) -> (Option<String>, Vec<Value>) {
    let algorithm = crate::engine::algorithms::choose_for_agent(
        state,
        "context.selection",
        scope.agent_id.as_deref(),
        execution_id,
    );
    let method_limit = algorithm
        .as_ref()
        .and_then(|item| item["parameters"]["methodLimit"].as_u64())
        .unwrap_or(3) as usize;
    let failure_boost = algorithm
        .as_ref()
        .and_then(|item| item["parameters"]["failureBoost"].as_f64())
        .unwrap_or(2.0);
    let mut cases = retrieve_with_execution(state, scope, query, 5, execution_id);
    if failure_boost > 1.0 {
        cases.sort_by(|a, b| {
            let score = |case: &Value| {
                if case["outcome"].as_str() == Some("failure") {
                    failure_boost
                } else {
                    1.0
                }
            };
            score(b)
                .partial_cmp(&score(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    let query_tokens = tokens(query);
    let observation = json!({"task": query});
    let mut methods = state
        .store
        .all("experience_rule")
        .into_iter()
        .filter(|method| {
            rule_visible(method, scope)
                && method["status"].as_str() == Some("candidate")
                && method["supportingCases"].as_array().is_some_and(|v| !v.is_empty())
        })
        .filter_map(|method| {
            let verdict = method_applicability(&method, &observation);
            if verdict == "false" {
                return None;
            }
            let trigger = method["trigger"].as_str().unwrap_or("").to_lowercase();
            let overlap = query_tokens
                .iter()
                .filter(|token| trigger.contains(token.as_str()))
                .count();
            if verdict == "unknown" && overlap == 0 {
                return None;
            }
            Some((overlap, verdict, method))
        })
        .collect::<Vec<_>>();
    methods.sort_by(|a, b| b.0.cmp(&a.0));
    methods.truncate(method_limit.min(8));
    if cases.is_empty() && methods.is_empty() {
        return (None, Vec::new());
    }
    let mut manifest = cases
        .iter()
        .map(|case| {
            let id = case["id"].as_str().unwrap_or("");
            json!({
                "kind": "experience",
                "id": id,
                "outcome": case["outcome"],
                "revision": memory::stable_id(&[
                    id,
                    case["signature"].as_str().unwrap_or(""),
                    case["lesson"].as_str().unwrap_or(""),
                    case["lastUsedAt"].as_str().unwrap_or(""),
                ]),
            })
        })
        .collect::<Vec<_>>();
    let mut block = String::from("Similar past task cases (failure cases are warnings):");
    for case in &cases {
        let outcome = case["outcome"].as_str().unwrap_or("");
        let request = case["request"].as_str().unwrap_or("").chars().take(220).collect::<String>();
        let lesson = case["lesson"].as_str().unwrap_or("").chars().take(320).collect::<String>();
        let avoid = case["avoid"].as_str().unwrap_or("").chars().take(220).collect::<String>();
        block.push_str(&format!("\n- [{outcome}] task: {request}\n  lesson: {lesson}"));
        if !avoid.is_empty() {
            block.push_str("\n  avoid: ");
            block.push_str(&avoid);
        }
    }
    if !methods.is_empty() {
        block.push_str("\n\nConditional learned methods (apply only after checking the predicate):");
        for (_, verdict, method) in &methods {
            block.push_str(&format!(
                "\n- [{}] revision {}: use {} ({}) on `{}`; confidence {}",
                verdict,
                method["revision"].as_str().unwrap_or(""),
                method["intervention"]["tool"].as_str().unwrap_or(""),
                method["intervention"]["inputShape"].as_str().unwrap_or(""),
                method["trigger"].as_str().unwrap_or(""),
                method["confidence"].as_f64().unwrap_or(0.0),
            ));
            manifest.push(json!({
                "kind": "learned_method",
                "id": method["id"],
                "revision": method["revision"],
                "applicability": verdict,
            }));
        }
    }
    (Some(block), manifest)
}

/// A deliberately narrow, observable applicability feature. Unknown errors
/// stay unknown: they must not be generalized into a reusable rule.
pub(crate) fn failure_class(error: &str) -> Option<&'static str> {
    let lower = error.to_lowercase();
    if lower.contains("permission denied") || lower.contains("access denied") {
        Some("permission_denied")
    } else if lower.contains("not found") || lower.contains("no such file")
        || lower.contains("does not exist") {
        Some("not_found")
    } else if lower.contains("timed out") || lower.contains("timeout") {
        Some("timeout")
    } else if lower.contains("rate limit") || lower.contains("too many requests") {
        Some("rate_limited")
    } else if lower.contains("conflict") || lower.contains("stale")
        || lower.contains("version mismatch") || lower.contains("sha mismatch")
        || lower.contains("precondition") || lower.contains(" 409")
    {
        Some("stale_state")
    } else if lower.contains("invalid argument") || lower.contains("invalid params")
        || lower.contains("schema validation") {
        Some("invalid_inputs")
    } else {
        None
    }
}

/// Names, not values, allow the same intervention to be considered for a
/// different path/document without replaying the old tool arguments.
fn input_shape(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    let mut names = object.keys().map(String::as_str).collect::<Vec<_>>();
    names.sort_unstable();
    Some(names.join(","))
}

fn observed_success(value: &Value) -> bool {
    value.is_object()
        && !value.as_object().is_some_and(|v| v.is_empty())
        && value["outcomeUnknown"].as_bool() != Some(true)
        && value["isError"].as_bool() != Some(true)
        && value["success"].as_bool() != Some(false)
        && value["error"].is_null()
        && value.get("exitCode").is_none_or(|v| v.as_i64() == Some(0))
        && value.get("status").and_then(Value::as_u64).is_none_or(|v| v < 400)
}

fn observed_failure(value: &Value) -> bool {
    value["isError"].as_bool() == Some(true)
        || value["success"].as_bool() == Some(false)
        || !value["error"].is_null()
        || value.get("exitCode").is_some_and(|v| v.as_i64().is_some_and(|n| n != 0))
        || value.get("status").and_then(Value::as_u64).is_some_and(|v| v >= 400)
}

/// Only a verified successful root attempt can contribute a supporting case.
/// The learned artifact is an observed *candidate* and never auto-executes.
pub fn learn_verified_adaptations(state: &AppState, execution: &Value) -> Vec<Value> {
    if execution["taskOutcome"].as_str() != Some("succeeded") {
        return Vec::new();
    }
    if execution["effectSummary"]["unverifiedEffects"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        return Vec::new();
    }
    let Some(execution_id) = execution["id"].as_str() else {
        return Vec::new();
    };
    let Some(session_id) = execution["sessionId"].as_str() else {
        return Vec::new();
    };
    let scope = MemoryScope::for_session(&state.store, session_id, execution["agentId"].as_str());
    let mut saved_rules = Vec::new();
    for adaptation in execution["adaptations"].as_array().into_iter().flatten().take(20) {
        let Some(failed_tool) = adaptation["observation"]["tool"].as_str() else {
            continue;
        };
        let Some(kind) = adaptation["observation"]["actual"]
            .as_str().and_then(failure_class) else {
            continue;
        };
        let Some(failed_shape) = input_shape(&adaptation["observation"]["arguments"]) else {
            continue;
        };
        let Some(selected_tool) = adaptation["selected"]["tool"].as_str() else {
            continue;
        };
        let Some(selected_shape) = input_shape(&adaptation["selected"]["inputs"]) else {
            continue;
        };
        if !observed_success(&adaptation["result"]) {
            continue;
        }
        let id = format!("experience-rule-{}", memory::stable_id(&[
            &scope.partition_key, failed_tool, kind, &failed_shape,
            selected_tool, &selected_shape,
        ]));
        let before = adaptation["beforeObservation"].clone();
        let mut predicates = vec![
            json!({"type":"equals","from":"observation","path":"/tool","value":failed_tool}),
            json!({"type":"equals","from":"observation","path":"/failureClass","value":kind}),
            json!({"type":"equals","from":"observation","path":"/inputShape","value":failed_shape}),
        ];
        if let Some(paths) = before["pathState"].as_object() {
            for (key, state) in paths {
                if let Some(exists) = state["exists"].as_bool() {
                    predicates.push(json!({
                        "type":"equals",
                        "from":"observation",
                        "path": format!("/pathState/{key}/exists"),
                        "value": exists,
                    }));
                }
            }
        }
        let mut rule = state.store.get("experience_rule", &id).unwrap_or_else(|| json!({
            "id": id,
            "scope": if scope.agent_id.is_some() { "agent" } else { "shared" },
            "agentId": scope.agent_id,
            "partitionKey": scope.partition_key,
            "status": "candidate",
            "evidenceGrade": "E1_observed",
            "applicabilityPredicates": predicates,
            "trigger": format!("{failed_tool} {kind}"),
            "applicability": {
                "failedTool": failed_tool,
                "failureClass": kind,
                "failedInputShape": failed_shape,
            },
            "intervention": {
                "tool": selected_tool,
                "inputShape": selected_shape,
                // Store the parameter *schema*, not old file contents, API
                // tokens, paths or other potentially private argument values.
            },
            "predictedEffects": adaptation["selected"]["evaluation"],
            "verificationChecks": adaptation["selected"]["expectedChecks"],
            "effectByEnvironment": {},
            "confidence": 0.4,
            "supportingCases": [],
            "supportingAgents": [],
            "counterexamples": [],
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }));
        if !rule["applicabilityPredicates"].is_array() {
            rule["applicabilityPredicates"] = json!(predicates);
        }
        // Personal experience stays with its bot. Two different agents
        // independently supporting the same verified method promotes it to
        // shared, so the group benefits without trusting a single bot.
        if let Some(agent) = scope.agent_id.clone() {
            let mut agents = rule["supportingAgents"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if !agents.iter().any(|value| value.as_str() == Some(agent.as_str())) {
                agents.push(json!(agent));
                agents.truncate(16);
            }
            rule["supportingAgents"] = json!(agents);
        }
        let distinct_agents = rule["supportingAgents"]
            .as_array()
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
            })
            .unwrap_or(0);
        let min_agents = crate::engine::algorithms::choose_for_agent(
            state,
            "multiagent.sharing",
            scope.agent_id.as_deref(),
            Some(execution_id),
        )
        .and_then(|algorithm| algorithm["parameters"]["minAgents"].as_u64())
        .unwrap_or(2) as usize;
        if distinct_agents >= min_agents.max(1) {
            rule["scope"] = json!("shared");
            rule["agentId"] = Value::Null;
            if rule["sharedAt"].is_null() {
                rule["sharedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
        }
        let environment = before["environmentFingerprint"]
            .as_str()
            .unwrap_or("unknown");
        rule["effectByEnvironment"][environment] = json!({
            "successes": rule["supportingCases"].as_array().map(Vec::len).unwrap_or(0) + 1,
            "failures": rule["counterexamples"].as_array().map(Vec::len).unwrap_or(0),
        });
        // One root task, not one agent/room speaker, counts as one case.
        let case_id = execution["taskId"].as_str().unwrap_or(execution_id);
        let mut supports = rule["supportingCases"].as_array().cloned().unwrap_or_default();
        if !supports.iter().any(|v| v.as_str() == Some(case_id)) {
            supports.push(json!(case_id));
            supports.truncate(24);
        }
        rule["supportingCases"] = json!(supports);
        rule["revision"] = json!(memory::stable_id(&[
            &id, &rule["supportingCases"].to_string(), &rule["counterexamples"].to_string(),
        ]));
        rule["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(saved) = state.store.put("experience_rule", &rule) {
            state.bus.record("experience_rule", &saved);
            let learning_id = format!(
                "learning-case-{}",
                memory::stable_id(&[execution_id, &id])
            );
            let learning_case = json!({
                "id": learning_id,
                "agentId": execution["agentId"],
                "taskId": case_id,
                "sourceMessageIds": [execution_id],
                "originalRequest": execution["request"],
                "previousBehavior": adaptation["observation"],
                "ownerCorrection": Value::Null,
                "expectedOutcome": "the selected tool succeeds",
                "observedOutcome": adaptation["result"],
                "failureHypotheses": adaptation["hypotheses"],
                "verifiedCause": adaptation["selected"]["evaluation"],
                "improvedMethod": saved["intervention"],
                "evidenceRefs": [execution_id, adaptation["id"]],
                "environmentFeatures": before,
                "scope": "shared",
                "partitionKey": scope.partition_key,
                "createdAt": chrono::Utc::now().to_rfc3339(),
            });
            state.store.put("learning_case", &learning_case);
            rule["confidence"] = json!((
                1.0 + rule["supportingCases"].as_array().map(Vec::len).unwrap_or(0) as f64
            ) / (
                2.0 + rule["supportingCases"].as_array().map(Vec::len).unwrap_or(0) as f64
                    + rule["counterexamples"].as_array().map(Vec::len).unwrap_or(0) as f64
            ));
            state.store.put("experience_rule", &rule);
            saved_rules.push(saved);
        }
    }
    saved_rules
}

/// True/False/Unknown applicability: this returns only True. Unclassified
/// errors are Unknown and never receive a suggested intervention.
#[allow(dead_code)]
pub fn matching_rules(
    state: &AppState,
    scope: &MemoryScope,
    failed_tool: &str,
    failed_inputs: &Value,
    error: &str,
    limit: usize,
) -> Vec<Value> {
    matching_rules_with_execution(
        state, scope, failed_tool, failed_inputs, error, limit, None,
    )
}

pub fn matching_rules_with_execution(
    state: &AppState,
    scope: &MemoryScope,
    failed_tool: &str,
    failed_inputs: &Value,
    error: &str,
    limit: usize,
    execution_id: Option<&str>,
) -> Vec<Value> {
    let observation = method_observation(state, failed_tool, failed_inputs, error);
    let probe_unknown = crate::engine::algorithms::choose_for_agent(
        state,
        "learning.applicability",
        scope.agent_id.as_deref(),
        execution_id,
    )
    .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
    .as_deref()
        == Some("probe-unknown");
    let mut rules = state
        .store
        .all("experience_rule")
        .into_iter()
        .filter_map(|rule| {
            if !rule_visible(&rule, scope)
                || rule["status"].as_str() != Some("candidate")
                || !rule["supportingCases"].as_array().is_some_and(|v| !v.is_empty())
            {
                return None;
            }
            let has_predicates = rule["applicabilityPredicates"]
                .as_array()
                .is_some_and(|v| !v.is_empty());
            if !has_predicates
                && !rule["counterexamples"].as_array().is_some_and(Vec::is_empty)
            {
                return None;
            }
            let mut verdict = method_applicability(&rule, &observation);
            let mut probed = observation.clone();
            let mut probe_applied = false;
            if verdict == "unknown" && probe_unknown {
                let (next, resolved) = probe_rule_observation(state, &rule, &observation);
                probed = next;
                probe_applied = true;
                if resolved {
                    verdict = "true";
                }
            }
            if verdict != "true" {
                return None;
            }
            let mut rule = rule;
            rule["applicabilityVerdict"] = json!(verdict);
            rule["probeApplied"] = json!(probe_applied);
            rule["observation"] = probed;
            Some(rule)
        })
        .collect::<Vec<_>>();
    rules.sort_by(|a, b| {
        b["supportingCases"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            .cmp(&a["supportingCases"].as_array().map(Vec::len).unwrap_or(0))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    rules.truncate(limit.min(5));
    rules
}

/// Performs the observation a probe-unknown rule asked for. Currently this
/// observes path existence for the fields named in its applicability
/// predicates; the rule is then re-evaluated against that observation.
fn probe_rule_observation(
    state: &AppState,
    rule: &Value,
    observation: &Value,
) -> (Value, bool) {
    let mut next = observation.clone();
    if !next["pathState"].is_object() {
        next["pathState"] = json!({});
    }
    for predicate in rule["applicabilityPredicates"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(path) = predicate["path"].as_str() else {
            continue;
        };
        let Some(rest) = path.strip_prefix("/pathState/") else {
            continue;
        };
        let key = rest.split('/').next().unwrap_or("");
        let Some(value) = observation["arguments"][key].as_str() else {
            continue;
        };
        let target = if std::path::Path::new(value).is_absolute() {
            std::path::PathBuf::from(value)
        } else {
            state.config.workspace.join(value)
        };
        match std::fs::metadata(&target) {
            Ok(metadata) => {
                next["pathState"][key] = json!({
                    "exists": true,
                    "isDir": metadata.is_dir(),
                    "len": metadata.len(),
                });
            }
            Err(_) => {
                next["pathState"][key] = json!({"exists": false});
            }
        }
    }
    let verdict = method_applicability(rule, &next);
    (next, verdict == "true")
}

/// An independently observed failure of the same intervention shape is a
/// counterexample. Merely consulting a rule is not itself failure evidence.
pub fn record_rule_counterexamples(
    state: &AppState,
    considered: &[Value],
    selected: &Value,
    result: &Value,
    execution_id: &str,
) {
    if !observed_failure(result) {
        return;
    }
    let agent_id = state
        .store
        .get("execution", execution_id)
        .and_then(|execution| execution["agentId"].as_str().map(str::to_string));
    let mode = crate::engine::algorithms::choose_for_agent(
        state,
        "learning.counterexample",
        agent_id.as_deref(),
        Some(execution_id),
    )
    .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
    .unwrap_or_else(|| "narrow".into());
    if mode == "disable" {
        crate::engine::algorithms::mark_signal(
            state, execution_id, "learning.counterexample", true,
        );
        return;
    }
    let mut changed = false;
    for hint in considered {
        let Some(id) = hint["id"].as_str() else { continue };
        let Some(mut rule) = state.store.get("experience_rule", id) else { continue };
        if rule["intervention"]["tool"] != selected["tool"]
            || rule["intervention"]["inputShape"].as_str()
                != input_shape(&selected["inputs"]).as_deref() {
            continue;
        }
        let mut examples = rule["counterexamples"].as_array().cloned().unwrap_or_default();
        if !examples.iter().any(|v| v.as_str() == Some(execution_id)) {
            examples.push(json!(execution_id));
            examples.truncate(24);
        }
        rule["counterexamples"] = json!(examples);
        // Narrow, do not discard: exclude the exact observed environment and
        // keep the method available where the failure was not reproduced.
        let mut narrowed = false;
        if let Some(execution) = state.store.get("execution", execution_id) {
            if let Some(fingerprint) = execution["lastAdaptation"]["environmentFingerprint"]
                .as_str()
                .or_else(|| execution["adaptationCheckpoint"]["environmentFingerprint"].as_str())
            {
                let mut predicates = rule["applicabilityPredicates"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let already = predicates.iter().any(|predicate| {
                    predicate["type"].as_str() == Some("not")
                        && predicate["predicate"]["path"].as_str()
                            == Some("/environmentFingerprint")
                });
                if !already {
                    predicates.push(json!({
                        "type":"not",
                        "predicate": {
                            "type":"equals",
                            "from":"observation",
                            "path":"/environmentFingerprint",
                            "value": fingerprint,
                        }
                    }));
                    rule["applicabilityPredicates"] = json!(predicates);
                    narrowed = true;
                }
            }
        }
        if !narrowed || mode == "hold" {
            // No observable environment difference is available, or the
            // selected algorithm holds instead of narrowing. Keep the method
            // as an unresolved hypothesis rather than applying it.
            rule["status"] = json!("needs_split");
        }
        rule["revision"] = json!(memory::stable_id(&[
            id, &rule["supportingCases"].to_string(), &rule["counterexamples"].to_string()
        ]));
        rule["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if let Some(saved) = state.store.put("experience_rule", &rule) {
            state.bus.record("experience_rule", &saved);
            changed = true;
        }
    }
    crate::engine::algorithms::mark_signal(
        state,
        execution_id,
        "learning.counterexample",
        changed,
    );
}

/// Merges duplicates and archives the least useful cases. Called by the
/// learning worker so the bank does not grow without bound.
pub fn consolidate(state: &AppState) -> Value {
    let mut seen: HashMap<String, Value> = HashMap::new();
    let mut archived = 0usize;
    for record in state.store.all("experience") {
        if record["status"].as_str() != Some("active") {
            continue;
        }
        let key = format!(
            "{}:{}:{}:{}",
            record["partitionKey"].as_str().unwrap_or(""),
            match record["scope"].as_str() {
                Some(scope) if scope == "shared" || scope == "global" => scope,
                _ => record["agentId"].as_str().unwrap_or("default"),
            },
            record["signature"].as_str().unwrap_or(""),
            record["outcome"].as_str().unwrap_or("")
        );
        match seen.get(&key) {
            Some(existing)
                if existing["occurrences"].as_u64().unwrap_or(1)
                    >= record["occurrences"].as_u64().unwrap_or(1) =>
            {
                if let Some(mut old) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
                    old["status"] = json!("archived");
                    old["archivedAt"] = json!(chrono::Utc::now().to_rfc3339());
                    state.store.put("experience", &old);
                    archived += 1;
                }
            }
            _ => {
                seen.insert(key, record);
            }
        }
    }
    let mut active = state
        .store
        .all("experience")
        .into_iter()
        .filter(|record| record["status"] == "active")
        .collect::<Vec<_>>();
    active.sort_by(|a, b| {
        b["occurrences"].as_u64().unwrap_or(0)
            .cmp(&a["occurrences"].as_u64().unwrap_or(0))
            .then_with(|| {
                b["lastSeenAt"].as_str().unwrap_or("").cmp(a["lastSeenAt"].as_str().unwrap_or(""))
            })
    });
    for record in active.into_iter().skip(300) {
        if let Some(mut old) = state.store.get("experience", record["id"].as_str().unwrap_or("")) {
            old["status"] = json!("archived");
            old["archivedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("experience", &old);
            archived += 1;
        }
    }
    json!({ "archived": archived })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-exp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn records_dedupes_and_retrieves_similar_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let first = record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m1".into()], &["workspace_write".into()], &["file_changed".into()],
        ).unwrap();
        record(
            &state, &scope, "README.md 파일을 만들어줘", "failure",
            "파일을 만들지 않고 말로만 완료했다", "도구 호출 없이 완료 선언하지 말 것",
            &["m2".into()], &[], &[],
        ).unwrap();
        let stored = state.store.get("experience", first["id"].as_str().unwrap()).unwrap();
        assert_eq!(stored["occurrences"], 2);
        let cases = retrieve(&state, &scope, "README.md 파일 작성", 5);
        assert_eq!(cases.len(), 1);
        assert!(context_block(&state, &scope, "README.md 파일 작성")
            .unwrap()
            .contains("말로만"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scopes_do_not_leak_between_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record(&state, &a, "파일 작성", "success", "A의 절차", "", &[], &[], &[]).unwrap();
        assert!(retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_cases_reach_other_agents() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"t","agentId":"b"}));
        let a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let b = MemoryScope::for_session(&state.store, "t", Some("b"));
        record_shared(
            &state, &a, "파일 작성", "success", "공유 절차를 재사용한다", "", &[], &[], &[],
        )
        .unwrap();
        assert!(!retrieve(&state, &b, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_agents_supporting_a_method_share_it_with_the_group() {
        let (state, dir) = state();
        let attempt = |agent: &str, id: &str| json!({
            "id": id, "sessionId": "s", "agentId": agent, "taskOutcome": "succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        learn_verified_adaptations(&state, &attempt("agent-a", "run-a"));
        let a_scope = MemoryScope::for_session(&state.store, "s", Some("agent-a"));
        let b_scope = MemoryScope::for_session(&state.store, "s", Some("agent-b"));
        assert!(matching_rules(&state, &b_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).is_empty());
        learn_verified_adaptations(&state, &attempt("agent-b", "run-b"));
        let shared = matching_rules(&state, &b_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3);
        assert_eq!(shared.len(), 1, "{shared:?}");
        assert_eq!(shared[0]["scope"], "shared");
        let c_scope = MemoryScope::for_session(&state.store, "s", Some("agent-c"));
        assert_eq!(matching_rules(&state, &c_scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).len(), 1);
        let _ = a_scope;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sharing_threshold_algorithm_controls_cross_agent_promotion() {
        let (state, dir) = state();
        crate::engine::algorithms::seed(&state);
        let three = state
            .store
            .all("algorithm")
            .into_iter()
            .find(|algorithm| algorithm["implementation"] == "three-agents")
            .unwrap();
        crate::engine::algorithms::activate(&state, three["id"].as_str().unwrap()).unwrap();
        let attempt = |agent: &str, id: &str| json!({
            "id": id, "sessionId": "s", "agentId": agent, "taskOutcome": "succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        for (agent, id) in [("agent-a","r1"), ("agent-b","r2")] {
            learn_verified_adaptations(&state, &attempt(agent, id));
        }
        let third = MemoryScope::for_session(&state.store, "s", Some("agent-c"));
        assert!(matching_rules(&state, &third, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).is_empty());
        learn_verified_adaptations(&state, &attempt("agent-c", "r3"));
        assert_eq!(matching_rules(&state, &third, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn consolidation_archives_duplicate_cases() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let base = state.store.put("experience", &json!({
            "id":"e1","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":5,"status":"active","lastSeenAt":"2026-01-01"
        })).unwrap();
        state.store.put("experience", &json!({
            "id":"e2","scope":"shared","partitionKey":scope.partition_key,
            "agentId":"a","signature":"same task","outcome":"failure","request":"same task",
            "lesson":"same lesson","occurrences":1,"status":"active","lastSeenAt":"2026-01-02"
        })).unwrap();
        consolidate(&state);
        assert_eq!(state.store.get("experience", base["id"].as_str().unwrap()).unwrap()["status"], "active");
        assert_eq!(state.store.get("experience", "e2").unwrap()["status"], "archived");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verified_intervention_is_conditionally_reused_and_counterexamples_exclude_it() {
        let (state, dir) = state();
        let attempt = json!({
            "id":"verified-run","sessionId":"s","agentId":"a","taskOutcome":"succeeded",
            "adaptations":[{
                "observation":{"kind":"tool_failure","tool":"workspace_read",
                    "arguments":{"path":"first.txt"},"actual":"file not found"},
                "selected":{"tool":"workspace_write",
                    "inputs":{"path":"first.txt","content":"created"}},
                "result":{"success":true}
            }]
        });
        let created = learn_verified_adaptations(&state, &attempt);
        assert_eq!(created.len(), 1);
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let matched = matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "no such file or directory", 3);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0]["id"], created[0]["id"]);
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "permission denied", 3).is_empty());
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"different":"second.txt"}), "file not found", 3).is_empty());
        // The same observed outcome is not counted as independent evidence.
        let again = learn_verified_adaptations(&state, &attempt);
        assert_eq!(again[0]["supportingCases"].as_array().unwrap().len(), 1);
        state.store.put("session", &json!({
            "id":"another","agentId":"b",
            "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
        }));
        let foreign = MemoryScope::for_session(&state.store, "another", Some("b"));
        assert!(matching_rules(&state, &foreign, "workspace_read",
            &json!({"path":"second.txt"}), "file not found", 3).is_empty());
        record_rule_counterexamples(&state, &matched,
            &json!({"tool":"workspace_write",
                "inputs":{"path":"other.txt","content":"failed"}}),
            &json!({"error":"permission denied"}), "other-run");
        assert!(matching_rules(&state, &scope, "workspace_read",
            &json!({"path":"second.txt"}), "file not found", 3).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_or_unknown_failures_never_become_reusable_rules() {
        let (state, dir) = state();
        let attempt = json!({
            "id":"incomplete","sessionId":"s","agentId":"a","taskOutcome":"answered_unverified",
            "adaptations":[{
                "observation":{"tool":"workspace_read","arguments":{"path":"a"},"actual":"file not found"},
                "selected":{"tool":"workspace_write","inputs":{"path":"a","content":"x"}},
                "result":{"success":true}
            }]
        });
        assert!(learn_verified_adaptations(&state, &attempt).is_empty());
        let mut passed = attempt.clone();
        passed["taskOutcome"] = json!("succeeded");
        passed["adaptations"][0]["observation"]["actual"] = json!("unexpected failure");
        assert!(learn_verified_adaptations(&state, &passed).is_empty());
        passed["adaptations"][0]["observation"]["actual"] = json!("file not found");
        passed["adaptations"][0]["result"] = json!({"outcomeUnknown":true});
        assert!(learn_verified_adaptations(&state, &passed).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_cases_cross_partitions_and_agents() {
        let (state, dir) = state();
        state.store.put(
            "session",
            &json!({
                "id":"d","agentId":"b",
                "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
            }),
        );
        let local = MemoryScope::for_session(&state.store, "s", Some("a"));
        let discord = MemoryScope::for_session(&state.store, "d", Some("b"));
        record_global(
            &state, &local, "파일 작성", "failure", "전역 교훈을 재사용한다", "", &[], &[], &[],
        )
        .unwrap();
        assert!(!retrieve(&state, &discord, "파일 작성", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}


#[cfg(test)]
mod hybrid_retrieval_tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    #[test]
    fn balanced_kernel_finds_korean_spacing_variants_without_an_llm() {
        let dir = std::env::temp_dir()
            .join(format!("dominion-hybrid-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {port:0, data_dir:dir.clone(),
            web_dir:dir.join("web"), workspace:dir.clone()};
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"hybrid-session","agentId":"agent-a"}));
        let scope = MemoryScope::for_session(&state.store, "hybrid-session", Some("agent-a"));
        state.store.put("experience", &json!({
            "id":"hybrid-1","status":"active","scope":"agent",
            "partitionKey":scope.partition_key,"agentId":"agent-a",
            "request":"데이터 베이스 백업", "signature":"데이터 베이스 백업",
            "lesson":"백업 파일을 검증한다","outcome":"success","occurrences":1
        }));
        state.store.put("experience", &json!({
            "id":"hybrid-2","status":"active","scope":"agent",
            "partitionKey":scope.partition_key,"agentId":"agent-a",
            "request":"비디오 렌더링", "signature":"비디오 렌더링",
            "lesson":"프레임 레이트 확인","outcome":"failure","occurrences":5
        }));
        let lexical = retrieve(&state, &scope, "데이터베이스", 5);
        assert!(lexical.is_empty(), "lexical must not pretend to be semantic");
        crate::engine::algorithms::seed(&state);
        for mut item in state.store.all("algorithm") {
            if item["kind"] == "memory.retrieval" {
                item["status"] = json!("disabled");
                state.store.put("algorithm", &item);
            }
        }
        state.store.put("algorithm", &json!({
            "id":"balanced-test","kind":"memory.retrieval",
            "status":"active","revision":"balanced-test",
            "implementation":"balanced","parameters":{"failureBoost":2.0}
        }));
        let matches = retrieve(&state, &scope, "데이터베이스", 5);
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert_eq!(matches[0]["id"], "hybrid-1");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn probe_unknown_actually_observes_before_applying_the_method() {
        let (state, dir) = state();
        crate::engine::algorithms::seed(&state);
        let probe = state
            .store
            .all("algorithm")
            .into_iter()
            .find(|algorithm| algorithm["implementation"] == "probe-unknown")
            .unwrap();
        crate::engine::algorithms::activate(&state, probe["id"].as_str().unwrap()).unwrap();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "experience_rule",
            &json!({
                "id":"rule-probe",
                "scope":"agent",
                "agentId":"a",
                "partitionKey":scope.partition_key,
                "status":"candidate",
                "applicabilityPredicates":[
                    {"type":"equals","from":"observation","path":"/tool","value":"workspace_read"},
                    {"type":"equals","from":"observation","path":"/failureClass","value":"not_found"},
                    {"type":"equals","from":"observation","path":"/inputShape","value":"alternate,path"},
                    {"type":"equals","from":"observation","path":"/pathState/alternate/exists","value":false}
                ],
                "intervention":{"tool":"workspace_write","inputShape":"content,path"},
                "supportingCases":["case-1"],
                "counterexamples":[]
            }),
        );
        let matched = matching_rules(
            &state,
            &scope,
            "workspace_read",
            &json!({"path":"target.txt","alternate":"target.txt"}),
            "no such file or directory",
            3,
        );
        assert_eq!(matched.len(), 1, "{matched:?}");
        assert_eq!(matched[0]["probeApplied"], true);
        assert_eq!(matched[0]["observation"]["pathState"]["alternate"]["exists"], false);

        std::fs::write(dir.join("target.txt"), "now exists").unwrap();
        let after = matching_rules(
            &state,
            &scope,
            "workspace_read",
            &json!({"path":"target.txt","alternate":"target.txt"}),
            "no such file or directory",
            3,
        );
        assert!(after.is_empty(), "probe did not use the real file state: {after:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
