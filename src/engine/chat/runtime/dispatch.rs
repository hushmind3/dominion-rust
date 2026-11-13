//! dispatch: isolated native chat runtime responsibility; preserve executable RSI behavior.
use super::*;

/// A fully specified replacement is executable without a model only when every
/// acceptance requirement is the same deterministic operation and its expected
/// hash was frozen before the turn. Anything less falls back to the model.
pub(super) fn frozen_replacements(checks: &Value) -> Option<Vec<Value>> {
    let items = checks.as_array()?;
    (!items.is_empty()
        && items.len() <= 32
        && items.iter().all(|check| {
            check["type"] == "file_replace_exact"
                && check["path"].as_str().is_some_and(|path| !path.is_empty())
                && check["old"].as_str().is_some_and(|old| !old.is_empty())
                && check["new"].as_str().is_some()
                && check["expectedSha256"].as_str().is_some()
        }))
    .then(|| items.to_vec())
}

/// Applies the literal replacement only when the old text occurs once and the
/// result matches the hash captured in the contract. A mismatch is not a
/// failure of the task; it means the deterministic path does not apply.
pub(super) fn replacement_content(check: &Value, content: &str) -> Option<String> {
    let old = check["old"].as_str()?;
    let new = check["new"].as_str()?;
    if content.matches(old).count() != 1 {
        return None;
    }
    let replaced = content.replacen(old, new, 1);
    (format!("{:x}", Sha256::digest(replaced.as_bytes())) == check["expectedSha256"].as_str()?)
        .then_some(replaced)
}

/// Runs one deterministic tool call through the same approval, file lock,
/// effect receipt and evidence store as a model-issued call.
#[allow(clippy::too_many_arguments)]
pub(super) async fn deterministic_tool(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    tool: &str,
    arguments: &Value,
    op_seq: u64,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<(Value, Value), String> {
    let mut operations = state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|operation| operation["sessionId"].as_str() == Some(session_id))
        .collect::<Vec<_>>();
    operations.reverse();
    let effect = crate::engine::effect::prepare(session_id, tool, arguments, &operations);
    let operation_id = format!("{execution_id}:frozen:{op_seq}:{tool}");
    let started_at = chrono::Utc::now().to_rfc3339();
    let started = std::time::Instant::now();
    let _ = tx.send(json!({"type":"tool_start","name":tool,"id":operation_id}));
    let allowed = if tools::requires_approval(tool) {
        request_approval(
            state,
            tx,
            session_id,
            tool,
            arguments,
            execution_id,
            &operation_id,
            None,
        )
        .await
    } else {
        true
    };
    let initial_state = if allowed { "started" } else { "denied" };
    state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id, "runId": execution_id, "sessionId": session_id,
                "callId": operation_id, "tool": tool, "arguments": arguments,
                "opSeq": op_seq, "origin": "deterministic_shortcut",
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": initial_state,
                "toolReceipt": {
                    "effectId": effect["effectId"], "tool": tool,
                    "requestDigest": effect["requestDigest"],
                    "resourceKey": effect["resourceKey"],
                    "status": initial_state, "verified": false,
                    "verifier": {"kind": "pending", "revision": "effect-v1"},
                    "beforeVersion": effect["beforeVersion"], "startedAt": started_at,
                },
                "status": "running", "startedAt": started_at,
            }),
        )
        .ok_or("결정적 실행 시작을 저장할 수 없습니다.")?;
    let (mut result, ok) = if allowed {
        let context = ToolContext {
            workspace: &state.config.workspace,
            store: &state.store,
            state,
            session_id,
            agent_id: request.agent_id.as_deref(),
            tools_enabled: request.tools_enabled,
            tools_execution_enabled: request.tools_execution_enabled,
        };
        match tools::execute(&context, tool, arguments, execution_id).await {
            Ok(value) => {
                let ok = tool_succeeded(&value);
                (value, ok)
            }
            Err(error) => (json!({"error": error}), false),
        }
    } else {
        (json!({"error":crate::engine::rsi_evolve::auto_write_denial_reason(
            state,session_id,tool,arguments).unwrap_or("Denied by the user")}), false)
    };
    // checked_write finds the last observed version on the operation record,
    // not inside the tool result. Keep that contract identical to the normal
    // dispatch path so the opt-in guard can see this read.
    let file_observation = if matches!(
        tool,
        "workspace_read" | "workspace_read_range" | "file_read" | "workspace_replace" | "workspace_write"
    ) {
        result
            .as_object_mut()
            .and_then(|value| value.remove("fileObservation"))
    } else {
        None
    };
    {
        let mut counters = metrics.lock().unwrap();
        counters.tool_calls += 1;
        counters.tool_elapsed_ms += started.elapsed().as_millis() as u64;
    }
    let sealed = crate::engine::effect::seal(
        &state.config.workspace,
        tool,
        arguments,
        &result,
        ok,
        !allowed,
        false,
        &effect,
    )
    .await;
    let operation = state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id, "runId": execution_id, "sessionId": session_id,
                "callId": operation_id, "tool": tool, "arguments": arguments,
                "result": result, "fileObservation": file_observation,
                "opSeq": op_seq, "origin": "deterministic_shortcut",
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": sealed["effectState"],
                "observedVersion": sealed["observedVersion"],
                "toolReceipt": sealed["toolReceipt"],
                "status": if ok { "completed" } else { "failed" },
                "startedAt": started_at,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("결정적 실행 결과를 저장하지 못했습니다.")?;
    state.bus.record("execution_operation", &operation);
    let _ = tx.send(json!({
        "type": if ok { "tool_end" } else { "tool_error" },
        "name": tool,
        "id": operation_id,
        "result": operation["result"],
    }));
    Ok((operation["result"].clone(), operation))
}

/// Records what was already applied when a later write cannot proceed. The
/// operations remain the primary evidence; this marker makes the partial state
/// explicit on the execution record.
pub(super) fn record_partial_shortcut(
    state: &AppState,
    execution_id: &str,
    applied: &[Value],
    failed: &std::path::Path,
    reason: &str,
) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "partial": true,
            "appliedEffectIds": applied,
            "failedPath": failed.to_string_lossy(),
            "reason": reason,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
}

/// Executes every frozen replacement before any model call. Reads are all
/// completed and validated first; a single mismatch falls back to the normal
/// model path without mutating anything.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_frozen_replacements(
    state: &AppState,
    request: &TurnRequest,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    execution_id: &str,
    task_id: &str,
    text: &str,
    checks: Vec<Value>,
    metrics: &std::sync::Mutex<Metrics>,
) -> Result<Option<Value>, String> {
    if !request.tools_enabled
        || !request.tools_execution_enabled
        || std::env::var("DOMINION_DETERMINISTIC_REPLACEMENT").as_deref() == Ok("0")
    {
        return Ok(None);
    }
    let existing = state.store.execution_operations(execution_id)?;
    let mut op_seq = existing
        .iter()
        .filter_map(|operation| operation["opSeq"].as_u64())
        .max()
        .unwrap_or(0);
    let mut paths = Vec::new();
    for check in &checks {
        let raw = check["path"].as_str().ok_or("경로가 없습니다.")?;
        let path = crate::engine::verification::resolve_path(state, raw)?;
        if paths.iter().any(|existing| existing == &path) {
            // Two independently frozen hashes on one file cannot both be
            // validated after the first change. Let the model handle it.
            return Ok(None);
        }
        paths.push(path);
    }
    let mut prepared = Vec::new();
    for (check, path) in checks.iter().zip(paths) {
        op_seq += 1;
        let (read, _) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_read",
            &json!({"path": path.to_string_lossy()}),
            op_seq,
            metrics,
        )
        .await?;
        let Some(content) = read["content"].as_str() else {
            return Ok(None);
        };
        let source_sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
        let Some(expected) = replacement_content(check, content) else {
            return Ok(None);
        };
        prepared.push((path, source_sha256, expected));
    }
    let mut effect_ids = Vec::new();
    for (path, source_sha256, expected) in prepared {
        op_seq += 1;
        let (_, operation) = deterministic_tool(
            state,
            request,
            tx,
            session_id,
            execution_id,
            "file_write",
            &json!({
                "path": path.to_string_lossy(),
                "content": expected,
                // Compare immediately before writing, under the per-file lock.
                "expectedSha256": source_sha256,
            }),
            op_seq,
            metrics,
        )
        .await?;
        if operation["result"]["staleVersion"].as_bool() == Some(true) {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "source version changed before write",
            );
            return Err(format!(
                "{} 파일이 읽은 뒤 변경되어 쓰기를 중단했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        if operation["effectState"] != "verified" {
            record_partial_shortcut(
                state,
                execution_id,
                &effect_ids,
                &path,
                "write was not independently verified",
            );
            return Err(format!(
                "{} 변경을 독립적으로 검증하지 못했습니다. 이미 적용된 변경은 실행 기록에 남아 있습니다.",
                path.display()
            ));
        }
        effect_ids.push(operation["effectId"].clone());
    }
    let answer = "요청한 정확 치환을 모델 호출 없이 수행하고 SHA-256으로 검증했습니다.".to_string();
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["acceptancePassed"] = json!(true);
        execution["finalAnswer"] = json!(answer);
        execution["toolsUsed"] = json!(["file_read", "file_write"]);
        execution["deterministicShortcut"] = json!({
            "kind": "file_replace_exact",
            "modelCalls": 0,
            "tokens": 0,
            "checks": checks.len(),
            "effectIds": effect_ids,
            "note": "deterministic execution optimization; excluded from learning evidence",
        });
        state.store.put("execution", &execution);
    }
    let saved = state
        .store
        .write_message(&json!({
            "session_id": session_id,
            "role": "assistant",
            "content": answer,
            "agent_id": request.agent_id,
            "metadata": {
                "authorKind": "agent",
                "executionId": execution_id,
                "taskId": task_id,
                "deterministicShortcut": true,
                "acceptanceText": text,
            },
        }))
        .ok_or("결정적 실행의 최종 응답을 저장할 수 없습니다.")?;
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["finalMessageId"] = saved["id"].clone();
        state.store.put("execution", &execution);
    }
    state.bus.message(&saved);
    Ok(Some(saved))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn dispatch_tool_call(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    call: &ToolCall,
    arguments: Value,
    metrics: &std::sync::Mutex<Metrics>,
    op_seq: u64,
    round: usize,
    call_index: usize,
    _compat: bool,
    origin: &str,
    procedure_node: Option<&str>,
    procedure_strategy: Option<&str>,
    procedure_revision: Option<&str>,
) -> Result<(Value, bool, String), String> {
    let argument_strategy = crate::engine::algorithms::choose_for_agent(
        state,
        "tool.arguments",
        request.agent_id.as_deref(),
        Some(execution_id),
    )
    .and_then(|algorithm| {
        algorithm["implementation"]
            .as_str()
            .or_else(|| algorithm["variant"].as_str())
            .map(str::to_string)
    })
    .unwrap_or_default();
    let arguments = normalize_tool_arguments(arguments, &argument_strategy);
    crate::engine::algorithms::mark_signal(state, execution_id, "tool.arguments", true);
    // Record what the effect would be before asking for approval or running it.
    // Only mutating calls need the session history scan.
    let mut effect_operations = if crate::engine::effect::is_effectful(&call.name, &arguments) {
        state
            .store
            .all("execution_operation")
            .into_iter()
            .filter(|operation| operation["sessionId"].as_str() == Some(session_id))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    effect_operations.reverse();
    let effect = crate::engine::effect::prepare(
        session_id,
        &call.name,
        &arguments,
        &effect_operations,
    );
    let effect_block = crate::engine::effect::unresolved(&effect, &effect_operations);
    let started_at = chrono::Utc::now().to_rfc3339();
    let tool_started = std::time::Instant::now();
    let _ = tx.send(json!({ "type": "tool_start", "name": call.name, "id": call.id }));

    let needs_approval = if call.name.starts_with("mcp__") {
        mcp::requires_approval(state, &call.name)
    } else {
        tools::requires_approval(&call.name)
    };
    let operation_id = format!("{execution_id}:{round}:{call_index}:{}", call.id);
    let allowed = if effect_block.is_some() {
        false
    } else if needs_approval {
        request_approval(
            state,
            tx,
            session_id,
            &call.name,
            &arguments,
            execution_id,
            &operation_id,
            procedure_node,
        )
        .await
    } else {
        true
    };

    let initial_effect_state = if effect_block.is_some() {
        "needs_review"
    } else if allowed {
        "started"
    } else {
        "denied"
    };
    state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id, "runId": execution_id, "sessionId": session_id,
                "callId": call.id, "tool": call.name, "arguments": arguments,
                "opSeq": op_seq, "origin": origin, "procedureNodeId": procedure_node,
                "procedureStrategyId": procedure_strategy,
                "procedureRevision": procedure_revision,
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": initial_effect_state,
                "toolReceipt": {
                    "effectId": effect["effectId"],
                    "tool": call.name,
                    "requestDigest": effect["requestDigest"],
                    "resourceKey": effect["resourceKey"],
                    "status": initial_effect_state,
                    "verified": false,
                    "verifier": {"kind": "pending", "revision": "effect-v1"},
                    "beforeVersion": effect["beforeVersion"],
                    "startedAt": started_at,
                },
                "status": "running", "startedAt": started_at,
            }),
        )
        .ok_or("도구 실행 시작을 저장할 수 없습니다.")?;
    let mut file_observation = None;
    let (result, ok) = if let Some(prior) = &effect_block {
        // The previous attempt has no confirmed outcome. Sending the same
        // non-idempotent change again would risk a duplicate external effect.
        let prior_state = prior["effectState"].as_str().unwrap_or("unknown");
        let error = format!(
            "이전 시도의 효과 상태가 확인되지 않았습니다({prior_state}). 같은 변경을 다시 보내지 말고 대상의 현재 상태를 먼저 조회한 뒤 판단하세요."
        );
        let value = json!({
            "error": error,
            "effectUnknown": true,
            "requiresInspection": true,
            "effectId": effect["effectId"],
            "priorEffectState": prior_state,
            "priorOperationId": prior["id"],
        });
        let _ = tx.send(json!({
            "type": "tool_error",
            "name": call.name,
            "id": call.id,
            "error": "확인되지 않은 이전 효과가 있어 재실행을 보류했습니다.",
            "result": value,
        }));
        (value, false)
    } else if allowed {
        let context = ToolContext {
            workspace: &state.config.workspace,
            store: &state.store,
            state,
            session_id,
            agent_id: request.agent_id.as_deref(),
            tools_enabled: request.tools_enabled,
            tools_execution_enabled: request.tools_execution_enabled,
        };
        let outcome = if state.store.get("session",session_id).is_some_and(|s|s["rsiRun"]==true)
            && !tools::rsi_allowed(&call.name) {
            Err("자기개선 작업의 허용 도구 범위를 벗어났습니다.".into())
        } else if call.name.starts_with("mcp__") {
            mcp::call(state, &call.name, &arguments).await
        } else {
            tools::execute(&context, &call.name, &arguments, execution_id).await
        };
        match outcome {
            Ok(value) => {
                let mut value = value;
                if matches!(call.name.as_str(), "workspace_read" | "workspace_read_range" | "file_read" | "workspace_replace" | "workspace_write") {
                    file_observation = value.as_object_mut()
                        .and_then(|value| value.remove("fileObservation"));
                }
                if tool_succeeded(&value) {
                    let _ = tx.send(json!({
                        "type": "tool_end",
                        "name": call.name,
                        "id": call.id,
                        "arguments": arguments,
                        "result": value,
                    }));
                    (value, true)
                } else {
                    let _ = tx.send(json!({ "type": "tool_error", "name": call.name,
                        "id": call.id, "error": "도구가 실패 결과를 반환했습니다.", "result": value }));
                    (value, false)
                }
            }
            Err(error) if call.name.starts_with("workspace_") && error == tools::WORKSPACE_ESCAPE => {
                let error = format!("{error}. Use the relative path inside the workspace, for example bench/report.txt.");
                let _ = tx.send(json!({"type":"tool_error","name":call.name,"id":call.id,"error":error}));
                (json!({"error":error,"invalidWorkspacePath":true}), false)
            }
            Err(error) => {
                let _ = tx.send(json!({
                    "type": "tool_error",
                    "name": call.name,
                    "id": call.id,
                    "error": error,
                }));
                (json!({ "error": error }), false)
            }
        }
    } else {
        let _ = tx.send(json!({
            "type": "tool_error",
            "name": call.name,
            "id": call.id,
            "error": "사용자가 실행을 거부했습니다.",
        }));
        (json!({"error":crate::engine::rsi_evolve::auto_write_denial_reason(
            state,session_id,&call.name,&arguments).unwrap_or("Denied by the user")}), false)
    };

    {
        let mut counters = metrics.lock().unwrap();
        counters.tool_calls += 1;
        counters.tool_elapsed_ms += tool_started.elapsed().as_millis() as u64;
    }
    record_tool_stat(state, session_id, request.agent_id.as_deref(), &call.name, ok);
    state.store.audit(
        "tool.call",
        json!({
            "tool": call.name,
            "args": arguments,
            "allowed": allowed,
            "round": round,
        }),
    );
    let sealed = crate::engine::effect::seal(
        &state.config.workspace,
        &call.name,
        &arguments,
        &result,
        ok,
        !allowed && effect_block.is_none(),
        effect_block.is_some(),
        &effect,
    )
    .await;
    let operation = state
        .store
        .put(
            "execution_operation",
            &json!({
                "id": operation_id,
                "runId": execution_id,
                "sessionId": session_id,
                "callId": call.id,
                "tool": call.name,
                "arguments": arguments,
                "result": result,
                "fileObservation": file_observation,
                "opSeq": op_seq,
                "origin": origin,
                "procedureNodeId": procedure_node,
                "procedureStrategyId": procedure_strategy,
                "procedureRevision": procedure_revision,
                "effectId": effect["effectId"],
                "resourceKey": effect["resourceKey"],
                "requestDigest": effect["requestDigest"],
                "beforeVersion": effect["beforeVersion"],
                "effectState": sealed["effectState"],
                "observedVersion": sealed["observedVersion"],
                "toolReceipt": sealed["toolReceipt"],
                "status": if ok { "completed" } else { "failed" },
                "startedAt": started_at,
                "finishedAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .ok_or("도구 수행 결과를 저장하지 못했습니다. 실행 기록을 확인하세요.")?;
    state.bus.record("execution_operation", &operation);

    // A verified observation of the same resource closes a pending effect for
    // retry purposes. It does not upgrade the old effect to verified.
    if !crate::engine::effect::is_effectful(&call.name, &arguments)
        && sealed["toolReceipt"]["verified"].as_bool() == Some(true)
    {
        if let Some(resource) = effect["resourceKey"].as_str() {
            let operations = state.store.all("execution_operation");
            for updated in crate::engine::effect::mark_inspected(
                &operations,
                resource,
                operation["id"].as_str().unwrap_or(""),
            ) {
                if let Some(saved) = state.store.put("execution_operation", &updated) {
                    state.bus.record("execution_operation", &saved);
                }
            }
        }
    }

    // Keep the full result. The runtime selects a model-specific context
    // window when assembling the next provider request; local and cloud
    // models are treated according to declared capacity, not arbitrary modes.
    let tool_content = result.to_string();
    let saved = state
        .store
        .write_message(&json!({
            "session_id": session_id,
            "role": "tool",
            "content": tool_content,
            "tool_call_id": call.id,
            "agent_id": request.agent_id,
            "metadata": {"authorKind":"tool","roomId":session.get("roomId"),"executionId":execution_id},
        }))
        .ok_or("도구 결과 메시지를 저장할 수 없습니다.")?;
    state.bus.message(&saved);
    Ok((result, ok, tool_content))
}

pub(super) fn record_soft_tool_error(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session_id: &str,
    request: &TurnRequest,
    execution_id: &str,
    name: &str,
    call_id: &str,
    error: &str,
    metrics: &std::sync::Mutex<Metrics>,
) {
    let _ = tx.send(json!({"type":"tool_error","name":name,"id":call_id,"error":error}));
    metrics.lock().unwrap().tool_calls += 1;
    state.store.audit("tool.soft_error", json!({"tool":name,"error":error,"executionId":execution_id}));
    if let Some(saved) = state.store.write_message(&json!({
        "session_id":session_id,"role":"tool","content":json!({"error":error}).to_string(),
        "tool_call_id":call_id,"agent_id":request.agent_id,
        "metadata":{"authorKind":"tool","executionId":execution_id},
    })) {
        state.bus.message(&saved);
    }
}

