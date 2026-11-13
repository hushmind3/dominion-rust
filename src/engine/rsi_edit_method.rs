//! Learned edit-method policy for RSI generations.
//!
//! A generation earns an edit reward only when its written policy rule actually
//! executed and an independent paired evaluation accepted the produced source.

use crate::engine::rsi_evolve::{research_scope, target_focus_objective};
use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn executed_policy_rule(generation:&Value)->bool {
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
pub(crate) fn edit_method_reward(generation:&Value)->f64 {
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
pub(crate) fn synthesize_edit_method(history:&[Value])->Value {
    // DOMINION now composes new runnable edit/verification controllers from
    // observed outcomes. It does not pick from the old three named modes.
    crate::engine::rsi_edit_genome::evolve(history,edit_method_reward)
}

/// Learn across completed generations and follow-up campaigns for the same
/// source partition and exact objective. A different repository, task or
/// merely proposed action cannot influence this job's improvement policy.
pub(crate) fn learned_edit_method(state:&AppState,input:&Value,history:&[Value])->Value {
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
pub(crate) fn history_with_verified_payoffs(state:&AppState,history:&[Value])->Vec<Value>{
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

