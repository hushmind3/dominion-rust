//! program responsibilities of the chat runtime.
use super::*;

fn save_program_checkpoint(state: &AppState, execution_id: &str, checkpoint: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["programCheckpoint"] = checkpoint.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

/// The real settlement contract used by ordinary and delegated tool nodes.
/// A failed node is only closed when the plan explicitly says `onFailure:continue`.
fn program_tool_settled(node:&Value,result:&Value)->bool{
    tool_succeeded(result) || node["onFailure"] == "continue"
}

/// A failed operation can deliberately feed a typed recovery branch.
/// Only the next substantive node may handle it, and its predicate must
/// reference the specific saved observation. Later unrelated branches never
/// swallow failed writes, and unknown predicates still pause independently.
fn predicate_reads_observation(predicate: &Value, observation: &str) -> bool {
    match predicate {
        Value::Object(fields) => fields.get("from").and_then(Value::as_str) == Some(observation)
            || fields.values().any(|part| predicate_reads_observation(part, observation)),
        Value::Array(items) => items.iter().any(|part| predicate_reads_observation(part, observation)),
        _ => false,
    }
}

fn next_recovery_branch<'a>(nodes: &'a [Value], index: usize, observation: &str) -> Option<&'a Value> {
    nodes.get(index + 1..)?.iter().find(|node| node["type"] != "observe")
        .filter(|node| node["type"] == "branch"
            && node["then"].as_array().is_some()
            && node["else"].as_array().is_some()
            && predicate_reads_observation(&node["predicate"], observation))
}

/// Record a failed tool as handled by an upcoming conditional rather than
/// pretending it succeeded or blindly retrying that external operation.
fn settle_conditional_failure(
    nodes: &[Value], index: usize, node: &Value, result: &Value,
    checkpoint: &mut Value,
) -> bool {
    // Explicit failure-handling instructions override inferred branch recovery.
    if node.get("onFailure").is_some_and(|value| !value.is_null()) { return false; }
    let node_id = node["id"].as_str().unwrap_or("");
    let result_key = node["saveAs"].as_str().filter(|key| !key.is_empty()).unwrap_or(node_id);
    let Some(branch) = next_recovery_branch(nodes,index,result_key) else { return false; };
    let mut handled = checkpoint["handledToolFailures"].as_array().cloned().unwrap_or_default();
    handled.push(json!({"nodeId":node_id,"observation":result_key,"result":result,
        "handler":branch["id"]}));
    checkpoint["handledToolFailures"] = json!(handled);
    mark_program_completed(checkpoint,node_id);
    true
}

#[allow(clippy::too_many_arguments)]
async fn execute_program_tool(
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
    // A failed effect may need a new plan. Recording it as completed before
    // interpreting its receipt makes replacePendingNodes impossible.
    // Explicit onFailure=continue means the owner intended a best-effort
    // operation and deliberately accepts its failure as a settled step.
    if program_tool_settled(node,&result) {
        mark_program_completed(checkpoint, node_id);
    }
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

fn mark_program_completed(checkpoint: &mut Value, node_id: &str) {
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

struct ProgramCtx<'a> {
    state: &'a AppState,
    tx: &'a UnboundedSender<Value>,
    session: &'a Value,
    request: &'a TurnRequest,
    session_id: &'a str,
    execution_id: &'a str,
    metrics: &'a std::sync::Mutex<Metrics>,
    generation: u64,
}

struct ProgramFrame {
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
fn scope_program_node_ids(node: &Value, prefix: &str) -> Value {
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
fn execute_nodes<'a>(
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
                    let recovered_by_branch = failed && settle_conditional_failure(
                        &nodes,index,node,&result,&mut checkpoint);
                    if failed && !program_tool_settled(node,&result) && !recovered_by_branch {
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
                    // A failed nested call is not a successful branch/sequence.
                    // Bubble its durable receipt to the program repair loop;
                    // otherwise the enclosing node would mask the failure.
                    if frame.paused || frame.failure.is_some() {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: frame.paused, failure: frame.failure,
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
                    // A failed nested call is not a successful branch/sequence.
                    // Bubble its durable receipt to the program repair loop;
                    // otherwise the enclosing node would mask the failure.
                    if frame.paused || frame.failure.is_some() {
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: frame.paused, failure: frame.failure,
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
                        let mut child_failure = None;
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
                            if frame.paused || frame.failure.is_some() {
                                paused = frame.paused;
                                child_failure = frame.failure;
                                break;
                            }
                        }
                        checkpoint["pendingOperationIds"] = json!([]);
                        checkpoint["parallelMode"] = json!("sequential");
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if paused || child_failure.is_some() {
                            if paused { checkpoint["status"] = json!("paused"); }
                            if persist {
                                save_program_checkpoint(
                                    ctx.state, ctx.execution_id, &checkpoint,
                                );
                            }
                            return Ok(ProgramFrame {
                                resolved, checkpoint, budget, op_seq, messages,
                                paused, failure: child_failure,
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
                    let mut futures = Vec::new();
                    // Split both the tool budget and durable opSeq namespace.
                    // Parallel children must never reuse the same operation
                    // sequence, even if they finish in a different order.
                    let mut reserved_end=op_seq;
                    for (child_index, child) in children.clone().into_iter().enumerate() {
                        let branch_budget=budget.partition(child_index, children.len());
                        let child_start=reserved_end;
                        reserved_end=reserved_end.saturating_add(
                            branch_budget.remaining_tool_slots() as u64
                        );
                        futures.push(execute_nodes(
                            ctx,
                            vec![child],
                            resolved.clone(),
                            checkpoint.clone(),
                            branch_budget,
                            child_start,
                            depth + 1,
                            false,
                            true,
                        ));
                    }
                    let results = futures_util::future::join_all(futures).await;
                    let mut paused = false;
                    let mut parallel_failure = None;
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
                        if frame.failure.is_none() && !frame.paused {
                            mark_program_completed(&mut checkpoint, child_id);
                        } else if parallel_failure.is_none() {
                            parallel_failure = frame.failure.clone();
                        }
                        budget.merge_usage(&frame.budget);
                        op_seq = op_seq.max(frame.op_seq);
                        messages.extend(frame.messages);
                        paused |= frame.paused;
                    }
                    // A later operation must start AFTER all reserved branch
                    // ranges, including slots left unused by an early exit.
                    op_seq=op_seq.max(reserved_end);
                    resolved["last"] = Value::Object(parallel_last);
                    checkpoint["pendingOperationIds"] = json!([]);
                    checkpoint["resolvedInputs"] = resolved.clone();
                    if let Some(failure) = parallel_failure {
                        // Multiple branches ran, but one failed and remains
                        // pending for revision. Preserve every sibling receipt.
                        checkpoint["programFailure"] = failure.clone();
                        if persist { save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint); }
                        return Ok(ProgramFrame { resolved, checkpoint, budget,
                            op_seq, messages, paused: false, failure: Some(failure) });
                    }
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
                    let failed = !tool_succeeded(&result);
                    resolved["last"] = result.clone();
                    let recovered_by_branch = failed && settle_conditional_failure(
                        &nodes,index,node,&result,&mut checkpoint);
                    if failed && !program_tool_settled(node,&result) && !recovered_by_branch {
                        let failure = json!({
                            "nodeId": node_id,
                            "tool": "delegate_task",
                            "result": result,
                            "observation": failure_observation(
                                ctx.state, "delegate_task", &node["inputs"],
                                result["error"].as_str().unwrap_or("delegated task failed"),
                            ),
                        });
                        checkpoint["programFailure"] = failure.clone();
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
                        }
                        return Ok(ProgramFrame {
                            resolved, checkpoint, budget, op_seq, messages,
                            paused: false, failure: Some(failure),
                        });
                    }
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
                        if frame.paused || frame.failure.is_some() {
                            // An incomplete loop body is not one completed
                            // iteration. Keep its operation receipts and the
                            // old loop cursor for repair/restart.
                            if frame.paused { checkpoint["status"] = json!("paused"); }
                            checkpoint["resolvedInputs"] = resolved.clone();
                            if persist { save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint); }
                            return Ok(ProgramFrame { resolved, checkpoint, budget,
                                op_seq, messages, paused: frame.paused, failure: frame.failure });
                        }
                        iterations += 1;
                        checkpoint["loopIterations"] = json!(iterations);
                        checkpoint["loopProgress"][node_id] = json!(iterations);
                        checkpoint["resolvedInputs"] = resolved.clone();
                        if persist {
                            save_program_checkpoint(ctx.state, ctx.execution_id, &checkpoint);
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

fn save_revised_program(state: &AppState, execution_id: &str, program: &Value) {
    if let Some(mut execution) = state.store.get("execution", execution_id) {
        execution["revisedProgram"] = program.clone();
        if let Some(saved) = state.store.put("execution", &execution) {
            state.bus.record("execution", &saved);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn propose_program_revision(
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

pub(super) async fn run_program(
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


#[cfg(test)]
mod program_effect_receipt_tests {
    use super::*;
    #[test]
    fn explicit_repair_is_not_swallowed_by_adjacent_failure_branch() {
        let nodes=json!([
            {"type":"call_tool","id":"try","onFailure":"revise","tool":"shell_exec",
                "inputs":{"command":"exit 7"}},
            {"type":"branch","id":"recover","predicate":{
                "type":"equals","from":"try","path":"/exitCode","value":0},
                "then":[],"else":[]}
        ]);
        let arr=nodes.as_array().unwrap();
        let mut checkpoint=crate::engine::program::new_checkpoint(&json!({
            "revision":"explicit","nodes":arr}));
        assert!(!settle_conditional_failure(arr,0,&arr[0],&json!({"exitCode":7}),&mut checkpoint));
        assert!(!crate::engine::program::checkpoint_skip_completed(&checkpoint,"try"));
    }

    #[test]
    fn failed_tool_can_feed_immediately_following_conditional_fallback() {
        let nodes=json!([
            {"type":"call_tool","id":"try","saveAs":"try","tool":"shell_exec",
                "inputs":{"command":"exit 7"}},
            {"type":"branch","id":"recover",
                "predicate":{"type":"equals","from":"try","path":"/exitCode","value":0},
                "then":[{"type":"observe","id":"unused"}],
                "else":[{"type":"call_tool","id":"fallback","tool":"workspace_write",
                    "inputs":{"path":"fallback.txt","content":"FALLBACK"}}]}
        ]);
        let items=nodes.as_array().unwrap();
        assert!(next_recovery_branch(items,0,"try").is_some());
        assert!(next_recovery_branch(items,0,"unrelated").is_none());
        let mut checkpoint=crate::engine::program::new_checkpoint(&json!({
            "revision":"fallback","nodes":items}));
        let failed=json!({"exitCode":7});
        assert!(!tool_succeeded(&failed));
        assert!(settle_conditional_failure(items,0,&items[0],&failed,&mut checkpoint));
        assert!(crate::engine::program::checkpoint_skip_completed(&checkpoint,"try"));
        assert_eq!(checkpoint["handledToolFailures"][0]["handler"],"recover");
        assert_eq!(checkpoint["handledToolFailures"][0]["result"]["exitCode"],7);
        let intervening=json!({"id":"write","type":"call_tool"});
        assert!(next_recovery_branch(&[items[0].clone(),intervening,items[1].clone()],0,"try").is_none());
        assert!(predicate_reads_observation(&json!({
            "type":"not","predicate":items[1]["predicate"]}),"try"));
        assert!(!predicate_reads_observation(&json!({
            "type":"equals","from":"other","value":"try"}),"try"));
    }

    #[test]
    fn failed_tool_operation_remains_pending_for_repair_instead_of_being_sealed() {
        let fail=json!({"error":"tool executable missing"});
        let mut checkpoint=crate::engine::program::new_checkpoint(
            &json!({"revision":"v1","nodes":[
                {"id":"write","type":"call_tool","tool":"workspace_write","inputs":{"path":"a","content":"b"}}
            ]})
        );
        let required=json!({"onFailure":"revise"});
        let best_effort=json!({"onFailure":"continue"});
        assert!(!program_tool_settled(&required,&fail));
        assert!(!program_tool_settled(&json!({}),&fail));
        assert!(program_tool_settled(&best_effort,&fail));
        assert!(program_tool_settled(&required,&json!({"success":true})));
        if program_tool_settled(&required,&fail) {
            mark_program_completed(&mut checkpoint,"write");
        }
        assert!(!crate::engine::program::checkpoint_skip_completed(&checkpoint,"write"));
        // The actual repair function must permit replacing this failed node,
        // but refuse to touch one that settled successfully.
        let p=json!({"revision":"v1","nodes":[
            {"id":"write","type":"call_tool","tool":"workspace_write","inputs":{"path":"a","content":"b"}}
        ]});
        let revision=json!({"baseRevision":"v1","reason":"repair bad tool input",
            "replacePendingNodes":[{"id":"write","type":"call_tool",
                "tool":"workspace_write","inputs":{"path":"a","content":"fixed"}}]});
        let available=vec!["workspace_write".to_string()];
        assert!(crate::engine::program::apply_revision_proposal(
            &p,&checkpoint,&revision,&available,64,"replace").is_ok());
        mark_program_completed(&mut checkpoint,"write");
        assert!(crate::engine::program::apply_revision_proposal(
            &p,&checkpoint,&revision,&available,64,"replace").is_err());
    }
}
