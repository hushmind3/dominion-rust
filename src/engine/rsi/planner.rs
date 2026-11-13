//! Source-grounded action policy, successor design and compiled descendant planning.
//! Private to the RSI coordinator; no frozen evaluator or model weight changes.
use super::*;

pub(super) fn actual_edit_observation(tools:&[Value], candidate_created:bool)->Value {
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
pub(super) fn observed_edit_outcome(
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
pub(super) fn action_learning_context(row:&Value)->&'static str {
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
pub(super) fn action_choices(context:&str)->&'static [&'static str] {
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
pub(super) fn observed_action_value(row:&Value)->f64 {
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
pub(super) fn evolved_execution_action(state:&AppState,input:&Value,history:&[Value])->Value {
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
pub(super) fn next_execution_action(history:&[Value])->Value {
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
pub(super) fn execution_action_contract(action:&Value)->&'static str {
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
pub(super) fn research_continuation(history: &[Value]) -> Option<String> {
    // Research is useful only when it leads to an experiment. After three
    // source-reading generations without an executable patch, force the
    // next generation to change its strategy instead of endlessly reading.
    let research_only=history.iter().rev().take(3)
        .take_while(|row|row["accepted"]!=true && row["candidateId"].is_null()
            && row["newFindings"].as_u64().unwrap_or(0)>0)
        .count();
    if research_only>=3 {return None;}
    history.last()
        .filter(|row| row["candidateId"].is_null()
            && row["newFindings"].as_u64().unwrap_or(0) > 0)
        .and_then(|row| row["domain"].as_str())
        .filter(|domain| DOMAINS.contains(domain))
        .map(str::to_string)
}

pub(super) fn generation_target_plan(domain: &str, selection: &str, strategy: &Value, commit: &str) -> Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}

/// A synthesized method is a next-generation executable trial, not a
/// permanent override of the search algorithm. Require that its ID is in
/// the concrete reuse decision, and preserve higher-priority policy trials.
/// A real compiler-failed parent candidate is concrete executable work.
/// The child must select its source domain before the ordinary search policy
/// can replan toward unrelated files. Match campaign/base/patch identity;
/// only a saved failed source candidate can pin a descendant's work.
pub(super) fn pending_failed_patch_domain(
    state:&AppState,input:&Value,generator_commit:&str
)->Option<String> {
    let recovery=&input["rsiFailedPatchRecovery"];
    let id=recovery["candidateId"].as_str()?;
    let candidate=state.store.get("rsi_candidate",id)?;
    if recovery["baseCommit"]!=generator_commit
        || candidate["baseCommit"]!=generator_commit
        || candidate["experimentCampaignId"]!=input["rsiCampaignId"]
        || candidate["patchSha256"].as_str().is_none_or(str::is_empty)
        || recovery["checks"].as_array().is_none_or(Vec::is_empty) {
        return None;
    }
    let paths=candidate["paths"].as_array()?;
    let focus=recovery["focusPath"].as_str().filter(|path|
        crate::engine::rsi::safe_path(path)
        && paths.iter().any(|value|value.as_str()==Some(*path)));
    let path=focus.or_else(||paths.iter().filter_map(Value::as_str)
        .find(|path|crate::engine::rsi::safe_path(path)))?;
    Some(focus_domain_hint(path).to_string())
}

pub(super) fn pending_method_recovery_domain(input:&Value)->Option<String> {
    let id=input["rsiDecisionInput"]["methodRecoveryCandidateId"].as_str()?;
    let selected=input["rsiDecisionInput"]["reuseDecision"]["methodIds"].as_array()?;
    if !selected.iter().any(|item|item.as_str()==Some(id)) {return None}
    input["rsiSelectedDomain"].as_str().filter(|domain|
        DOMAINS.contains(domain)).map(str::to_string)
}

pub(super) fn select_generation_strategy(
    state: &AppState,
    history: &[Value],
    objective: &str,
    continuation: Option<String>,
    trial_id: Option<String>,
    selected_domain: Option<String>,
    generator_commit: &str,
) -> (String, &'static str, Value) {
    let active = crate::engine::algorithms::active(state, "selfimprove.search");
    // A genuine queued policy is work to execute, not another research note.
    // Continue reading in the worker prompt, but give its trial/control turn
    // priority so a four-generation budget can reach a real experiment.
    let executable_trial=trial_id.as_deref()
        .and_then(|id|state.store.get("algorithm",id))
        .is_some_and(|algorithm|matches!(algorithm["rsiTrial"]["status"].as_str(),
            Some("pending_trial"|"pending_second_trial")));
    if let Some(domain) = continuation.filter(|_|!executable_trial) {
        return (domain, "research_continuation", Value::Null);
    }
    if let Some(id) = trial_id {
        if let Some(algorithm) = state.store.get("algorithm", &id).filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["status"] != "trial_rejected"
        }) {
            if valid_domain_scores(&algorithm,history,objective).is_none() {
                let baseline=active.unwrap_or_else(||json!({
                    "implementation":"failure-clusters","parameters":{}
                }));
                let mut strategy=search_strategy(&baseline,"active");
                strategy["trialFallback"]=json!("candidate_expression_invalid_for_runtime_features");
                return (select_domain(&baseline,history,objective),"algorithm_active",strategy);
            }
            // A prior control for this exact content-addressed policy pins
            // its domain. Re-scoring after the control adds new history can
            // otherwise change the domain and prevent the policy from ever
            // reaching its comparable next-generation trial.
            let pinned=history.iter().rev().find(|row|
                row["generatorCommit"]==generator_commit
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["pendingCandidateId"]==algorithm["id"]
                    && row["searchStrategy"]["pendingCandidateRevision"]==algorithm["revision"]
                    && row["domain"].as_str().is_some_and(|domain|DOMAINS.contains(&domain)));
            let domain=pinned.and_then(|row|row["domain"].as_str())
                .map(str::to_string)
                .unwrap_or_else(||select_domain(&algorithm, history, objective));
            let generation = json!({"domain":domain,"generatorCommit":generator_commit});
            if comparable_trial_baseline(history, &generation, &algorithm).is_some() {
                return (domain, "algorithm_trial", search_strategy(&algorithm, "trial"));
            }
            // Repeated active-policy generations may produce no patch at all.
            // They have no frozen measurements, and can never satisfy the
            // comparable-control gate. Try the proposed policy once to let a
            // different target search actually produce a source patch. Its
            // policy trial is still INCONCLUSIVE without a verified control;
            // a separately verified patch may still improve the next binary.
            let no_patch_controls=history.iter().rev()
                .filter(|row|row["generatorCommit"]==generator_commit
                    && row["domain"]==domain
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["pendingCandidateId"]==algorithm["id"]
                    && row["candidateId"].is_null()
                    && row["accepted"]==false)
                .count();
            if no_patch_controls>=2 && algorithm["rsiTrial"]["trials"].as_array()
                .is_none_or(Vec::is_empty) {
                let mut exploratory=search_strategy(&algorithm,"trial");
                exploratory["trialFallback"]=json!("exploratory_after_two_no_patch_controls");
                exploratory["policyEvidence"]=json!("unpaired_exploration_not_promotion");
                return (domain,"algorithm_trial",exploratory);
            }
            // The active policy must supply a control in the domain actually
            // selected by the candidate; unrelated controls cannot test it.
            let active = active
                .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
            let mut selected = search_strategy(&active, "active");
            selected["trialFallback"] = json!("collecting_same_domain_control");
            selected["decisionSource"] = json!("candidate_target_active_control");
            selected["pendingCandidateId"] = json!(id);
            selected["pendingCandidateRevision"] = algorithm["revision"].clone();
            selected["controlDomainPinned"] = json!(domain);
            return (domain, "algorithm_active", selected);
        }
        let algorithm = active
            .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
        let mut selected = search_strategy(&algorithm, "active");
        selected["trialFallback"] = json!("candidate_missing_or_rejected");
        return (
            select_domain(&algorithm, history, objective),
            "algorithm_active",
            selected,
        );
    }
    let algorithm =
        active.unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
    if let Some(domain) = selected_domain.filter(|domain| DOMAINS.contains(&domain.as_str())) {
        let mut strategy = search_strategy(&algorithm, "active");
        strategy["decisionSource"] = json!("parent_next_decision");
        return (domain, "algorithm_active", strategy);
    }
    (
        select_domain(&algorithm, history, objective),
        "algorithm_active",
        search_strategy(&algorithm, "active"),
    )
}

/// Replan against the descendant's actual selector. A parent selected source
/// excerpt cannot be reused under a different strategy or domain; record both
/// decisions and rebuild the prompt from the original bounded ingredients.
pub(super) fn reconcile_descendant_target(
    state:&AppState,job:&mut Value,history:&[Value],
    domain:&str,selection:&str,strategy:&Value,commit:&str,
)->Result<bool,String> {
    let actual=generation_target_plan(domain,selection,strategy,commit);
    let previous=job["input"]["rsiTargetPlan"].clone();
    if previous==actual {return Ok(false);}
    // Legacy jobs without a predeclared plan are still executable; they have
    // no frozen coordinator claim to overwrite and are not re-labelled.
    if !previous.is_object() {return Ok(false);}
    let base=job["input"]["rsiPromptBase"].as_str()
        .ok_or("rsi_target_replan: 원본 목표 프롬프트 없음")?.to_string();
    let evidence=job["input"]["rsiPromptEvidence"].as_str().unwrap_or("").to_string();
    let research=job["input"]["rsiPromptResearch"].as_str().unwrap_or("").to_string();
    job["input"]["rsiTargetDomain"]=json!(domain);
    if job["input"]["rsiAutoFocus"]==true {
        let source=PathBuf::from(job["input"]["rsiSourceDir"].as_str()
            .ok_or("rsi_target_replan: 소스 폴더 없음")?);
        let index=crate::engine::code_index::refresh(state,&source)?;
        job["input"]["rsiIndexId"]=index["id"].clone();
        let policy=strategy["id"].as_str()
            .and_then(|id|state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
        let ranked=choose_focus_files(state,&job["input"],history,policy.as_ref());
        job["input"]["rsiFocusFiles"]=json!(ranked.iter().filter_map(|r|r["path"].as_str())
            .collect::<Vec<_>>());
        job["input"]["rsiFocusSelection"]=json!({
            "source":"descendant_indexed_evidence",
            "candidates":ranked,
            "domain":domain,
            "policyRevision":policy.as_ref().map(|p|p["revision"].clone()),
            "sourceIndexId":index["id"]
        });
        // Rebinding is mandatory after a descendant selector changes its
        // focus. Otherwise coordinator and rebuilt worker choose different
        // targets and source-edit checkpoint remains disconnected.
        bind_action_to_source(state,&mut job["input"],history);
    }
    let source=job["input"]["rsiSourceDir"].as_str()
        .ok_or("rsi_target_replan: 소스 경로 없음")?.to_string();
    let focus=focus_file_context(state,&json!({"labDir":source}),&job["input"]);
    job["input"]["rsiFocusGrounded"]=json!(focus_excerpt_has_source(&job["input"],&focus));
    let composed=format!("{base}\n\n{evidence}{research}{focus}");
    let bound=job["input"]["model"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["providerId"].as_str().is_some_and(|s|!s.is_empty());
    let limit=job["input"]["rsiRemainingCampaignTokens"].clone();
    if bound && !generation_fits_remaining_budget(
        &json!({"limits":{"tokenBudget":limit},"tokens":0}),
        &json!({"prompt":composed})) {
        return Err("rsi_token_budget: 차세대 목표 재계획 문맥이 잔여 예산을 초과합니다.".into());
    }
    job["input"]["prompt"]=json!(composed);
    job["input"]["rsiFocusContextChars"]=json!(focus.chars().count());
    job["input"]["rsiPromptContextChars"]=json!(composed.chars().count());
    job["input"]["rsiTargetPlan"]=actual.clone();
    if job["input"]["rsiDecisionInput"].is_object() {
        job["input"]["rsiDecisionInput"]["nextDomain"]=json!(domain);
        job["input"]["rsiDecisionInput"]["decisionSource"]=json!("descendant_worker");
    }
    let receipt=json!({
        "coordinatorPlan":previous,
        "descendantPlan":actual,
        "generatorCommit":commit,
        "sourceIndexId":job["input"]["rsiIndexId"],
        "reason":"descendant_authoritative_selection"
    });
    job["rsiTargetReplan"]=receipt.clone();
    job["input"]["rsiTargetReplan"]=receipt;
    Ok(true)
}

/// A model-authored edit rule earns credit ONLY if that exact rule fired
/// before a verified source write to a file present in the independently
/// adopted post-change source ledger. A neutral paired trial is a small
/// feasibility observation, never a claimed capability gain.
pub(super) fn sub2_edit_policy_reward(g:&Value)->f64 {
    // Never promote a source-edit controller on a request's claimed model
    // identity. The full, untruncated actual session audit must match.
    if g["modelAttributionConsistent"]!=true
        || g["candidateOrigin"]!="model_driven_detached_worktree"
        || g["editPolicy"].is_null(){return 0.0;}
    // Merely carrying an inherited policy in a plan is not execution.
    // Match its revision to a real program-stage or rule-fire receipt
    // from the same worker before assigning credit for source quality.
    let executable=g["programStages"].as_array().is_some_and(|rows|
        rows.iter().any(|row|
            row["policyRevision"]==g["editPolicy"]["revision"]
            && row["generationJobId"]==g["jobId"]
            && row["runId"].is_string()))
        || g["ruleExecutions"].as_array().is_some_and(|rows|
            rows.iter().any(|row|
                row["policyRevision"]==g["editPolicy"]["revision"]
                && row["generationJobId"]==g["jobId"]
                && row["runId"].is_string()));
    if !executable{return 0.0;}
    let accepted=g["accepted"]==true && g["pairedRevalidated"]==true
        && g["evaluation"]["metrics"]["eligible"]==true
        && g["evidenceVerified"]==true;
    if accepted {
        let Some(rules)=g["editPolicy"]["rules"].as_array()
            else{return 0.0};
        let Some(timeline)=g["toolTimeline"].as_array()
            else{return 0.0};
        let adopted_after=|op:&Value,run:&str,before:u64|{
            let expected=op["path"].as_str().and_then(|path|
                g["acceptedSourceHashes"].get(path))
                .and_then(Value::as_str);
            op["runId"].as_str()==Some(run)
                && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                && matches!(op["tool"].as_str(),
                    Some("workspace_replace"|"workspace_replace_range"
                        |"workspace_replace_function"|"workspace_write"))
                && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true
                && expected.is_some_and(|sha|sha.len()==64
                    && op["result"]["postSha256"].as_str()==Some(sha))
        };
        if rules.is_empty() {
            // A source improvement can be attributed to the inherited edit
            // PROGRAM only if its edit phase preceded the actual accepted
            // source bytes. A discovery or grounding stage alone is not
            // proof that a rewritten procedure changed the implementation.
            let witnessed=g["programStages"].as_array().is_some_and(|events|
                events.iter().any(|event|{
                    let Some(run)=event["runId"].as_str() else{return false};
                    let Some(before)=event["opSeqBefore"].as_u64() else{return false};
                    event["generationJobId"]==g["jobId"]
                        && event["policyRevision"]==g["editPolicy"]["revision"]
                        && event["phase"]=="edit"
                        && timeline.iter().any(|op|adopted_after(op,run,before))
                }));
            return if witnessed {1.0}else{0.0};
        }
        let proved=g["ruleExecutions"].as_array().is_some_and(|events|
            events.iter().any(|event|{
                let Some(run)=event["runId"].as_str() else{return false};
                let Some(before)=event["opSeqBefore"].as_u64() else{return false};
                let matched_rule=event["generationJobId"]==g["jobId"]
                    && event["policyRevision"]==g["editPolicy"]["revision"]
                    && rules.iter().any(|r|r["id"]==event["ruleId"]);
                let acted=timeline.iter().any(|op|
                    op["runId"]==run
                    && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                    && op["status"]=="completed"
                    && event["allow"].as_array().is_some_and(|tools|
                        tools.iter().any(|name|name==&op["tool"])));
                let adopted=timeline.iter().any(|op|adopted_after(op,run,before));
                matched_rule && acted && adopted
            }));
        return if proved {1.0}else{0.0};
    }
    if g["pairedRevalidated"]==true
        && g["candidateId"].is_string()
        && g["evaluation"]["metrics"]["regressions"].as_u64()==Some(0) {
        return 0.1;
    }
    0.0
}
/// The old supervisor does not synthesize source-edit rules. The current
/// compiled child learns from its own executed receipts AND persisted earlier
/// campaigns with the same branch, objective, partition and model identity.
pub(super) fn inherited_edit_policy(
    state:&AppState,input:&Value,history:&[Value],
    selected_model:&Value,domain:&str,
)->Value{
    let objective=crate::engine::memory::stable_id(&[
        input["rsiObjective"].as_str().unwrap_or("")
    ]);
    let partition=research_scope(input).partition_key;
    let provider=selected_model["providerId"].as_str()
        .or_else(||input["providerId"].as_str()).unwrap_or("");
    let model=selected_model["model"].as_str()
        .or_else(||input["model"].as_str()).unwrap_or("");
    let branch=input["rsiSourceBranch"].as_str().unwrap_or("");
    let mut seen=BTreeSet::<String>::new();
    let mut rows=Vec::<Value>::new();
    for g in history.iter().chain(state.store.all("rsi_generation").iter()) {
        let Some(id)=g["jobId"].as_str() else{continue};
        if !seen.insert(id.to_string())
            || g["objectiveId"]!=objective
            || g["knowledgePartition"]!=partition
            || g["sourceBranch"]!=branch
            || g["generatorProviderId"]!=provider
            || g["generatorModel"]!=model {continue;}
        rows.push(g.clone());
    }
    rows.sort_by(|a,b|a["createdAt"].as_str()
        .cmp(&b["createdAt"].as_str())
        .then_with(||a["jobId"].as_str().cmp(&b["jobId"].as_str())));
    // Local rule mutation is bounded to recent executions. Confirmed
    // productivity evidence is a DIFFERENT, durable learning horizon:
    // truncating it to 128 rows can erase both arms of earlier independently
    // matched source trials after many campaigns.
    let recent=if rows.len()>128{
        &rows[rows.len()-128..]
    }else{rows.as_slice()};
    let generated=crate::engine::rsi_edit_genome::evolve(recent,sub2_edit_policy_reward);
    // Prefer a policy only after it produced more independently adopted
    // FIRST rebuilt successors than a control in >=2 matched source contexts.
    // This decision is compiled into the current child's executable policy,
    // not a recommendation authored by the older campaign coordinator.
    if let Some(profile)=crate::engine::rsi_meta_controller::comparison_profile(input){
        crate::engine::rsi_successor_productivity::choose(
            state,&rows,generated,&profile,domain)
    }else{generated}
}

// Allow the compiled model's *own* meta-method redesign to reach the
// executable source selector, without overriding an owner's explicit focus
// or an independently adopted predecessor programme.
pub(super) fn evidence_based_meta_repair_focus(
    meta:&Value,successor_stage:&Value,explicit_focus:bool
)->bool{
    !explicit_focus && !successor_stage.is_object()
        && meta["metaRepairChosen"]==true
        && matches!(meta["mode"].as_str(),
            Some("repair_own_prediction_mechanism"|"repair_own_transfer_method"))
        && meta["target"]["path"]=="src/engine/rsi_meta_controller.rs"
}

/// Compute the NEXT system architecture proposal inside the currently
/// compiled descendant. The resident coordinator has no authority to replace
/// this decision; it may validate a plan and then run a frozen evaluator.
pub(super) fn compiled_system_generation_plan(
    state:&AppState,input:&Value,history:&[Value],commit:&str
)->Result<Value,String> {
    if input["rsiSystemEvolution"]!=true
        || input["rsiGeneratorCommit"]!=commit
        || !input["rsiSourceDir"].is_string() {
        return Err("컴파일된 후손 계획의 revision 또는 시스템 모드가 올바르지 않습니다.".into());
    }
    let root=Path::new(input["rsiSourceDir"].as_str().unwrap_or(""));
    // Derive the entire compiled successor plan from replayed immutable
    // experimental observations, not stored success/failure labels.
    let audited_history=crate::engine::rsi_system::revalidated_method_history(
        state,history);
    let history=audited_history.as_slice();
    let blueprint=crate::engine::rsi_system::blueprint(root,commit,history);
    if blueprint["sourceTreeSha256"].as_str().is_none_or(|s|s.len()!=64)
        || blueprint["architecture"].as_array().is_none_or(|a|a.len()<7||a.len()>64) {
        return Err("후손이 실제 전체 시스템 자기모형을 구축하지 못했습니다.".into());
    }
    let objective=selection_objective(input);
    // The accepted model's independently measured design is executable
    // successor policy. Failure/research history still drives the evolved
    // search algorithm when no adopted model-designed successor exists.
    let inherited=crate::engine::rsi_system::inherited_domain(history);
    // The compiled descendant searches its own source graph for whole-system
    // coverage gaps, independently of the coordinator's historical ranking.
    // A predecessor's verified design remains authoritative for its child.
    // An uncovered module is a search opportunity, NEVER a demonstrated gain.
    let frontier=crate::engine::rsi_system::architecture_frontier(
        state,&blueprint,history,input["rsiIndexId"].as_str().unwrap_or(""),root);
    // This editable module runs INSIDE the newly compiled descendant.
    // Its learned prediction dynamics select the NEXT architecture target.
    let explicit_focus=input["rsiAutoFocus"]!=true
        && input["rsiFocusFiles"].as_array().is_some_and(|files|!files.is_empty());
    // Source-grounded coverage, inherited design, prequential forecasts,
    // transferable methods and the next recursive plan share the same
    // independently revalidated history snapshot.
    let baseline_meta=crate::engine::rsi_meta_controller::synthesize(
        history,&frontier,DOMAINS,inherited.as_deref());
    // Portable method hypotheses require verified peer receipts and at
    // least one identical indexed source. Only the COMPILED child can
    // re-rank its own target; the frozen resident validates the raw receipts.
    // Explicit user-selected paths and adopted successor programs keep
    // their existing precedence.
    let peers=crate::engine::rsi_meta_controller::peer_lineages(
        state,input["rsiCampaignId"].as_str().unwrap_or(""),input,None);
    let meta=if explicit_focus{baseline_meta}else{
        crate::engine::rsi_meta_controller::evolve_from_peer_methods(
            baseline_meta,&peers,history)
    };
    // Learned laws are compiled into this actual successor, not a prompt-only
    // retrospective. A task-intelligence bottleneck can supersede another
    // purely meta-level tweak. The child may rewrite this scoring law itself.
    let principle=crate::engine::rsi_principle_lab::plan(history,&frontier,commit);
    // An actual adopted model-authored experiment graph supersedes the old
    // fixed phase scheduler; its interpreter is compiled into THIS child.
    let open_program=if explicit_focus{Value::Null}else{
        crate::engine::rsi_open_program::synthesize(history,&frontier,commit)
    };
    let mut architecture_target=if open_program["target"].is_object(){
        open_program["target"].clone()
    }else if principle["shouldProbeTaskMethod"]==true
        && principle["target"].is_object()
        && principle["methodTarget"].is_object(){
        principle["target"].clone()
    }else if meta["target"].is_object(){
        meta["target"].clone()
    }else{frontier.first().cloned().unwrap_or(Value::Null)};
    // A predecessor can AUTHOR the successor's task program. The compiled
    // descendant interprets it against its own newly built source graph and
    // silently drops stages whose path/role/hash is no longer current.
    let successor_stage=if open_program["stage"].is_object(){
        open_program["stage"].clone()
    }else{
        crate::engine::rsi_system::inherited_stage(history,&frontier)
            .or_else(||crate::engine::rsi_mutual_evolution::inherited_programme_stage(
                history,&frontier))
            .or_else(||crate::engine::rsi_mutual_evolution::reciprocal_return(
                history,&frontier))
            .unwrap_or(Value::Null)
    };
    // An independently observed meta-rule failure should reach the actual
    // compiled source target. Existing adopted successor programmes and
    // explicit owner focus retain their higher priority.
    let enforce_meta_repair=evidence_based_meta_repair_focus(
        &meta,&successor_stage,explicit_focus);
    if enforce_meta_repair{
        architecture_target=meta["target"].clone();
    }
    // Owner-constrained source targets must remain authoritative. The
    // autonomous factorial scheduler may not silently redirect an explicit
    // user target or forbid its ordinary source edits at the tool gateway.
    // A live, adopted predecessor's programme takes precedence. Otherwise
    // the compiled meta agent authors the next A->B->A source curriculum.
    let mutual=if successor_stage.is_object() || explicit_focus || enforce_meta_repair{
        Value::Null
    }else{
        crate::engine::rsi_mutual_evolution::curriculum(&frontier,&meta,DOMAINS)
    };
    // This experiment designer is compiled into the NEW child and can evolve.
    let raw_factorial=if successor_stage.is_object() || explicit_focus || enforce_meta_repair{
        Value::Null
    }else{
        crate::engine::rsi_causal_curriculum::design(
            history,&mutual,&frontier,commit)
    };
    let suggested=crate::engine::rsi_recursive_discovery::plan(
        history,&frontier,&blueprint["openTopology"],&meta,
        &raw_factorial,&mutual,commit);
    let phase=suggested["phase"].as_str().unwrap_or("").to_string();
    // Never interrupt an already accepted recursive invention lineage.
    // When the scientific programme is only inventing from no evidence,
    // let the descendant first create an independently testable task↔method
    // principle and measure whether it makes its NEXT descendant stronger.
    let principle_active=!explicit_focus && !enforce_meta_repair
        && !successor_stage.is_object()
        && principle["target"].is_object()
        && principle["methodTarget"].is_object()
        && principle["shouldProbeTaskMethod"]==true
        && matches!(phase.as_str(),"invent_new_intelligence"|"observe_factorial");
    let scientific_active=!explicit_focus && !enforce_meta_repair
        && !principle_active
        && !open_program["stage"].is_object()
        && suggested["target"].is_object()
        && (matches!(phase.as_str(),
            "develop_invented_capability"|"return_to_origin"|"rewrite_discovery_method"
                |"repair_discovery_method")
            || !successor_stage.is_object());
    // An adopted recursive-science continuation owns the ONE source focus:
    // no unrelated isolated factorial arm may block its actual write tools.
    let factorial=if principle_active
        || (scientific_active && phase!="invent_new_intelligence"){
        Value::Null
    }else{raw_factorial};
    let scientific=if scientific_active && phase!="invent_new_intelligence"{
        crate::engine::rsi_recursive_discovery::plan(
            history,&frontier,&blueprint["openTopology"],&meta,
            &factorial,&mutual,commit)
    }else{suggested};
    let empty_scientist=Value::Null;
    let scientific_target=if scientific_active{
        &scientific["target"]
    }else{&empty_scientist};
    let scientific_domain=scientific_target["domain"].as_str()
        .filter(|d|DOMAINS.contains(d)).map(str::to_string);
    let factorial_target=&factorial["selected"]["target"];
    let factorial_domain=factorial_target["domain"].as_str()
        .filter(|d|DOMAINS.contains(d)).map(str::to_string);
    let successor_domain=successor_stage["domain"].as_str()
        .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
    let architecture_domain=architecture_target["domain"].as_str()
        .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
    let continuation=if successor_domain.is_some()||inherited.is_some()
        ||architecture_domain.is_some(){None}else{research_continuation(history)};
    let selected=if enforce_meta_repair{
        architecture_domain.clone()
    }else{
        scientific_domain.or(successor_domain).or(factorial_domain)
            .or(inherited.clone()).or(architecture_domain)
            .or_else(||input["rsiSelectedDomain"].as_str()
                .filter(|domain|DOMAINS.contains(domain)).map(str::to_string))
    };
    let (domain,selection,strategy)=select_generation_strategy(
        state,history,&objective,continuation,
        None,selected,commit
    );
    let mut focus_input=input.clone();
    focus_input["rsiTargetDomain"]=json!(domain);
    let mut ranked=choose_focus_files(state,&focus_input,history,None);
    // Keep the indexed source evidence and SHA attached to the actual
    // descendant-selected target. An inherited model design can change the
    // domain, in which case use that domain's indexed focus instead.
    let preferred=if scientific_active && scientific_target["domain"]==domain {
        scientific_target["path"].as_str()
    }else if successor_stage["domain"]==domain {
        successor_stage["path"].as_str()
    }else if factorial_target["domain"]==domain {
        factorial_target["path"].as_str()
    }else if architecture_target["domain"]==domain {
        architecture_target["path"].as_str()
    }else{None};
    if let Some(path)=preferred {
        if root.join(path).is_file() {
            ranked.retain(|row|row["path"]!=path);
            let inherited_focus=successor_stage["path"]==path;
            let factorial_focus=factorial_target["path"]==path
                && factorial_target["domain"]==domain;
            let scientific_focus=scientific_active
                && scientific_target["path"]==path
                && scientific_target["domain"]==domain;
            let chosen=if scientific_focus{scientific_target}
                else if inherited_focus{&successor_stage}
                else if factorial_focus{factorial_target}
                else{&architecture_target};
            let mut prioritised=json!({
                "path":path,
                "sourceHash":chosen["sourceSha256"],
                "source":if scientific_focus{
                    "compiled_recursive_discovery_experiment"
                }else if inherited_focus {
                    "inherited_model_authored_successor_program"
                }else if factorial_focus{"compiled_factorial_source_intervention"}
                else{"compiled_architecture_frontier"},
                "role":chosen["role"],
                "domain":domain,
                "unmodified":if inherited_focus {
                    true
                }else{chosen["verifiedRoleImproved"]==false}
            });
            // Keep exact current-file index binding, so stale source graph
            // choices never become model-authorized write targets.
            prioritised["sourceIndexId"]=input["rsiIndexId"].clone();
            ranked.insert(0,prioritised);
        }
    }
    let original_paths=ranked.iter().filter_map(|row|row["path"].as_str())
        .filter(|path|crate::engine::rsi::safe_path(path)
            && root.join(path).is_file())
        .take(3).collect::<Vec<_>>();
    // Both source modules enter the NATIVE edit context, not merely the
    // prompt text. The B module may belong to a different domain than A.
    let mut paths=Vec::<String>::new();
    if enforce_meta_repair {
        // A meta-repair generation must edit the method-learning source,
        // not merely mention it alongside cheaper unrelated focus files.
        if let (Some(path),Some(sha))=(architecture_target["path"].as_str(),
            architecture_target["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path) && root.join(path).is_file()
                && crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha) {
                paths.push(path.to_string());
            }
        }
    }else if principle_active {
        // Source-ground both ends of the proposed learning law. This is an
        // actual two-file edit target, not just an abstract method label.
        for key in ["target","methodTarget"] {
            let item=&principle[key];
            if let (Some(path),Some(sha))=(
                item["path"].as_str(),item["sourceSha256"].as_str()) {
                if crate::engine::rsi::safe_path(path)
                    && root.join(path).is_file()
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                        .as_deref()==Some(sha)
                    && !paths.iter().any(|p|p==path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }else if scientific_active {
        if let Some(path)=scientific_target["path"].as_str()
            .filter(|p|crate::engine::rsi::safe_path(p)){
            paths.push(path.to_string());
        }
        if phase=="invent_new_intelligence" {
            for candidate in ["src/engine/rsi_evolve.rs","src/engine/mod.rs"] {
                if paths.len()>=3{break;}
                if !paths.iter().any(|p|p==candidate)
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),candidate)
                        .is_some(){paths.push(candidate.into());}
            }
        }
    }else if factorial["selected"]["isolationRequired"]==true {
        if let (Some(path),Some(sha))=(
            factorial_target["path"].as_str(),factorial_target["sourceSha256"].as_str()) {
            if crate::engine::rsi::safe_path(path)
                && crate::engine::code_index::indexed_source_hash(
                    state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                    .as_deref()==Some(sha){paths.push(path.to_string());}
        }
    }else if mutual["pair"].is_object()
        && architecture_target["domain"]==domain {
        for (key,sha_key) in [("fromPath","fromSourceSha256"),
            ("toPath","toSourceSha256")] {
            if let (Some(path),Some(sha))=(mutual["pair"][key].as_str(),
                mutual["pair"][sha_key].as_str()) {
                if crate::engine::rsi::safe_path(path) && root.join(path).is_file()
                    && crate::engine::code_index::indexed_source_hash(
                        state,input["rsiIndexId"].as_str().unwrap_or(""),path)
                        .as_deref()==Some(sha) && !paths.iter().any(|p|p==path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }
    if enforce_meta_repair {
        // Preserve only the observed self-repair source for this experiment.
    }else if scientific_active {
        // Invention may need a new module plus existing consumers; leave the
        // execution target no broader than its SOURCE-GROUNDED programme.
    }else if principle_active {
        // Keep both actual intelligence and method-learning sources as the
        // executable model context. Generic ranked files must not evict them.
    }else if factorial["selected"]["isolationRequired"]!=true{
        for path in original_paths{
            if paths.len()>=3{break;}
            if !paths.iter().any(|p|p==path){paths.push(path.to_owned());}
        }
    }
    let directive=crate::engine::rsi_system::improvement_directive(history);
    // Choose an independently replayed persistent stepping stone INSIDE the
    // rebuilt descendant. Frozen evidence is shared; model-authored and
    // executable selection methods may evolve with the child binary.
    let archive=if !explicit_focus && input["rsiAutoFocus"]==true
        && input["rsiFailedPatchRecovery"]["candidateId"].is_null() {
        crate::engine::rsi_archive::select_bound(
            state,history,commit,&input["rsiFrozenCases"],
            input["rsiSourceBranch"].as_str().unwrap_or(""))
    }else{Value::Null};
    // The NEWLY COMPILED child, not the old campaign supervisor, learns
    // which real configured API/local model will author this generation.
    // Updating this source or rsi_model_ecology.rs changes future routing.
    let model_route=crate::engine::rsi_model_ecology::choose(
        state,input,history,commit,&domain,
        &crate::engine::memory::stable_id(&[
            input["rsiObjective"].as_str().unwrap_or("")
        ]));
    let edit_policy=inherited_edit_policy(state,input,history,
        &model_route["selected"],&domain);
    Ok(json!({
        "source":"compiled_descendant_system_controller_v3",
        "generatorCommit":commit,
        "historyCount":history.len(),
        "sourceTreeSha256":blueprint["sourceTreeSha256"],
        "objectiveId":crate::engine::memory::stable_id(&[&objective]),
        "indexId":input["rsiIndexId"],
        "domain":domain,
        "selection":selection,
        "strategy":strategy,
        "targetPlan":generation_target_plan(&domain,selection,&strategy,commit),
        "focusFiles":paths,
        "ranked":ranked,
        "systemBlueprint":blueprint,
        "architectureFrontier":frontier,
        "architectureTarget":architecture_target,
        "metaPolicy":meta,
        "metaActionPolicy":meta["controlPolicy"],
        "peerLineages":peers,
        "mutualCurriculum":mutual,
        "factorialCurriculum":factorial,
        "recursiveDiscovery":scientific,
        "recursiveScienceActive":scientific_active,
        "principleProgramme":principle,
        "principleActive":principle_active,
        "openProgramPlan":open_program,
        "archiveSelection":archive,
        "editPolicy":edit_policy,
        "modelRoute":model_route,
        "successorStage":successor_stage,
        "improvementDirective":directive,
        "decisionOwner":"rebuilt_descendant_binary",
        "frozenAuthority":"resident_independent_evaluator"
    }))
}

/// Spawned by the earlier coordinator from the newly built binary, before
/// any model call. Reads the shared read-only lineage, owns the actual next
/// selection, and emits a plan bound to its embedded build SHA.
