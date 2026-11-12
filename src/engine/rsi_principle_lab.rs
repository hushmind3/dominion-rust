//! Empirical improvement-principle laboratory owned by the rebuilt descendant.
//!
//! A model-authored principle must connect a task-intelligence source with the
//! source that changes its improvement method. Its hypothesis, edits, evaluated
//! task effects and NEXT descendant's work are distinct records. Nothing here
//! can accept a candidate or reveal hidden evaluator cases.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const TASK_ROLES: &[&str] = &["models", "learning", "memory", "tools",
    "harness", "algorithms"];

fn revision(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap_or_default()))
}
fn sha40(value: &Value) -> bool {
    value.as_str().is_some_and(|v| v.len() == 40
        && v.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn adopted(g: &Value) -> bool {
    g["accepted"] == true && g["pairedRevalidated"] == true
        && g["evaluation"]["metrics"]["eligible"] == true
        && g["candidateOrigin"] == "model_driven_detached_worktree"
        && sha40(&g["acceptedCommit"]) && sha40(&g["generatorCommit"])
        && g["systemDesign"]["source"] == "model_authored_system_design"
        && g["systemDesign"]["generatorCommit"] == g["generatorCommit"]
        && g["systemDesign"]["generationJobId"] == g["jobId"]
        && crate::engine::rsi_system::design_implemented(
            &g["systemDesign"],
            &g["acceptedSourceHashes"].as_object()
                .map(|m| m.keys().map(|p| json!(p)).collect::<Vec<_>>())
                .unwrap_or_default())
}
fn paths_for_role(design: &Value, role: &str) -> Vec<String> {
    if let Some(files) = design["componentFiles"][role].as_array() {
        return files.iter().filter_map(Value::as_str)
            .map(str::to_owned).collect();
    }
    if design["component"] == role {
        return design["targetFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(str::to_owned).collect();
    }
    Vec::new()
}
fn role_adopted(g: &Value, role: &str) -> bool {
    if !adopted(g) { return false; }
    let files = paths_for_role(&g["systemDesign"], role);
    !files.is_empty() && files.iter().all(|path|
        g["acceptedSourceHashes"][path.as_str()].as_str().is_some_and(|sha|
            sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit())))
}
fn task_measurement(g: &Value) -> (u64, u64, u64) {
    let mut wins = 0;
    let mut losses = 0;
    let mut cases = 0;
    for row in g["evaluation"]["metrics"]["measurements"]
        .as_array().into_iter().flatten() {
        // Hidden case identifiers and answers NEVER enter the successor's
        // training context. Only aggregate independent outcomes are retained.
        if row["holdout"] != true { continue; }
        let (Some(baseline), Some(candidate)) =
            (row["baselinePass"].as_bool(), row["candidatePass"].as_bool())
        else { continue; };
        cases += 1;
        if !baseline && candidate { wins += 1; }
        if baseline && !candidate { losses += 1; }
    }
    (wins, losses, cases)
}
fn immediate_children<'a>(history: &'a [Value], parent: &Value) -> Vec<&'a Value> {
    let Some(head) = parent["acceptedCommit"].as_str() else { return Vec::new(); };
    history.iter().filter(|g| g["generatorCommit"].as_str() == Some(head)
        && g["jobId"] != parent["jobId"]).collect()
}
fn principle(g: &Value) -> Option<&Value> {
    let p = &g["systemDesign"]["improvementPrinciple"];
    (p["source"] == "model_authored_improvement_principle_v1"
        && p["name"].as_str().is_some_and(|s| !s.is_empty())
        && p["taskRole"].as_str().is_some_and(|s| TASK_ROLES.contains(&s))
        && p["methodRole"] == "improvement_methods")
        .then_some(p)
}

/// Validate a *model-authored* general mechanism against actual planned
/// task and method source edits. An experiment principle cannot be an essay
/// attached to an unrelated one-file optimization.
pub(crate) fn validate_authored(
    authored: &Value, assignments: &serde_json::Map<String, Value>,
    target_files: &[Value],
) -> Result<Value, String> {
    if authored.is_null() { return Ok(Value::Null); }
    let field = |key: &str, min: usize, max: usize| -> Result<String, String> {
        let value = authored[key].as_str()
            .filter(|s| (min..=max).contains(&s.chars().count()))
            .ok_or_else(|| format!("개선 원리 {key}의 구체적인 설명이 필요합니다."))?;
        Ok(value.to_string())
    };
    let name = field("name", 4, 80)?;
    let failure_mode = field("failureMode", 25, 1400)?;
    let mechanism = field("mechanism", 30, 2400)?;
    let counterfactual = field("counterfactual", 25, 1400)?;
    let transfer_prediction = field("transferPrediction", 25, 1400)?;
    let learning_signal = field("learningSignal", 20, 800)?;
    let role = authored["taskRole"].as_str()
        .filter(|r| TASK_ROLES.contains(r))
        .ok_or("개선 원리는 실제 모델·기억·학습·도구·하네스·알고리즘을 대상으로 해야 합니다.")?;
    let task_path = authored["taskFile"].as_str()
        .filter(|path| crate::engine::rsi::safe_path(path))
        .ok_or("실제 지능 소스 경로가 필요합니다.")?;
    let method_path = authored["methodFile"].as_str()
        .filter(|path| crate::engine::rsi::safe_path(path))
        .ok_or("수정 가능한 개선 방법 소스 경로가 필요합니다.")?;
    if task_path == method_path
        || !target_files.iter().any(|v| v.as_str() == Some(task_path))
        || !target_files.iter().any(|v| v.as_str() == Some(method_path))
        || !assignments.get(role).and_then(Value::as_array).is_some_and(|files|
            files.iter().any(|v| v.as_str() == Some(task_path)))
        || !assignments.get("improvement_methods").and_then(Value::as_array).is_some_and(|files|
            files.iter().any(|v| v.as_str() == Some(method_path))) {
        return Err("원리 학습은 서로 다른 지능 소스와 개선기 소스를 같은 실제 결합 설계에서 수정해야 합니다.".into());
    }
    let identity=revision(&json!([name,role,mechanism]));
    Ok(json!({
        "source": "model_authored_improvement_principle_v1",
        "id": identity, "name": name, "taskRole": role,
        "methodRole": "improvement_methods",
        "taskFile": task_path, "methodFile": method_path,
        "failureMode": failure_mode, "mechanism": mechanism,
        "counterfactual": counterfactual,
        "transferPrediction": transfer_prediction,
        "learningSignal": learning_signal,
        "qualityProven": false, "transferProven": false
    }))
}

/// Interpret paired task outcomes and later descendant productivity as
/// separate empirical signals. The first is not evidence of the second.
/// The reconstructed archive can be revised by editing THIS Rust source.
pub(crate) fn plan(history: &[Value], frontier: &[Value], commit: &str) -> Value {
    let mut family = BTreeMap::<String, Vec<&Value>>::new();
    let mut attempts = BTreeMap::<String, u64>::new();
    let mut accepted_roles = BTreeMap::<String, u64>::new();
    for g in history {
        if g["systemDesign"]["source"] == "model_authored_system_design" {
            for role in TASK_ROLES {
                if !paths_for_role(&g["systemDesign"], role).is_empty() {
                    *attempts.entry((*role).to_string()).or_default() += 1;
                }
            }
        }
        if adopted(g) {
            for role in TASK_ROLES {
                if role_adopted(g, role) {
                    *accepted_roles.entry((*role).to_string()).or_default() += 1;
                }
            }
        }
        // Failed AND successful source-authored hypotheses enter the
        // empirical archive. Ignoring rejected theories would manufacture
        // a perfect prior and teach nothing about discovery failure.
        if let Some(p) = principle(g) {
            let family_id=p["id"].as_str().map(str::to_owned)
                .unwrap_or_else(||revision(&json!([
                    p["name"],p["taskRole"],p["mechanism"]])));
            family.entry(family_id).or_default().push(g);
        }
    }
    let mut principles = Vec::<Value>::new();
    for (identity, sources) in family {
        let mut task_wins = 0;
        let mut task_losses = 0;
        let mut holdout_cases = 0;
        let mut descendants = 0;
        let mut accepted_pairs = 0;
        let mut productive = 0;
        let mut cross_domain = 0;
        let mut consecutive = 0;
        let mut forecaster_error = 0.0f64;
        let mut forecast_count = 0u64;
        for g in &sources {
            // Only individually measured, paired results can supply a task
            // outcome. An unverified proposal has no observed task score.
            if g["pairedRevalidated"] != true
                || g["evaluation"]["metrics"]["eligible"] != true {
                continue;
            }
            let (w, l, c) = task_measurement(g);
            task_wins += w;
            task_losses += l;
            holdout_cases += c;
            if adopted(g) {
                accepted_pairs += 1;
            }
            if adopted(g) {
                // Multiple failed children may be tried against the SAME
                // accepted parent before one succeeds. Learn ALL observed
                // attempts, not only the first failure.
                for child in immediate_children(history, g) {
                    descendants += 1;
                    if adopted(child) {
                        productive += 1;
                        if child["systemDesign"]["nextDomain"] !=
                            g["systemDesign"]["nextDomain"] {
                            cross_domain += 1;
                        }
                        if child["systemDesign"]["improvementPrinciple"]["id"]
                            == g["systemDesign"]["improvementPrinciple"]["id"] {
                            consecutive += 1;
                        }
                    }
                }
            }
            if let Some(predicted) = g["systemDesign"]["expectedOutcomes"]["adoption"].as_f64() {
                let actual = if adopted(g) { 1.0 } else { 0.0 };
                forecaster_error += (predicted - actual).powi(2);
                forecast_count += 1;
            }
        }
        let assessed = sources.len();
        let next_rate = (productive as f64 + 1.0) / (descendants as f64 + 2.0);
        let task_rate = (task_wins as f64 + 1.0)
            / ((task_wins + task_losses) as f64 + 2.0);
        let p = &sources[sources.len() - 1]["systemDesign"]["improvementPrinciple"];
        principles.push(json!({
            "id": identity, "name": p["name"], "taskRole": p["taskRole"],
            "mechanism": p["mechanism"], "counterfactual": p["counterfactual"],
            "learningSignal": p["learningSignal"],
            "observedModelAuthoredTrials": assessed,
            "independentlyAdoptedSourcePairs": accepted_pairs,
            "holdoutCaseCount": holdout_cases,
            "aggregateHoldoutWins": task_wins,
            "aggregateHoldoutRegressions": task_losses,
            "observedImmediateDescendants": descendants,
            "subsequentAcceptedDescendants": productive,
            "crossDomainDescendantAdoptions": cross_domain,
            "samePrincipleDescendantAdoptions": consecutive,
            "posteriorTaskWinRate": task_rate,
            "posteriorNextGenerationProductivity": next_rate,
            "meanAdoptionForecastBrier": if forecast_count == 0 { Value::Null }
                else { json!(forecaster_error / forecast_count as f64) },
            "transferNotYetDemonstrated": descendants == 0 || productive == 0,
            "performanceClaim": false
        }));
    }
    principles.sort_by(|a, b| {
        let score = |v: &Value| {
            let w = v["aggregateHoldoutWins"].as_u64().unwrap_or(0) as f64;
            let l = v["aggregateHoldoutRegressions"].as_u64().unwrap_or(0) as f64;
            let d = v["observedImmediateDescendants"].as_u64().unwrap_or(0) as f64;
            let p = v["subsequentAcceptedDescendants"].as_u64().unwrap_or(0) as f64;
            (w - l) / (w + l + 2.0) + (p - (d - p)) / (d + 2.0)
        };
        score(b).partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });
    // Outcome-conditioned GENERALIZATION is part of the executable
    // successor policy, not merely an LLM instruction. A principle with
    // witnessed task wins and productive children is transferred to another
    // task role; a regressing principle is repaired on its original role.
    // These rules are themselves editable by the newly compiled child.
    let strongest=principles.first().cloned().unwrap_or(Value::Null);
    let source_role=strongest["taskRole"].as_str().unwrap_or("").to_string();
    let strong_wins=strongest["aggregateHoldoutWins"].as_u64().unwrap_or(0);
    let strong_losses=strongest["aggregateHoldoutRegressions"].as_u64().unwrap_or(0);
    let strong_children=strongest["subsequentAcceptedDescendants"].as_u64().unwrap_or(0);
    let learned_transfer=strong_wins>strong_losses && strong_children>0;
    let learned_repair=strong_losses>strong_wins;
    // Prefer a real trainable/task subsystem over another meta-controller
    // tweak when the empirical record does not contain usable task effects.
    // The preference is a LEARNABLE policy: the rebuilt child can edit this
    // module, change the criterion or invent an entirely different method.
    let mut task_candidates = frontier.iter().filter(|f|
        f["role"].as_str().is_some_and(|r| TASK_ROLES.contains(&r))
        && f["sourceSha256"].as_str().is_some_and(|s| s.len() == 64)
        && f["path"].as_str().is_some_and(crate::engine::rsi::safe_path))
        .collect::<Vec<_>>();
    task_candidates.sort_by(|a, b| {
        let score = |f: &Value| {
            let role = f["role"].as_str().unwrap_or("");
            let n = attempts.get(role).copied().unwrap_or(0) as f64;
            let wins = accepted_roles.get(role).copied().unwrap_or(0) as f64;
            let bytes = f["sourceBytes"].as_u64().unwrap_or(0) as f64;
            let coverage = if f["verifiedRoleImproved"] == true { 0.0 } else { 1.0 };
            let trainability_gap = match role {
                "models" => 0.30 / (1.0 + wins),
                "learning" => 0.16 / (1.0 + wins),
                _ => 0.0
            };
            let empirical_transfer = if learned_transfer && role!=source_role {
                0.42
            } else if learned_repair && role==source_role {
                0.42
            } else { 0.0 };
            coverage + trainability_gap + empirical_transfer
                + 0.35 / (1.0 + n)
                + 0.20 / (1.0 + wins)
                - 0.05 * (bytes / 25_000.0).min(1.0)
        };
        score(b).partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
    });
    let task = task_candidates.first().map(|f| (*f).clone())
        .unwrap_or(Value::Null);
    let method = frontier.iter().find(|f| f["role"] == "improvement_methods"
        && f["path"] == "src/engine/rsi_principle_lab.rs")
        .or_else(|| frontier.iter().find(|f|
            f["role"] == "improvement_methods" &&
            f["path"] == "src/engine/rsi_recursive_discovery.rs"))
        .or_else(|| frontier.iter().find(|f|
            f["role"] == "improvement_methods"))
        .cloned().unwrap_or(Value::Null);
    let measured = principles.iter().any(|p|
        p["holdoutCaseCount"].as_u64().unwrap_or(0) > 0);
    let fertile = principles.iter().any(|p|
        p["subsequentAcceptedDescendants"].as_u64().unwrap_or(0) > 0);
    let disproved = principles.iter().any(|p|
        p["aggregateHoldoutRegressions"].as_u64().unwrap_or(0)
            > p["aggregateHoldoutWins"].as_u64().unwrap_or(0)
        && p["holdoutCaseCount"].as_u64().unwrap_or(0) > 0);
    let mode = if disproved {
        "replace_failed_task_learning_principle_with_new_mechanism"
    } else if measured && !fertile {
        "rewrite_the_improvement_law_from_task_to_descendant_transfer_failure"
    } else if measured {
        "generalize_a_successful_principle_across_new_intelligence_roles"
    } else {
        "invent_and_test_coupled_task_meta_learning_principle"
    };
    // Portfolio-level self-observation: never let the new principle lab
    // permanently starve invention of DIFFERENT intelligence mechanisms.
    // After consecutive task↔method probes, the next rebuilt child can
    // resume the separate open-topology invention programme. This scheduling
    // law is editable in the same successor source as the learned forecasts.
    let consecutive_probes = history.iter().rev()
        .take_while(|g| g["principleActive"] == true).count();
    let should_probe = consecutive_probes < 2;
    let mut result = json!({
        "source": "compiled_descendant_empirical_principle_lab_v1",
        "consecutivePrincipleProbes": consecutive_probes,
        "shouldProbeTaskMethod": should_probe,
        "generatorCommit": commit, "historyCount": history.len(),
        "mode": mode, "target": task, "methodTarget": method,
        "learnedTransferPolicy": {
            "referencePrinciple": strongest["id"],
            "referenceRole": source_role,
            "observedTaskWins": strong_wins,
            "observedTaskRegressions": strong_losses,
            "productiveDescendants": strong_children,
            "action": if learned_transfer {
                "apply_learned_principle_to_different_intelligence_role"
            } else if learned_repair {
                "rewrite_failed_principle_on_original_task_role"
            } else { "invent_testable_task_and_improvement_method" },
            "selectedRole": task["role"],
            "sourceTargetRecomputedByCompiledDescendant": true,
            "transferAlreadyProven": false
        },
        "priorPrinciples": principles.into_iter().take(8).collect::<Vec<_>>(),
        "experimentContract": {
            "treatment": "change_actual_task_and_improvement_method_sources",
            "control": "source-identical_parent_before_edit",
            "taskOutcome": "independent_paired_task_holdout_aggregate",
            "recursiveOutcome": "next_rebuilt_descendant_independent_adoption",
            "generalization": "cross_domain_descendant_adoption",
            "methodDiscovery": "model_proposes_editable_source_bound_principle",
            "acceptedOnlyBy": "resident_frozen_independent_evaluator",
            "rejectTextOnlyImprovements": true
        },
        "modelWeightEvolution": {
            "targetRole": "models",
            "eligibleMethods": ["training_recipe_and_checkpoint_provenance",
                "adapter_training_and_inference_rebind",
                "routing_architecture_with_independent_task_receipt"],
            "mustConnectTrainingToRuntimeAndNextChild": true,
            "weightsUpdatedOrImproved": false
        },
        "canRewriteThisLearningLaw": true,
        "qualityProven": false, "transferProven": false
    });
    result["revision"] = json!(revision(&result));
    result
}

/// A task improvement must first survive independent holdout comparison,
/// rewrite the self-improvement method in real source, and then produce a
/// DIFFERENT task-intelligence improvement in an actual recompiled child.
/// This proves only a witnessed finite transfer, not unbounded future gains.
pub(crate) fn established_transfer(history: &[Value]) -> bool {
    history.iter().filter(|g| adopted(g)).any(|parent| {
        let Some(p) = principle(parent) else { return false; };
        let Some(source_role) = p["taskRole"].as_str() else { return false; };
        if !role_adopted(parent, source_role)
            || !role_adopted(parent, "improvement_methods") {
            return false;
        }
        let (wins, regressions, cases) = task_measurement(parent);
        if cases < 2 || wins <= regressions { return false; }
        immediate_children(history, parent).into_iter().any(|child| {
            if !adopted(child)
                || child["generatorBinarySha256"].as_str().is_none_or(str::is_empty)
                || child["generatorBinarySha256"] == parent["generatorBinarySha256"] {
                return false;
            }
            let cross_role = TASK_ROLES.iter().copied().any(|other|
                other != source_role && role_adopted(child, other));
            let (next_wins, next_losses, next_cases) = task_measurement(child);
            cross_role && next_cases >= 2 && next_wins > next_losses
        })
    })
}

/// Just-observed evidence; later descendant outcomes are computed from the
/// full archive in plan(), never fabricated at the time of adoption.
pub(crate) fn observation(g: &Value) -> Value {
    let p = &g["systemDesign"]["improvementPrinciple"];
    if p.is_null() {
        return json!({"source": "empirical_principle_observation_v1",
            "modelAuthoredPrinciple": false, "qualityProven": false});
    }
    let task_role = p["taskRole"].as_str().unwrap_or("");
    let (wins, losses, cases) = task_measurement(g);
    json!({
        "source": "empirical_principle_observation_v1",
        "id": p["id"], "name": p["name"],
        "modelAuthoredPrinciple": principle(g).is_some(),
        "taskRole": task_role,
        "taskAndMethodSourcesIndependentlyAdopted":
            role_adopted(g, task_role) && role_adopted(g, "improvement_methods"),
        "aggregatedHoldoutWins": wins, "aggregatedHoldoutLosses": losses,
        "measuredHoldoutCases": cases,
        "nextDescendantProductivity": Value::Null,
        "causalImprovementPrincipleProven": false
    })
}
pub(crate) fn instruction(programme: &Value) -> String {
    let target = &programme["target"];
    if !target.is_object() { return String::new(); }
    format!(
        "[REBUILT DESCENDANT EMPIRICAL PRINCIPLE LAB]\n\
        taskRole={} taskFile={} sourceSha256={}\n\
        methodRole=improvement_methods methodFile={} methodSha256={}\n\
        modelLearnedPriorPrinciples={}\n\
        Invent or replace an ACTUAL intelligence-learning mechanism and the \
        method that discovers such mechanisms. Call rsi_system_plan with \
        component=system, two DISJOINT componentFiles assignments, the task \
        source AND method source in targetFiles, and improvementPrinciple: \
        {{name,taskRole,taskFile,methodFile,failureMode,mechanism,counterfactual,\
        transferPrediction,learningSignal}}. Then REALLY edit BOTH files. \
        A model-learning proposal should connect trainable checkpoint/adapter \
        updates, versioned training data, and inference binding instead of \
        merely changing model routing prompts. Record a testable prediction \
        about task outcomes and the NEXT rebuilt descendant's ability to \
        invent another improvement. The frozen evaluator is independent; \
        do not read holdout identities or claim transfer from the proposal.\n\
        Current empirical mechanism programme: {}. Update THIS learning law \
        (src/engine/rsi_principle_lab.rs) if the observed task-to-descendant \
        link is weak. Reconstruct the principle from outcome evidence; do \
        not copy old rules as an unexplained ritual.",
        target["role"], target["path"], target["sourceSha256"],
        programme["methodTarget"]["path"],
        programme["methodTarget"]["sourceSha256"],
        programme["priorPrinciples"], programme["mode"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_unverified_gain_or_hidden_test_id_in_training_archive() {
        let g = json!({"accepted": false,
            "systemDesign": {"source":"model_authored_system_design",
                "improvementPrinciple": {"source":
                    "model_authored_improvement_principle_v1","name":"guess",
                    "taskRole":"models","methodRole":"improvement_methods"}},
            "evaluation":{"metrics":{"measurements":[{"id":"secret",
                "holdout":true,"baselinePass":false,"candidatePass":true}]}}});
        let p = plan(&[g], &[], "0123456789abcdef0123456789abcdef01234567");
        assert!(p["priorPrinciples"].as_array().unwrap().is_empty());
        assert!(!p.to_string().contains("secret"));
    }
    #[test]
    fn finite_transfer_cannot_be_faked_by_an_unverified_proposal() {
        let g=json!({"accepted":true,"pairedRevalidated":false,
            "systemDesign":{"improvementPrinciple":{"name":"proposed"}},
            "evaluation":{"metrics":{"eligible":true}}});
        assert!(!established_transfer(&[g]));
    }
    #[test]
    fn task_meta_probe_cannot_starve_open_invention_forever() {
        let history=vec![
            json!({"principleActive":true,"accepted":false}),
            json!({"principleActive":true,"accepted":false})
        ];
        let p=plan(&history,&[], "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(p["shouldProbeTaskMethod"],false);
        assert_eq!(p["consecutivePrincipleProbes"],2);
    }
    #[test]
    fn coupling_requires_two_distinct_witnessed_sources() {
        let mut assignments = serde_json::Map::new();
        assignments.insert("models".into(), json!(["src/engine/models.rs"]));
        assignments.insert("improvement_methods".into(),
            json!(["src/engine/rsi_principle_lab.rs"]));
        let proposal = json!({
            "name":"learned_analysis","taskRole":"models",
            "taskFile":"src/engine/models.rs",
            "methodFile":"src/engine/rsi_principle_lab.rs",
            "failureMode":"The existing learner is fixed before future tasks arrive.",
            "mechanism":"Let the next binary change its training and discovery law from outcomes.",
            "counterfactual":"A fixed training loop is expected to improve fewer tasks than the learned one.",
            "transferPrediction":"A new child should adopt a change in an independent domain.",
            "learningSignal":"Compare aggregate paired task gains and later child adoptions."
        });
        assert!(validate_authored(&proposal, &assignments,
            &[json!("src/engine/models.rs"),
              json!("src/engine/rsi_principle_lab.rs")]).is_ok());
        assert!(validate_authored(&proposal, &assignments,
            &[json!("src/engine/models.rs")]).is_err());
    }
}
