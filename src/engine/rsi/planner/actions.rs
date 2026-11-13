//! actions: source-grounded compiled descendant planning responsibility.
use super::*;

pub(crate) fn actual_edit_observation(tools:&[Value], candidate_created:bool)->Value {
    let calls=tools.iter()
        .filter(|row|matches!(row["tool"].as_str(),Some("workspace_replace"|"workspace_replace_function"|"workspace_write")))
        .map(|row|row["calls"].as_u64().unwrap_or(0)).sum::<u64>();
    json!({"status":if calls>0 {"edit_attempted"} else {"no_edit"},
        "editToolCalls":calls,"candidateCreated":candidate_created})
}

/// Distinguish tool calls, verified writes, actual Git diff, and candidates.
/// Failed edits are absent from session_tool_usage (which counts completed
/// calls only); reading those counts alone caused a failed source write to
/// appear indistinguishable from a research-only generation.
pub(crate) fn observed_edit_outcome(
    tools:&[Value], actions:&Value, candidate_created:bool, diff_bytes:u64,
)->Value {
    let mut result=actual_edit_observation(tools,candidate_created);
    let Some(rows)=actions.as_array().filter(|rows|!rows.is_empty()) else {
        result["sourceDiffBytes"]=json!(diff_bytes);
        return result;
    };
    let applied=rows.iter().filter(|row|row["written"]==true).count() as u64;
    let failed=rows.len() as u64-applied;
    let last_error=rows.iter().rev().filter_map(|row|row["error"].as_str())
        .find(|value|!value.trim().is_empty()).unwrap_or("");
    result["editToolCalls"]=json!(rows.len());
    result["appliedEditCalls"]=json!(applied);
    result["failedEditCalls"]=json!(failed);
    result["sourceDiffBytes"]=json!(diff_bytes);
    result["lastError"]=json!(last_error.chars().take(500).collect::<String>());
    result["status"]=json!(if candidate_created {"candidate_created"}
        else if applied>0 && diff_bytes>0 {"source_changed_unverified"}
        else if applied>0 {"write_without_diff"}
        else {"edit_failed"});
    result
}

/// Classify the system failure that existed *before* an action was chosen.
/// This is a feature of the DOMINION execution trace, not an assessment of
/// the language model's raw coding quality.
pub(crate) fn action_learning_context(row:&Value)->&'static str {
    let verdict=row["evaluation"]["verdict"].as_str().unwrap_or("");
    let observed=&row["actionObservation"];
    if row["accepted"]==true {return "adopted";}
    if matches!(verdict,"regressed"|"build_failed") {return "failed_verifier";}
    if row["candidateId"].is_string() {return "unadopted_candidate";}
    if observed["status"]=="edit_failed" || observed["status"]=="write_without_diff" {
        return "failed_edit";
    }
    if observed["sourceDiffBytes"].as_u64().unwrap_or(0)>0
        || observed["editToolCalls"].as_u64().unwrap_or(0)>0 {
        return "uncaptured_edit";
    }
    if row["selfRepair"]["outcome"]=="no_patch_after_recovery" {
        return "exhausted_recovery";
    }
    if row["newFindings"].as_u64().unwrap_or(0)>0 {return "research_only";}
    "barren"
}

/// Context-sensitive *executable* recovery strategies. The learner below
/// changes their ranking using observed consequences, rather than mapping
/// every error to a fixed next action forever.
pub(crate) fn action_choices(context:&str)->&'static [&'static str] {
    match context {
        "adopted"=>&["extend_verified_capability"],
        "failed_verifier"=>&["repair_failed_case","revise_candidate","change_approach"],
        "unadopted_candidate"=>&["revise_candidate","change_approach","complete_candidate"],
        "failed_edit"=>&["repair_failed_edit","change_approach","implement_research"],
        "uncaptured_edit"=>&["complete_candidate","repair_failed_edit","change_approach"],
        "research_only"=>&["implement_research","change_approach","complete_candidate"],
        "exhausted_recovery"=>&["change_approach","repair_failed_edit","implement_research"],
        _=>&["change_approach","implement_research","repair_failed_edit"],
    }
}

/// A credit signal for the *DOMINION method* used in this generation.
/// Merely issuing a write call earns nothing. Adoption receives the strongest
/// reward, a verified candidate or Git patch receives only partial credit,
/// regressions lose credit, and excessive model cost is penalized.
pub(crate) fn observed_action_value(row:&Value)->f64 {
    let mut value=if row["accepted"]==true {5.0}
        else if row["candidateId"].is_string() {1.5}
        else if row["actionObservation"]["sourceDiffBytes"].as_u64().unwrap_or(0)>0 {0.7}
        else if row["newFindings"].as_u64().unwrap_or(0)>0 {0.15}
        else {-1.0};
    if matches!(row["evaluation"]["verdict"].as_str(),
        Some("regressed"|"build_failed")) {
        value-=2.5;
    }
    if matches!(row["stopReason"].as_str(),Some("time_budget"|"token_budget")) {
        value-=0.5;
    }
    // A cost term prevents the learner from celebrating a 100k-token
    // research loop that did not produce an actual implementation.
    value-(row["tokens"].as_u64().unwrap_or(0) as f64/60_000.0).min(1.0)
}

/// Rebuilds a scoped training timeline across past campaigns and the current
/// live campaign. Only matching repository/owner partitions and the exact
/// objective participate. Do not accidentally learn from other owners, goals
/// or an earlier generation twice.
pub(super) fn action_learning_history(
    state:&AppState,input:&Value,current:&[Value],
)->Vec<Value> {
    let partition=research_scope(input).partition_key;
    let objective_id=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")
    ]);
    let mut episodes=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["knowledgePartition"]==partition
            && row["objectiveId"]==objective_id)
        .collect::<Vec<_>>();
    episodes.extend(current.iter().cloned());
    episodes.sort_by(|a,b|{
        a["createdAt"].as_str().cmp(&b["createdAt"].as_str())
            .then_with(||a["attempt"].as_u64().cmp(&b["attempt"].as_u64()))
            .then_with(||a["id"].as_str().cmp(&b["id"].as_str()))
    });
    let mut seen=BTreeSet::<String>::new();
    episodes.retain(|row|{
        let key=row["jobId"].as_str().or_else(||row["id"].as_str())
            .unwrap_or("");
        !key.is_empty() && seen.insert(key.to_string())
    });
    // Preserve full campaigns when possible. 80 recent generations put an
    // upper bound on recomputation and on the inherited policy's footprint.
    episodes.into_iter().rev().take(80).collect::<Vec<_>>()
        .into_iter().rev().collect()
}

/// Learns a contextual action-selection policy from verified outcomes.
/// Every training example links a previous context, the action subsequently
/// executed, and that action's observed result. The solver creates a new
/// score function (posterior utility + bounded exploration) from actual
/// evidence; it can replace a repeatedly unsuccessful default with a new
/// recovery route. No replay result alone is counted as quality improvement.
/// The next generation is its real trial, with the policy revision recorded.
pub(crate) fn evolved_execution_action(state:&AppState,input:&Value,history:&[Value])->Value {
    let mut decision=next_execution_action(history);
    let Some(last)=history.last() else {return decision};
    let context=action_learning_context(last);
    let alternatives=action_choices(context);
    if alternatives.len()<=1 {return decision}
    let baseline=decision["mode"].as_str().unwrap_or("change_approach").to_string();
    let episodes=action_learning_history(state,input,history);
    let mut observations=BTreeMap::<String,Vec<(f64,String)>>::new();
    let mut count=0usize;
    let mut retained_scores=Vec::<f64>::new();
    let mut mutation_scores=Vec::<f64>::new();
    for pair in episodes.windows(2) {
        let prior=&pair[0];
        let next=&pair[1];
        // A transition across unrelated campaigns or missing attempts is
        // not the consequence of the preceding source-recovery decision.
        if prior["campaignId"]!=next["campaignId"]
            || prior["attempt"].as_u64().and_then(|n|n.checked_add(1))
                !=next["attempt"].as_u64() {
            continue;
        }
        if action_learning_context(prior)!=context {continue}
        let Some(action)=next["actionDirective"]["mode"].as_str() else {continue};
        if !alternatives.contains(&action) {continue}
        // Once learned policies exist, an intended action does not earn
        // learning credit unless it actually altered the native runtime's
        // source target. Old generations without this evidence still supply
        // baseline statistics, not claimed policy improvement.
        if next["actionDirective"]["policy"]["kind"]=="selfimprove.action"
            && next["actionExecution"]["focusApplied"]!=true {
            continue;
        }
        let id=next["id"].as_str().unwrap_or("").to_string();
        let outcome=observed_action_value(next);
        observations.entry(action.to_string()).or_default()
            .push((outcome,id));
        if next["actionDirective"]["policy"]["status"]=="exploratory_policy_mutation" {
            mutation_scores.push(outcome);
        } else if next["actionDirective"]["policy"]["status"]=="evidence_based_retention" {
            retained_scores.push(outcome);
        }
        count+=1;
    }
    if count<2 {
        decision["policy"]=json!({"kind":"selfimprove.action","status":"insufficient_evidence",
            "context":context,"trainingTransitions":count,"activeAction":baseline});
        return decision;
    }
    // Meta-adaptation: the exploration coefficient is itself revised from
    // *later* results of previously mutated policies versus retained ones
    // in the same observed failure context. It is deliberately a bounded,
    // observational update, never a claim of causal superiority.
    let mean=|values:&[f64]|values.iter().sum::<f64>()/values.len().max(1) as f64;
    let exploration_coefficient=if mutation_scores.len()>=2 && retained_scores.len()>=2 {
        (0.55+0.12*(mean(&mutation_scores)-mean(&retained_scores))).clamp(0.15,1.15)
    } else {0.55};
    // The DOMINION harness synthesizes an executable scoring program, not
    // just a label chosen by a hardcoded failure switch. Each observed failure
    // or adoption can change both the weights and the expression's structure.
    // A symbolic program is interpreted by the existing safe algorithm
    // expression evaluator; it does not rely on model-generated prose.
    let any_failures=observations.values().any(|xs|
        xs.iter().any(|(value,_)|*value<0.0));
    let any_adoption=episodes.iter().any(|g|g["accepted"]==true);
    let failure_coefficient=(0.12+0.08*count as f64).min(0.65);
    let adoption_coefficient=(0.2+0.03*count as f64).min(0.5);
    let mut program=format!("posterior + exploration * {exploration_coefficient:.6}");
    if any_failures {
        program.push_str(&format!(" - failureRate * {failure_coefficient:.6}"));
    }
    if any_adoption {
        program.push_str(&format!(" + verifiedAdoption * {adoption_coefficient:.6}"));
    }
    let mut ranking=Vec::<(String,f64,usize)>::new();
    let mut evidence=Vec::<String>::new();
    let total=count as f64;
    for &action in alternatives {
        let samples=observations.get(action).cloned().unwrap_or_default();
        let n=samples.len();
        let sum=samples.iter().map(|(value,_)|*value).sum::<f64>();
        let prior=if action==baseline {0.25}else{-0.20};
        let posterior=(sum+prior)/(n as f64+1.0);
        let features=BTreeMap::from([
            ("posterior".to_string(),posterior),
            ("exploration".to_string(),((1.0+total).ln()/(n as f64+1.0)).sqrt()),
            ("failureRate".to_string(),if n==0 {0.0}else{
                samples.iter().filter(|(v,_)|*v<0.0).count() as f64/n as f64}),
            ("verifiedAdoption".to_string(),if n==0 {0.0}else{
                samples.iter().filter(|(_,id)|episodes.iter().any(|row|
                    row["id"].as_str()==Some(id.as_str()) && row["accepted"]==true))
                    .count() as f64/n as f64}),
        ]);
        let score=crate::engine::algorithms::eval_expression(&program,&features)
            .ok().filter(|score|score.is_finite()).unwrap_or(posterior);
        ranking.push((action.to_string(),score,n));
        evidence.extend(samples.into_iter().map(|(_,id)|id));
    }
    ranking.sort_by(|a,b|b.1.total_cmp(&a.1).then_with(||a.0.cmp(&b.0)));
    // A passing/accepted action is never displaced by a score that lacks
    // comparison. Adapt only when the last relevant outcomes show stalled
    // progress; an untried path stays an experiment, not a promotion.
    let recent=history.iter().rev().take(2).any(|g|g["accepted"]==true);
    let chosen=if recent {baseline.clone()} else {ranking[0].0.clone()};
    evidence.sort();
    evidence.dedup();
    let revision=crate::engine::memory::stable_id(&[
        &research_scope(input).partition_key,
        input["rsiObjective"].as_str().unwrap_or(""),
        context,&json!({"rank":ranking,"evidence":evidence,"program":program}).to_string()
    ]);
    // The full evidence set contributes to the revision hash, but passing
    // 80 source IDs back to a 4B model wastes tokens. Keep a bounded manifest.
    let recent_evidence=evidence.iter().rev().take(12).cloned().collect::<Vec<_>>();
    let evidence_count=evidence.len();
    decision["mode"]=json!(chosen);
    decision["policy"]=json!({
        "kind":"selfimprove.action","implementation":"synthesized_symbolic_action_program",
        "revision":revision,"context":context,"trainingTransitions":count,
        "scoreExpression":program,
        "programStructure":{"hasFailurePenalty":any_failures,
            "hasAdoptionReward":any_adoption},
        "trainingGenerationIds":recent_evidence,
        "trainingEvidenceCount":evidence_count,
        "scoreComponents":["observed_action_value","token_cost","posterior_utility","exploration"],
        "metaTuning":{"explorationCoefficient":exploration_coefficient,
            "retainedObservations":retained_scores.len(),
            "mutatedObservations":mutation_scores.len(),
            "retainedMean":if retained_scores.is_empty(){Value::Null}else{json!(mean(&retained_scores))},
            "mutatedMean":if mutation_scores.is_empty(){Value::Null}else{json!(mean(&mutation_scores))},
            "causalAttribution":"unmeasured"},
        "rankedAlternatives":ranking.iter().map(|(name,score,n)|json!({
            "action":name,"score":score,"observations":n
        })).collect::<Vec<_>>(),
        "previousDefault":baseline,
        "selected":chosen,
        "status":if chosen==baseline {"evidence_based_retention"}else{"exploratory_policy_mutation"},
        "measuredGain":Value::Null,
        "note":"Policy selection based on past observed actions; only later real work can validate improvement"
    });
    decision
}

/// Convert the latest observed outcome into a specific next executable action.
pub(crate) fn next_execution_action(history:&[Value])->Value {
    let Some(last)=history.last() else {return json!({"mode":"discover","previousAttempt":null});};
    let previous=last["actionDirective"]["mode"].as_str().unwrap_or("");
    let verdict=last["evaluation"]["verdict"].as_str().unwrap_or("");
    let edited=last["actionObservation"]["editToolCalls"].as_u64().unwrap_or(0)>0;
    let failed_edit=edited && last["actionObservation"]["appliedEditCalls"].as_u64()==Some(0)
        || last["actionObservation"]["status"]=="write_without_diff";
    let mode=if last["accepted"]==true {"extend_verified_capability"}
        else if matches!(verdict,"regressed"|"build_failed")
            || (last["unverifiedCandidateId"].is_string()
                && last["staticChecksPassed"]==false) {
            if previous=="repair_failed_case" {"change_approach"} else {"repair_failed_case"}
        } else if last["candidateId"].is_string() {
            if previous=="revise_candidate" {"change_approach"} else {"revise_candidate"}
        } else if failed_edit {"repair_failed_edit"}
        else if edited {"complete_candidate"}
        else if last["selfRepair"]["outcome"]=="no_patch_after_recovery" {
            "change_approach"
        }
        else if last["newFindings"].as_u64().unwrap_or(0)>0
            && previous!="implement_research" {"implement_research"}
        else {"change_approach"};
    json!({"mode":mode,"previousAttempt":last["attempt"],
        "previousDomain":last["domain"],"previousCandidateId":last["candidateId"],
        "previousError":last["error"].as_str()
            .filter(|error|!error.is_empty())
            .or_else(||last["actionObservation"]["lastError"].as_str())
            .unwrap_or("").chars().take(500).collect::<String>(),
        "previousEdit":last["actionObservation"],
        "previousTools":last["toolsUsed"]})
}
pub(crate) fn execution_action_contract(action:&Value)->&'static str {
    match action["mode"].as_str().unwrap_or("discover") {
        "implement_research" => "이전 세대의 저장된 소스 근거를 재사용하고 workspace_read_range의 SHA 고정 범위로 workspace_replace를 실행해 기존 코드를 수정하세요. 같은 조사만 반복하지 마세요.",
        "repair_failed_case" => "지난 후보의 실패한 holdout 경로를 수정하는 다른 코드 패치를 도구로 실행하세요. 실패한 패치의 재제출은 해결이 아닙니다.",
        "revise_candidate" => "지난 후보가 독립 평가를 통과하지 못한 원인을 좁히고 수정안을 실제 파일 편집 도구로 적용하세요.",
        "complete_candidate" => "지난 세대에는 파일 수정 도구를 호출했으나 유효한 후보가 없습니다. 변경의 저장·컴파일·검증 단계 오류를 고쳐 후보를 완성하세요.",
        "repair_failed_edit" => "직전 파일 수정 도구 호출의 실측 오류와 파일 경로를 먼저 사용하세요. 읽었던 원문과 현재 내용을 맞춘 뒤 workspace_replace로 최소 수정하세요. 파일을 실제로 바꾸지 못한 호출은 성공으로 세지 마세요.",
        "change_approach" => "이전 세대는 수정 없이 종료됐습니다. 실패 원인에 따라 대상 범위나 수정 전략을 바꾸고 실제 파일 편집을 시도하세요.",
        "extend_verified_capability" => "검증된 개선은 보존하면서 다른 문제의 새 개선을 실제 코드 수정과 독립 평가로 증명하세요.",
        _ => "구체적인 문제 한 가지를 찾아 실제 소스 변경 도구와 검증을 실행하세요. 설명이나 조사만으로 완료하지 마세요."
    }
}

// A generation's research continuation must be decided identically by the
// coordinator (which chooses source files) and by the native child worker
// (which actually runs the algorithm). Otherwise a trial policy can appear
// in focusSelection even though no trial was executed.
