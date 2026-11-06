//! Model-agnostic principles extracted from verified adaptations.
//!
//! A task-specific rule says "after `github_api` failed with 409, call
//! `github_api` read". That is useful but does not transfer. A principle says
//! "before mutating an external resource, observe its current state/version",
//! which applies to GitHub, databases, HTTP APIs and filesystem writes.
//!
//! Principles are global, carry no provider/model gate, and are only created
//! from a verified successful adaptation. They are suggestions, never
//! auto-executed arguments.
use crate::{engine::experience, state::AppState};
use serde_json::{json, Value};

fn domain(tool: &str) -> &'static str {
    if tool.starts_with("github") {
        "source_control"
    } else if tool.starts_with("workspace_") || tool.starts_with("file_") {
        "filesystem"
    } else if tool.starts_with("web_") || tool.starts_with("http_") {
        "http_api"
    } else if tool.starts_with("memory_") {
        "memory"
    } else if tool.starts_with("shell") || tool.starts_with("terminal") {
        "system"
    } else if tool.contains("sql") || tool.contains("database") {
        "database"
    } else {
        "generic"
    }
}

fn is_mutation(tool: &str) -> bool {
    [
        "write", "update", "delete", "remove", "patch", "put", "post", "create", "move",
        "rename", "edit", "commit", "push", "archive", "insert", "replace", "exec",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

fn is_observation(tool: &str) -> bool {
    [
        "read", "list", "get", "status", "head", "search", "inspect", "show", "diff",
        "fetch", "verify", "check", "query", "describe",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

fn observed_success(value: &Value) -> bool {
    value.is_object()
        && !value.as_object().is_some_and(|object| object.is_empty())
        && value["outcomeUnknown"].as_bool() != Some(true)
        && value["isError"].as_bool() != Some(true)
        && value["success"].as_bool() != Some(false)
        && value["error"].is_null()
        && value.get("exitCode").is_none_or(|code| code.as_i64() == Some(0))
        && value
            .get("status")
            .and_then(Value::as_u64)
            .is_none_or(|status| status < 400)
}

fn principle_text(kind: &str, class: &str) -> &'static str {
    match (kind, class) {
        ("verify-before-mutate", "stale_state") => {
            "변경 전에 대상의 현재 상태와 버전을 읽고, 기대한 revision과 일치하는지 확인한 뒤 수정한다."
        }
        ("verify-before-mutate", "not_found") => {
            "행동 전에 대상이 실제로 존재하는지 확인하고, 없으면 생성·경로·대상을 먼저 확정한다."
        }
        ("verify-before-mutate", _) => {
            "변경 전에 읽기·상태 확인 도구로 현재 상태를 관찰한 뒤 수정한다."
        }
        _ => "실패한 작업의 조건을 먼저 관찰하고, 확인된 조건에 맞춰 다음 행동을 바꾼다.",
    }
}

fn principle_id(kind: &str, class: &str) -> String {
    format!(
        "principle-{}",
        crate::engine::memory::stable_id(&[kind, class])
    )
}

fn union_strings(record: &mut Value, key: &str, value: &str, limit: usize) {
    if value.is_empty() {
        return;
    }
    let mut items = record[key].as_array().cloned().unwrap_or_default();
    if !items.iter().any(|item| item.as_str() == Some(value)) {
        items.push(json!(value));
        items.truncate(limit);
    }
    record[key] = json!(items);
}

/// Learns principles only from a verified successful adaptation. The source
/// tool/domain is evidence, never an applicability requirement.
pub fn learn_from_execution(state: &AppState, execution: &Value) -> usize {
    if execution["taskOutcome"].as_str() != Some("succeeded") {
        return 0;
    }
    let Some(execution_id) = execution["id"].as_str() else {
        return 0;
    };
    let model = execution["model"]
        .as_str()
        .or_else(|| execution["effectiveModel"].as_str())
        .unwrap_or("");
    let agent = execution["agentId"].as_str().unwrap_or("");
    let mut learned = 0usize;
    for adaptation in execution["adaptations"].as_array().into_iter().flatten().take(20) {
        let failed_tool = adaptation["observation"]["tool"].as_str().unwrap_or("");
        let selected_tool = adaptation["selected"]["tool"].as_str().unwrap_or("");
        if !is_mutation(failed_tool) || !is_observation(selected_tool) {
            continue;
        }
        if !observed_success(&adaptation["result"]) {
            continue;
        }
        let error = adaptation["observation"]["actual"]
            .as_str()
            .or_else(|| adaptation["observation"]["error"].as_str())
            .unwrap_or("");
        let class = experience::failure_class(error).unwrap_or("unknown");
        let kind = "verify-before-mutate";
        let id = principle_id(kind, class);
        let mut principle = state.store.get("experience_principle", &id).unwrap_or_else(|| {
            json!({
                "id": id,
                "kind": kind,
                "scope": "global",
                "status": "active",
                "failureClasses": [class],
                "sourceDomains": [],
                "principle": principle_text(kind, class),
                "suggestedActions": ["read", "list", "get", "status", "head", "diff"],
                "evidenceGrade": "E1_observed",
                "confidence": 0.5,
                "evidenceExecutionIds": [],
                "supportingCases": [],
                "supportingModels": [],
                "supportingAgents": [],
                "counterexamples": [],
                "createdAt": chrono::Utc::now().to_rfc3339(),
            })
        });
        union_strings(&mut principle, "evidenceExecutionIds", execution_id, 64);
        union_strings(&mut principle, "supportingModels", model, 16);
        union_strings(&mut principle, "supportingAgents", agent, 16);
        union_strings(
            &mut principle,
            "sourceDomains",
            domain(failed_tool),
            16,
        );
        union_strings(
            &mut principle,
            "sourceDomains",
            domain(selected_tool),
            16,
        );
        let cases = principle["evidenceExecutionIds"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        principle["confidence"] = json!((0.5 + 0.1 * cases as f64).min(0.95));
        principle["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        if state.store.put("experience_principle", &principle).is_some() {
            learned += 1;
        }
    }
    learned
}

/// Returns model-agnostic guidance for a mutation failure. The match is on the
/// structural failure class and the fact that the failed tool mutates; the tool
/// name and provider model are deliberately not part of applicability.
pub fn context_block(
    state: &AppState,
    agent_id: Option<&str>,
    model: &str,
    failed_tool: &str,
    error: &str,
    execution_id: Option<&str>,
) -> Option<String> {
    if !is_mutation(failed_tool) {
        return None;
    }
    let class = experience::failure_class(error).unwrap_or("unknown");
    let mut applicable = state
        .store
        .all("experience_principle")
        .into_iter()
        .filter(|principle| {
            principle["status"].as_str() == Some("active")
                && principle["failureClasses"].as_array().is_some_and(|classes| {
                    classes.is_empty()
                        || classes.iter().any(|value| {
                            value.as_str() == Some(class) || value.as_str() == Some("unknown")
                        })
                })
        })
        .collect::<Vec<_>>();
    applicable.sort_by(|a, b| {
        b["supportingCases"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0)
            .cmp(&a["supportingCases"].as_array().map(Vec::len).unwrap_or(0))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    if applicable.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for principle in applicable.iter().take(3) {
        if let (Some(execution_id), Some(principle_id)) =
            (execution_id, principle["id"].as_str())
        {
            let usage_id = format!(
                "principle-use-{}",
                crate::engine::memory::stable_id(&[execution_id, principle_id])
            );
            if state.store.get("principle_usage", &usage_id).is_none() {
                let usage = json!({
                    "id": usage_id,
                    "executionId": execution_id,
                    "principleId": principle_id,
                    "agentId": agent_id,
                    "model": model,
                    "failureClass": class,
                    "status": "pending",
                    "createdAt": chrono::Utc::now().to_rfc3339(),
                });
                let _ = state.store.put("principle_usage", &usage);
            }
        }
        lines.push(format!(
            "- {} (검증 사례 {}건, 모델 {}개, 출처 도메인: {})",
            principle["principle"].as_str().unwrap_or(""),
            principle["supportingCases"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            principle["supportingModels"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            principle["sourceDomains"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Some(format!(
        "Generalized principles from VERIFIED past runs (model-agnostic; apply to the current resource, never copy old arguments):\n{}",
        lines.join("\n")
    ))
}

/// Closes the loop: a principle used in a successful run gains a supporting
/// case; a failed run is kept as a counterexample. This is what lets the same
/// principle improve across models and agents.
pub fn finalize_usage(state: &AppState, execution: &Value, succeeded: bool) -> usize {
    let Some(execution_id) = execution["id"].as_str() else {
        return 0;
    };
    let mut finalized = 0usize;
    for mut usage in state.store.all("principle_usage") {
        if usage["executionId"].as_str() != Some(execution_id)
            || usage["status"].as_str() != Some("pending")
        {
            continue;
        }
        let Some(principle_id) = usage["principleId"].as_str() else {
            continue;
        };
        let Some(mut principle) = state.store.get("experience_principle", principle_id) else {
            continue;
        };
        if succeeded {
            union_strings(&mut principle, "supportingCases", execution_id, 64);
            union_strings(
                &mut principle,
                "supportingModels",
                usage["model"].as_str().unwrap_or(""),
                16,
            );
            union_strings(
                &mut principle,
                "supportingAgents",
                usage["agentId"].as_str().unwrap_or(""),
                16,
            );
            let cases = principle["supportingCases"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0);
            principle["confidence"] = json!((0.5 + 0.1 * cases as f64).min(0.95));
            usage["status"] = json!("supported");
        } else {
            let mut counterexamples = principle["counterexamples"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let counterexample = json!({
                "executionId": execution_id,
                "failureClass": usage["failureClass"],
                "model": usage["model"],
                "agentId": usage["agentId"],
            });
            if !counterexamples.iter().any(|item| item == &counterexample) {
                counterexamples.push(counterexample);
                counterexamples.truncate(16);
            }
            principle["counterexamples"] = json!(counterexamples);
            principle["confidence"] =
                json!((principle["confidence"].as_f64().unwrap_or(0.5) - 0.1).max(0.1));
            usage["status"] = json!("counterexample");
        }
        principle["lastUsedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("experience_principle", &principle);
        usage["finalizedAt"] = json!(chrono::Utc::now().to_rfc3339());
        state.store.put("principle_usage", &usage);
        finalized += 1;
    }
    finalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-principle-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(), web_dir: dir.join("web"), workspace: dir.clone(),
        };
        (
            AppState::new(
                config.clone(),
                Store::open(&config.db_path()).unwrap(),
                Vault::open(&config.key_path()),
            ),
            dir,
        )
    }

    fn execution(model: &str, agent: &str, failed: &str, selected: &str, error: &str) -> Value {
        json!({
            "id": format!("run-{agent}"),
            "taskOutcome": "succeeded",
            "model": model,
            "agentId": agent,
            "adaptations": [{
                "observation": {"tool": failed, "actual": error},
                "selected": {"tool": selected, "inputs": {"path": "safe"}},
                "result": {"success": true}
            }]
        })
    }

    #[test]
    fn verified_github_failure_creates_a_global_principle() {
        let (state, dir) = state();
        assert_eq!(
            learn_from_execution(
                &state,
                &execution(
                    "model-a",
                    "agent-a",
                    "github_api_update",
                    "github_api_get",
                    "409 conflict: sha mismatch"
                )
            ),
            1
        );
        let principles = state.store.all("experience_principle");
        assert_eq!(principles.len(), 1);
        assert_eq!(principles[0]["kind"], "verify-before-mutate");
        assert_eq!(principles[0]["scope"], "global");
        assert!(principles[0]["sourceDomains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "source_control"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn principle_transfers_to_another_domain_and_model() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "github_api_update",
                "github_api_get",
                "409 conflict: sha mismatch",
            ),
        );
        for (index, (agent, model, tool, run)) in [
            ("agent-b", "model-b", "sql_update", "run-b"),
            ("agent-c", "model-c", "http_post", "run-c"),
            ("agent-d", "model-d", "workspace_write", "run-d"),
        ]
        .into_iter()
        .enumerate()
        {
            let guidance = context_block(
                &state,
                Some(agent),
                model,
                tool,
                "409 conflict: stale state",
                Some(run),
            )
            .unwrap_or_else(|| panic!("cross-domain principle missing for {tool}"));
            assert!(guidance.contains("현재 상태와 버전"), "{guidance}");
            let execution = json!({"id":run,"model":model,"agentId":agent});
            assert_eq!(finalize_usage(&state, &execution, true), 1);
            assert_eq!(state.store.all("principle_usage").len(), index + 1);
        }
        let principle = &state.store.all("experience_principle")[0];
        for model in ["model-b", "model-c", "model-d"] {
            assert!(principle["supportingModels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == model), "{principle}");
        }
        assert_eq!(principle["supportingCases"].as_array().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn observations_do_not_receive_mutation_principles() {
        let (state, dir) = state();
        learn_from_execution(
            &state,
            &execution(
                "model-a",
                "agent-a",
                "workspace_write",
                "workspace_read",
                "no such file",
            ),
        );
        assert!(context_block(
            &state,
            None,
            "model-b",
            "workspace_read",
            "no such file",
            Some("run-c"),
        )
        .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
