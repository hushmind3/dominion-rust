use crate::engine::adaptation::types::revision_hash;
use crate::engine::memory::{self, MemoryScope};
use crate::state::AppState;
use serde_json::{json, Value};

const PROBATION_USES: u64 = 5;
const ROLLBACK_MARGIN: f64 = 0.2;

/// A validated improvement that can be injected into future turns and is
/// monitored for regression. Harness code changes are proposals only.
pub fn propose(
    state: &AppState,
    scope: &MemoryScope,
    kind: &str,
    text: &str,
    rationale: &str,
    evidence_ids: &[String],
    tool: Option<&str>,
    requested_scope: &str,
) -> Option<Value> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4000 {
        return None;
    }
    if !matches!(kind, "strategy" | "tool_policy" | "harness") {
        return None;
    }
    if evidence_ids.is_empty() && kind != "harness" {
        return None;
    }
    if kind != "harness" && rationale.trim().is_empty() {
        return None;
    }

    let record_scope = if kind == "harness" {
        "shared"
    } else if requested_scope == "global" {
        "global"
    } else if requested_scope != "agent" {
        "shared"
    } else {
        "agent"
    };
    if kind == "strategy" && rationale.trim().chars().count() < 12 {
        return None;
    }
    let key = memory::stable_id(&[
        if record_scope == "global" {
            "global"
        } else {
            scope.partition_key.as_str()
        },
        if record_scope == "agent" {
            scope.agent_id.as_deref().unwrap_or("default")
        } else {
            record_scope
        },
        kind,
        &normalize(text),
    ]);
    let id = format!("improvement-{key}");
    if let Some(existing) = state.store.get("improvement", &id) {
        return Some(existing);
    }

    // A similar strategy that already passed probation wins. A similar one
    // still on probation is superseded by newer evidence instead of silently
    // accumulating duplicates.
    let mut supersedes: Option<String> = None;
    if kind == "strategy" {
        for existing in state.store.all("improvement") {
            if existing["kind"].as_str() != Some("strategy")
                || existing["id"].as_str() == Some(id.as_str())
                || !visible(&existing, scope)
            {
                continue;
            }
            let status = existing["status"].as_str().unwrap_or("");
            if !matches!(status, "adopted" | "validated") {
                continue;
            }
            let existing_text = existing["text"].as_str().unwrap_or("");
            if similarity(existing_text, text) < 0.6 {
                continue;
            }
            if status == "validated" {
                return Some(existing);
            }
            let mut old = existing;
            old["status"] = json!("superseded");
            old["supersededBy"] = json!(id);
            old["supersededAt"] = json!(chrono::Utc::now().to_rfc3339());
            if let Some(saved) = state.store.put("improvement", &old) {
                state.bus.record("improvement", &saved);
            }
            supersedes = Some(old["id"].as_str().unwrap_or("").to_string());
            break;
        }
    }

    let mut status = "adopted";
    let mut reason = String::new();
    let tool_name = tool.map(str::to_string);
    match kind {
        "tool_policy" => {
            let Some(name) = tool_name.clone() else {
                return reject(state, scope, kind, text, rationale, evidence_ids, &id);
            };
            let Some(stat) = state.store.get("tool_stat", &name) else {
                return reject(state, scope, kind, text, rationale, evidence_ids, &id);
            };
            let attempts = stat["attempts"].as_u64().unwrap_or(0);
            let failures = stat["failures"].as_u64().unwrap_or(0);
            if attempts < 3 || failures * 2 < attempts {
                return reject(state, scope, kind, text, rationale, evidence_ids, &id);
            }
            if !text.to_lowercase().contains(&name.to_lowercase()) {
                return reject(state, scope, kind, text, rationale, evidence_ids, &id);
            }
        }
        "strategy" => {
            if evidence_ids.len() == 0 {
                return reject(state, scope, kind, text, rationale, evidence_ids, &id);
            }
        }
        "harness" => {
            status = "proposed";
            reason = "하네스 코드 변경은 주인 승인이 필요합니다.".into();
        }
        _ => {}
    }
    if status == "rejected" {
        return reject(state, scope, kind, text, rationale, evidence_ids, &id);
    }

    let baseline = if kind == "tool_policy" {
        tool_name
            .as_deref()
            .and_then(|name| state.store.get("tool_stat", name))
            .map(|stat| {
                let attempts = stat["attempts"].as_u64().unwrap_or(0).max(1);
                stat["successes"].as_u64().unwrap_or(0) as f64 / attempts as f64
            })
            .unwrap_or(0.5)
    } else {
        baseline_success(state, scope)
    };

    let record = seal(json!({
        "id": id,
        "kind": kind,
        "scope": record_scope,
        "agentId": scope.agent_id,
        "partitionKey": scope.partition_key,
        "sessionId": scope.session_id,
        "text": text,
        "rationale": rationale,
        "tool": tool_name,
        "evidenceMessageIds": evidence_ids,
        "supersedes": supersedes,
        "verification": {"type": "unverified"},
        "authority": "adaptive",
        "assistanceMode": "model_guided",
        "version": 1,
        "scoreVersion": 2,
        "evidenceGrade": "E0",
        "status": status,
        "reason": reason,
        "baselineSuccess": baseline,
        "uses": 0,
        "successes": 0,
        "failures": 0,
        "createdAt": chrono::Utc::now().to_rfc3339(),
        "adoptedAt": if status == "adopted" { json!(chrono::Utc::now().to_rfc3339()) } else { Value::Null },
    }));
    let saved = state.store.put("improvement", &record);
    if let Some(saved) = &saved {
        state.bus.record("improvement", saved);
        state.store.audit(
            "improvement.proposed",
            json!({"id": saved["id"], "kind": kind, "status": status}),
        );
    }
    saved
}

fn reject(
    state: &AppState,
    scope: &MemoryScope,
    kind: &str,
    text: &str,
    rationale: &str,
    evidence_ids: &[String],
    id: &str,
) -> Option<Value> {
    let saved = state.store.put(
        "improvement",
        &json!({
            "id": id,
            "kind": kind,
            "scope": "shared",
            "agentId": scope.agent_id,
            "partitionKey": scope.partition_key,
            "sessionId": scope.session_id,
            "text": text,
            "rationale": rationale,
            "evidenceMessageIds": evidence_ids,
            "status": "rejected",
            "reason": "검증 조건을 충족하지 못했습니다.",
            "createdAt": chrono::Utc::now().to_rfc3339(),
        }),
    );
    if let Some(saved) = &saved {
        state.bus.record("improvement", saved);
    }
    saved
}

fn revision_of(record: &Value) -> String {
    let mut clean = record.clone();
    if let Some(map) = clean.as_object_mut() {
        map.remove("revision");
        map.remove("previousRevision");
        map.remove("createdAt");
        map.remove("updatedAt");
    }
    revision_hash(&clean)
}

fn seal(mut record: Value) -> Value {
    record["revision"] = json!(revision_of(&record));
    record
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}

fn similarity(left: &str, right: &str) -> f64 {
    let grams = |text: &str| -> std::collections::HashSet<(char, char)> {
        let chars = normalize(text).chars().collect::<Vec<_>>();
        chars
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect()
    };
    let left_grams = grams(left);
    let right_grams = grams(right);
    if left_grams.is_empty() || right_grams.is_empty() {
        return f64::from(normalize(left) == normalize(right));
    }
    let intersection = left_grams.intersection(&right_grams).count() as f64;
    let union = left_grams.union(&right_grams).count().max(1) as f64;
    intersection / union
}

fn visible(record: &Value, scope: &MemoryScope) -> bool {
    if record["scope"].as_str() == Some("global") {
        return true;
    }
    if record["partitionKey"].as_str() != Some(scope.partition_key.as_str()) {
        return false;
    }
    if record["scope"].as_str() == Some("shared") {
        return true;
    }
    record["agentId"].as_str() == scope.agent_id.as_deref()
}

fn baseline_success(state: &AppState, scope: &MemoryScope) -> f64 {
    let mut total = 0u64;
    let mut success = 0u64;
    for execution in state.store.all("execution").into_iter().rev().take(40) {
        let Some(session_id) = execution["sessionId"].as_str() else {
            continue;
        };
        let partition = MemoryScope::for_session(
            &state.store,
            session_id,
            execution["agentId"].as_str(),
        );
        if partition.partition_key != scope.partition_key {
            continue;
        }
        match execution["status"].as_str() {
            Some("completed") => {
                total += 1;
                success += 1;
            }
            Some("failed") => total += 1,
            _ => {}
        }
        if total >= 20 {
            break;
        }
    }
    if total == 0 {
        0.5
    } else {
        success as f64 / total as f64
    }
}

pub fn active_values(state: &AppState, scope: &MemoryScope) -> Vec<Value> {
    state
        .store
        .all("improvement")
        .into_iter()
        .filter(|improvement| {
            matches!(
                improvement["status"].as_str(),
                Some("adopted" | "validated")
            ) && matches!(
                improvement["kind"].as_str(),
                Some("strategy" | "tool_policy")
            ) && visible(improvement, scope)
        })
        .take(6)
        .collect()
}

/// IDs of the improvements that will be injected into this turn's prompt.
pub fn injected_ids(state: &AppState, scope: &MemoryScope) -> Vec<String> {
    active_values(state, scope)
        .into_iter()
        .filter_map(|improvement| improvement["id"].as_str().map(str::to_string))
        .collect()
}

/// Formats a frozen improvement snapshot without re-querying the database.
pub fn context_block_values(values: &[Value]) -> Option<String> {
    let mut lines = Vec::new();
    for improvement in values.iter().take(6) {
        if !matches!(
            improvement["status"].as_str(),
            Some("adopted" | "validated")
        ) {
            continue;
        }
        let text = improvement["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let label = match improvement["kind"].as_str() {
            Some("tool_policy") => "도구 정책",
            Some("strategy") => "실행 전략",
            _ => continue,
        };
        lines.push(format!("- [{label}] {text}"));
        if improvement["verification"]["type"].as_str() == Some("procedure") {
            lines.push(format!(
                "  required verification: {}",
                improvement["verification"]
                    .to_string()
                    .chars()
                    .take(800)
                    .collect::<String>()
            ));
        }
    }
    (!lines.is_empty()).then(|| {
        format!(
            "Improvements adopted from prior execution outcomes (follow unless evidence says otherwise):\n{}",
            lines.join("\n")
        )
    })
}

/// Adopted strategies/tool policies for the current agent and shared partition.
pub fn context_block(state: &AppState, scope: &MemoryScope) -> Option<String> {
    let mut lines = Vec::new();
    for improvement in active_values(state, scope) {
        let text = improvement["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let label = match improvement["kind"].as_str() {
            Some("tool_policy") => "도구 정책",
            Some("strategy") => "실행 전략",
            _ => continue,
        };
        lines.push(format!("- [{label}] {text}"));
        if improvement["verification"]["type"].as_str() == Some("procedure") {
            lines.push(format!(
                "  required verification: {}",
                improvement["verification"]
                    .to_string()
                    .chars()
                    .take(800)
                    .collect::<String>()
            ));
        }
    }
    (!lines.is_empty()).then(|| {
        format!(
            "Improvements adopted from prior execution outcomes (follow unless evidence says otherwise):\n{}",
            lines.join("\n")
        )
    })
}

/// Computes the improvement records that a terminal execution should apply.
/// This function never writes; `finalize_once` commits the result atomically.
pub fn execution_updates(
    state: &AppState,
    execution: &Value,
) -> Result<Vec<Value>, String> {
    let Some(session_id) = execution["sessionId"].as_str() else {
        return Ok(Vec::new());
    };
    let agent_id = execution["agentId"].as_str();
    let scope = MemoryScope::for_session(&state.store, session_id, agent_id);
    let run_id = execution["id"].as_str().unwrap_or("");
    let task_outcome = execution["taskOutcome"].as_str().unwrap_or("");
    let applied = execution["appliedImprovementIds"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    let compliance = execution["strategyCompliance"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item["strategyId"].as_str()?;
                    Some((id.to_string(), item.clone()))
                })
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();
    let operations = state.store.try_all("execution_operation")?;
    let mut updates = Vec::new();
    for mut improvement in state.store.try_all("improvement")? {
        if improvement["scoreVersion"].as_u64().unwrap_or(1) < 2 {
            continue;
        }
        let id = improvement["id"].as_str().unwrap_or("");
        if improvement["status"].as_str() != Some("adopted")
            || !visible(&improvement, &scope)
            || !applied.contains(id)
        {
            continue;
        }
        let execution_id = execution["id"].as_str().unwrap_or("");
        if !execution_id.is_empty()
            && improvement["appliedExecutionIds"]
                .as_array()
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|item| item.as_str() == Some(execution_id))
                })
        {
            continue;
        }
        let original_uses = improvement["uses"].as_u64().unwrap_or(0);
        let original_successes = improvement["successes"].as_u64().unwrap_or(0);
        let original_failures = improvement["failures"].as_u64().unwrap_or(0);
        let mut uses = original_uses;
        let mut successes = original_successes;
        let mut failures = original_failures;
        if improvement["kind"].as_str() == Some("tool_policy") {
            let Some(tool) = improvement["tool"].as_str() else {
                continue;
            };
            let matching = operations
                .iter()
                .filter(|operation| {
                    operation["runId"].as_str() == Some(run_id)
                        && operation["tool"].as_str() == Some(tool)
                })
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            uses += matching.len() as u64;
            successes += matching
                .iter()
                .filter(|operation| operation["status"] == "completed")
                .count() as u64;
            failures += matching
                .iter()
                .filter(|operation| operation["status"] == "failed")
                .count() as u64;
        } else {
            let Some(entry) = compliance.get(id) else {
                continue;
            };
            if entry["applicable"].as_bool() != Some(true)
                || entry["result"]["verified"].as_bool() != Some(true)
                || entry["result"]["triggered"].as_bool() != Some(true)
                || entry["result"]["status"].as_str() == Some("unknown")
            {
                continue;
            }
            let followed = entry["result"]["followed"].as_bool() == Some(true);
            let success = task_outcome == "succeeded" && followed;
            let failure = task_outcome == "failed"
                || (task_outcome == "succeeded" && !followed);
            if !success && !failure {
                continue;
            }
            uses += 1;
            if success {
                successes += 1;
            } else {
                failures += 1;
            }
            improvement["lastCompliance"] = entry["result"].clone();
        }
        improvement["uses"] = json!(uses);
        improvement["successes"] = json!(successes);
        improvement["failures"] = json!(failures);
        // The counters above are absolute values computed from a stale read.
        // `finalize_once` applies these deltas atomically so two attempts that
        // finish concurrently cannot overwrite each other's observation.
        improvement["__counterDelta"] = json!({
            "uses": uses as i64 - original_uses as i64,
            "successes": successes as i64 - original_successes as i64,
            "failures": failures as i64 - original_failures as i64,
        });
        improvement["lastObservedAt"] = json!(chrono::Utc::now().to_rfc3339());
        let mut applied_ids = improvement["appliedExecutionIds"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !execution_id.is_empty() {
            applied_ids.push(json!(execution_id));
            if applied_ids.len() > 200 {
                applied_ids = applied_ids
                    .split_off(applied_ids.len().saturating_sub(200));
            }
        }
        improvement["appliedExecutionIds"] = json!(applied_ids);
        if uses >= PROBATION_USES {
            let rate = successes as f64 / uses.max(1) as f64;
            let baseline = improvement["baselineSuccess"].as_f64().unwrap_or(0.5);
            if rate + ROLLBACK_MARGIN < baseline {
                improvement["status"] = json!("rolled_back");
                improvement["rolledBackAt"] = json!(chrono::Utc::now().to_rfc3339());
                improvement["reason"] = json!(format!(
                    "관찰 성공률 {:.2} 가 기준 {:.2} 보다 낮아 롤백했습니다.",
                    rate, baseline
                ));
            } else {
                improvement["status"] = json!("validated");
                improvement["evidenceGrade"] = json!("E2");
                improvement["validatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
        }
        updates.push(improvement);
    }
    Ok(updates)
}

/// Legacy wrapper for tests and non-transactional callers.
pub fn record_execution(state: &AppState, execution: &Value) {
    match execution_updates(state, execution) {
        Ok(updates) => {
            for mut saved in updates {
                if let Some(object) = saved.as_object_mut() {
                    object.remove("__counterDelta");
                }
                let rolled_back = saved["rolledBackAt"].is_string();
                let id = saved["id"].clone();
                if let Some(stored) = state.store.put("improvement", &saved) {
                    state.bus.record("improvement", &stored);
                    if rolled_back {
                        state.store.audit(
                            "improvement.rolled_back",
                            json!({"id": id}),
                        );
                    }
                }
            }
        }
        Err(error) => {
            state.store.audit(
                "improvement.update_failed",
                json!({"executionId": execution["id"], "error": error}),
            );
        }
    }
}

/// Selects model-guided or harness-assisted execution for a verified revision.
pub fn set_assistance_mode(state: &AppState, id: &str, mode: &str) -> Option<Value> {
    if !matches!(mode, "model_guided" | "harness_assisted") {
        return None;
    }
    let mut record = state.store.get("improvement", id)?;
    record["assistanceMode"] = json!(mode);
    let saved = state.store.put("improvement", &record)?;
    state.bus.record("improvement", &saved);
    Some(saved)
}

/// Owner corrections are standing constraints; learned strategies are adaptive.
pub fn set_authority(state: &AppState, id: &str, authority: &str) -> Option<Value> {
    let mut record = state.store.get("improvement", id)?;
    let previous = record["revision"].as_str().unwrap_or("").to_string();
    if !previous.is_empty() {
        let mut snapshot = record.clone();
        snapshot["id"] = json!(previous);
        state.store.put("improvement_revision", &snapshot);
    }
    record["previousRevision"] = json!(previous);
    record["authority"] = json!(authority);
    record["revision"] = json!(revision_of(&record));
    let saved = state.store.put("improvement", &record)?;
    state.bus.record("improvement", &saved);
    Some(saved)
}

/// Attaches a normalized, machine-checkable procedure to a strategy.
pub fn attach_verification(
    state: &AppState,
    id: &str,
    verification: &Value,
) -> Option<Value> {
    let mut record = state.store.get("improvement", id)?;
    let previous = record["revision"].as_str().unwrap_or("").to_string();
    if !previous.is_empty() {
        let mut snapshot = record.clone();
        snapshot["id"] = json!(previous);
        state.store.put("improvement_revision", &snapshot);
    }
    record["previousRevision"] = json!(previous);
    record["verification"] = verification.clone();
    record["version"] = json!(record["version"].as_u64().unwrap_or(1) + 1);
    record["verifiedAt"] = json!(chrono::Utc::now().to_rfc3339());
    record["revision"] = json!(revision_of(&record));
    let saved = state.store.put("improvement", &record)?;
    state.bus.record("improvement", &saved);
    state.store.audit(
        "improvement.verification_attached",
        json!({"id": saved["id"], "type": verification["type"]}),
    );
    Some(saved)
}

/// Re-applies finalization statistics after a crash. `record_execution` is
/// idempotent per execution id, so replay is safe.
pub fn recover_finalizations(state: &AppState) {
    for mut marker in state.store.all("execution_finalization") {
        if marker["statsApplied"].as_bool() != Some(false) {
            continue;
        }
        if let Some(execution) = marker["execution"].as_object() {
            record_execution(state, &Value::Object(execution.clone()));
            marker["statsApplied"] = json!(true);
            marker["recoveredAt"] = json!(chrono::Utc::now().to_rfc3339());
            if let Some(saved) = state.store.put("execution_finalization", &marker) {
                state.bus.record("execution_finalization", &saved);
            }
        }
    }
}

pub fn list(state: &AppState) -> Vec<Value> {
    state.store.all("improvement")
}

pub fn set_status(state: &AppState, id: &str, status: &str) -> Option<Value> {
    if !matches!(status, "adopted" | "rejected" | "proposed") {
        return None;
    }
    let mut record = state.store.get("improvement", id)?;
    record["status"] = json!(status);
    record["reviewedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("improvement", &record)?;
    state.bus.record("improvement", &saved);
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-improve-{}", uuid::Uuid::new_v4()));
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
        state
            .store
            .put("session", &json!({"id":"s","agentId":"a"}));
        (state, dir)
    }

    #[test]
    fn tool_policy_requires_failure_statistics_and_names_the_tool() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "tool_stat",
            &json!({"id":"shell_exec","attempts":4,"successes":1,"failures":3}),
        );
        let adopted = propose(
            &state,
            &scope,
            "tool_policy",
            "shell_exec 대신 workspace_write를 먼저 사용한다",
            "셸 실패가 반복됨",
            &["m1".into()],
            Some("shell_exec"),
            "shared",
        )
        .unwrap();
        assert_eq!(adopted["status"], "adopted");
        assert!(context_block(&state, &scope)
            .unwrap()
            .contains("workspace_write"));

        let rejected = propose(
            &state,
            &scope,
            "tool_policy",
            "다른 도구를 써라",
            "근거",
            &["m2".into()],
            Some("shell_exec"),
            "shared",
        )
        .unwrap();
        assert_eq!(rejected["status"], "rejected");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn probation_rolls_back_a_regressing_improvement() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let adopted = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write로 먼저 시도한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(adopted["status"], "adopted");
        for index in 0..5 {
            let execution = json!({"id":format!("e{index}"),"sessionId":"s","agentId":"a","status":"failed","toolCalls":1,"taskOutcome":"failed","appliedImprovementIds":[adopted["id"]],
                "strategyCompliance":[{"strategyId":adopted["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":false}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", adopted["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "rolled_back");
        assert!(context_block(&state, &scope).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_and_agent_scopes_are_separate_and_harness_is_proposal_only() {
        let (state, dir) = state();
        state
            .store
            .put("session", &json!({"id":"t","agentId":"b"}));
        let scope_a = MemoryScope::for_session(&state.store, "s", Some("a"));
        let scope_b = MemoryScope::for_session(&state.store, "t", Some("b"));
        propose(
            &state,
            &scope_a,
            "strategy",
            "A만 쓰는 전략",
            "A 에이전트의 실행에서 확인된 근거",
            &["m1".into()],
            None,
            "agent",
        )
        .unwrap();
        assert!(context_block(&state, &scope_b).is_none());
        let harness = propose(
            &state,
            &scope_a,
            "harness",
            "도구 루프에 재시도 예산을 추가한다",
            "반복 실패",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(harness["status"], "proposed");
        assert!(!context_block(&state, &scope_a)
            .unwrap_or_default()
            .contains("재시도 예산"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validated_improvements_are_injected_after_probation() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "검증된 파일 절차를 재사용한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({"id":format!("ok{index}"),"sessionId":"s","agentId":"a","status":"completed","toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[record["id"]],
                "strategyCompliance":[{"strategyId":record["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":true}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "validated");
        assert!(context_block(&state, &scope)
            .unwrap_or_default()
            .contains("검증된 파일 절차"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validated_strategy_wins_over_a_similar_duplicate() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let first = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write로 먼저 시도한다",
            "실패 후 성공한 절차를 다음 턴에 재사용한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({"id":format!("v{index}"),"sessionId":"s","agentId":"a","status":"completed","toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[first["id"]],
                "strategyCompliance":[{"strategyId":first["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":true}}]});
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let duplicate = propose(
            &state,
            &scope,
            "strategy",
            "파일 작업은 workspace_write를 먼저 시도한다",
            "같은 절차가 다시 관찰되어 중복 생성을 막는다",
            &["m2".into()],
            None,
            "shared",
        )
        .unwrap();
        assert_eq!(duplicate["id"], first["id"]);
        assert_eq!(duplicate["status"], "validated");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_without_tool_work_does_not_validate_a_strategy() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "도구 없이 완료 선언하지 않는다",
            "도구 실행이 없으면 전략 검증으로 집계하지 않는다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for index in 0..5 {
            let execution = json!({
                "id":format!("bare{index}"),"sessionId":"s","agentId":"a",
                "status":"completed","toolCalls":0,
                "appliedImprovementIds":[record["id"]]
            });
            state.store.put("execution", &execution);
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["status"], "adopted");
        assert_eq!(stored["uses"], 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn global_strategies_cross_partitions() {
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
        let record = propose(
            &state,
            &local,
            "strategy",
            "전역 파일 절차를 재사용한다",
            "채널과 무관하게 적용할 절차",
            &["m1".into()],
            None,
            "global",
        )
        .unwrap();
        assert_eq!(record["scope"], "global");
        assert!(context_block(&state, &discord)
            .unwrap_or_default()
            .contains("전역 파일 절차"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_execution_is_counted_once() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "같은 실행을 두 번 집계하지 않는다",
            "finalization 재시도에도 카운터가 한 번만 증가해야 한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        let execution = json!({
            "id":"exec-once","sessionId":"s","agentId":"a","status":"completed",
            "toolCalls":1,"taskOutcome":"succeeded","appliedImprovementIds":[record["id"]],
            "strategyCompliance":[{
                "strategyId":record["id"],"applicable":true,
                "result":{"verified":true,"triggered":true,"followed":true}
            }]
        });
        record_execution(&state, &execution);
        record_execution(&state, &execution);
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["uses"], 1);
        assert_eq!(stored["successes"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn revisions_are_immutable_snapshots() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "revision 스냅샷을 남긴다",
            "검증 정의가 바뀌어도 이전 revision을 보존한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        let first = record["revision"].as_str().unwrap().to_string();
        assert_eq!(record["scoreVersion"], 2);
        let updated = attach_verification(
            &state,
            record["id"].as_str().unwrap(),
            &json!({"type":"procedure","after":[{"kind":"file_exists","path":"a.txt"}]}),
        )
        .unwrap();
        assert_ne!(updated["revision"], first);
        assert_eq!(updated["previousRevision"], first);
        let old = state
            .store
            .get("improvement_revision", &first)
            .unwrap();
        assert_eq!(old["revision"], first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_outcome_gates_strategy_success() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let record = propose(
            &state,
            &scope,
            "strategy",
            "taskOutcome으로 전략 성과를 판정한다",
            "completed와 검증된 성공을 분리한다",
            &["m1".into()],
            None,
            "shared",
        )
        .unwrap();
        for (id, outcome, followed) in [
            ("e1", "succeeded", true),
            ("e2", "succeeded", false),
            ("e3", "answered_unverified", true),
            ("e4", "failed", true),
        ] {
            let execution = json!({
                "id":id,"sessionId":"s","agentId":"a","status":"completed",
                "taskOutcome":outcome,"toolCalls":1,
                "appliedImprovementIds":[record["id"]],
                "strategyCompliance":[{
                    "strategyId":record["id"],"applicable":true,
                    "result":{"verified":true,"triggered":true,"followed":followed}
                }]
            });
            record_execution(&state, &execution);
        }
        let stored = state
            .store
            .get("improvement", record["id"].as_str().unwrap())
            .unwrap();
        assert_eq!(stored["uses"], 3);
        assert_eq!(stored["successes"], 1);
        assert_eq!(stored["failures"], 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
