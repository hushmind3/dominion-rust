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
) -> (String, String) {
    let system = "You are the DOMINION adaptation planner. Given one failed action, propose distinct causal hypotheses and DIFFERENT interventions. Output JSON only: {\"hypotheses\":[{\"id\":\"h1\",\"claim\":\"...\",\"predictions\":[{\"predicate\":{\"type\":\"equals\",\"from\":\"result\",\"path\":\"/exitCode\",\"value\":0}}]}],\"interventions\":[{\"id\":\"i1\",\"hypothesisIds\":[\"h1\"],\"tool\":\"workspace_write\",\"inputs\":{...},\"expectedChecks\":[\"file_exists\"],\"estimatedCost\":0.2}]}. A hypothesis claims why the action failed; each intervention must change the tool, arguments, or approach.";
    let prompt = format!(
        "Failed action: tool={tool}\nArguments: {}\nObserved error: {}\nEnvironment version: {environment_version}\nAvailable tools include workspace_read, workspace_write, workspace_list, shell_exec, terminal_exec.\nReturn JSON only.",
        arguments, error
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
    }
    Ok(())
}

fn intervention_key(tool: &str, inputs: &Value) -> String {
    format!("{tool}:{}", inputs)
}

/// Picks the highest-scoring distinct intervention. A repeat of the exact
/// failed call is only allowed when the environment changed.
pub fn choose_intervention(
    adaptation: &Value,
    failed_tool: &str,
    failed_arguments: &Value,
    environment_changed: bool,
) -> Option<Value> {
    let failed_key = intervention_key(failed_tool, failed_arguments);
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
            let progress = item["expectedChecks"].as_array().map(Vec::len).unwrap_or(0) as f64;
            let distinction = adaptation["hypotheses"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter(|hypothesis| {
                            item["hypothesisIds"]
                                .as_array()
                                .is_some_and(|ids| ids.contains(&hypothesis["id"]))
                        })
                        .count()
                })
                .unwrap_or(0) as f64;
            let cost = item["estimatedCost"].as_f64().unwrap_or(0.5).max(0.0);
            let score = progress + distinction - cost;
            Some((score, item.clone()))
        })
        .max_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(score, mut item)| {
            item["score"] = json!(score);
            item
        })
}

/// Compares each hypothesis prediction with the actual result.
pub fn evaluate_hypotheses(adaptation: &Value, result: &Value) -> Vec<Value> {
    let resolved = json!({"result": result});
    adaptation["hypotheses"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hypothesis| {
            let predictions = hypothesis["predictions"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut unknown = 0;
            let mut refuted = false;
            for prediction in &predictions {
                match crate::engine::program::predicate_holds(
                    &prediction["predicate"],
                    &resolved,
                ) {
                    Ok(true) => {}
                    Ok(false) => refuted = true,
                    Err(_) => unknown += 1,
                }
            }
            let status = if refuted {
                "refuted"
            } else if unknown > 0 || predictions.is_empty() {
                "unknown"
            } else {
                "supported"
            };
            json!({"id": hypothesis["id"], "status": status})
        })
        .collect()
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
