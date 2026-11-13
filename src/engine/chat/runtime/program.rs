//! Resumable program checkpoints, bounded tool plans and agent control flow.
//! Active Rust subtree of chat::runtime.
use super::*;

pub(crate) fn save_program_checkpoint(state: &AppState, execution_id: &str, checkpoint: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["programCheckpoint"] = checkpoint.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_program_tool(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    node: &Value,
    resolved: &mut Value,
    checkpoint: &mut Value,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::super::budget::BudgetLedger,
    op_seq: &mut u64,
    round: usize,
    persist: bool,
) -> Result<Value, String> {
    let node_id = node["id"].as_str().unwrap_or("");
    if node_id.is_empty() {
        return Err("program call_tool node needs an id".into());
    }
    let tool = node["tool"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("program call_tool needs a tool")?;
    let arguments = crate::engine::program::resolve_inputs(&node["inputs"], resolved)?;
    if !budget.charge_tool() {
        return Err(format!(
            "program tool budget exhausted at node {node_id}"
        ));
    }
    *op_seq += 1;
    let call = ToolCall {
        id: format!("program-{node_id}-{}", uuid::Uuid::new_v4()),
        name: tool.to_string(),
        arguments: arguments.to_string(),
    };
    checkpoint["pendingOperationIds"] = json!([node_id]);
    checkpoint["resolvedInputs"] = resolved.clone();
    if persist {
        save_program_checkpoint(state, execution_id, checkpoint);
    }
    let (result, _ok, _content) = dispatch_tool_call(
        state,
        tx,
        session,
        request,
        session_id,
        execution_id,
        &call,
        arguments,
        metrics,
        *op_seq,
        round,
        0,
        false,
        "program",
        None,
        None,
        None,
    )
    .await?;
    checkpoint["pendingOperationIds"] = json!([]);
    mark_program_completed(checkpoint, node_id);
    if !checkpoint["observationRefs"].is_object() {
        checkpoint["observationRefs"] = json!({});
    }
    checkpoint["observationRefs"][node_id] = json!(
        crate::engine::memory::stable_id(&[&result.to_string()])
    );
    let key = node["saveAs"].as_str().filter(|value| !value.is_empty()).unwrap_or(node_id);
    resolved[key] = result.clone();
    checkpoint["resolvedInputs"] = resolved.clone();
    if persist {
        save_program_checkpoint(state, execution_id, checkpoint);
    }
    Ok(result)
}

pub(crate) fn mark_program_completed(checkpoint: &mut Value, node_id: &str) {
    if node_id.is_empty() {
        return;
    }
    let mut completed = checkpoint["completedOperationIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if !completed.iter().any(|value| value.as_str() == Some(node_id)) {
        completed.push(json!(node_id));
    }
    checkpoint["completedOperationIds"] = json!(completed);
}

pub(crate) struct ProgramCtx<'a> {
    state: &'a AppState,
    tx: &'a UnboundedSender<Value>,
    session: &'a Value,
    request: &'a TurnRequest,
    session_id: &'a str,
    execution_id: &'a str,
    metrics: &'a std::sync::Mutex<Metrics>,
    generation: u64,
}

pub(crate) struct ProgramFrame {
    resolved: Value,
    checkpoint: Value,
    budget: super::super::budget::BudgetLedger,
    op_seq: u64,
    messages: Vec<Value>,
    paused: bool,
    failure: Option<Value>,
}

/// Rewrites ids inside a loop body so the same node in two iterations is a
/// different durable operation. `saveAs` keeps the original binding name.
pub(crate) fn scope_program_node_ids(node: &Value, prefix: &str) -> Value {
    let mut copy = node.clone();
    let node_type = node["type"].as_str().unwrap_or("");
    if node_type != "observe" {
        if let Some(id) = node["id"].as_str().filter(|value| !value.is_empty()) {
            copy["id"] = json!(format!("{prefix}:{id}"));
            if matches!(node_type, "call_tool" | "delegate") && copy["saveAs"].is_null() {
                copy["saveAs"] = json!(id);
            }
        }
    }
    for key in ["nodes", "then", "else", "body"] {
        if let Some(children) = node[key].as_array() {
            copy[key] = json!(children
                .iter()
                .map(|child| scope_program_node_ids(child, prefix))
                .collect::<Vec<_>>());
        }
    }
    copy
}

/// Executes nodes recursively so Sequence/Branch/Parallel/Loop can contain any
/// supported node. All side effects go through the same tool gateway.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_nodes<'a>(
    ctx: &'a ProgramCtx<'a>,
    nodes: Vec<Value>,
    mut resolved: Value,
    mut checkpoint: Value,
    mut budget: super::super::budget::BudgetLedger,
    mut op_seq: u64,
    depth: usize,
    persist: bool,
    skip_completed: bool,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<ProgramFrame, String>> + Send + 'a>,
> {
    Box::pin(async move {
        if depth > 16 {
            return Err("program nesting is too deep".into());
        }
        let verification_strict = crate::engine::algorithms::choose_for_agent(
            ctx.state,
            "verification.predicate",
            ctx.request.agent_id.as_deref(),
            Some(ctx.execution_id),
        )
        .and_then(|algorithm| algorithm["implementation"].as_str().map(str::to_string))
        .as_deref()
            == Some("strict");
        let mut messages = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            if ctx.state.is_cancelled(ctx.session_id)
                || ctx.state.cancellation_generation(ctx.session_id) != ctx.generation
            {
                return Err("실행이 중단되었습니다.".into());
            }
            let node_id = node["id"].as_str().unwrap_or("");
            if skip_completed
                && !node_id.is_empty()
                && crate::engine::program::checkpoint_skip_completed(&checkpoint, node_id)
            {
                continue;
            }
            if persist {
                checkpoint["nextIndex"] = json!(index);
                checkpoint["frameStack"] = json!([{
                    "nodeId": node_id,
                    "index": index,
                }]);
                checkpoint["resolvedInputs"] = resolved.clone();
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
            }
            match node["type"].as_str().unwrap_or("") {
                "observe" => {
                    let last = resolved.get("last").cloned().unwrap_or(Value::Null);
                    if !node_id.is_empty() {
                        resolved[node_id] = last;
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "call_tool" => {
                    let result = execute_program_tool(
                        ctx.state,
                        ctx.tx,
                        ctx.session,
                        ctx.request,
                        ctx.session_id,
                        ctx.execution_id,
                        node,
                        &mut resolved,
                        &mut checkpoint,
                        ctx.metrics,
                        &mut budget,
                        &mut op_seq,
                        index,
                        persist,
                    )
                    .await?;
                    let failed = !tool_succeeded(&result);
                    resolved["last"] = result.clone();
                    if failed && node["onFailure"].as_str() == Some("revise") {
                        let failure = json!({
                            "nodeId": node_id,
                            "tool": node["tool"],
                            "result": result,
                            "observation": failure_observation(
                                ctx.state,
                                node["tool"].as_str().unwrap_or(""),
                                &node["inputs"],
                                result["error"].as_str().unwrap_or("tool failed"),
                            ),
                        });
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(
                                ctx.state, ctx.execution_id, &checkpoint,
                            );
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
                }
                "branch" => {
                    use crate::engine::program::PredicateVerdict;
                    let chosen = match crate::engine::program::predicate_verdict(
                        &node["predicate"],
                        &resolved,
                    )? {
                        PredicateVerdict::True => {
                            node["then"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::False => {
                            node["else"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::Unknown if verification_strict => {
                            node["else"].as_array().cloned().unwrap_or_default()
                        }
                        PredicateVerdict::Unknown => {
                            // Do not send an unknown condition to `else`.
                            // Pause and let the model add the observation.
                            let mut checkpoint = checkpoint;
                            checkpoint["status"] = json!("paused");
                            checkpoint["pausedReason"] = json!("predicate_unknown");
                            checkpoint["pausedNodeId"] = json!(node_id);
                            checkpoint["resolvedInputs"] = resolved.clone();
                            if persist {
                                save_program_checkpoint(
                                    ctx.state, ctx.execution_id, &checkpoint,
                                );
                            }
                            messages.push(json!({
                                "role":"system",
                                "content": format!(
                                    "program branch `{node_id}` is Unknown for {}. Add the missing observation or choose another node; the else branch was not taken.",
                                    node["predicate"]
                                )
                            }));
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused: true,
                                failure: None,
                            });
                        }
                    };
                    let frame = execute_nodes(
                        ctx, chosen, resolved, checkpoint, budget, op_seq, depth + 1,
                        persist, true,
                    )
                    .await?;
                    resolved = frame.resolved;
                    checkpoint = frame.checkpoint;
                    budget = frame.budget;
                    op_seq = frame.op_seq;
                    messages.extend(frame.messages);
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "sequence" => {
                    let children = node["nodes"].as_array().cloned().unwrap_or_default();
                    let frame = execute_nodes(
                        ctx, children, resolved, checkpoint, budget, op_seq, depth + 1,
                        persist, true,
                    )
                    .await?;
                    resolved = frame.resolved;
                    checkpoint = frame.checkpoint;
                    budget = frame.budget;
                    op_seq = frame.op_seq;
                    messages.extend(frame.messages);
                    if frame.paused {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "parallel" => {
                    let children = node["nodes"].as_array().cloned().unwrap_or_default();
                    let child_ids = children
                        .iter()
                        .map(|child| json!(child["id"]))
                        .collect::<Vec<_>>();
                    checkpoint["pendingOperationIds"] = json!(child_ids);
                    checkpoint["parallelMode"] = json!("concurrent");
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if persist {
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                    }
                    let parallel_mode = crate::engine::algorithms::choose_for_agent(
                        ctx.state,
                        "execution.parallelism",
                        ctx.request.agent_id.as_deref(),
                        Some(ctx.execution_id),
                    )
                    .and_then(|algorithm| {
                        algorithm["implementation"].as_str().map(str::to_string)
                    })
                    .unwrap_or_else(|| "concurrent".into());
                    if parallel_mode == "sequential" {
                        let mut paused = false;
                        for child in &children {
                            let frame = execute_nodes(
                                ctx,
                                vec![child.clone()],
                                resolved,
                                checkpoint,
                                budget,
                                op_seq,
                                depth + 1,
                                false,
                                true,
                            )
                            .await?;
                            resolved = frame.resolved;
                            checkpoint = frame.checkpoint;
                            budget = frame.budget;
                            op_seq = frame.op_seq;
                            messages.extend(frame.messages);
                            if frame.paused {
                                paused = true;
                                break;
                            }
                        }
                        checkpoint["pendingOperationIds"] = json!([]);
                        checkpoint["parallelMode"] = json!("sequential");
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if paused {
                            checkpoint["status"] = json!("paused");
                            if persist {
                                save_program_checkpoint(
                                    ctx.state, ctx.execution_id, &checkpoint,
                                );
                            }
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused: true, failure: None,
                            });
                        }
                        mark_program_completed(&mut checkpoint, node_id);
                        crate::engine::algorithms::mark_signal(
                            ctx.state,
                            ctx.execution_id,
                            "execution.parallelism",
                            true,
                        );
                        if persist {
                            save_program_checkpoint(
                                ctx.state, ctx.execution_id, &checkpoint,
                            );
                        }
                        continue;
                    }
                    let base = budget.branch();
                    let mut futures = Vec::new();
                    for child in children.clone() {
                        futures.push(execute_nodes(
                            ctx,
                            vec![child],
                            resolved.clone(),
                            checkpoint.clone(),
                            base.branch(),
                            op_seq,
                            depth + 1,
                            false,
                            true,
                        ));
                    }
                    let results = futures_util::future::join_all(futures).await;
                    let mut paused = false;
                    let mut parallel_last = serde_json::Map::new();
                    for (child, result) in children.iter().zip(results.into_iter()) {
                        let frame = result?;
                        let child_id = child["id"].as_str().unwrap_or("");
                        if let Some(map) = frame.resolved.as_object() {
                            for (key, value) in map {
                                if key != "last" {
                                    resolved[key] = value.clone();
                                }
                            }
                            parallel_last.insert(
                                child_id.to_string(),
                                frame.resolved["last"].clone(),
                            );
                        }
                        if let Some(completed) =
                            frame.checkpoint["completedOperationIds"].as_array()
                        {
                            for id in completed {
                                mark_program_completed(
                                    &mut checkpoint,
                                    id.as_str().unwrap_or(""),
                                );
                            }
                        }
                        mark_program_completed(&mut checkpoint, child_id);
                        budget.merge_usage(&frame.budget);
                        op_seq = op_seq.max(frame.op_seq);
                        messages.extend(frame.messages);
                        paused |= frame.paused;
                    }
                    resolved["last"] = Value::Object(parallel_last);
                    checkpoint["pendingOperationIds"] = json!([]);
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if paused {
                        checkpoint["status"] = json!("paused");
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved,
                            checkpoint,
                            budget,
                            op_seq,
                            messages,
                            paused: true,
                                failure: None,
                        });
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                    crate::engine::algorithms::mark_signal(
                        ctx.state,
                        ctx.execution_id,
                        "execution.parallelism",
                        true,
                    );
                }
                "verify" => {
                    use crate::engine::program::PredicateVerdict;
                    let checks = node["checks"].as_array().cloned().unwrap_or_default();
                    let mut results = Vec::new();
                    let mut ok = true;
                    let mut unknown = false;
                    for check in checks {
                        let verdict = crate::engine::program::predicate_verdict(
                            &check["predicate"],
                            &resolved,
                        )?;
                        match verdict {
                            PredicateVerdict::True => {}
                            PredicateVerdict::False => ok = false,
                            PredicateVerdict::Unknown => {
                                if verification_strict {
                                    ok = false;
                                } else {
                                    unknown = true;
                                }
                            }
                        }
                        results.push(json!({
                            "predicate": check["predicate"],
                            "verdict": verdict.as_str(),
                        }));
                    }
                    resolved["lastVerify"] = json!({
                        "ok": ok && !unknown,
                        "unknown": unknown,
                        "checks": results,
                    });
                    crate::engine::algorithms::mark_signal(
                        ctx.state,
                        ctx.execution_id,
                        "verification.predicate",
                        ok && !unknown,
                    );
                    if !ok {
                        messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "program verify failed: {}",
                                resolved["lastVerify"]
                            )
                        }));
                    } else if unknown {
                        messages.push(json!({
                            "role":"system",
                            "content": format!(
                                "program verify is Unknown (not failed): {}. Add the missing observation before concluding.",
                                resolved["lastVerify"]
                            )
                        }));
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "delegate" => {
                    let mut child = node.clone();
                    child["tool"] = json!("delegate_task");
                    child["inputs"] = json!({
                        "agentId": node["agentId"],
                        "prompt": node["prompt"],
                        "wait": node["wait"],
                        "timeoutSeconds": node["timeoutSeconds"],
                    });
                    let result = execute_program_tool(
                        ctx.state,
                        ctx.tx,
                        ctx.session,
                        ctx.request,
                        ctx.session_id,
                        ctx.execution_id,
                        &child,
                        &mut resolved,
                        &mut checkpoint,
                        ctx.metrics,
                        &mut budget,
                        &mut op_seq,
                        index,
                        persist,
                    )
                    .await?;
                    resolved["last"] = result;
                }
                "loop" => {
                    let body = node["body"].as_array().cloned().unwrap_or_default();
                    let max = node["maxIterations"].as_u64().unwrap_or(3).min(16);
                    if node_id.is_empty() {
                        return Err("loop node needs an id for durable iteration recovery".into());
                    }
                    if !checkpoint["loopProgress"].is_object() {
                        checkpoint["loopProgress"] = json!({});
                    }
                    let mut iterations = checkpoint["loopProgress"][node_id]
                        .as_u64()
                        .unwrap_or(0);
                    while iterations < max {
                        if !crate::engine::program::predicate_holds(
                            &node["predicate"],
                            &resolved,
                        )? {
                            break;
                        }
                        let prefix = format!("{node_id}:{iterations}");
                        let scoped = body
                            .iter()
                            .map(|child| scope_program_node_ids(child, &prefix))
                            .collect::<Vec<_>>();
                        let frame = execute_nodes(
                            ctx,
                            scoped,
                            resolved,
                            checkpoint,
                            budget.clone(),
                            op_seq,
                            depth + 1,
                            persist,
                            true,
                        )
                        .await?;
                        resolved = frame.resolved;
                        checkpoint = frame.checkpoint;
                        budget = frame.budget;
                        op_seq = frame.op_seq;
                        messages.extend(frame.messages);
                        iterations += 1;
                        checkpoint["loopIterations"] = json!(iterations);
                        checkpoint["loopProgress"][node_id] = json!(iterations);
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        if frame.paused {
                            checkpoint["status"] = json!("paused");
                            return Ok(ProgramFrame {
                                resolved,
                                checkpoint,
                                budget,
                                op_seq,
                                messages,
                                paused: true,
                                failure: None,
                            });
                        }
                    }
                    mark_program_completed(&mut checkpoint, node_id);
                }
                "model_decision" => {
                    mark_program_completed(&mut checkpoint, node_id);
                    checkpoint["status"] = json!("paused");
                    checkpoint["pausedNodeId"] = json!(node_id);
                    checkpoint["nextIndex"] = json!(index + 1);
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if persist {
                        save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                    }
                    messages.push(json!({
                        "role":"system",
                        "content":"program requested a model decision; control returns to the model loop with saved progress."
                    }));
                    return Ok(ProgramFrame {
                        resolved,
                        checkpoint,
                        budget,
                        op_seq,
                        messages,
                        paused: true,
                                failure: None,
                    });
                }
                other => return Err(format!("unsupported program node type: {other}")),
            }
            checkpoint["resolvedInputs"] = resolved.clone();
            if persist {
                save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
            }
        }
        Ok(ProgramFrame {
            resolved,
            checkpoint,
            budget,
            op_seq,
            messages,
            paused: false,
                                failure: None,
        })
    })
}

pub(crate) fn save_revised_program(state: &AppState, execution_id: &str, program: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["revisedProgram"] = program.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn propose_program_revision(
    ctx: &ProgramCtx<'_>,
    program: &Value,
    checkpoint: &Value,
    failure: &Value,
    budget: &mut super::super::budget::BudgetLedger,
) -> Result<Option<Value>, String> {
    let provider_id = ctx
        .request
        .provider_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("program revision has no provider")?;
    let model = ctx
        .request
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("program revision has no model")?;
    if !budget.charge_round() {
        return Ok(None);
    }
    let available = crate::engine::tools::definitions()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let system = "You are the DOMINION program repair planner. A typed execution program failed. Propose a revision that only changes PENDING nodes. Output JSON only: {\"baseRevision\":\"...\",\"reason\":\"...\",\"evidenceIds\":[],\"replacePendingNodes\":[],\"addNodes\":[],\"dependencyChanges\":[],\"expectedObservations\":[]}. Nodes use the existing types (observe, call_tool, branch, sequence, parallel, loop, verify, delegate, model_decision). Completed node ids must not be replaced. call_tool nodes need id, tool, inputs, saveAs.";
    let prompt = format!(
        "Current program: {program}\nCheckpoint: {checkpoint}\nFailure observation: {failure}\nAvailable tools: {available:?}\nReturn only the JSON proposal."
    );
    let raw = llm::complete_for_session(
        ctx.state,
        provider_id,
        model,
        &system,
        &prompt,
        ctx.session_id,
    )
    .await?;
    ctx.metrics.lock().unwrap().model_calls += 1;
    let proposal = crate::engine::program::parse_json_object(&raw)
        .ok_or("program revision response is not a JSON object")?;
    let repair_mode = crate::engine::algorithms::choose_for_agent(
        ctx.state,
        "program.repair",
        ctx.request.agent_id.as_deref(),
        Some(ctx.execution_id),
    )
    .and_then(|algorithm| algorithm["parameters"]["mode"].as_str().map(str::to_string))
    .unwrap_or_else(|| "replace".into());
    let revised = crate::engine::program::apply_revision_proposal(
        program, checkpoint, &proposal, &available, 64, &repair_mode,
    )?;
    Ok(Some(revised))
}

pub(crate) async fn run_program(
    state: &AppState,
    tx: &UnboundedSender<Value>,
    session: &Value,
    request: &TurnRequest,
    session_id: &str,
    execution_id: &str,
    program: &Value,
    metrics: &std::sync::Mutex<Metrics>,
    budget: &mut super::super::budget::BudgetLedger,
    op_seq: &mut u64,
    generation: u64,
) -> Result<Vec<Value>, String> {
    let existing = state
        .store
        .get("execution", execution_id)
        .and_then(|execution| execution.get("programCheckpoint").cloned());
    let mut checkpoint = existing
        .filter(|checkpoint| {
            checkpoint["programRevision"].as_str() == program["revision"].as_str()
                && checkpoint["status"].as_str() != Some("completed")
        })
        .unwrap_or_else(|| crate::engine::program::new_checkpoint(program));
    let mut current_program = program.clone();
    if checkpoint["programRevision"].as_str() != program["revision"].as_str() {
        match state
            .store
            .get("execution", execution_id)
            .and_then(|execution| execution.get("revisedProgram").cloned())
            .filter(|revised| {
                revised["revision"].as_str()
                    == checkpoint["programRevision"].as_str()
            }) {
            Some(revised) => current_program = revised,
            None => {
                checkpoint = crate::engine::program::new_checkpoint(program);
            }
        }
    }
    let mut resolved = checkpoint["resolvedInputs"].clone();
    let start = checkpoint["nextIndex"].as_u64().unwrap_or(0) as usize;
    let mut remaining = current_program["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .skip(start)
        .collect::<Vec<_>>();
    checkpoint["status"] = json!("running");
    let ctx = ProgramCtx {
        state,
        tx,
        session,
        request,
        session_id,
        execution_id,
        metrics,
        generation,
    };
    let mut budget_local = budget.clone();
    let mut op_seq_local = *op_seq;
    let mut messages = Vec::new();
    let mut revisions = 0usize;
    let max_revisions = crate::engine::algorithms::active(state, "program.repair")
        .and_then(|algorithm| algorithm["parameters"]["maxRevisions"].as_u64())
        .unwrap_or(2) as usize;
    loop {
        let frame = execute_nodes(
            &ctx,
            remaining,
            resolved,
            checkpoint,
            budget_local,
            op_seq_local,
            0,
            true,
            true,
        )
        .await?;
        budget_local = frame.budget;
        op_seq_local = frame.op_seq;
        resolved = frame.resolved;
        checkpoint = frame.checkpoint;
        messages.extend(frame.messages);
        let Some(failure) = frame.failure else {
            break;
        };
        if revisions >= max_revisions {
            messages.push(json!({
                "role":"system",
                "content":"program revision limit reached; stop and report the observed failure."
            }));
            break;
        }
        revisions += 1;
        match propose_program_revision(
            &ctx,
            &current_program,
            &checkpoint,
            &failure,
            &mut budget_local,
        )
        .await
        {
            Ok(Some(revised)) => {
                let mut history = checkpoint["revisionHistory"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                history.push(json!({
                    "from": current_program["revision"],
                    "to": revised["revision"],
                    "reason": revised["appliedProposal"]["reason"],
                    "evidenceIds": revised["appliedProposal"]["evidenceIds"],
                }));
                checkpoint["revisionHistory"] = json!(history);
                checkpoint["programRevision"] = revised["revision"].clone();
                checkpoint["programFailure"] = Value::Null;
                checkpoint["nextIndex"] = json!(0);
                checkpoint["status"] = json!("running");
                save_revised_program(state, execution_id, &revised);
                save_program_checkpoint(state, execution_id, &checkpoint);
                crate::engine::algorithms::mark_signal(
                    state, execution_id, "program.repair", true,
                );
                messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "program revised: {} -> {} (reason: {}). Completed effects are preserved; only pending nodes continue.",
                        current_program["revision"], revised["revision"],
                        revised["appliedProposal"]["reason"]
                    )
                }));
                current_program = revised;
                remaining = current_program["nodes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                continue;
            }
            Ok(None) => break,
            Err(error) => {
                crate::engine::algorithms::mark_signal(
                    state, execution_id, "program.repair", false,
                );
                record_adaptation_error(
                    state,
                    execution_id,
                    &format!("program revision rejected: {error}"),
                );
                messages.push(json!({
                    "role":"system",
                    "content": format!(
                        "program revision was rejected: {error}. The original failure observation is preserved."
                    )
                }));
                break;
            }
        }
    }
    *budget = budget_local;
    *op_seq = op_seq_local;
    let node_count = current_program["nodes"]
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    checkpoint["resolvedInputs"] = resolved;
    if checkpoint["status"].as_str() == Some("paused") {
        save_program_checkpoint(state, execution_id, &checkpoint);
        return Ok(messages);
    }
    if !checkpoint["programFailure"].is_null() {
        // The program could not be repaired. Do not pretend it completed:
        // preserve the failure and require review/model continuation.
        checkpoint["status"] = json!("needs_review");
        save_program_checkpoint(state, execution_id, &checkpoint);
        messages.push(json!({
            "role":"system",
            "content": format!(
                "program stopped with an unresolved failure at {}: {}",
                checkpoint["programFailure"]["nodeId"],
                checkpoint["programFailure"]["result"]
            )
        }));
        return Ok(messages);
    }
    checkpoint["nextIndex"] = json!(node_count);
    checkpoint["status"] = json!("completed");
    save_program_checkpoint(state, execution_id, &checkpoint);
    messages.push(json!({
        "role": "system",
        "content": format!(
            "A durable execution program (revision {}) completed through the tool gateway. Use its real observations; do not repeat successful actions.",
            program["revision"].as_str().unwrap_or("")
        )
    }));
    Ok(messages)
}
