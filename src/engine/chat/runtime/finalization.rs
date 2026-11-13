//! finalization: isolated native chat runtime responsibility; preserve executable RSI behavior.
use super::*;

pub(crate) fn tool_succeeded(value: &Value) -> bool {
    value.get("isError").and_then(Value::as_bool) != Some(true)
        && value.get("success").and_then(Value::as_bool) != Some(false)
        && value.get("error").is_none_or(Value::is_null)
        && value
            .get("exitCode")
            .is_none_or(|code| code.as_i64() == Some(0))
        && value
            .get("status")
            .and_then(Value::as_u64)
            .is_none_or(|code| code < 400)
}

/// Overall compliance for an attempt. A single violated standing strategy must
/// not be masked by another followed strategy in the same bundle.
/// Evaluator version is separate from the payload hash, so re-running the same
/// terminal attempt with a new evaluator is a new evaluation revision rather
/// than a conflicting payload.
pub(super) const FINALIZATION_EVALUATOR_VERSION: &str = "finalize-semantic-v1";

/// Immutable semantic content of a terminal attempt. Counters, timings and
/// `finishedAt` are excluded: a retried finalization of the same attempt must
/// produce the same hash even though those mutable fields differ.
pub(super) fn finalization_semantic_payload(record: &Value, operations: &[Value]) -> Value {
    let mut normalized = operations
        .iter()
        .map(|operation| {
            json!({
                "id": operation["id"],
                "runId": operation["runId"],
                "tool": operation["tool"],
                "arguments": operation["arguments"],
                "origin": operation["origin"],
                "procedureNodeId": operation["procedureNodeId"],
                "procedureStrategyId": operation["procedureStrategyId"],
                "procedureRevision": operation["procedureRevision"],
                "status": operation["status"],
                "result": operation["result"],
                "outcomeUnknown": operation["outcomeUnknown"],
            })
        })
        .collect::<Vec<_>>();
    normalized.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or("")
            .cmp(right["id"].as_str().unwrap_or(""))
    });
    json!({
        "schemaVersion": 1,
        "evaluatorVersion": FINALIZATION_EVALUATOR_VERSION,
        "executionId": record["id"],
        "sessionId": record["sessionId"],
        "agentId": record["agentId"],
        "request": record["request"],
        "status": record["status"],
        "taskOutcome": record["taskOutcome"],
        "failureClass": record["failureClass"],
        "acceptancePassed": record["acceptancePassed"],
        "acceptanceText": record["acceptanceText"],
        "appliedImprovementIds": record["appliedImprovementIds"],
        "strategyCompliance": record["strategyCompliance"],
        "finalMessageId": record["finalMessageId"],
        "operations": normalized,
    })
}

pub(super) fn finalization_payload_hash(record: &Value, operations: &[Value]) -> String {
    crate::engine::memory::stable_id(&[
        &finalization_semantic_payload(record, operations).to_string(),
    ])
}

/// Overall compliance for an attempt. Contract:
/// 1. any confirmed `violated` wins — never masked by another arm's `followed`
///    or by an `unknown` sibling;
/// 2. then `unknown/invalid` (evidence not trustworthy);
/// 3. then `followed`; 4. `not_triggered`; 5. otherwise `not_applicable`.
pub(super) fn compliance_summary(
    entries: &[Value],
) -> crate::engine::adaptation::types::Compliance {
    use crate::engine::adaptation::types::Compliance;
    if entries.iter().any(|entry| {
        entry["applicable"] == true
            && entry["result"]["triggered"] == true
            && entry["result"]["followed"] != true
            && !matches!(entry["result"]["status"].as_str(), Some("unknown" | "invalid"))
    }) {
        return Compliance::Violated;
    }
    if entries.iter().any(|entry| {
        matches!(entry["result"]["status"].as_str(), Some("unknown" | "invalid"))
    }) {
        return Compliance::Unknown;
    }
    if entries.iter().any(|entry| {
        entry["applicable"] == true
            && entry["result"]["triggered"] == true
            && entry["result"]["followed"] == true
    }) {
        return Compliance::Followed;
    }
    if entries.iter().any(|entry| entry["applicable"] == true) {
        return Compliance::NotTriggered;
    }
    Compliance::NotApplicable
}

/// Lossless per-verdict counts stored next to the single summary so a violated
/// arm stays visible even when another arm is unknown.
pub(super) fn compliance_detail(entries: &[Value]) -> Value {
    let mut violated = Vec::new();
    let mut unknown = Vec::new();
    let mut followed = Vec::new();
    let mut not_triggered = Vec::new();
    let mut not_applicable = Vec::new();
    for entry in entries {
        let Some(id) = entry["strategyId"].as_str() else {
            continue;
        };
        let status = entry["result"]["status"].as_str();
        if matches!(status, Some("unknown" | "invalid")) {
            unknown.push(json!(id));
        } else if entry["applicable"] != true {
            not_applicable.push(json!(id));
        } else if entry["result"]["triggered"] != true {
            not_triggered.push(json!(id));
        } else if entry["result"]["followed"] == true {
            followed.push(json!(id));
        } else {
            violated.push(json!(id));
        }
    }
    json!({
        "summary": compliance_summary(entries),
        "total": entries.len(),
        "violated": violated,
        "unknown": unknown,
        "followed": followed,
        "notTriggered": not_triggered,
        "notApplicable": not_applicable,
    })
}

pub(super) fn standing_violation(execution: &Value) -> bool {
    execution["strategyCompliance"]
        .as_array()
        .is_some_and(|items| {
            items.iter().any(|item| {
                item["authority"].as_str() == Some("standing")
                    && item["applicable"].as_bool() == Some(true)
                    && item["result"]["verified"].as_bool() == Some(true)
                    && item["result"]["triggered"].as_bool() == Some(true)
                    && item["result"]["followed"].as_bool() == Some(false)
            })
        })
}

pub(super) fn determine_task_outcome(execution: &Value) -> TaskOutcome {
    match execution["status"].as_str() {
        Some("cancelled") => return TaskOutcome::Cancelled,
        Some("completed") => {}
        _ => return TaskOutcome::Failed,
    }
    if standing_violation(execution) {
        return TaskOutcome::Failed;
    }
    if execution["acceptancePassed"] == true {
        return if crate::engine::verification::has_result_verifier(&execution["acceptanceChecks"]) {
            TaskOutcome::Succeeded
        } else {
            TaskOutcome::AnsweredUnverified
        };
    }
    if execution["acceptanceText"].is_null() {
        return TaskOutcome::AnsweredUnverified;
    }
    TaskOutcome::Failed
}

pub(super) fn record_success_experience(
    state: &AppState,
    record: &Value,
    execution_id: &str,
    task_outcome: TaskOutcome,
) -> Option<Value> {
    if task_outcome != TaskOutcome::Succeeded {
        return None;
    }
    // Deterministic execution is a shortcut, not evidence that the harness
    // learned to solve a new task.
    if record["deterministicShortcut"].is_object() {
        return None;
    }
    let acceptance_text = record["acceptanceText"].as_str()?;
    let final_message_id = record["finalMessageId"].as_str()?;
    // One verified success sample per root task. Nine room speakers reusing
    // the same owner message are not nine independent successes.
    let task_id = record["taskId"].as_str().unwrap_or(execution_id);
    let marker_id = crate::engine::episode::success_marker_id(task_id);
    let marker = json!({
        "id": marker_id,
        "taskId": task_id,
        "executionId": execution_id,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    if state
        .store
        .put_if_absent("success_experience_marker", &marker)
        .is_none()
    {
        return None;
    }
    let scope = crate::engine::memory::MemoryScope::for_session(
        &state.store,
        record["sessionId"].as_str().unwrap_or(""),
        record["agentId"].as_str(),
    );
    let lesson = format!(
        "검증 조건을 실제 도구 결과로 충족함. 결과 요약: {}",
        record["finalAnswer"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(300)
            .collect::<String>()
    );
    let tools = record["toolsUsed"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    crate::engine::experience::record_shared(
        state,
        &scope,
        acceptance_text,
        "success",
        &lesson,
        "",
        &[final_message_id.to_string(), execution_id.to_string()],
        &tools,
        &[],
    )
}

/// The single terminal judgment for every turn path. `finalize_execution`
/// remains as the implementation used by existing tests.
/// Freezes the exact policy bundle, context manifest, memory watermark and
/// model settings used by this attempt. Recorded before the first model call
/// and before any harness node so the attempt cannot drift from its evidence.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_selection_snapshot(
    state: &AppState,
    execution_id: &str,
    task_id: &str,
    session_id: &str,
    request: &TurnRequest,
    strategies: &[Value],
    context_items: Vec<Value>,
    memory_watermark: String,
    model: Value,
) -> Result<(), String> {
    use crate::engine::adaptation::types::{
        revision_hash, BundleArm, CandidateScore, Decision, PolicyBundle, SelectionSnapshot,
        StrategyRevision, TaskContract,
    };
    let contract = state
        .store
        .get("task_episode", task_id)
        .and_then(|episode| serde_json::from_value::<TaskContract>(episode["contract"].clone()).ok())
        .ok_or("실행에 연결된 작업 계약을 찾을 수 없습니다.")?;
    let strategy_revisions = strategies
        .iter()
        .filter_map(|strategy| {
            let revision = StrategyRevision {
                strategy_id: strategy["id"].as_str()?.to_string(),
                revision: strategy["revision"].as_str()?.to_string(),
                parent: strategy["previousRevision"].as_str().map(str::to_string),
                applicability: strategy["verification"]["appliesTo"].clone(),
                authority: strategy["authority"].as_str().unwrap_or("adaptive").to_string(),
                procedure: Some(strategy["verification"].clone()),
                advice: strategy["text"].as_str().map(str::to_string),
                checks: Vec::new(),
                provenance: Vec::new(),
                compatibility: json!({}),
                scope: json!({"sessionId": session_id}),
            };
            revision.validate().ok()?;
            Some(revision)
        })
        .collect::<Vec<_>>();
    let arms = strategies
        .iter()
        .filter_map(|strategy| {
            let kind = strategy["kind"].as_str().unwrap_or("strategy");
            let id = strategy["id"].as_str()?.to_string();
            let policy = kind == "strategy"
                && strategy["assistanceMode"].as_str() == Some("harness_assisted")
                && crate::engine::procedure::applies(strategy, &request.input, None, None);
            // Every injected arm must appear, even if a legacy record predates
            // the revision field. A missing revision is derived, not dropped.
            let revision = strategy["revision"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    crate::engine::memory::stable_id(&[
                        &id,
                        strategy["text"].as_str().unwrap_or(""),
                    ])
                });
            Some(BundleArm {
                arm_id: id,
                kind: kind.to_string(),
                authority: strategy["authority"]
                    .as_str()
                    .unwrap_or("adaptive")
                    .to_string(),
                revision,
                role: if policy { "policy" } else { "advice" }.to_string(),
                probability: strategy["selectionProbability"].as_f64().unwrap_or(1.0),
            })
        })
        .collect::<Vec<_>>();
    let bundle_id = format!("bundle:{execution_id}");
    let bundle = PolicyBundle {
        id: bundle_id.clone(),
        task_id: task_id.to_string(),
        slot: "strategy".into(),
        policy_revision: "legacy-bundle-v1".into(),
        selection: "deterministic".into(),
        arms,
        probability: 1.0,
        rng_seed: 0,
        context: json!({"sessionId": session_id, "armCount": strategies.len()}),
    };
    // The deterministic injector always plays the whole bundle, so the played
    // arm is the bundle itself with propensity 1. Individual arms are listed
    // inside the bundle and are not misreported as sampled arms.
    let decision = Decision {
        id: execution_id.to_string(),
        task_id: task_id.to_string(),
        slot: "strategy".into(),
        context: json!({
            "sessionId": session_id,
            "bundleId": bundle_id,
            "selection": "deterministic",
            "appliedArmIds": bundle.arms.iter().map(|arm| arm.arm_id.clone()).collect::<Vec<_>>(),
        }),
        candidates: vec![CandidateScore {
            arm_id: bundle_id.clone(),
            mean: 0.5,
            uncertainty: 0.5,
            cost: 0.0,
            risk: 0.0,
            probability: 1.0,
        }],
        selected: bundle_id,
        probability: 1.0,
        policy_revision: "legacy-bundle-v1".into(),
        rng_seed: 0,
    };
    let completeness = json!({
        "context": "recorded",
        "memory": "recorded",
        "model": "recorded",
        "harnessOutput": "recorded as execution_operation evidence after this freeze",
        "providerTemplate": "unavailable: the provider may apply its own chat template outside the engine",
        "reproducible": true,
    });
    let snapshot_hash = revision_hash(&json!({
        "schemaVersion": 2,
        "contract": contract,
        "strategies": strategy_revisions,
        "decisions": [decision.clone()],
        "contextItems": context_items,
        "memoryWatermark": memory_watermark,
        "model": model,
        "completeness": completeness,
        "verificationOwner": request.verification_owner,
    }));
    let snapshot = SelectionSnapshot {
        contract: contract.clone(),
        strategies: strategy_revisions,
        decisions: vec![decision.clone()],
        context_items,
        memory_watermark,
        model,
        policy_bundle: Some(bundle),
        completeness,
        snapshot_hash,
    };
    if contract.validate().is_err() || decision.validate().is_err() || snapshot.validate().is_err() {
        return Err("실행 선택 스냅샷이 계약을 충족하지 못했습니다.".into());
    }
    if !snapshot.is_reproducible() {
        return Err("실행 선택 스냅샷의 재현 정보가 불완전합니다.".into());
    }
    if state
        .store
        .put(
            "strategy_decision",
            &serde_json::to_value(&decision).unwrap_or_else(|_| json!({})),
        )
        .is_none()
    {
        return Err("정책 결정을 저장할 수 없습니다.".into());
    }
    if state
        .store
        .put(
            "selection_snapshot",
            &serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({})),
        )
        .is_none()
    {
        return Err("실행 선택 스냅샷을 저장할 수 없습니다.".into());
    }
    Ok(())
}

pub(crate) fn finish_attempt(
    state: &AppState,
    execution_id: &str,
    result: &Result<Option<Value>, String>,
    metrics: &std::sync::Mutex<Metrics>,
) {
    finalize_execution(state, execution_id, result, metrics);
}

pub(crate) fn finalize_execution(
    state: &AppState,
    execution_id: &str,
    result: &Result<Option<Value>, String>,
    metrics: &std::sync::Mutex<Metrics>,
) {
    if execution_id.is_empty() {
        return;
    }
    let Some(mut record) = state.store.get("execution", execution_id) else {
        return;
    };
    let counters = metrics.lock().unwrap();
    record["iterations"] = json!(counters.iterations);
    record["tokens"] = json!(counters.tokens);
    record["modelCalls"] = json!(counters.model_calls);
    record["toolCalls"] = json!(counters.tool_calls);
    record["firstTokenMs"] = json!(counters.first_token_ms);
    record["modelElapsedMs"] = json!(counters.model_elapsed_ms);
    record["toolElapsedMs"] = json!(counters.tool_elapsed_ms);
    record["contextElapsedMs"] = json!(counters.context_elapsed_ms);
    record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    let final_phase = |status: &str| crate::engine::run_state::phase_for_status(status);
    match result {
        Ok(_) => record["status"] = json!("completed"),
        Err(message) if message.contains("중단") => {
            record["status"] = json!("cancelled");
            record["reason"] = json!(message);
        }
        Err(message) => {
            record["status"] = json!("failed");
            record["reason"] = json!(message);
        }
    }
    let status = record["status"].as_str().unwrap_or("failed").to_string();
    record["phase"] = json!(final_phase(&status));
    record["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    for mut operation in state.store.all("execution_operation") {
        if operation["runId"] == execution_id && operation["status"] == "running" {
            operation["status"] = json!("interrupted");
            operation["outcomeUnknown"] = json!(true);
            operation["effectState"] = json!("effect_unknown");
            if operation["toolReceipt"].is_object() {
                operation["toolReceipt"]["status"] = json!("interrupted");
                operation["toolReceipt"]["outcomeUnknown"] = json!(true);
                operation["toolReceipt"]["verified"] = json!(false);
            }
            operation["reason"] = json!("실행이 중단되어 도구 결과를 확정할 수 없습니다.");
            operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            if let Some(saved) = state.store.put("execution_operation", &operation) {
                state.bus.record("execution_operation", &saved);
            }
        }
    }
    let mut operations = state
        .store
        .all("execution_operation")
        .into_iter()
        .filter(|operation| operation["runId"].as_str() == Some(execution_id))
        .collect::<Vec<_>>();
    crate::engine::procedure::sort_operations(&mut operations);
    record["effectSummary"] = crate::engine::effect::summary(&operations);
    let request_text = record["request"].as_str().unwrap_or("");
    let compliance = record["strategyCandidates"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|strategy| {
            let result = crate::engine::procedure::evaluate(
                strategy,
                &operations,
                &state.config.workspace,
            );
            let applicable = crate::engine::procedure::applies(
                strategy,
                request_text,
                None,
                None,
            );
            json!({
                "strategyId": strategy["id"],
                "text": strategy["text"],
                "authority": strategy["authority"],
                "applicable": applicable,
                "result": result,
            })
        })
        .collect::<Vec<_>>();
    record["strategyCompliance"] = json!(compliance);
    record["complianceSummary"] = compliance_detail(&compliance);
    if record["acceptancePassed"] == true {
        if let (Some(text), Some(source), Some(session)) = (
            record["acceptanceText"].as_str(),
            record["acceptanceSourceMessageId"].as_str(),
            record["sessionId"].as_str(),
        ) {
            let evidence = evidence_for_source(state, session, source, execution_id);
            let gaps = goals::check_acceptance(state, text, &record["acceptanceChecks"], &evidence);
            if !gaps.is_empty() {
                record["acceptancePassed"] = json!(false);
                record["acceptanceFinalGaps"] = json!(gaps);
            }
        }
    }
    let task_outcome = determine_task_outcome(&record);
    record["taskOutcome"] =
        serde_json::to_value(task_outcome).unwrap_or_else(|_| json!("unknown"));
    if !matches!(task_outcome, TaskOutcome::Succeeded) {
        let failure = crate::engine::recovery::failure_record(&record, &operations);
        record["failureClass"] = failure["failureClass"].clone();
        record["recoveryHint"] = failure["recoveryHint"].clone();
    }
    let updates = match crate::engine::improve::execution_updates(state, &record) {
        Ok(updates) => updates,
        Err(error) => {
            state.store.audit(
                "execution.finalization_update_failed",
                json!({"executionId": execution_id, "error": error}),
            );
            if let Some(saved) = state.store.put("execution", &record) {
                state.bus.record("execution", &saved);
            }
            return;
        }
    };
    let payload_hash = finalization_payload_hash(&record, &operations);
    let marker_id = format!("execution:{execution_id}");
    let marker = json!({
        "id": marker_id,
        "executionId": execution_id,
        "payloadHash": payload_hash,
        "statsApplied": false,
        "execution": record.clone(),
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });

    use crate::engine::adaptation::types::{
        EvidenceKind, EvidenceRef, LabelProvenance, Observation,
        ObservationRegime,
    };
    let compliance_entries = record["strategyCompliance"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let compliance = compliance_summary(&compliance_entries);
    let evidence = operations
        .iter()
        .filter_map(|operation| {
            let id = operation["id"].as_str()?;
            Some(EvidenceRef {
                kind: EvidenceKind::Operation,
                id: id.to_string(),
                revision: None,
                scope: json!({"sessionId": record["sessionId"]}),
                observed_at: operation["startedAt"].as_str().map(str::to_string),
                digest: None,
            })
        })
        .collect::<Vec<_>>();
    let observation = Observation {
        decision_id: execution_id.to_string(),
        attempt_id: execution_id.to_string(),
        outcome: task_outcome,
        compliance,
        checks: compliance_entries.clone(),
        evidence,
        costs: json!({
            "tokens": record["tokens"],
            "toolCalls": record["toolCalls"],
            "iterations": record["iterations"],
        }),
        regime: ObservationRegime::ProductionObserved,
        labels: LabelProvenance::DeterministicOracle,
    };
    let observations = if observation.validate().is_ok() {
        let mut value = serde_json::to_value(&observation).unwrap_or_else(|_| json!({}));
        value["id"] = json!(format!("observation:{execution_id}"));
        vec![value]
    } else {
        Vec::new()
    };

    let mut applied = false;
    match state
        .store
        .finalize_once(&record, &marker, &updates, &observations)
    {
        Ok(crate::db::FinalizeOutcome::Applied) => {
            applied = true;
            if let Some(saved) = state.store.get("execution", execution_id) {
                state.bus.record("execution", &saved);
            }
            for update in &updates {
                state.bus.record("improvement", update);
            }
            for observation in &observations {
                state.bus.record("strategy_observation", observation);
            }
            let queued = crate::engine::improve::schedule_experiments(state);
            crate::engine::policy::meta_improve(state);
            let proposed = crate::engine::policy::propose_candidates(state);
            if !queued.is_empty() || !proposed.is_empty() {
                state.store.audit(
                    "experiment.scheduled",
                    json!({
                        "executionId": execution_id,
                        "requests": queued.len(),
                        "policyCandidates": proposed.len()
                    }),
                );
            }
        }
        Ok(crate::db::FinalizeOutcome::AlreadyApplied) => {
            state.store.audit(
                "execution.finalization_already_applied",
                json!({"executionId": execution_id}),
            );
        }
        Ok(crate::db::FinalizeOutcome::Conflict) => {
            state.store.audit(
                "execution.finalization_conflict",
                json!({"executionId": execution_id}),
            );
            record["finalizationConflict"] = json!(true);
            if let Some(saved) = state.store.put("execution", &record) {
                state.bus.record("execution", &saved);
            }
        }
        Err(error) => {
            state.store.audit(
                "execution.finalization_failed",
                json!({"executionId": execution_id, "error": error}),
            );
            if let Some(saved) = state.store.put("execution", &record) {
                state.bus.record("execution", &saved);
            }
        }
    }

    // Link the attempt outcome to the root task. A room with many speakers,
    // or a goal with many iterations, still has one root outcome.
    if let Some(task_id) = record["taskId"].as_str() {
        crate::engine::episode::complete_attempt(
            state,
            task_id,
            execution_id,
            task_outcome,
        );
    }

    // Every terminal execution updates only this agent's growth record:
    // runs, failures, failing tools and concrete guidance for next time.
    // A deterministic shortcut is excluded: zero model calls is not growth.
    if record["deterministicShortcut"].is_null() {
        if let Some(agent_id) = record["agentId"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                record["sessionId"].as_str().and_then(|session_id| {
                    state
                        .store
                        .get("session", session_id)
                        .and_then(|session| session["agentId"].as_str().map(str::to_string))
                })
            })
        {
            let mut growth_record = record.clone();
            growth_record["agentId"] = json!(agent_id);
            let _ = crate::engine::agent_growth::record(state, &growth_record);
        }
    }
    // Close the principle loop before learning new ones: a used principle
    // gains a supporting case or counterexample for this run.
    let _ = crate::engine::principle::finalize_usage(
        state,
        &record,
        task_outcome == TaskOutcome::Succeeded,
    );
    // A success experience requires Succeeded and a first-time finalization.
    if applied && task_outcome == TaskOutcome::Succeeded {
        // Use the same first-success-per-root marker as the original case
        // bank: room speakers and repeat task attempts are not independent
        // evidence for conditional intervention rules.
        if record_success_experience(state, &record, execution_id, task_outcome).is_some() {
            let _ = crate::engine::experience::learn_verified_adaptations(state, &record);
            let _ = crate::engine::principle::learn_from_execution(state, &record);
        }
    }
    mark_run_signals(state, &record);
    // Finalize after learning so algorithm usages created while learning
    // (for example multiagent.sharing) are included in this execution.
    crate::engine::algorithms::finalize_execution(state, &record);
    if let Some(mut saved) = state.store.get("execution", execution_id) {
        saved["growthFinalized"] = json!(true);
        state.store.put("execution", &saved);
    }

}

