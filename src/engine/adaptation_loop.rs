//! Section 11-1 of design 2.4: observation -> adaptation inside the running
//! loop. A failing tool call is recorded as an observation with a hypothesis
//! and a changed-action intervention. The runtime also blocks an unchanged
//! repeat of that failed call so the model has to pick a different action.
use crate::state::AppState;
use serde_json::{json, Value};

/// Records a tool failure and the adaptation it implies. Bounded so a long run
/// cannot grow the execution record without limit.
pub fn record_tool_failure(
    state: &AppState,
    execution_id: &str,
    round: usize,
    call_id: &str,
    tool: &str,
    arguments: &Value,
    error: &str,
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let digest = crate::engine::memory::stable_id(&[tool, &arguments.to_string()]);
    let adaptation = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, call_id, &digest])
        ),
        "runId": execution_id,
        "stepId": round,
        "operationId": call_id,
        "observation": {
            "kind": "tool_failure",
            "tool": tool,
            "argsDigest": digest,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "hypothesis": {
            "claim": "the same call will repeat the same failure",
            "evidenceIds": [call_id],
        },
        "intervention": {
            "change": "different tool, arguments, or approach",
            "instruction": "do not repeat the identical failed call; choose a changed action or explain the blocker",
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(adaptation.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = adaptation.clone();
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(adaptation)
}

pub fn repeated_failure_message(tool: &str) -> String {
    format!(
        "변경 없는 `{tool}` 호출이 이미 실패했습니다. 같은 호출을 다시 실행하지 않았습니다. \
         다른 도구, 다른 인자, 또는 원인을 확인하는 접근을 선택하세요."
    )
}

/// Builds the model prompt for hypothesis/intervention generation.
pub fn adaptation_prompt(
    tool: &str,
    arguments: &Value,
    error: &str,
    environment_version: u64,
    before_observation: &Value,
    environment_fingerprint: &str,
) -> (String, String) {
    let system = "You are the DOMINION adaptation planner. Given one failed action, propose DISTINCT causal hypotheses and DIFFERENT interventions. Output JSON only: {\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"...\",\"evidenceIds\":[\"before\"],\"preconditions\":[],\"predictions\":[{\"observationTarget\":\"before\",\"observationPhase\":\"before_intervention\",\"predicate\":{\"type\":\"equals\",\"from\":\"before\",\"path\":\"/pathState/path/exists\",\"value\":false},\"expectedValue\":false}]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"preconditions\":[],\"tool\":\"workspace_write\",\"inputs\":{...},\"expectedChecks\":[\"file_exists\"],\"expectedProgress\":0.7,\"uncertainty\":0.2,\"estimatedCost\":0.2}]}. Predictions about why the failure happened must use observationPhase=before_intervention and target the before observation; predictions about the result of an intervention use after_intervention. Missing fields are Unknown, never false. Each intervention must change the tool, arguments, or approach.";
    let prompt = format!(
        "Failed action: tool={tool}\nArguments: {}\nObserved error: {}\nEnvironment version: {environment_version}\nEnvironment fingerprint: {environment_fingerprint}\nBefore observation (authoritative): {}\nAvailable tools include workspace_read, workspace_write, workspace_list, shell_exec, terminal_exec.\nReturn JSON only.",
        arguments, error, before_observation
    );
    (system.to_string(), prompt)
}

/// Extracts the first JSON object that actually contains hypotheses and
/// interventions. Local thinking models often wrap JSON in prose or markdown,
/// so parsing the outermost braces is not reliable.
pub fn parse_adaptation(raw: &str) -> Option<Value> {
    let bytes = raw.as_bytes();
    for (start, _) in raw.match_indices('{') {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for index in start..bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escape {
                    escape = false;
                } else if byte == b'\\' {
                    escape = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(value) = serde_json::from_str::<Value>(&raw[start..=index]) {
                            if value["hypotheses"].is_array()
                                && value["interventions"].is_array()
                            {
                                return Some(value);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Structural validation used before execution. Field errors are returned to
/// the caller so the model can repair its output instead of being trusted.
pub fn validate_adaptation(adaptation: &Value) -> Result<(), String> {
    let hypotheses = adaptation["hypotheses"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("hypotheses must be a non-empty array")?;
    for hypothesis in hypotheses {
        if hypothesis["id"].as_str().is_none_or(str::is_empty) {
            return Err("each hypothesis needs an id".into());
        }
        if hypothesis["claim"].as_str().is_none_or(str::is_empty) {
            return Err(format!(
                "hypothesis {} needs a claim",
                hypothesis["id"]
            ));
        }
        if !hypothesis["predictions"].is_array() {
            return Err(format!(
                "hypothesis {} predictions must be an array",
                hypothesis["id"]
            ));
        }
        if !hypothesis["preconditions"].is_null()
            && !hypothesis["preconditions"].is_array()
        {
            return Err(format!(
                "hypothesis {} preconditions must be an array",
                hypothesis["id"]
            ));
        }
        for prediction in hypothesis["predictions"].as_array().into_iter().flatten() {
            if let Some(phase) = prediction["observationPhase"].as_str() {
                if !matches!(phase, "before_intervention" | "after_intervention") {
                    return Err(format!(
                        "hypothesis {} has invalid observationPhase {phase}",
                        hypothesis["id"]
                    ));
                }
            }
            if !prediction["predicate"].is_object() {
                return Err(format!(
                    "hypothesis {} prediction needs a predicate object",
                    hypothesis["id"]
                ));
            }
        }
    }
    let interventions = adaptation["interventions"]
        .as_array()
        .filter(|items| !items.is_empty())
        .ok_or("interventions must be a non-empty array")?;
    for intervention in interventions {
        if intervention["id"].as_str().is_none_or(str::is_empty) {
            return Err("each intervention needs an id".into());
        }
        if intervention["tool"].as_str().is_none_or(str::is_empty) {
            return Err(format!(
                "intervention {} needs a tool",
                intervention["id"]
            ));
        }
        if !intervention["inputs"].is_object() {
            return Err(format!(
                "intervention {} inputs must be an object",
                intervention["id"]
            ));
        }
        if !intervention["hypothesisIds"].is_array() {
            return Err(format!(
                "intervention {} hypothesisIds must be an array",
                intervention["id"]
            ));
        }
        if !intervention["preconditions"].is_null()
            && !intervention["preconditions"].is_array()
        {
            return Err(format!(
                "intervention {} preconditions must be an array",
                intervention["id"]
            ));
        }
    }
    Ok(())
}

fn intervention_key(tool: &str, inputs: &Value) -> String {
    format!("{tool}:{}", inputs)
}

/// Picks an intervention using observable preconditions, expected progress,
/// hypothesis discrimination, historical effect, uncertainty and cost.
pub fn choose_intervention_with_context(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
    resolved: &Value,
    refuted_hypotheses: &[String],
) -> Option<Value> {
    choose_intervention_with_policy(
        adaptation,
        failed_tool,
        failed_arguments,
        environment_changed,
        resolved,
        refuted_hypotheses,
        &json!({}),
    )
}

/// Same selection, with weights supplied by the algorithm space. The harness
/// owns execution; the selected algorithm owns only this scoring choice.
#[allow(clippy::too_many_arguments)]
pub fn choose_intervention_with_policy(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
    resolved: &Value,
    refuted_hypotheses: &[String],
    algorithm: &Value,
) -> Option<Value> {
    use crate::engine::program::{predicate_verdict, PredicateVerdict};

    let weight = |name: &str, fallback: f64| {
        algorithm["parameters"][name].as_f64().unwrap_or(fallback)
    };
    let discrimination_weight = weight("discriminationWeight", 1.0);
    let progress_weight = weight("progressWeight", 1.0);
    let cost_weight = weight("costWeight", 1.0);
    let uncertainty_weight = weight("uncertaintyWeight", 0.5);
    let failed_key = intervention_key(failed_tool, failed_arguments);
    let hypotheses = adaptation["hypotheses"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let live_hypotheses = hypotheses
        .iter()
        .filter(|hypothesis| {
            !hypothesis["id"]
                .as_str()
                .is_some_and(|id| refuted_hypotheses.iter().any(|item| item == id))
        })
        .count();
    let cause_unclear = live_hypotheses >= 2;

    adaptation["interventions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let tool = item["tool"].as_str()?;
            let inputs = &item["inputs"];
            let key = intervention_key(tool, inputs);
            if key == failed_key && !environment_changed {
                return None;
            }
            let hypothesis_ids = item["hypothesisIds"].as_array().cloned().unwrap_or_default();
            let only_refuted = !hypothesis_ids.is_empty()
                && hypothesis_ids.iter().all(|id| {
                    id.as_str()
                        .is_some_and(|id| refuted_hypotheses.iter().any(|item| item == id))
                });
            if only_refuted {
                return None;
            }
            let mut precondition_status = "true";
            let mut preconditions = Vec::new();
            for precondition in item["preconditions"].as_array().into_iter().flatten() {
                let verdict = predicate_verdict(precondition, resolved).ok()?;
                preconditions.push(json!({
                    "predicate": precondition,
                    "verdict": verdict.as_str(),
                }));
                match verdict {
                    PredicateVerdict::False => {
                        precondition_status = "false";
                    }
                    PredicateVerdict::Unknown => {
                        if precondition_status != "false" {
                            precondition_status = "unknown";
                        }
                    }
                    PredicateVerdict::True => {}
                }
            }
            if precondition_status == "false" {
                return None;
            }
            let expected_checks = item["expectedChecks"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0) as f64;
            let unresolved_requirements = item["unresolvedRequirements"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0) as f64;
            let expected_progress = item["expectedProgress"]
                .as_f64()
                .unwrap_or(expected_checks / 3.0 + unresolved_requirements);
            let discrimination = hypothesis_ids
                .iter()
                .filter(|id| {
                    !id.as_str()
                        .is_some_and(|id| refuted_hypotheses.iter().any(|item| item == id))
                })
                .count() as f64;
            let historical_effect = item["historicalEffect"].as_f64();
            let uncertainty = item["uncertainty"].as_f64().unwrap_or(0.5);
            let cost = item["estimatedCost"].as_f64().unwrap_or(0.5).max(0.0);
            let mut score = if cause_unclear {
                discrimination * 2.0 * discrimination_weight
                    + expected_progress * 0.5 * progress_weight
            } else {
                expected_progress * 2.0 * progress_weight
                    + discrimination * 0.5 * discrimination_weight
            };
            score += historical_effect.unwrap_or(0.0);
            score -= cost * cost_weight + uncertainty * uncertainty_weight;
            if precondition_status == "unknown" {
                score -= 0.5;
            }
            let evaluation = json!({
                "unresolvedRequirements": unresolved_requirements,
                "expectedProgress": expected_progress,
                "hypothesisDiscrimination": discrimination,
                "historicalEffect": historical_effect,
                "uncertainty": uncertainty,
                "estimatedCost": cost,
                "preconditionStatus": precondition_status,
                "preconditions": preconditions,
                "causeUnclear": cause_unclear,
                "algorithmRevision": algorithm["revision"],
                "score": score,
            });
            let mut item = item.clone();
            item["score"] = json!(score);
            item["evaluation"] = evaluation;
            Some((score, item))
        })
        .max_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, item)| item)
}

/// Backwards-compatible wrapper for callers without an observation context.
#[allow(dead_code)]
pub fn choose_intervention(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
) -> Option<Value> {
    choose_intervention_with_context(
        adaptation,
        failed_tool,
        failed_arguments,
        environment_changed,
        &json!({}),
        &[],
    )
}

/// Compares hypotheses against the correct observation phase. A prediction
/// about the state before an intervention is never refuted by the state after
/// it. Missing fields remain Unknown.
pub fn evaluate_hypotheses_phased(
    adaptation: &Value,
    before: &Value,
    after: &Value,
) -> Vec<Value> {
    use crate::engine::program::predicate_verdict;

    adaptation["hypotheses"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hypothesis| {
            let predictions = hypothesis["predictions"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut supported = 0;
            let mut unknown = 0;
            let mut refuted = false;
            let mut verdicts = Vec::new();
            for prediction in &predictions {
                let phase = prediction["observationPhase"]
                    .as_str()
                    .unwrap_or("after_intervention");
                let target = prediction["observationTarget"]
                    .as_str()
                    .unwrap_or("result");
                let observation = if phase == "before_intervention" {
                    before
                } else {
                    after
                };
                let mut scope = json!({"before": before, "after": after, "result": observation});
                scope[target] = observation.clone();
                let verdict = match predicate_verdict(&prediction["predicate"], &scope) {
                    Ok(verdict) => verdict,
                    Err(error) => {
                        return json!({
                            "id": hypothesis["id"],
                            "status": "unknown",
                            "error": error,
                            "predictions": [],
                        });
                    }
                };
                match verdict {
                    crate::engine::program::PredicateVerdict::True => supported += 1,
                    crate::engine::program::PredicateVerdict::False => refuted = true,
                    crate::engine::program::PredicateVerdict::Unknown => unknown += 1,
                }
                verdicts.push(json!({
                    "observationPhase": phase,
                    "observationTarget": target,
                    "verdict": verdict.as_str(),
                }));
            }
            let status = if refuted {
                "refuted"
            } else if predictions.is_empty() || unknown > 0 {
                "unknown"
            } else if supported > 0 {
                "supported"
            } else {
                "unknown"
            };
            json!({
                "id": hypothesis["id"],
                "status": status,
                "supported": supported,
                "unknown": unknown,
                "predictions": verdicts,
            })
        })
        .collect()
}

/// Legacy wrapper: uses the same value for before/after so old callers keep
/// working, but missing fields are still Unknown instead of refuted.
#[allow(dead_code)]
pub fn evaluate_hypotheses(adaptation: &Value, result: &Value) -> Vec<Value> {
    evaluate_hypotheses_phased(adaptation, result, result)
}

#[allow(clippy::too_many_arguments)]
pub fn record_adaptation(
    state: &AppState,
    execution_id: &str,
    round: usize,
    failed_tool: &str,
    failed_arguments: &Value,
    error: &str,
    adaptation: &Value,
    selected: &Value,
    environment_version: u64,
    result: Option<&Value>,
    hypothesis_results: &[Value],
) -> Option<Value> {
    let mut execution = state.store.get("execution", execution_id)?;
    let failure_signature = crate::engine::memory::stable_id(&[
        failed_tool,
        &failed_arguments.to_string(),
    ]);
    let record = json!({
        "id": format!(
            "adaptation:{}",
            crate::engine::memory::stable_id(&[execution_id, &failure_signature, &selected["id"].as_str().unwrap_or("")])
        ),
        "runId": execution_id,
        "stepId": round,
        "failureSignature": failure_signature,
        "environmentVersion": environment_version,
        "observation": {
            "kind": "tool_failure",
            "tool": failed_tool,
            "arguments": failed_arguments,
            "expected": "success",
            "actual": error.chars().take(600).collect::<String>(),
        },
        "beforeObservation": adaptation["beforeObservation"],
        "hypotheses": adaptation["hypotheses"],
        "interventions": adaptation["interventions"],
        "consultedExperienceRules": adaptation["consultedExperienceRules"],
        "selected": selected,
        "result": result,
        "hypothesisResults": hypothesis_results,
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });
    let mut adaptations = execution["adaptations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    adaptations.push(record.clone());
    if adaptations.len() > 20 {
        adaptations = adaptations.split_off(adaptations.len() - 20);
    }
    execution["adaptations"] = json!(adaptations);
    execution["lastAdaptation"] = record.clone();
    execution["adaptationCheckpoint"] = json!({
        "environmentVersion": environment_version,
        "failureSignature": record["failureSignature"],
        "selectedIntervention": selected["id"],
        "selectedEvaluation": selected["evaluation"],
        "environmentFingerprint": adaptation["environmentFingerprint"],
        "adaptationRevision": execution["adaptationCheckpoint"]["adaptationRevision"]
            .as_u64().unwrap_or(0) + 1,
        "beforeObservation": adaptation["beforeObservation"],
        "hypothesisResults": hypothesis_results,
    });
    let saved = state.store.put("execution", &execution)?;
    state.bus.record("execution", &saved);
    Some(record)
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn adaptation() -> Value {
        json!({
            "hypotheses":[
                {"id":"h1","claim":"permission","predictions":[{"predicate":{"type":"equals","from":"result","path":"/exitCode","value":0}}]},
                {"id":"h2","claim":"wrong path","predictions":[{"predicate":{"type":"exists","from":"result","path":"/missing"}}]}
            ],
            "interventions":[
                {"id":"same","hypothesisIds":["h1"],"tool":"shell_exec","inputs":{"command":"exit 7"},"expectedChecks":["x"],"estimatedCost":0.1},
                {"id":"change","hypothesisIds":["h1","h2"],"tool":"workspace_write","inputs":{"path":"a.txt","content":"ok"},"expectedChecks":["a","b"],"estimatedCost":0.2}
            ]
        })
    }

    #[test]
    fn identical_failed_call_is_filtered_until_the_environment_changes() {
        let failed = json!({"command":"exit 7"});
        let chosen = choose_intervention(&adaptation(), "shell_exec", &failed, false).unwrap();
        assert_eq!(chosen["id"], "change");
        let retry = choose_intervention(&adaptation(), "shell_exec", &failed, true).unwrap();
        assert_eq!(retry["id"], "change"); // still higher score than identical retry
        let only_same = json!({"hypotheses":[],"interventions":[adaptation()["interventions"][0].clone()]});
        assert!(choose_intervention(&only_same, "shell_exec", &failed, false).is_none());
        assert!(choose_intervention(&only_same, "shell_exec", &failed, true).is_some());
    }

    #[test]
    fn noisy_model_output_is_parsed_and_invalid_schema_is_rejected() {
        let raw = "Here is the plan {not json} then:\n```json\n{\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"missing file\",\"predictions\":[]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"tool\":\"workspace_list\",\"inputs\":{},\"expectedChecks\":[\"entries\"],\"estimatedCost\":0.1}]}\n```\nDone.";
        let parsed = parse_adaptation(raw).expect("prose-wrapped JSON");
        assert_eq!(parsed["interventions"][0]["tool"], "workspace_list");
        assert!(validate_adaptation(&parsed).is_ok());
        let invalid = json!({
            "hypotheses":[{"id":"h1","claim":"x","predictions":[]}],
            "interventions":[{"id":"i1","hypothesisIds":["h1"],"tool":"workspace_list","inputs":"not-an-object"}]
        });
        assert!(validate_adaptation(&invalid).is_err());
    }

    #[test]
    fn predictions_refute_the_wrong_hypothesis() {
        let results = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 7}));
        assert_eq!(results[0]["status"], "refuted");
        assert_eq!(results[1]["status"], "refuted");
        let success = evaluate_hypotheses(&adaptation(), &json!({"exitCode": 0, "missing": 1}));
        assert_eq!(success[0]["status"], "supported");
        assert_eq!(success[1]["status"], "supported");
    }
}
