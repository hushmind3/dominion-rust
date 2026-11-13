//! Agent policy guidance, registration, activation, disable and generation.
use super::*;

/// A compact per-agent view of which judgment kernels have worked.
pub fn agent_context_block(state: &AppState, agent_id: &str) -> Option<(String, Vec<Value>)> {
    let stats = state
        .store
        .all("algorithm_stat")
        .into_iter()
        .filter(|stat| {
            stat["agentId"].as_str() == Some(agent_id)
                || stat["agentId"].as_str() == Some("global")
        })
        .collect::<Vec<_>>();
    if stats.is_empty() {
        return None;
    }
    let mut best = std::collections::BTreeMap::new();
    for stat in &stats {
        let kind = stat["kind"].as_str().unwrap_or("");
        let rate = stat["successRate"].as_f64().unwrap_or(0.0);
        let uses = stat["uses"].as_u64().unwrap_or(0);
        let entry = best.entry(kind.to_string()).or_insert((0.0, 0_u64, String::new()));
        if (rate, uses) > (entry.0, entry.1) {
            *entry = (rate, uses, stat["revision"].as_str().unwrap_or("").to_string());
        }
    }
    if best.is_empty() {
        return None;
    }
    let mut block = String::from("Algorithm performance for this bot:");
    let mut items = Vec::new();
    for (kind, (rate, uses, revision)) in best {
        block.push_str(&format!(
            "\n- {kind}: {:.0}% success over {uses} uses",
            rate * 100.0
        ));
        items.push(json!({
            "kind": "algorithm_stat",
            "algorithmKind": kind,
            "revision": revision,
            "uses": uses,
            "successRate": rate,
        }));
    }
    Some((block, items))
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
    if implementation == "expression" {
        let expression = body["parameters"]["expression"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .ok_or("expression kernel requires parameters.expression")?;
        eval_expression(expression, &expression_sample(kind))?;
    }
    if let Some(target_expression) = body["parameters"]["targetExpression"].as_str() {
        if kind != "selfimprove.search" || implementation != "expression" {
            return Err("targetExpression requires selfimprove.search expression kernel".into());
        }
        eval_expression(target_expression, &expression_sample("selfimprove.target"))?;
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

/// Promotion with evidence. Deterministic evaluation must recommend the exact
/// winner for this kind unless the caller explicitly asks for a manual switch.
pub fn activate_checked(state: &AppState, id: &str, force: bool) -> Result<Value, String> {
    let Some(algorithm) = state.store.get("algorithm", id) else {
        return Err("algorithm not found".into());
    };
    if !force && algorithm["status"].as_str() != Some("active") {
        let kind = algorithm["kind"].as_str().unwrap_or("");
        let implementation = algorithm["implementation"].as_str().unwrap_or("");
        let promoted = state.store.all("algorithm_eval").into_iter().any(|record| {
            record["dimensions"].as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row["kind"].as_str() == Some(kind)
                        && row["winner"].as_str() == Some(implementation)
                        && row["recommendation"].as_str() == Some("promote")
                })
            })
        });
        if !promoted {
            return Err(
                "실험 평가에서 승격 추천을 받지 못했습니다. 검증 후 승급하거나 force=true로 수동 전환하세요."
                    .into(),
            );
        }
    }
    let activated = activate(state, id)?;
    state.store.audit(
        "algorithm.activation_mode",
        json!({"id": id, "force": force, "kind": activated["kind"]}),
    );
    Ok(activated)
}

pub fn disable(state: &AppState, id: &str) -> Option<Value> {
    let mut algorithm = state.store.get("algorithm", id)?;
    algorithm["status"] = json!("disabled");
    algorithm["disabledAt"] = json!(chrono::Utc::now().to_rfc3339());
    let saved = state.store.put("algorithm", &algorithm)?;
    state.bus.record("algorithm", &saved);
    state.store.audit(
        "algorithm.disabled",
        json!({"id": id, "kind": algorithm["kind"], "revision": algorithm["revision"]}),
    );
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

