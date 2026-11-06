//! Model-agnostic principles extracted from verified adaptations.
//!
//! A task-specific rule says "after `github_api` failed with 409, call
//! `github_api` read". That is useful but does not transfer. A principle says
//! "before mutating an external resource, observe its current state/version",
//! which applies to GitHub, databases, HTTP APIs and filesystem writes.
//!
//! Principles are global, carry no provider/model gate, and are only created
//! from a verified successful adaptation. Policy promotion is intentionally
//! opt-in and is not performed by this module.
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
    } else if tool.starts_with("mcp__") {
        "external_tool"
    } else {
        "generic"
    }
}

fn is_mutation(tool: &str, arguments: &Value) -> bool {
    if tool == "github_api" {
        return arguments["method"].as_str().is_some_and(|method| {
            matches!(method.to_ascii_uppercase().as_str(), "PUT" | "POST" | "PATCH" | "DELETE")
        });
    }
    [
        "write", "update", "delete", "remove", "patch", "put", "post", "create", "move",
        "rename", "edit", "commit", "push", "archive", "insert", "replace", "exec",
    ]
    .iter()
    .any(|word| tool.to_ascii_lowercase().contains(word))
}

fn is_observation(tool: &str, arguments: &Value) -> bool {
    if tool == "github_api" {
        return arguments["method"].as_str().is_none_or(|method| method.eq_ignore_ascii_case("GET"));
    }
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

const DOMAINS: &[&str] = &[
    "filesystem",
    "source_control",
    "http_api",
    "database",
    "external_tool",
    "system",
];

/// Capabilities are described by role, not by a concrete argument. A domain
/// only supports a composition when it can observe a version, mutate, and
/// independently verify. `available:false` means the principle may still be
/// retrieved as advice, but it must not become an executable strategy there.
fn capability_bindings(domain: &str) -> Value {
    match domain {
        "filesystem" => json!({
            "observe": ["workspace_read", "file_read"],
            "mutate": ["workspace_write", "file_write"],
            "verify": {"kind": "sha256_readback", "available": true},
            "version_source": "sha256",
            "conditional_update": "content_addressed_write",
        }),
        "source_control" => json!({
            "observe": ["github_api:GET"],
            "mutate": ["github_api:PUT", "github_api:PATCH", "github_api:DELETE"],
            "verify": {"kind": "read_after_write", "available": true},
            "version_source": "git_object_sha_or_etag",
            "conditional_update": "sha_conditional_update",
        }),
        "http_api" => json!({
            "observe": ["http_fetch"],
            "mutate": ["http_request"],
            "verify": {"kind": "read_after_write", "available": false},
            "version_source": "etag_or_last_modified",
            "conditional_update": "if_match",
        }),
        "database" => json!({
            "observe": ["sql_query"],
            "mutate": ["sql_exec"],
            "verify": {"kind": "read_after_write", "available": false},
            "version_source": "row_version_or_updated_at",
            "conditional_update": "compare_and_set",
        }),
        "external_tool" => json!({
            "observe": ["mcp__*__read", "mcp__*__list"],
            "mutate": ["mcp__*__write", "mcp__*__update"],
            "verify": {"kind": "server_declared_readback", "available": false},
            "version_source": "server_declared_etag",
            "conditional_update": "server_declared_idempotency_key",
        }),
        _ => json!({
            "observe": ["shell_exec", "terminal_exec"],
            "mutate": ["shell_exec", "terminal_exec"],
            "verify": {"kind": "artifact_or_independent_test", "available": false},
            "version_source": "none",
            "conditional_update": "none",
        }),
    }
}

fn failure_cause(class: &str) -> &'static str {
    match class {
        "stale_state" => "변경 대상의 현재 버전과 요청이 가정한 버전이 달랐다.",
        "not_found" => "대상 경로나 리소스가 실제로 존재하지 않았다.",
        "timeout" => "호출이 끝났지만 외부 효과의 상태가 확정되지 않았다.",
        "permission_denied" => "호출자에게 해당 변경 권한이 없었다.",
        "rate_limited" => "제공자 쿼터가 소진되어 관찰이나 변경이 거절됐다.",
        "invalid_inputs" => "인자가 도구 스키마나 대상의 형식과 맞지 않았다.",
        _ => "실패 원인이 관찰되지 않았다. 이 원리는 자동 적용하지 않는다.",
    }
}

fn failure_layer(class: &str) -> &'static str {
    match class {
        "timeout" => "execution",
        "permission_denied" | "rate_limited" => "authority",
        "invalid_inputs" => "tool_arguments",
        "stale_state" | "not_found" => "state",
        _ => "unknown",
    }
}

/// Structured form of a principle. This keeps the original one-line text for
/// context injection, but adds the fields a composer needs: conditions,
/// exclusions, typed procedure roles, expected evidence and cost.
fn synthesize(kind: &str, class: &str, failed_tool: &str, selected_tool: &str) -> Value {
    let source_domain = domain(failed_tool);
    let bindings = capability_bindings(source_domain);
    let condition = match class {
        "stale_state" => json!({
            "kind": "version_match",
            "expr": "beforeVersion is known and equals the version just observed",
            "required": true,
        }),
        "not_found" => json!({
            "kind": "existence",
            "expr": "target existence is confirmed before creating or retrying",
            "required": true,
        }),
        "timeout" => json!({
            "kind": "effect_state",
            "expr": "previous attempt has no confirmed effect, or has been inspected",
            "required": true,
        }),
        _ => json!({
            "kind": "observed_failure",
            "expr": "the same structural failure class is observed on the current task",
            "required": true,
        }),
    };
    let mut exclusions = vec![
        "대상의 현재 버전을 읽을 수단이 없다.",
        "변경이 관찰한 버전에 조건부로 묶여 있지 않다.",
        "치환 대상이 여러 개라 하나로 특정할 수 없다.",
        "권한이 거절되었거나 요청 자체가 승인되지 않았다.",
        "비가역적 외부 효과이고 idempotency key나 조회 수단이 없다.",
    ];
    if class == "timeout" {
        exclusions.push("이전 시도의 효과 상태를 조회하기 전에는 재전송하지 않는다.");
    }
    json!({
        "schemaVersion": 2,
        "kind": kind,
        "failureCause": failure_cause(class),
        "failureLayer": failure_layer(class),
        "conditions": [
            condition,
            json!({
                "kind": "mutation",
                "expr": "the current tool mutates the same resource_key role",
                "required": true,
            }),
            json!({
                "kind": "version_capability",
                "expr": "the target exposes a revision, etag, sha or row version",
                "required": true,
            }),
        ],
        "exclusions": exclusions,
        "procedure": [
            {
                "step": 1, "operator": "observe", "role": "read_current_version",
                "bindings": bindings["observe"],
                "produces": ["observedVersion", "resourceKey"],
            },
            {
                "step": 2, "operator": "compare", "role": "match_expected_version",
                "requires": ["observedVersion is not null"],
                "onMismatch": "reobserve_once_then_stop",
            },
            {
                "step": 3, "operator": "mutate", "role": "conditional_write",
                "bindings": bindings["mutate"],
                "requires": ["compare matched"],
                "versionSource": bindings["version_source"],
                "conditionalUpdate": bindings["conditional_update"],
            },
            {
                "step": 4, "operator": "verify", "role": "independent_readback",
                "bindings": bindings["verify"],
                "acceptsModelClaim": false,
            },
        ],
        "expectedEvidence": {
            "kind": "version_equals",
            "description": "관찰한 버전과 변경 후 버전이 일치하거나, 되읽은 내용의 해시가 요청과 일치한다.",
            "independent": true,
        },
        "verification": {
            "owner": "harness",
            "method": bindings["verify"]["kind"],
            "modelClaimAccepted": false,
        },
        "cost": {"modelCalls": 0, "extraToolCalls": 1, "estTokens": 0},
        "provenance": {
            "sourceTool": failed_tool,
            "recoveryTool": selected_tool,
            "sourceDomain": source_domain,
            "capabilities": bindings,
        },
        "counterexamples": [],
        "autoApply": false,
    })
}

/// Composes an executable strategy candidate from a principle only when the
/// evidence is independent and the domain exposes observe/mutate/verify. It
/// never marks the candidate promotable: a paired holdout must run first.
pub fn compose(principle: &Value) -> Option<Value> {
    let principle_id = principle["id"].as_str()?;
    let evidence = principle["evidenceExecutionIds"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let cases = principle["supportingCases"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    let counterexamples = principle["counterexamples"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    if evidence < 2 || cases < 2 || counterexamples >= cases + 1 {
        return None;
    }
    let mut supported = Vec::new();
    let mut unavailable = Vec::new();
    for domain in DOMAINS {
        let bindings = capability_bindings(domain);
        let executable = bindings["mutate"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
            && bindings["verify"]["available"].as_bool() == Some(true);
        if executable {
            supported.push(json!({
                "domain": domain,
                "observe": bindings["observe"],
                "mutate": bindings["mutate"],
                "verify": bindings["verify"],
                "versionSource": bindings["version_source"],
            }));
        } else {
            unavailable.push(json!({
                "domain": domain,
                "reason": "관찰·변경·독립 검증 중 하나 이상이 도구 카탈로그에 없다.",
            }));
        }
    }
    if supported.is_empty() {
        return None;
    }
    let revision = crate::engine::memory::stable_id(&[
        principle_id,
        principle["updatedAt"].as_str().unwrap_or(""),
    ]);
    let id = format!("strategy-composition-{revision}");
    Some(json!({
        "id": id,
        "schemaVersion": 2,
        "kind": "strategy_composition",
        "status": "candidate",
        "promotionEligible": false,
        "promotionReason": "paired holdout evidence가 없으므로 운영 정책으로 승격하지 않는다.",
        "principleId": principle_id,
        "parentRevision": revision,
        "procedure": principle["procedure"],
        "conditions": principle["conditions"],
        "exclusions": principle["exclusions"],
        "expectedEvidence": principle["expectedEvidence"],
        "supportedDomains": supported,
        "unavailableDomains": unavailable,
        "evidence": {
            "executionIds": principle["evidenceExecutionIds"],
            "supportingCases": principle["supportingCases"],
            "supportingModels": principle["supportingModels"],
            "supportingAgents": principle["supportingAgents"],
            "counterexamples": principle["counterexamples"],
            "evidenceGrade": principle["evidenceGrade"],
        },
        "costBound": {"extraToolCalls": 1, "modelCalls": 0},
        "experiment": {
            "status": "not_run",
            "baseline": "existing path",
            "treatment": ["observe_version", "compare", "conditional_mutate", "independent_readback"],
            "gates": {
                "minSuccessGain": 0.10,
                "tokenRatioMax": 1.25,
                "forbidDuplicateEffect": true,
                "forbidAuthorityWidening": true,
            },
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    }))
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

/// A model may recover in its ordinary tool loop without invoking the separate
/// adaptation planner. Require all three operations on one typed resource.
fn observed_recoveries(state: &AppState, execution_id: &str) -> Vec<Value> {
    let Ok(operations) = state.store.execution_operations(execution_id) else {
        return Vec::new();
    };
    fn resource(operation: &Value) -> Option<(&str, &str, &str)> {
        let tool = operation["tool"].as_str()?;
        let arguments = &operation["arguments"];
        let (scope, keys): (_, &[&str]) = match tool {
            "github_api" => ("github_api", &["path"]),
            "workspace_read" | "workspace_write" => ("workspace", &["path"]),
            "file_read" | "file_write" => ("file", &["path"]),
            tool if tool.starts_with("mcp__") => {
                (tool.rsplit_once("__")?.0, &["key", "id", "path"])
            }
            _ => return None,
        };
        keys.iter()
            .find_map(|key| arguments[*key].as_str().map(|value| (scope, *key, value)))
    }
    let mut recoveries = Vec::new();
    for (index, failed) in operations.iter().enumerate() {
        let tool = failed["tool"].as_str().unwrap_or_default();
        if failed["status"] != "failed" || !is_mutation(tool, &failed["arguments"]) {
            continue;
        }
        let error = failed["result"]["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| failed["result"].to_string());
        if failed["result"]["status"] != 409
            && experience::failure_class(&error) != Some("stale_state")
        {
            continue;
        }
        let Some(target) = resource(failed) else {
            continue;
        };
        let mut observation = None;
        for operation in &operations[index + 1..] {
            if operation["status"] != "completed" || resource(operation) != Some(target) {
                continue;
            }
            let name = operation["tool"].as_str().unwrap_or_default();
            if is_observation(name, &operation["arguments"])
                && observed_success(&operation["result"])
            {
                observation = Some(operation);
            } else if name == tool && is_mutation(name, &operation["arguments"]) {
                if let Some(observation) = observation {
                    recoveries.push(json!({
                        "observation":{"tool":tool,"arguments":failed["arguments"],"actual":format!("stale state: {error}")},
                        "selected":{"tool":observation["tool"],"inputs":observation["arguments"]},
                        "result":observation["result"],
                    }));
                }
                break;
            }
        }
    }
    recoveries
}

/// Stores a structured composition once the principle has independent cases.
/// The composition is never activated here; it only becomes an experiment
/// candidate that the existing promotion path may later evaluate.
fn ensure_composition(state: &AppState, principle: &mut Value) -> bool {
    if principle["strategyCompositionId"].as_str().is_some() {
        return false;
    }
    let Some(composition) = compose(principle) else {
        return false;
    };
    let Some(saved) = state.store.put("strategy_composition", &composition) else {
        return false;
    };
    principle["strategyCompositionId"] = saved["id"].clone();
    principle["strategyComposedAt"] = json!(chrono::Utc::now().to_rfc3339());
    if let Some(candidate_id) = principle["strategyCandidateId"].as_str() {
        if let Some(mut candidate) = state.store.get("improvement", candidate_id) {
            candidate["compositionId"] = saved["id"].clone();
            candidate["compositionRevision"] = saved["parentRevision"].clone();
            state.store.put("improvement", &candidate);
        }
    }
    true
}

/// Learns principles only from a verified successful adaptation. The source
/// tool/domain is evidence, never an applicability requirement.
pub fn learn_from_execution(state: &AppState, execution: &Value) -> usize {
    if execution["taskOutcome"].as_str() != Some("succeeded") {
        return 0;
    }
    if execution["deterministicShortcut"].is_object() {
        return 0;
    }
    // A succeeded task can still contain an unresolved external effect. It is
    // evidence for the task result, not for a reusable positive rule.
    if execution["effectSummary"]["unverifiedEffects"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        return 0;
    }
    let Some(execution_id) = execution["id"].as_str() else {
        return 0;
    };
    let _guard = state.principle_learning.lock().unwrap();
    let model = execution["effectiveModel"]
        .as_str()
        .or_else(|| execution["model"].as_str())
        .unwrap_or("");
    let agent = execution["agentId"].as_str().unwrap_or("");
    let mut learned = 0usize;
    let recoveries = observed_recoveries(state, execution_id);
    for adaptation in execution["adaptations"].as_array().into_iter().flatten().take(20)
        .chain(&recoveries)
    {
        let failed_tool = adaptation["observation"]["tool"].as_str().unwrap_or("");
        let selected_tool = adaptation["selected"]["tool"].as_str().unwrap_or("");
        if !is_mutation(failed_tool, &adaptation["observation"]["arguments"])
            || !is_observation(selected_tool, &adaptation["selected"]["inputs"])
        {
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
        // Structured conditions, exclusions, typed procedure, expected
        // evidence and cost. The counterexample list stays owned by usage.
        let synthesized = synthesize(kind, class, failed_tool, selected_tool);
        if let Some(fields) = synthesized.as_object() {
            for (key, value) in fields {
                if key != "counterexamples" {
                    principle[key] = value.clone();
                }
            }
        }
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
        // The growth cycle does not stop at memory. Once the same principle is
        // backed by two verified executions it becomes a strategy candidate in
        // the existing experiment/promotion pipeline.
        if cases >= 2 && principle["strategyCandidateId"].is_null() {
            let evidence = principle["evidenceExecutionIds"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>();
            let scope = crate::engine::memory::MemoryScope::for_session(
                &state.store,
                execution["sessionId"].as_str().unwrap_or(""),
                execution["agentId"].as_str(),
            );
            let text = format!(
                "{} 적용: 변경/외부 작업 전에 현재 상태와 버전을 관찰하고 기대 조건과 비교한다.",
                principle["principle"].as_str().unwrap_or("")
            );
            if let Some(candidate) = crate::engine::improve::propose(
                state,
                &scope,
                "strategy",
                &text,
                "검증된 실행에서 추출한 일반 원리를 실행 전 검증 전략 후보로 승격",
                &evidence,
                None,
                "global",
            ) {
                principle["strategyCandidateId"] = candidate["id"].clone();
                principle["strategyCandidateAt"] = json!(chrono::Utc::now().to_rfc3339());
            }
            ensure_composition(state, &mut principle);
        }
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
    arguments: &Value,
    error: &str,
    execution_id: Option<&str>,
) -> Option<String> {
    if !is_mutation(failed_tool, arguments) {
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
    let _guard = state.principle_learning.lock().unwrap();
    let mut finalized = 0usize;
    for mut usage in state.store.all("principle_usage") {
        if usage["executionId"].as_str() != Some(execution_id)
            || usage["status"].as_str() != Some("pending")
        {
            continue;
        }
        if matches!(execution["taskOutcome"].as_str(), Some("answered_unverified" | "cancelled")) {
            usage["status"] = json!("unverified");
            usage["finalizedAt"] = json!(chrono::Utc::now().to_rfc3339());
            state.store.put("principle_usage", &usage);
            finalized += 1;
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
        ensure_composition(state, &mut principle);
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
    fn direct_recovery_requires_the_same_resource_to_be_read_and_retried() {
        let (state, dir) = state();
        let execution = json!({"id":"direct","taskOutcome":"succeeded","model":"real-tool-loop"});
        for (seq, tool, method, path, status, result) in [
            (1, "github_api", "PUT", "/target", "failed", json!({"error":"409 conflict"})),
            (2, "github_api", "GET", "/other", "completed", json!({"status":200})),
            (4, "github_api", "PUT", "/target", "completed", json!({"status":200})),
        ] {
            state.store.put("execution_operation", &json!({
                "id":format!("operation-{seq}"),"runId":"direct","opSeq":seq,"tool":tool,
                "arguments":{"method":method,"path":path},"status":status,"result":result,
            })).unwrap();
        }
        assert_eq!(learn_from_execution(&state, &execution), 0);
        state.store.put("execution_operation", &json!({
            "id":"operation-3","runId":"direct","opSeq":3,"tool":"github_api",
            "arguments":{"method":"GET","path":"/target"},"status":"completed","result":{"status":200},
        })).unwrap();
        assert_eq!(learn_from_execution(&state, &execution), 1);
        assert_eq!(state.store.all("experience_principle")[0]["supportingModels"], json!(["real-tool-loop"]));
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
                &json!({}),
                "409 conflict: stale state",
                Some(run),
            )
            .unwrap_or_else(|| panic!("cross-domain principle missing for {tool}"));
            assert!(guidance.contains("현재 상태와 버전"), "{guidance}");
            let execution = json!({"id":run,"model":model,"agentId":agent});
            assert_eq!(finalize_usage(&state, &execution, true), 1);
            assert_eq!(state.store.all("principle_usage").len(), index + 1);
        }
        // A second verified source run promotes the principle into the
        // existing strategy-candidate pipeline.
        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2",
                "taskOutcome":"succeeded",
                "model":"model-a2",
                "agentId":"agent-a2",
                "sessionId":"s-source-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        let candidate = state.store.all("improvement").into_iter()
            .find(|record| record["kind"].as_str() == Some("strategy"))
            .expect("principle strategy candidate");
        assert_eq!(candidate["scope"], "global", "{candidate}");
        assert!(candidate["text"].as_str().unwrap_or("").contains("현재 상태와 버전"), "{candidate}");
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["strategyCandidateId"], candidate["id"], "{principle}");
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
    fn synthesized_principle_is_typed_not_a_prompt_sentence() {
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
        let principle = &state.store.all("experience_principle")[0];
        assert_eq!(principle["schemaVersion"], 2);
        assert!(principle["conditions"].as_array().unwrap().len() >= 2);
        assert!(principle["exclusions"].as_array().unwrap().len() >= 5);
        assert_eq!(principle["failureLayer"], "state");
        assert_eq!(principle["procedure"][0]["operator"], "observe");
        assert_eq!(principle["procedure"][3]["operator"], "verify");
        assert_eq!(principle["verification"]["modelClaimAccepted"], false);
        assert_eq!(principle["autoApply"], false);
        assert_eq!(principle["cost"]["modelCalls"], 0);
        assert!(principle["provenance"]["capabilities"].is_object());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn composition_requires_independent_cases_and_stays_unpromoted() {
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
        assert!(context_block(
            &state,
            Some("agent-b"),
            "model-b",
            "workspace_write",
            &json!({}),
            "409 conflict",
            Some("run-b"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-b"}), true);
        assert!(
            state.store.all("strategy_composition").is_empty(),
            "one source and one supporting case are not enough"
        );

        learn_from_execution(
            &state,
            &json!({
                "id":"run-source-2","taskOutcome":"succeeded","model":"model-a2",
                "agentId":"agent-a2","sessionId":"s-2",
                "adaptations":[{
                    "observation":{"tool":"github_api_update","actual":"409 conflict"},
                    "selected":{"tool":"github_api_get","inputs":{}},
                    "result":{"success":true}
                }]
            }),
        );
        assert!(context_block(
            &state,
            Some("agent-c"),
            "model-c",
            "file_write",
            &json!({}),
            "409 conflict: stale state",
            Some("run-c"),
        )
        .is_some());
        finalize_usage(&state, &json!({"id":"run-c"}), true);

        let composition = state
            .store
            .all("strategy_composition")
            .into_iter()
            .next()
            .expect("composition from independent evidence");
        assert_eq!(composition["promotionEligible"], false);
        assert_eq!(composition["experiment"]["status"], "not_run");
        assert_eq!(composition["experiment"]["gates"]["tokenRatioMax"], 1.25);
        let domains = composition["supportedDomains"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["domain"].as_str())
            .collect::<Vec<_>>();
        assert!(domains.contains(&"filesystem"));
        assert!(domains.contains(&"source_control"));
        assert!(domains.contains(&"system") == false);
        let principle = state.store.all("experience_principle")[0].clone();
        assert_eq!(principle["strategyCompositionId"], composition["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unresolved_effects_are_not_learned_as_success() {
        let (state, dir) = state();
        let mut execution = execution(
            "model-a",
            "agent-a",
            "github_api_update",
            "github_api_get",
            "409 conflict: sha mismatch",
        );
        execution["effectSummary"] = json!({"unverifiedEffects": 1});
        assert_eq!(learn_from_execution(&state, &execution), 0);
        assert!(state.store.all("experience_principle").is_empty());
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
            &json!({}),
            "no such file",
            Some("run-c"),
        )
        .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
