//! editable search policy prediction and evaluation.
//! Only policy_planner owns these implementations. Other RSI stages use explicit
//! names imported through the stable rsi_evolve facade.
use super::{AppState, BTreeSet, Value, json, research_scope, run, select_domain, target_focus_objective};
pub(super) fn search_strategy(algorithm: &Value, mode: &str) -> Value {
    json!({
        "id": algorithm["id"],
        "revision": algorithm["revision"],
        "name": algorithm["name"],
        "implementation": algorithm["implementation"],
        "mode": mode,
    })
}

/// An improvement method, not a model: each executable strategy changes how
/// DOMINION allocates the source-observation budget and which editing tool it
/// presents first. Selection is re-estimated from actual *prior* executions.
/// Candidate creation is useful intermediate evidence, never equal to quality
/// improvement or permission to adopt an unverified policy.
/// A learned Boolean decision can earn a source-improvement reward only if
/// that exact rule really ran in the descendant responsible for the patch.
pub(super) fn executed_policy_rule(generation:&Value)->bool {
    let Some(rules)=generation["editPolicy"]["rules"].as_array() else {return true;};
    if rules.is_empty(){return true;}
    let Some(events)=generation["ruleExecutions"].as_array() else {return false;};
    let Some(operations)=generation["toolTimeline"].as_array() else {return false;};
    events.iter().any(|event|{
        if event["policyRevision"]!=generation["editPolicy"]["revision"]
            || !rules.iter().any(|rule|rule["id"]==event["ruleId"]) {
            return false;
        }
        let Some(run)=event["runId"].as_str().filter(|s|!s.is_empty()) else {return false;};
        let Some(before)=event["opSeqBefore"].as_u64() else {return false;};
        // The generated action must actually have occurred *after* this rule
        // fired, and a verified source write must follow in the same run.
        // A rule observed after a patch cannot steal that patch's reward.
        let acted=operations.iter().any(|op|{
            op["runId"].as_str()==Some(run)
                && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                && op["status"]=="completed"
                && event["allow"].as_array().is_some_and(|allowed|
                    allowed.iter().any(|tool|tool==&op["tool"]))
        });
        let patched=operations.iter().any(|op|{
            // Match the final accepted file content. A verified edit to some
            // other file or an intermediate overwritten version is not the
            // rule's independently accepted implementation.
            let matches_adopted_source=generation["acceptedSourceHashes"].as_object()
                .and_then(|files|op["path"].as_str().and_then(|path|files.get(path)))
                .and_then(Value::as_str)
                .is_some_and(|hash|hash.len()==64 &&
                    op["result"]["postSha256"].as_str()==Some(hash));
            op["runId"].as_str()==Some(run)
                && op["opSeq"].as_u64().is_some_and(|seq|seq>before)
                && matches!(op["tool"].as_str(),
                    Some("workspace_replace"|"workspace_replace_range"|"workspace_write"))
                && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["toolReceipt"]["verified"]==true
                && matches_adopted_source
        });
        acted && patched
    })
}
pub(super) fn edit_method_reward(generation:&Value)->f64 {
    // A native source operator did not execute the model's editing policy,
    // even if an earlier unproductive model turn consumed tokens.
    if generation["candidateOrigin"]=="native_inductive_source_mutation" {
        return 0.0;
    }
    if !executed_policy_rule(generation) {return 0.0;}
    if generation["accepted"]==true
        && generation["evidenceVerified"]==true
        && generation["evaluation"]["metrics"]["eligible"]==true {
        return 1.0;
    }
    if generation["candidateId"].is_string()
        && generation["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
        && generation["evaluation"]["metrics"].is_object() {
        // A neutral independent paired result only proves that the system
        // completed an experiment. It does not establish better quality.
        return 0.10;
    }
    // Editing a file, generating a candidate or claiming success without an
    // independent evaluator is not a self-improvement payoff.
    0.0
}

/// A bounded UCB optimizer over executable improvement methods. Each input
/// observation must have an actual worker job, measured tokens, a recorded
/// policy and a generation outcome. No model prose or replayed hypothesis
/// supplies a reward. Untried methods get exploration priority so failure
/// can alter the *next algorithm*, not just the next explanation.
pub(super) fn synthesize_edit_method(history:&[Value])->Value {
    // DOMINION now composes new runnable edit/verification controllers from
    // observed outcomes. It does not pick from the old three named modes.
    crate::engine::rsi_edit_genome::evolve(history,edit_method_reward)
}

/// Learn across completed generations and follow-up campaigns for the same
/// source partition and exact objective. A different repository, task or
/// merely proposed action cannot influence this job's improvement policy.
pub(super) fn learned_edit_method(state:&AppState,input:&Value,history:&[Value])->Value {
    let objective=crate::engine::memory::stable_id(&[target_focus_objective(input)]);
    let partition=research_scope(input).partition_key;
    let mut seen=BTreeSet::<String>::new();
    let mut rows=Vec::new();
    let persisted=state.store.all("rsi_generation");
    for row in history.iter().chain(persisted.iter()) {
        if row["objectiveId"]!=objective || row["knowledgePartition"]!=partition {
            continue;
        }
        let Some(job)=row["jobId"].as_str().filter(|id|!id.is_empty()) else {continue};
        if seen.insert(job.to_string()) {rows.push(row.clone());}
    }
    // Store iteration is newest-first, while in-campaign history is oldest-
    // first. Older generation rows did not record createdAt, so comparing
    // empty timestamps fed the OLDEST (not latest) failure to the mutation
    // operator after a restart. The persisted child job has a real queuedAt;
    // use it as the historical fallback instead of insertion order.
    rows.sort_by_cached_key(|row| {
        row["createdAt"].as_str().map(str::to_string)
            .or_else(||row["jobId"].as_str()
                .and_then(|id|state.store.get("job",id))
                .and_then(|job|job["queuedAt"].as_str().map(str::to_string)))
            .unwrap_or_default()
    });
    // Proved payoff requires the original candidate's independent paired
    // receipt, matching adopted commit and verified source lineage.
    let verified=history_with_verified_payoffs(state,&rows);
    let mut policy=synthesize_edit_method(&verified);
    policy["objectiveId"]=json!(objective);
    policy["knowledgePartition"]=json!(partition);
    policy
}

/// Only raw, candidate-scoped, locally revalidated paired receipts may be
/// used as positive exploration rewards. A forged summary, replay match or
/// failed generation never gets a measured reward.
pub(super) fn history_with_verified_payoffs(state:&AppState,history:&[Value])->Vec<Value>{
    let mut seen=BTreeSet::new();
    history.iter().map(|row|{
        let mut enriched=row.clone();
        enriched["evidenceVerified"]=json!(false);
        enriched["verifiedHoldoutGains"]=json!(0);
        if row["accepted"]!=true {return enriched;}
        let Some(id)=row["candidateId"].as_str().filter(|id|seen.insert(id.to_string())) else {
            return enriched;
        };
        let Some(candidate)=state.store.get("rsi_candidate",id) else {return enriched;};
        let valid=candidate["status"]=="verified"
            && candidate["baseCommit"]==row["generatorCommit"]
            && candidate["experimentAdoption"]["commit"]==row["acceptedCommit"]
            && candidate["evaluation"]["source"]=="server_invoked_local_paired_git_worktrees"
            && candidate["evaluation"]["metrics"]["eligible"]==true
            && crate::engine::rsi::verified_pair_receipt(state,&candidate);
        if valid {
            enriched["evidenceVerified"]=json!(true);
            enriched["verifiedHoldoutGains"]=candidate["evaluation"]["metrics"]["improvedHoldouts"].clone();
        }
        enriched
    }).collect()
}

pub(super) fn select_domain_for_state(state: &AppState, history: &[Value], objective: &str) -> String {
    let algorithm = crate::engine::algorithms::active(state, "selfimprove.search")
        .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
    select_domain(&algorithm, &history_with_verified_payoffs(state,history), objective)
}

/// Replays recorded generations with two search algorithms. A hit means the
/// algorithm would have chosen the domain that later produced an accepted
/// change in that same campaign. Sparse history is not evidence, so promotion
/// stays disabled until at least two accepted generations exist.
/// Replays recorded generations with two search algorithms and reports only
/// how often each would have chosen the domain that later produced an accepted
/// change. This is a *past successful-domain match rate*, not evidence that
/// the candidate improves real development outcomes. The score never activates
/// an algorithm and never promotes one; callers must use it only to decide
/// whether a candidate is worth a bounded real trial in a scheduled generation.
pub fn evaluate_search_algorithm(
    candidate: &Value,
    active: &Value,
    history: &[Value],
    objective: &str,
) -> Value {
    let mut opportunities = 0u64;
    let mut candidate_hits = 0u64;
    let mut active_hits = 0u64;
    for index in 1..history.len() {
        if history[index]["accepted"] != true {
            continue;
        }
        let Some(actual) = history[index]["domain"].as_str().filter(|value| !value.is_empty())
        else {
            continue;
        };
        let prior = &history[..index];
        opportunities += 1;
        if select_domain(candidate, prior, objective) == actual {
            candidate_hits += 1;
        }
        if select_domain(active, prior, objective) == actual {
            active_hits += 1;
        }
    }
    let rate = |hits: u64| {
        if opportunities == 0 {
            0.0
        } else {
            hits as f64 / opportunities as f64
        }
    };
    json!({
        "kind": "selfimprove.search",
        "metric": "past_successful_domain_match_rate",
        "candidateId": candidate["id"],
        "candidateRevision": candidate["revision"],
        "activeId": active["id"],
        "activeRevision": active["revision"],
        "decisions": history.len().saturating_sub(1),
        "samples": opportunities,
        "candidateMatches": candidate_hits,
        "activeMatches": active_hits,
        "candidateMatchRate": rate(candidate_hits),
        "activeMatchRate": rate(active_hits),
        "evaluable": opportunities >= 2,
        "interpretation": "과거에 채택된 세대의 분야를 얼마나 맞혔는지일 뿐, 실제 개발 성과나 능력 향상의 증거가 아닙니다.",
        "source": "replay_of_recorded_rsi_generations",
        "generatedAt": chrono::Utc::now().to_rfc3339(),
    })
}
