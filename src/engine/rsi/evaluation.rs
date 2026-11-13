//! Search-policy trial proposals, comparisons and independent outcome accounting.
//! Private to the RSI coordinator; no frozen evaluator or model weight changes.
use super::{
    AppState, Value, BTreeMap, BTreeSet, json,
    rsi, eligible_domains, valid_domain_scores, evaluate_search_algorithm, save,
    elapsed, trial_owned_by,
};

pub(super) fn proposed_algorithms(state: &AppState, session: &str) -> Vec<Value> {
    let mut ids = Vec::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session)
            || operation["status"].as_str() != Some("completed")
            || operation["tool"].as_str() != Some("algorithm_propose")
        {
            continue;
        }
        if let Some(id) = operation["result"]["algorithmId"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            if !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_string());
            }
        }
    }
    ids.into_iter()
        .filter_map(|id| state.store.get("algorithm", &id))
        .collect()
}

/// Persist an evidence-based promotion. The replay corpus is the campaign's
/// own recorded generations, so a promotion can never be justified by a
/// candidate evaluating itself on the generation that produced it.
/// Stores a proposed search algorithm as a trial candidate. The replay match
/// rate is recorded for context only. It never activates the candidate and
/// never replaces the globally active strategy.
pub(super) fn queue_search_trial(
    state: &AppState,
    candidate: &Value,
    active: &Value,
    prior_history: &[Value],
    objective: &str,
    campaign_id: &str,
    attempt: u64,
) -> Value {
    // A proposal can be stale by the time its generation completes.
    // Always inspect the durable owner before attempting a trial transition.
    let latest=candidate["id"].as_str()
        .and_then(|id|state.store.get("algorithm",id))
        .unwrap_or_else(||candidate.clone());
    let candidate=&latest;
    let replay = evaluate_search_algorithm(candidate, active, prior_history, objective);
    let existing_status = candidate["rsiTrial"]["status"].as_str().unwrap_or("");
    if matches!(existing_status, "adopted" | "trial_rejected") {
        return json!({
            "status": existing_status,
            "candidateId": candidate["id"],
            "candidateRevision": candidate["revision"],
            "replay": replay,
            "alreadyResolved": true,
        });
    }
    // Content-addressed algorithm IDs may be shared by separate experiments.
    // Reproposing an identical formula cannot steal an unfinished campaign.
    if let Some(owner)=candidate["rsiTrial"]["campaignId"].as_str() {
        if owner!=campaign_id {
            return json!({"status":"owned_by_other_campaign",
                "candidateId":candidate["id"],"ownerCampaignId":owner});
        }
    }
    // The synthetic registration sample is not equivalent to live feature
    // values. Do not queue an expression which already fails on real history.
    if valid_domain_scores(candidate,prior_history,objective).is_none() {
        let invalid=json!({"status":"trial_inconclusive",
            "campaignId":campaign_id,
            "objectiveHash":crate::engine::memory::stable_id(&[objective]),
            "lastReason":"candidate_expression_invalid_for_runtime_features"});
        let saved=candidate["id"].as_str().and_then(|id|
            state.store.patch_if("algorithm",id,"rsiTrial",&candidate["rsiTrial"],
                &json!({"rsiTrial":invalid})));
        return json!({"status":if saved.is_some(){"trial_inconclusive"}else{"trial_conflict"},
            "candidateId":candidate["id"],
            "reason":"candidate_expression_invalid_for_runtime_features"});
    }
    let mut trial = candidate["rsiTrial"].clone();
    if !trial.is_object() {
        trial = json!({});
    }
    trial["status"] = json!("pending_trial");
    trial["campaignId"] = json!(campaign_id);
    trial["objectiveHash"] = json!(crate::engine::memory::stable_id(&[objective]));
    trial["proposedInGeneration"] = json!(attempt);
    trial["replay"] = replay.clone();
    trial["trials"] = trial["trials"].clone();
    if !trial["trials"].is_array() {
        trial["trials"] = json!([]);
    }
    trial["createdAt"] = json!(trial["createdAt"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()));
    // One SQLite compare-and-swap owns the transition, even when multiple
    // agents propose the same content-addressed algorithm simultaneously.
    let saved=candidate["id"].as_str().and_then(|id|
        state.store.patch_if("algorithm",id,"rsiTrial",&candidate["rsiTrial"],
            &json!({"rsiTrial":trial})));
    let Some(saved)=saved else {
        return json!({"status":"trial_conflict","candidateId":candidate["id"],
            "reason":"candidate_changed_before_claim"});
    };
    state.bus.record("algorithm",&saved);
    state.store.audit(
        "rsi.search_trial_queued",
        json!({
            "candidateId": candidate["id"],
            "candidateRevision": candidate["revision"],
            "campaignId": campaign_id,
            "replayMatchRate": replay["candidateMatchRate"],
            "samples": replay["samples"],
        }),
    );
    json!({
        "status": "pending_trial",
        "candidateId": candidate["id"],
        "candidateRevision": candidate["revision"],
        "replay": replay,
    })
}

/// Synthesize an executable search-policy experiment from observed failures,
/// even when a small model never calls algorithm_propose. This creates only a
/// candidate: promotion still requires independently measured live trials.
pub(super) fn autonomous_search_hypothesis(history:&[Value])->Option<Value> {
    let recent=history.iter().rev().take(4).collect::<Vec<_>>();
    if recent.len()<2 {return None;}
    // The last two attempts must have spent work without an accepted patch or
    // a new discovery. One bad model response is too little evidence.
    let failure=|row:&&Value| {
        row["accepted"]!=true && row["tokens"].as_u64().unwrap_or(0)>0
        && (row["newFindings"].as_u64().unwrap_or(0)==0
            || row["evaluation"]["verdict"]=="regressed")
    };
    let research_loop=recent.len()>=3 && recent.iter().take(3).all(|row|
        row["accepted"]!=true
        && row["candidateId"].is_null()
        && row["tokens"].as_u64().unwrap_or(0)>0);
    if !recent.iter().take(2).all(failure) && !research_loop {return None;}
    let wasted=recent.iter().filter(|row|failure(row)
        || research_loop && row["accepted"]!=true && row["candidateId"].is_null())
        .collect::<Vec<_>>();
    let regressions=wasted.iter().filter(|row|
        row["evaluation"]["verdict"]=="regressed").count();
    let stalled=wasted.iter().filter(|row|row["candidateId"].is_null()).count();
    let mut file_counts=BTreeMap::<String,usize>::new();
    for row in &wasted {
        for file in row["focusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str) {
            *file_counts.entry(file.to_string()).or_default()+=1;
        }
    }
    let repeated=file_counts.values().filter(|count|**count>=2).count();
    let novelty_weight=(2+repeated.min(4)+stalled.min(2)) as u64;
    let repeat_weight=(3+2*repeated.min(3)) as u64;
    let approach=if regressions>0 {"avoid_regressions"}
        else if research_loop && recent.iter().take(3)
            .any(|row|row["newFindings"].as_u64().unwrap_or(0)>0) {
            "escape_research_loop"
        } else {"escape_barren_research"};
    let domain_expression=if regressions>0 {
        format!("objectiveMatch * 3 + successRate * 4 + exploration * 1.5 + novelty * {novelty_weight} - regressions * 2 - barrenRate - tokenCost * 0.3")
    } else {
        format!("objectiveMatch * 3 + exploration * 2 + novelty * {novelty_weight} + successRate * 1.5 - barrenRate * 2 - tokenCost * 0.6")
    };
    let target_expression=format!(
        "bodyEvidence * 3 + symbolEvidence * 2 + pathEvidence + domainEvidence * 2 + productiveEvidence * 3 + novelty * {novelty_weight} - repeatPenalty * {repeat_weight} - sourceCost"
    );
    let anchors=wasted.iter().take(4).filter_map(|row|row["id"].as_str())
        .map(str::to_string).collect::<Vec<_>>();
    Some(json!({"kind":"selfimprove.search","name":format!("autonomous-{approach}-v1"),
        "implementation":"expression","source":"rsi-generated",
        "description":format!(
            "Engine-generated hypothesis from {} observed failed attempts: {}. Test different code targets and preserve the verified evaluator. Proposal, not measured improvement.",
            wasted.len(),approach),
        "parameters":{"expression":domain_expression,
            "targetExpression":target_expression},
        "hypothesisEvidence":{"class":approach,
            "wastedGenerations":wasted.len(),"regressions":regressions,
            "stalledWithoutCandidate":stalled,"repeatedFiles":repeated,
            "generationIds":anchors}}))
}

/// Never seed a second candidate over a pending or previously tried campaign
/// candidate. The generated strategy is claimed through the same durable
/// search-trial mechanism as a model-authored proposal.
pub(super) fn propose_autonomous_search_trial(
    state:&AppState,history:&[Value],objective:&str,campaign_id:&str,attempt:u64,auto_focus:bool
)->Value {
    if !auto_focus && eligible_domains(objective).len()==1 {
        return json!({"status":"user_pinned_search_target"});
    }
    let owned=state.store.all("algorithm").into_iter().filter(|row|
        row["kind"]=="selfimprove.search"
        && row["source"]=="rsi-generated"
        && row["rsiTrial"]["campaignId"]==campaign_id).collect::<Vec<_>>();
    if owned.iter().any(|row|matches!(row["rsiTrial"]["status"].as_str(),
        Some("pending_trial"|"pending_second_trial"))) {
        return json!({"status":"existing_trial"});
    }
    // A failed or inconclusive experiment must not permanently block
    // subsequent strategies. Keep the number of live hypotheses bounded.
    if owned.len()>=3 {
        return json!({"status":"campaign_trial_budget_exhausted","trialCount":owned.len()});
    }
    let failed=owned.iter().filter(|row|matches!(row["rsiTrial"]["status"].as_str(),
        Some("trial_inconclusive"|"trial_rejected"|"trial_supported"))).collect::<Vec<_>>();
    let Some(mut hypothesis)=autonomous_search_hypothesis(history) else {
        return json!({"status":"insufficient_failure_evidence"});
    };
    if !auto_focus {
        if let Some(parameters)=hypothesis["parameters"].as_object_mut() {
            parameters.remove("targetExpression");
        }
        hypothesis["hypothesisEvidence"]["targetMode"]=json!("owner_pinned");
    }
    // A rejected policy must alter the *next* search hypothesis instead of
    // causing the same formula to be registered again across new campaigns.
    let prior_rejections=state.store.all("algorithm").into_iter()
        .filter(|row|row["kind"]=="selfimprove.search"
            && row["name"].as_str().is_some_and(|n|n.starts_with("autonomous-"))
            && row["rsiTrial"]["status"]=="trial_rejected")
        .count().min(4);
    if prior_rejections>0 {
        if auto_focus {
            let original=hypothesis["parameters"]["targetExpression"].as_str().unwrap_or("");
            let revised=format!("{original} + productiveEvidence * {} - repeatPenalty * {}",
                2*prior_rejections,prior_rejections);
            hypothesis["parameters"]["targetExpression"]=json!(revised);
        } else {
            let original=hypothesis["parameters"]["expression"].as_str().unwrap_or("");
            hypothesis["parameters"]["expression"]=json!(format!(
                "{original} + exploration * {} - barrenRate * {}",
                prior_rejections,prior_rejections
            ));
        }
        hypothesis["hypothesisEvidence"]["previousRejectedPolicies"]=json!(prior_rejections);
    }
    // Learn from the failed local policy experiment. A retry must explore a
    // genuinely different executable search formula, not requeue an
    // inconclusive candidate under its former name and revision.
    if !failed.is_empty() {
        let n=failed.len() as u64;
        let source_scoring_unavailable=failed.iter().any(|row|
            row["rsiTrial"]["lastReason"]=="target_formula_not_executed");
        if auto_focus && source_scoring_unavailable {
            // The registered source-ranking formula never ran. A domain-only
            // retry can still execute and generate a patch instead of
            // repeatedly demanding an unavailable indexed source receipt.
            hypothesis["parameters"].as_object_mut().unwrap().remove("targetExpression");
            let old=hypothesis["parameters"]["expression"].as_str().unwrap_or("");
            hypothesis["parameters"]["expression"]=json!(format!(
                "{old} + exploration * {} + successRate * {}",3*n,n));
            hypothesis["hypothesisEvidence"]["recoveryMode"]=
                json!("domain_only_after_unexecuted_target");
        } else if auto_focus {
            let old=hypothesis["parameters"]["targetExpression"].as_str().unwrap_or("");
            hypothesis["parameters"]["targetExpression"]=json!(format!(
                "{old} + novelty * {} + productiveEvidence * {} - repeatPenalty * {}",
                3*n,n,2*n));
            hypothesis["hypothesisEvidence"]["recoveryMode"]=json!("novel_source_exploration");
        } else {
            let old=hypothesis["parameters"]["expression"].as_str().unwrap_or("");
            hypothesis["parameters"]["expression"]=json!(format!(
                "{old} + exploration * {} - barrenRate * {}",3*n,2*n));
            hypothesis["hypothesisEvidence"]["recoveryMode"]=json!("domain_exploration");
        }
        hypothesis["hypothesisEvidence"]["previousFailedTrials"]=json!(n);
        hypothesis["hypothesisEvidence"]["previousTrialReasons"]=json!(failed.iter()
            .map(|row|row["rsiTrial"]["lastReason"].as_str()
                .or_else(||row["rsiTrial"]["lastComparison"]["reason"].as_str())
                .unwrap_or("unknown")).collect::<Vec<_>>());
    }
    // Content-addressed IDs are campaign-specific: two independent trials
    // must not share mutable ownership just because their formulas match.
    hypothesis["name"]=json!(format!("{}-{}-trial{}",
        hypothesis["name"].as_str().unwrap_or("autonomous"),campaign_id,owned.len()+1));
    let mut candidate=match crate::engine::algorithms::register(state,&hypothesis) {
        Ok(candidate)=>candidate,
        Err(error)=>return json!({"status":"invalid_hypothesis","error":error})
    };
    // Store the source-generation receipts with the executable candidate.
    // Later generations must be able to inspect *why* this search formula
    // existed without depending on a transient model prompt.
    candidate["hypothesisEvidence"]=hypothesis["hypothesisEvidence"].clone();
    if state.store.put("algorithm",&candidate).is_none() {
        return json!({"status":"hypothesis_not_persisted"});
    }
    let active=crate::engine::algorithms::active(state,"selfimprove.search")
        .unwrap_or_else(||json!({"implementation":"failure-clusters","parameters":{}}));
    let mut receipt=queue_search_trial(state,&candidate,&active,history,objective,campaign_id,attempt);
    receipt["origin"]=json!("engine_synthesized_failure_hypothesis");
    receipt["hypothesisEvidence"]=hypothesis["hypothesisEvidence"].clone();
    receipt
}

/// Returns a candidate that is waiting for a real trial under the same
/// objective. It is used only inside an already scheduled generation; the
/// caller never creates another campaign or generation for it.
pub(super) fn pending_trial_candidate(state: &AppState, objective: &str, allowed_campaigns:&[String]) -> Option<Value> {
    let objective_hash = crate::engine::memory::stable_id(&[objective]);
    let mut candidates = state
        .store
        .all("algorithm")
        .into_iter()
        .filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["objectiveHash"] == objective_hash
                && trial_owned_by(algorithm,allowed_campaigns)
                && matches!(algorithm["rsiTrial"]["status"].as_str(),
                    Some("pending_trial"|"pending_second_trial"))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|algorithm| {
        algorithm["rsiTrial"]["createdAt"]
            .as_str()
            .unwrap_or("")
            .to_string()
    });
    candidates.into_iter().next()
}

/// Finds a prior generation that used the globally active strategy under the
/// same generator commit and target domain. Different domains, different
/// source revisions, a missing evaluation or a control already paired with
/// this candidate are not comparable.
pub(super) fn comparable_trial_baseline(
    prior_history: &[Value],
    generation: &Value,
    candidate: &Value,
) -> Option<Value> {
    prior_history
        .iter()
        .rev()
        .find(|row| {
            row["domainSelection"].as_str() == Some("algorithm_active")
                && row["searchStrategy"]["pendingCandidateId"] == candidate["id"]
                && row["generatorCommit"] == generation["generatorCommit"]
                && row["domain"] == generation["domain"]
                && row["accepted"].is_boolean()
                && row["evaluation"].is_object()
                && row["jobId"].as_str().is_some_and(|id| !id.is_empty())
                && !candidate["rsiTrial"]["trials"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|trial| trial["comparison"]["baselineJobId"] == row["jobId"])
        })
        .cloned()
}

/// Evidence from the same frozen experiment under two search policies.
/// A code-adoption flag or a cheaper failed generation cannot prove that the
/// algorithm found a better target. Missing or mismatched reports are unknown.
pub(super) fn paired_trial_effect(generation:&Value,baseline:&Value)->Option<(u64,u64,u64)> {
    for key in ["generatorCommit","campaignId","objectiveId","generatorModel","generatorProviderId"] {
        let expected=generation[key].as_str().filter(|v|!v.is_empty())?;
        if baseline[key].as_str()!=Some(expected){return None;}
    }
    if generation["domain"]!=baseline["domain"]{return None;}
    let trial=generation["evaluation"]["metrics"]["measurements"].as_array()?;
    let control=baseline["evaluation"]["metrics"]["measurements"].as_array()?;
    if trial.len()<8 || trial.len()!=control.len(){return None;}
    let mut seen=BTreeSet::new();
    let mut holdouts=0;
    let mut wins=0;
    let mut losses=0;
    for row in trial {
        let id=row["id"].as_str().filter(|v|!v.is_empty())?;
        if !seen.insert(id){return None;}
        let source=control.iter().find(|r|r["id"]==id)?;
        let holdout=row["holdout"].as_bool()?;
        let prior=row["baselinePass"].as_bool()?;
        let candidate_pass=row["candidatePass"].as_bool()?;
        let control_pass=source["candidatePass"].as_bool()?;
        if source["holdout"].as_bool()!=Some(holdout)
            || source["domain"]!=row["domain"]
            || source["baselinePass"].as_bool()!=Some(prior) {return None;}
        let mut row_won=candidate_pass && !control_pass;
        let mut row_lost=!candidate_pass && control_pass;
        // Quality improvements can matter even when both versions pass.
        // Compare signed metric results only when the frozen baseline and
        // metric directions agree on this exact case.
        let measures=row["metrics"].as_array()?;
        let previous=source["metrics"].as_array()?;
        if measures.len()!=previous.len(){return None;}
        let mut keys=BTreeSet::new();
        for metric in measures {
            let key=metric["key"].as_str().filter(|v|!v.is_empty())?;
            if !keys.insert(key){return None;}
            let former=previous.iter().find(|m|m["key"]==key)?;
            if metric["direction"]!=former["direction"]
                || metric["baseline"]!=former["baseline"]{return None;}
            let gain=metric["improved"].as_bool()?;
            let regression=metric["regressed"].as_bool()?;
            let prior_gain=former["improved"].as_bool()?;
            let prior_regression=former["regressed"].as_bool()?;
            if gain && regression || prior_gain && prior_regression {return None;}
            if candidate_pass && control_pass {
                row_won |= gain && !prior_gain && !prior_regression;
                row_lost |= regression || (!gain && prior_gain);
            }
        }
        if holdout {
            holdouts+=1;
            wins+=u64::from(row_won && !row_lost);
            losses+=u64::from(row_lost);
        }
    }
    if holdouts<2{return None;}
    Some((wins,losses,holdouts))
}

pub(super) fn trial_verdict(generation: &Value, baseline: &Value) -> (String, Value) {
    let trial_accepted = generation["accepted"] == true;
    let baseline_accepted = baseline["accepted"] == true;
    let trial_regressed = generation["evaluation"]["verdict"] == "regressed";
    let trial_error = generation["error"].as_str().is_some_and(|error| !error.is_empty());
    let baseline_error = baseline["error"].as_str().is_some_and(|error| !error.is_empty());
    let tokens = generation["tokens"].as_u64().unwrap_or(0) as f64;
    let baseline_tokens = baseline["tokens"].as_u64().unwrap_or(0) as f64;
    let elapsed = generation["elapsedMs"].as_u64().unwrap_or(0) as f64;
    let baseline_elapsed = baseline["elapsedMs"].as_u64().unwrap_or(0) as f64;
    let token_ratio = if baseline_tokens > 0.0 {
        tokens / baseline_tokens
    } else {
        1.0
    };
    let time_ratio = if baseline_elapsed > 0.0 {
        elapsed / baseline_elapsed
    } else {
        1.0
    };
    let proof=paired_trial_effect(generation,baseline);
    let (holdout_wins,holdout_losses,holdout_cases)=proof.unwrap_or((0,0,0));
    let verdict = if trial_regressed {
        "rejected"
    } else if trial_error && !baseline_error {
        "rejected"
    } else if trial_error || proof.is_none() {
        "inconclusive"
    } else if holdout_losses > 0 {
        "rejected"
    } else if trial_accepted && holdout_wins > 0 {
        "supported"
    } else if !trial_accepted && baseline_accepted {
        "rejected"
    } else if trial_accepted
        && baseline_accepted
        && tokens > 0.0
        && baseline_tokens > 0.0
        && elapsed > 0.0
        && baseline_elapsed > 0.0
        && token_ratio <= 0.9
        && time_ratio <= 1.1
    {
        "supported"
    } else if token_ratio >= 1.25 && time_ratio >= 1.25 {
        "rejected"
    } else {
        "inconclusive"
    };
    (
        verdict.to_string(),
        json!({
            "baselineJobId": baseline["jobId"],
            "baselineAttempt": baseline["attempt"],
            "baselineAccepted": baseline_accepted,
            "trialAccepted": trial_accepted,
            "matchedFrozenCases":proof.is_some(),
            "holdoutCases":holdout_cases,
            "holdoutWins":holdout_wins,
            "holdoutRegressions":holdout_losses,
            "baselineVerdict": baseline["evaluation"]["verdict"],
            "trialVerdict": generation["evaluation"]["verdict"],
            "baselineTokens": baseline_tokens,
            "trialTokens": tokens,
            "tokenRatio": token_ratio,
            "baselineElapsedMs": baseline_elapsed,
            "trialElapsedMs": elapsed,
            "timeRatio": time_ratio,
            "trialError": generation["error"],
            "reason": if trial_error && !baseline_error {
                "trial generation failed where the baseline did not"
            } else if trial_regressed {
                "trial generation regressed"
            } else if trial_error {
                "both generations failed; lower cost is not successful improvement"
            } else if proof.is_none() {
                "frozen paired observations or experiment identity missing"
            } else if holdout_losses > 0 {
                "candidate search regressed on previously held-out cases"
            } else if trial_accepted && holdout_wins > 0 {
                "candidate search improved frozen cases compared with control"
            } else if !trial_accepted && baseline_accepted {
                "baseline produced an accepted change where the trial did not"
            } else if verdict == "supported" {
                "both generations produced accepted changes with lower measured token cost"
            } else if token_ratio >= 1.25 && time_ratio >= 1.25 {
                "trial used more tokens and time with no better result"
            } else {
                "conditions or measurements do not separate the two strategies"
            },
        }),
    )
}

/// Appends the real trial outcome to the candidate and, only after two
/// independent supported trials with no rejected or inconclusive trial,
/// activates it. A single trial, replay match, tool call or registration never
/// counts as adoption.
pub(super) fn record_trial_outcome(
    state: &AppState,
    candidate_id: &str,
    generation: &Value,
    prior_history: &[Value],
) -> Value {
    let Some(mut candidate) = state.store.get("algorithm", candidate_id) else {
        return json!({"status":"candidate_missing"});
    };
    if generation["domainSelection"].as_str() != Some("algorithm_trial") {
        return json!({"status":"not_used","domainSelection":generation["domainSelection"],
            "reason":"이 세대는 후보 전략 계산식을 실행하지 않았습니다."});
    }
    if let Some(previous) = candidate["rsiTrial"]["trials"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["jobId"] == generation["jobId"])
    {
        return json!({"status":candidate["rsiTrial"]["status"],
            "verdict":previous["verdict"],"comparison":previous["comparison"],
            "alreadyRecorded":true});
    }
    // A targetExpression is not a successful trial just because the model
    // produced a patch. It must have actually scored source candidates at its
    // registered revision, with no per-file or global formula fallback.
    if candidate["parameters"]["targetExpression"].is_string() {
        let selection=&generation["focusSelection"];
        let rows=selection["candidates"].as_array();
        let executed=matches!(selection["source"].as_str(),
                Some("indexed_evidence"|"descendant_indexed_evidence"))
            && selection["policyRevision"]==candidate["revision"]
            && rows.is_some_and(|rows|!rows.is_empty()
                && rows.iter().all(|row|row["scoring"]=="expression"
                    && row["policyError"].is_null()));
        if !executed {
            let mut trial=candidate["rsiTrial"].clone();
            if !trial["trials"].is_array() {trial["trials"]=json!([]);}
            trial["status"]=json!("trial_inconclusive");
            trial["lastReason"]=json!("target_formula_not_executed");
            trial["trials"].as_array_mut().unwrap().push(json!({
                "jobId":generation["jobId"],"attempt":generation["attempt"],
                "verdict":"inconclusive","reason":"target_formula_not_executed",
                "at":chrono::Utc::now().to_rfc3339()
            }));
            candidate["rsiTrial"]=trial;
            state.store.put("algorithm",&candidate);
            state.store.audit("rsi.search_trial_invalid_target",
                json!({"candidateId":candidate_id,"jobId":generation["jobId"],
                    "reason":"target_formula_not_executed"}));
            return json!({"status":"inconclusive",
                "reason":"target_formula_not_executed"});
        }
    }
    let Some(baseline) = comparable_trial_baseline(prior_history, generation, &candidate) else {
        let mut trial = candidate["rsiTrial"].clone();
        trial["status"] = json!("trial_inconclusive");
        trial["lastReason"] = json!("comparable_baseline_missing");
        if !trial["trials"].is_array() {
            trial["trials"] = json!([]);
        }
        trial["trials"].as_array_mut().unwrap().push(json!({
            "jobId": generation["jobId"], "attempt": generation["attempt"],
            "domain": generation["domain"], "verdict": "inconclusive",
            "reason": "same generator commit, domain and active-strategy baseline이 없습니다.",
            "at": chrono::Utc::now().to_rfc3339(),
        }));
        candidate["rsiTrial"] = trial;
        state.store.put("algorithm", &candidate);
        return json!({"status":"inconclusive","reason":"comparable_baseline_missing"});
    };
    let (verdict, comparison) = trial_verdict(generation, &baseline);
    let mut trial = candidate["rsiTrial"].clone();
    if !trial["trials"].is_array() {
        trial["trials"] = json!([]);
    }
    trial["trials"].as_array_mut().unwrap().push(json!({
        "jobId": generation["jobId"], "attempt": generation["attempt"],
        "domain": generation["domain"], "verdict": verdict,
        "comparison": comparison, "at": chrono::Utc::now().to_rfc3339(),
    }));
    let trials = trial["trials"].as_array().cloned().unwrap_or_default();
    // Old records may contain repeated pairs. They cannot satisfy the two-trial gate.
    let mut baseline_ids = BTreeSet::new();
    let mut trial_ids = BTreeSet::new();
    let supported = trials.iter().filter(|row| {
        let Some(baseline_id) = row["comparison"]["baselineJobId"].as_str().filter(|id| !id.is_empty()) else {
            return false;
        };
        let Some(trial_id) = row["jobId"].as_str().filter(|id| !id.is_empty()) else {
            return false;
        };
        if row["verdict"] != "supported"
            || baseline_ids.contains(baseline_id)
            || trial_ids.contains(trial_id)
        {
            return false;
        }
        baseline_ids.insert(baseline_id);
        trial_ids.insert(trial_id);
        true
    }).count();
    let rejected = trials.iter().filter(|row| row["verdict"] == "rejected").count();
    let inconclusive = trials.iter().filter(|row| row["verdict"] == "inconclusive").count();
    let status = match verdict.as_str() {
        "rejected" => "trial_rejected",
        "supported" if supported >= 2 && rejected == 0 && inconclusive == 0 => "adopted",
        "supported" => "pending_second_trial",
        _ => "trial_inconclusive",
    };
    let mut status = status.to_string();
    trial["status"] = json!(status);
    trial["lastVerdict"] = json!(verdict);
    trial["lastComparison"] = comparison.clone();
    if status == "adopted" {
        match crate::engine::algorithms::activate(state, candidate_id) {
            Ok(activated) => {
                trial["activatedAt"] = activated["activatedAt"].clone();
                trial["activatedRevision"] = activated["revision"].clone();
                // Reuse the activated record so the later save does not write
                // the stale pre-activation status back over it.
                candidate = activated;
            }
            Err(error) => {
                status = "trial_supported".to_string();
                trial["status"] = json!(status);
                trial["activationError"] = json!(error);
            }
        }
    }
    candidate["rsiTrial"] = trial;
    let saved = state.store.put("algorithm", &candidate);
    if let Some(saved) = &saved {
        state.bus.record("algorithm", saved);
    }
    state.store.audit(
        "rsi.search_trial_result",
        json!({
            "candidateId": candidate_id,
            "jobId": generation["jobId"],
            "verdict": verdict,
            "status": status,
            "supportedTrials": supported,
            "rejectedTrials": rejected,
            "inconclusiveTrials": inconclusive,
        }),
    );
    json!({
        "status": status,
        "verdict": verdict,
        "candidateId": candidate_id,
        "comparison": comparison,
        "supportedTrials": supported,
        "rejectedTrials": rejected,
        "inconclusiveTrials": inconclusive,
    })
}

/// Re-reads only the stored line range from the current source and formats it
/// exactly like `workspace_read_range`. It does not fabricate a model read.
