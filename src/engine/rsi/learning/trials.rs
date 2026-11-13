//! learning trials: compiled descendant RSI responsibility.
use super::*;

/// Algorithms proposed by the current RSI generation through the
/// `algorithm_propose` tool. Only the stored records are returned; a proposal
/// is never treated as adopted before deterministic replay evaluation.
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

/// Propose a genuinely new executable search formula without an extra model
/// call. The weights are recomputed from measured prior generations, not a
/// fixed list of named policies. This is hypothesis generation, not proof:
/// queue_search_trial still requires real future trial outcomes to activate it.
pub(super) fn adaptive_search_expression(history:&[Value])->Option<String>{
    if history.len()<4 {return None;}
    let distinct=history.iter().filter_map(|row|row["domain"].as_str())
        .collect::<BTreeSet<_>>();
    if distinct.len()<2 {return None;}
    let measured=history.iter().filter(|row|row["evidenceVerified"]==true)
        .collect::<Vec<_>>();
    let gains=measured.iter().map(|row|
        row["verifiedHoldoutGains"].as_u64().unwrap_or(0).min(64)).sum::<u64>();
    if gains==0 {return None;}
    let failures=history.iter().filter(|row|row["accepted"]!=true
        && row["newFindings"].as_u64().unwrap_or(0)==0).count() as f64;
    let attempts=history.len() as f64;
    let fail_rate=failures/attempts;
    let measured_rate=gains as f64/(measured.len() as f64+1.0);
    let avg_tokens=history.iter().map(|row|row["tokens"].as_u64().unwrap_or(0))
        .sum::<u64>() as f64/attempts;
    let explore=(1.0+fail_rate).min(2.0);
    let gain=(1.0+measured_rate).min(4.0);
    let efficiency=(0.8+measured_rate*0.5).min(3.0);
    let barren=1.0+fail_rate*2.0;
    let cost=0.25+(avg_tokens/20000.0).min(1.0);
    Some(format!(
        "objectiveMatch * 2 + exploration * {explore:.3} + holdoutGains * {gain:.3} + gainPer10kTokens * {efficiency:.3} - barrenRate * {barren:.3} - tokenCost * {cost:.3}"
    ))
}

/// Without a model-authored candidate, build one from accumulated measured
/// outcomes and put it through the exact same bounded, non-promoting trial
/// path. Never learn from mutable summary fields alone.
/// A barren campaign still needs a *different executable search decision*
/// before it can learn how to improve. Only actual generation attempts with
/// distinct job IDs count; unverified accepted candidates cannot masquerade
/// as quality gains. This is exploratory recovery, never improvement proof.
pub(super) fn recovery_search_expression(history:&[Value])->Option<String>{
    let mut jobs=BTreeSet::new();
    let mut failed=Vec::new();
    for row in history {
        let Some(id)=row["jobId"].as_str().filter(|id|!id.is_empty()) else {continue};
        if row["accepted"]!=false
            || !row["candidateId"].is_null()
            || row["newFindings"].as_u64()!=Some(0)
            || !row["tokens"].as_u64().is_some_and(|tokens|tokens>0)
            || !row["domain"].as_str().is_some_and(|d|DOMAINS.contains(&d))
            || !jobs.insert(id)
        {continue;}
        failed.push(row);
    }
    if failed.len()<3 {return None;}
    let attempts=failed.len() as f64;
    let cost=failed.iter().map(|r|r["tokens"].as_u64().unwrap_or(0))
        .sum::<u64>() as f64 / (attempts*20000.0);
    let explore=(1.0+attempts/3.0).min(3.0);
    let novelty=(0.8+attempts/6.0).min(2.0);
    let barren=(1.0+attempts/2.0).min(4.0);
    Some(format!("objectiveMatch * 1.5 + exploration * {explore:.3} + novelty * {novelty:.3} - barrenRate * {barren:.3} - tokenCost * {:.3}",(0.3+cost).min(1.8)))
}

pub(super) fn queue_adaptive_search_trial(
    state:&AppState,history:&[Value],active:&Value,
    objective:&str,campaign_id:&str,attempt:u64,
)->Option<Value>{
    let enriched=history_with_verified_payoffs(state,history);
    let (formula,source)=if let Some(formula)=adaptive_search_expression(&enriched) {
        (formula,"verified_holdout_payoffs")
    } else {
        (recovery_search_expression(&enriched)?,"observed_unproductive_generations")
    };
    if active["parameters"]["expression"].as_str()==Some(formula.as_str()){
        return None;
    }
    let candidate=crate::engine::algorithms::register(state,&json!({
        "kind":"selfimprove.search",
        "name":"rsi-outcome-adaptive-search",
        "implementation":"expression",
        "parameters":{"expression":formula},
        "source":"rsi-generated",
        "description":"검증된 paired holdout 보상 또는 실제 실패 세대의 비용·탐색 경험으로 생성한 실행 가능한 탐색 가설. 독립적인 세대 실험 이전에는 개선 성과로 간주하지 않습니다."
    })).ok()?;
    let mut report=queue_search_trial(state,&candidate,active,history,objective,campaign_id,attempt);
    if report["status"]=="pending_trial"{
        report["hypothesisSource"]=json!(source);
        Some(report)
    }else{None}
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
    // Content-derived algorithm IDs are global. Reload current ownership
    // before writing; a stale proposal must not steal a sibling's trial.
    let latest = candidate["id"].as_str()
        .and_then(|id| state.store.get("algorithm", id))
        .unwrap_or_else(|| candidate.clone());
    let candidate = &latest;
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
    if let Some(owner) = candidate["rsiTrial"]["campaignId"].as_str() {
        if owner != campaign_id {
            return json!({"status":"owned_by_other_campaign",
                "candidateId":candidate["id"],"ownerCampaignId":owner});
        }
    }
    // The synthetic registration sample does not guarantee that division
    // or normalization works on the actual generation feature vector.
    if scored_domains(candidate, prior_history, objective).is_none() {
        let mut inconclusive = candidate.clone();
        inconclusive["rsiTrial"] = json!({
            "status":"trial_inconclusive","campaignId":campaign_id,
            "objectiveHash":crate::engine::memory::stable_id(&[objective]),
            "lastReason":"candidate_expression_invalid_for_runtime_features"
        });
        state.store.put("algorithm",&inconclusive);
        return json!({"status":"trial_inconclusive","candidateId":candidate["id"],
            "reason":"candidate_expression_invalid_for_runtime_features"});
    }
    let mut record = candidate.clone();
    let mut trial = record["rsiTrial"].clone();
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
    record["rsiTrial"] = trial;
    let saved = state.store.put("algorithm", &record);
    if let Some(saved) = &saved {
        state.bus.record("algorithm", saved);
    }
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

/// Returns a candidate that is waiting for a real trial under the same
/// objective. It is used only inside an already scheduled generation; the
/// caller never creates another campaign or generation for it.
pub(super) fn pending_trial_candidate(
    state: &AppState, objective: &str, allowed_campaigns: &[String],
) -> Option<Value> {
    let objective_hash = crate::engine::memory::stable_id(&[objective]);
    let mut candidates = state
        .store
        .all("algorithm")
        .into_iter()
        .filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["objectiveHash"] == objective_hash
                && algorithm["rsiTrial"]["campaignId"].as_str().is_some_and(|id|
                    allowed_campaigns.iter().any(|allowed| allowed == id))
                && matches!(
                    algorithm["rsiTrial"]["status"].as_str(),
                    Some("pending_trial" | "pending_second_trial")
                )
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
    let verdict = if trial_regressed {
        "rejected"
    } else if trial_error && !baseline_error {
        "rejected"
    } else if trial_error {
        "inconclusive"
    } else if trial_accepted && !baseline_accepted {
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
            } else if trial_accepted && !baseline_accepted {
                "trial produced an accepted change where the baseline did not"
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
    // A trial may improve the file even when its own formula failed and the
    // builtin selector took over. Such a fallback is not evidence that the
    // model-authored targetExpression improved source discovery.
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
            return json!({"status":"inconclusive","reason":"target_formula_not_executed"});
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

