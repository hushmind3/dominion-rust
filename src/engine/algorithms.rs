//! Algorithm space. The harness owns execution, observation, tools,
//! checkpointing and safety. Algorithms are versioned, selectable pieces the
//! harness calls by kind. Keeping them in the store makes addition, replacement
//! and generated candidates independent from engine code.
use crate::state::AppState;
use serde_json::{json, Value};

pub const KINDS: &[&str] = &[
    "adaptation.selection",
    "learning.applicability",
    "strategy.selection",
    "program.repair",
    "context.selection",
    "verification.predicate",
    "tool.selection",
];

/// Concrete kernels the Rust harness can execute. Registering a name that is
/// not here is rejected: an algorithm record must point at real behavior.
pub fn executable_kernels(kind: &str) -> &'static [&'static str] {
    match kind {
        "adaptation.selection" => &["balanced", "discrimination-first", "progress-first", "probe-first"],
        "learning.applicability" => &["conservative", "probe-unknown"],
        "strategy.selection" => &["estimated-softmax", "exploit-only"],
        "program.repair" => &["completed-preserving"],
        "context.selection" => &["failure-first"],
        "verification.predicate" => &["three-valued", "strict"],
        "tool.selection" => &["read-before-write", "none"],
        _ => &[],
    }
}

pub fn kernel_exists(kind: &str, implementation: &str) -> bool {
    executable_kernels(kind)
        .iter()
        .any(|kernel| *kernel == implementation)
}

pub fn kind_label(kind: &str) -> &str {
    match kind {
        "adaptation.selection" => "적응 개입 선택",
        "learning.applicability" => "경험 적용 조건",
        "strategy.selection" => "전략 선택",
        "program.repair" => "프로그램 수리",
        "context.selection" => "컨텍스트 선택",
        "verification.predicate" => "검증 predicate",
        "tool.selection" => "도구 선택",
        _ => "기타",
    }
}

fn builtin(
    kind: &str,
    name: &str,
    variant: &str,
    description: &str,
    parameters: Value,
) -> Value {
    let id = format!(
        "algorithm-{}",
        crate::engine::memory::stable_id(&["builtin", kind, name])
    );
    json!({
        "id": id,
        "kind": kind,
        "affects": kind,
        "name": name,
        "variant": variant,
        "description": description,
        "parameters": parameters,
        "source": "builtin",
        "status": "available",
        "revision": crate::engine::memory::stable_id(&[kind, name, variant]),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    })
}

pub fn builtins() -> Vec<Value> {
    vec![
        builtin(
            "adaptation.selection",
            "balanced",
            "balanced",
            "진전·구별·비용·불확실성을 함께 고려한다.",
            json!({"discriminationWeight": 1.0, "progressWeight": 1.0, "costWeight": 1.0, "uncertaintyWeight": 0.5}),
        ),
        builtin(
            "adaptation.selection",
            "discrimination-first",
            "discrimination-first",
            "원인이 불명확할 때 가설을 구별하는 probe를 우선한다.",
            json!({"discriminationWeight": 3.0, "progressWeight": 0.3, "costWeight": 1.0, "uncertaintyWeight": 0.2}),
        ),
        builtin(
            "adaptation.selection",
            "progress-first",
            "progress-first",
            "원인이 좁혀졌을 때 실제 문제 해결 가능성이 큰 개입을 우선한다.",
            json!({"discriminationWeight": 0.3, "progressWeight": 3.0, "costWeight": 0.8, "uncertaintyWeight": 0.5}),
        ),
        builtin(
            "adaptation.selection",
            "probe-first",
            "probe-first",
            "낮은 비용의 관측으로 원인을 먼저 구별한다.",
            json!({"discriminationWeight": 2.5, "progressWeight": 0.4, "costWeight": 1.5, "uncertaintyWeight": 0.2}),
        ),
        builtin(
            "learning.applicability",
            "conservative",
            "conservative",
            "Unknown 조건은 적용하지 않고 관측을 요구한다.",
            json!({"unknown": "hold"}),
        ),
        builtin(
            "learning.applicability",
            "probe-unknown",
            "probe-unknown",
            "Unknown 조건은 작은 probe로 확인한 뒤 적용한다.",
            json!({"unknown": "probe"}),
        ),
        builtin(
            "strategy.selection",
            "estimated-softmax",
            "estimated-softmax",
            "검증 성공/실패·불확실성·비용으로 순위를 정하고 softmax 확률을 기록한다.",
            json!({"uncertaintyPenalty": 0.3, "riskPenalty": 0.5, "exploration": 1}),
        ),
        builtin(
            "strategy.selection",
            "exploit-only",
            "exploit-only",
            "관측 표본이 충분한 전략만 선택한다.",
            json!({"uncertaintyPenalty": 0.6, "exploration": 0}),
        ),
        builtin(
            "program.repair",
            "completed-preserving",
            "completed-preserving",
            "완료된 외부 행동을 보존하고 미완료 노드만 교체한다.",
            json!({"maxRevisions": 2, "preserveCompleted": true}),
        ),
        builtin(
            "context.selection",
            "failure-first",
            "failure-first",
            "같은 실패 유형의 사례와 조건부 방법을 먼저 넣는다.",
            json!({"failureBoost": 2.0, "methodLimit": 3}),
        ),
        builtin(
            "verification.predicate",
            "three-valued",
            "three-valued",
            "True/False/Unknown으로 평가하고 Unknown을 실패로 바꾸지 않는다.",
            json!({"unknown": "hold"}),
        ),
        builtin(
            "verification.predicate",
            "strict",
            "strict",
            "Unknown을 충족 실패로 처리한다. 기본은 three-valued다.",
            json!({"unknown": "fail"}),
        ),
        builtin(
            "tool.selection",
            "read-before-write",
            "read-before-write",
            "쓰기 전에 읽기/목록 도구로 대상을 확인한다.",
            json!({"probeTools": ["workspace_list", "workspace_read"]}),
        ),
    ]
}

pub fn seed(state: &AppState) -> Vec<Value> {
    let mut saved = Vec::new();
    for mut algorithm in builtins() {
        // The first builtin of each kind is the explicit default. Later
        // activation never silently promotes another kind or another tool.
        let kind = algorithm["kind"].as_str().unwrap_or("").to_string();
        let records = state.store.all("algorithm");
        let kind_exists = records
            .iter()
            .any(|item| item["kind"].as_str() == Some(kind.as_str()));
        let has_active = records.iter().any(|item| {
            item["kind"].as_str() == Some(kind.as_str())
                && item["status"].as_str() == Some("active")
        });
        if !kind_exists && !has_active {
            algorithm["status"] = json!("active");
            algorithm["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        }
        let id = algorithm["id"].as_str().unwrap_or("").to_string();
        if state.store.get("algorithm", &id).is_none() {
            if let Some(record) = state.store.put("algorithm", &algorithm) {
                state.bus.record("algorithm", &record);
                saved.push(record);
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
    let kinds = KINDS
        .iter()
        .map(|kind| {
            let rows = items
                .iter()
                .filter(|item| item["kind"].as_str() == Some(*kind))
                .cloned()
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

pub fn variant(state: &AppState, kind: &str) -> String {
    active(state, kind)
        .and_then(|item| item["variant"].as_str().map(str::to_string))
        .unwrap_or_else(|| "balanced".into())
}

pub fn register(state: &AppState, body: &Value) -> Result<Value, String> {
    let kind = body["kind"]
        .as_str()
        .filter(|value| KINDS.contains(value))
        .ok_or("unknown algorithm kind")?;
    let name = body["name"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or("algorithm name is required")?;
    let implementation = body["implementation"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(name);
    if !kernel_exists(kind, implementation) {
        return Err(format!(
            "algorithm kind {kind} has no executable kernel {implementation}; available: {:?}",
            executable_kernels(kind)
        ));
    }
    let variant = body["variant"].as_str().unwrap_or(implementation);
    let parameters = body["parameters"]
        .as_object()
        .cloned()
        .map(Value::Object)
        .unwrap_or_else(|| json!({}));
    let id = format!(
        "algorithm-{}",
        crate::engine::memory::stable_id(&[kind, name, variant, &parameters.to_string()])
    );
    if let Some(existing) = state.store.get("algorithm", &id) {
        return Ok(existing);
    }
    let record = json!({
        "id": id,
        "kind": kind,
        "affects": kind,
        "name": name,
        "variant": variant,
        "implementation": implementation,
        "description": body["description"].as_str().unwrap_or(""),
        "parameters": parameters,
        "source": body["source"].as_str().unwrap_or("user"),
        "status": "available",
        "revision": crate::engine::memory::stable_id(&[kind, name, variant, &parameters.to_string()]),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    state
        .store
        .put("algorithm", &record)
        .ok_or_else(|| "algorithm could not be stored".into())
        .map(|saved| {
            state.bus.record("algorithm", &saved);
            state.store.audit(
                "algorithm.registered",
                json!({"id": saved["id"], "kind": kind, "name": name}),
            );
            saved
        })
}

pub fn activate(state: &AppState, id: &str) -> Result<Value, String> {
    let Some(algorithm) = state.store.get("algorithm", id) else {
        return Err("algorithm not found".into());
    };
    let kind = algorithm["kind"].as_str().unwrap_or("").to_string();
    let mut records = Vec::new();
    for mut other in state.store.all("algorithm") {
        if other["kind"].as_str() == Some(kind.as_str())
            && other["status"].as_str() == Some("active")
            && other["id"].as_str() != Some(id)
        {
            other["status"] = json!("available");
            other["deactivatedAt"] = json!(chrono::Utc::now().to_rfc3339());
            records.push((other["id"].as_str().unwrap_or("").to_string(), other));
        }
    }
    let mut activated = algorithm;
    activated["status"] = json!("active");
    activated["activatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    records.push((id.to_string(), activated.clone()));
    let mut transaction = Vec::new();
    for (rid, value) in records {
        let _ = rid;
        transaction.push(("algorithm".to_string(), value));
    }
    state.store.transaction(&transaction)?;
    for (_, value) in &transaction {
        state.bus.record("algorithm", value);
    }
    state.store.audit(
        "algorithm.activated",
        json!({
            "id": id,
            "kind": kind,
            "revision": activated["revision"],
            "activationScope": "kind",
            "selfImprovementIndependent": true,
            "toolGatewayIndependent": true,
        }),
    );
    Ok(activated)
}

pub fn disable(state: &AppState, id: &str) -> Option<Value> {
    let mut algorithm = state.store.get("algorithm", id)?;
    algorithm["status"] = json!("disabled");
    let saved = state.store.put("algorithm", &algorithm)?;
    state.bus.record("algorithm", &saved);
    Some(saved)
}

/// Generates new algorithm candidates from failure evidence. The candidates
/// are real parameterizations of existing harness interfaces; anything that
/// would require a new engine primitive stays a typed proposal.
pub fn generate(state: &AppState, body: &Value) -> Value {
    let kind = body["kind"].as_str().unwrap_or("adaptation.selection");
    let evidence = body["evidence"].as_array().cloned().unwrap_or_default();
    let evidence_text = evidence
        .iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let base = format!(
        "generated-{}",
        crate::engine::memory::stable_id(&[kind, &evidence_text])
    );
    let lower = evidence_text.to_lowercase();
    let timeout_heavy = lower.matches("timeout").count() + lower.matches("시간").count();
    let permission_heavy =
        lower.matches("permission").count() + lower.matches("권한").count();
    let not_found_heavy = lower.matches("not_found").count() + lower.matches("not found").count();
    let templates: Vec<Value> = match kind {
        "adaptation.selection" => {
            // Evidence changes the starting kernel: repeated timeouts/unknown
            // causes favour a cheap probe; permission failures favour a
            // discrimination-focused search; otherwise progress wins.
            let (kernel, description, discrimination, progress) = if timeout_heavy
                > permission_heavy.max(not_found_heavy)
            {
                ("probe-first", "시간 초과/불명확 원인 증거가 많아 저비용 probe를 먼저 둔다.", 2.8, 0.4)
            } else if permission_heavy > not_found_heavy {
                (
                    "discrimination-first",
                    "권한 실패 증거가 많아 원인을 구별하는 개입을 우선한다.",
                    3.0,
                    0.5,
                )
            } else {
                (
                    "progress-first",
                    "파일/경로 실패 증거가 많아 실제 해결 개입을 우선한다.",
                    0.4,
                    2.5,
                )
            };
            vec![
                json!({"name": format!("{base}-{kernel}"), "variant": kernel, "implementation": kernel,
                    "description": description,
                    "parameters": {"discriminationWeight": discrimination, "progressWeight": progress, "costWeight": 1.2}}),
                json!({"name": format!("{base}-{kernel}-explore"), "variant": kernel, "implementation": kernel,
                    "description": "증거 기반 후보의 탐색 비중을 높인 변형",
                    "parameters": {"discriminationWeight": discrimination, "progressWeight": progress, "costWeight": 1.8}}),
            ]
        }
        "strategy.selection" => vec![
            json!({"name": format!("{base}-exploit-only"), "variant": "exploit-only", "implementation": "exploit-only",
                "description": "관측 표본이 충분한 전략만 선택한다.",
                "parameters": {"uncertaintyPenalty": 0.4, "costWeight": 1.5, "exploration": 0}}),
            json!({"name": format!("{base}-estimated"), "variant": "estimated-softmax", "implementation": "estimated-softmax",
                "description": "불확실성을 반영해 탐색과 활용을 함께 사용한다.",
                "parameters": {"uncertaintyPenalty": 0.6, "costWeight": 0.8, "exploration": 1}}),
        ],
        _ => {
            let kernel = executable_kernels(kind)
                .first()
                .copied()
                .unwrap_or("");
            if kernel.is_empty() {
                Vec::new()
            } else {
                vec![json!({
                    "name": format!("{base}-{kernel}"),
                    "variant": kernel,
                    "implementation": kernel,
                    "description": "실패 증거에서 생성된 실행 커널 후보",
                    "parameters": {},
                })]
            }
        }
    };
    let mut created = Vec::new();
    for template in templates {
        let body = json!({
            "kind": kind,
            "name": template["name"],
            "variant": template["variant"],
            "implementation": template["implementation"].as_str().unwrap_or(template["variant"].as_str().unwrap_or("")),
            "description": template["description"],
            "parameters": template["parameters"],
            "source": "generated",
        });
        if let Ok(record) = register(state, &body) {
            created.push(record);
        }
    }
    json!({"kind": kind, "created": created, "evidence": evidence})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-alg-{}", uuid::Uuid::new_v4()));
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

    #[test]
    fn only_executable_kernels_register_and_activation_is_kind_isolated() {
        let (state, dir) = state();
        seed(&state);
        let adaptation_before = active(&state, "adaptation.selection").unwrap();
        let strategy_before = active(&state, "strategy.selection").unwrap();
        assert!(register(
            &state,
            &json!({"kind":"adaptation.selection","name":"fake","implementation":"does-not-exist"})
        )
        .is_err());
        let record = register(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "name":"cheap-probe",
                "implementation":"probe-first",
                "parameters":{"discriminationWeight":2.0}
            }),
        )
        .unwrap();
        activate(&state, record["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            active(&state, "adaptation.selection").unwrap()["id"],
            record["id"]
        );
        assert_ne!(
            active(&state, "adaptation.selection").unwrap()["id"],
            adaptation_before["id"]
        );
        assert_eq!(
            active(&state, "strategy.selection").unwrap()["id"],
            strategy_before["id"],
            "activation leaked into another kind"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generation_uses_evidence_and_returns_executable_kernels() {
        let (state, dir) = state();
        let generated = generate(
            &state,
            &json!({
                "kind":"adaptation.selection",
                "evidence":["timeout timeout permission denied"]
            }),
        );
        let created = generated["created"].as_array().unwrap();
        assert!(!created.is_empty());
        for algorithm in created {
            let kind = algorithm["kind"].as_str().unwrap();
            let implementation = algorithm["implementation"].as_str().unwrap();
            assert!(kernel_exists(kind, implementation), "{algorithm}");
        }
        assert!(created
            .iter()
            .any(|algorithm| algorithm["implementation"] == "probe-first"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn active_optimizer_is_found_by_status_not_a_fixed_id() {
        let (state, dir) = state();
        state.store.put(
            "optimizer_policy",
            &json!({
                "id":"optimizer-custom",
                "revision":"optimizer-custom-v2",
                "status":"active",
                "failureClustering":"custom"
            }),
        );
        let active = crate::engine::policy::active_optimizer(&state);
        assert_eq!(active["revision"], "optimizer-custom-v2");
        let _ = std::fs::remove_dir_all(dir);
    }
}
