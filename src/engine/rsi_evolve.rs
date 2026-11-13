//! Opt-in experimental evolution. The coordinator and its oracle stay frozen;
//! each accepted source revision is rebuilt and actually generates the next patch.
//! Only private detached worktrees are committed. Production refs never move.
use crate::{
    engine::{
        experience,
        jobs::{self, JobContext},
        memory::MemoryScope,
        rsi_worktree, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::process::Command;

#[path = "rsi/execution/start.rs"]
mod execution_start;
use self::execution_start::{elapsed, git, lineage_fingerprint, save, select, snapshot_source};
pub(crate) use self::execution_start::{command};
pub use self::execution_start::{choose, lineage_fingerprint_valid, run};

#[path = "rsi/planner/domain.rs"]
mod planner_domain;
use self::planner_domain::{candidate_domains, scored_domains, score_domain, search_features};

#[path = "rsi/planner/focus.rs"]
mod planner_focus;
use self::planner_focus::{choose_focus_files, focus_file_context, generation_context, identical_failed_patch, matching_prior_campaign, paired_behavioral_feedback, research_digest, target_focus_objective, verified_candidate_id};

#[path = "rsi/learning/methods.rs"]
mod learning_methods;
use self::learning_methods::{method_reuse_decision, record_method_feedback, research_artifacts, research_scope, selection_objective};
pub(crate) use self::learning_methods::{collect_method_executions, configure_method_session, method_strategies, propose_method, remember_research};

#[path = "rsi/learning/trials.rs"]
mod learning_trials;
use self::learning_trials::{adaptive_search_expression, comparable_trial_baseline, pending_trial_candidate, proposed_algorithms, queue_adaptive_search_trial, queue_search_trial, record_trial_outcome, recovery_search_expression, trial_verdict};

#[path = "rsi/learning/research.rs"]
mod learning_research;
pub(crate) use self::learning_research::{reuse_research};

#[path = "rsi/execution/context.rs"]
mod execution_context;
use self::execution_context::{fit_worker_prompt, generation_fits_remaining_budget, generation_prompt_variants, generation_stop_reason, new_research_findings, post_adoption_objective, session_research_fingerprint, session_tool_usage, should_stop_no_progress, unresolved_quality_case};

#[path = "rsi/execution/campaign.rs"]
mod execution_campaign;
use self::execution_campaign::{run_loop};

#[path = "rsi/lineage/adoption.rs"]
mod lineage_adoption;
use self::lineage_adoption::{compiled_system_target, generation_model_error, generation_target_plan, paired_descendant_target, proof_goal_reached, reconcile_descendant_target, select_generation_strategy};
pub(crate) use self::lineage_adoption::{adopt};

#[path = "rsi/planner/worker_plan.rs"]
mod planner_worker_plan;
pub use self::planner_worker_plan::{worker_plan};

#[path = "rsi/execution/worker.rs"]
mod execution_worker;
pub use self::execution_worker::{worker};

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "ui", "improver"];

pub(crate) fn objective_matches(domain: &str, objective: &str) -> bool {
    let text = objective.to_lowercase();
    let aliases: &[&str] = match domain {
        "memory" => &["기억", "회상", "메모리", "memory", "recall"],
        "context" => &["대화", "문맥", "컨텍스트", "토큰", "캐시", "context", "chat", "token", "cache"],
        "learning" => &["학습", "지식", "경험", "learning", "knowledge", "experience"],
        "tools" => &["도구", "검색", "외부 연결", "브라우저", "tools", "tool", "search", "browser"],
        "ui" => &["화면", "버튼", "사용성", "인터페이스", "ui", "interface"],
        "improver" => &["자기개선", "자기 개선", "알고리즘", "연구 전략", "improver", "rsi", "algorithm"],
        _ => &[],
    };
    aliases.iter().any(|alias| {
        if alias.is_ascii() {
            text.split(|ch: char| !ch.is_alphanumeric() && ch != '_').any(|word| word == *alias)
        } else {
            text.contains(alias)
        }
    })
}

#[cfg(test)]
#[path = "rsi_oracle_fixture.rs"]
mod oracle_tests;

pub fn limits(body: &Value) -> Result<Value, String> {
    let bound = |key: &str, default: u64, min: u64, max: u64| -> Result<u64, String> {
        let value = body
            .get(key)
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("{key}는 정수여야 합니다."))
            })
            .unwrap_or(Ok(default))?;
        if (min..=max).contains(&value) {
            Ok(value)
        } else {
            Err(format!("{key} 범위는 {min}~{max}입니다."))
        }
    };
    let token_budget = match body.get("tokenBudget") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value.as_u64().ok_or("tokenBudget은 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (1_000..=2_000_000).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "tokenBudget 범위는 1,000~2,000,000입니다. 0 또는 생략은 상한 없음입니다."
                        .into(),
                );
            }
        }
    };
    let time_budget = match body.get("timeBudgetSeconds") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or("timeBudgetSeconds는 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (60..=7200).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "timeBudgetSeconds 범위는 60~7200입니다. 0 또는 생략은 상한 없음입니다.".into(),
                );
            }
        }
    };
    Ok(json!({"maxAttempts":bound("maxAttempts",3,1,100)?,
        "tokenBudget":token_budget,
        "timeBudgetSeconds":time_budget}))
}

pub(crate) fn select_domain(algorithm: &Value, history: &[Value], objective: &str) -> String {
    let implementation=algorithm["implementation"].as_str()
        .or_else(||algorithm["variant"].as_str()).unwrap_or("failure-clusters");
    if !matches!(implementation,"expression"|"counterexample-first") {
        return fast_failure_domain(history,&candidate_domains(objective),
            |domain|objective_matches(domain,objective));
    }
    // An AI-authored expression can be invalid on real data despite passing
    // registration's representative sample. Fall back to a COMPLETE known
    // policy, rather than silently choosing the first domain ("memory").
    let scores = scored_domains(algorithm, history, objective)
        .or_else(|| scored_domains(&json!({"implementation":"failure-clusters"}),
            history, objective)).unwrap_or_default();
    let mut best = (f64::NEG_INFINITY, candidate_domains(objective)[0]);
    for (domain, score) in scores {
        if score > best.0 { best = (score, domain); }
    }
    best.1.to_string()
}

fn search_strategy(algorithm: &Value, mode: &str) -> Value {
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
fn edit_method_reward(generation:&Value)->f64 {
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
fn synthesize_edit_method(history:&[Value])->Value {
    // DOMINION now composes new runnable edit/verification controllers from
    // observed outcomes. It does not pick from the old three named modes.
    crate::engine::rsi_edit_genome::evolve(history,edit_method_reward)
}

/// Learn across completed generations and follow-up campaigns for the same
/// source partition and exact objective. A different repository, task or
/// merely proposed action cannot influence this job's improvement policy.
fn learned_edit_method(state:&AppState,input:&Value,history:&[Value])->Value {
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
fn history_with_verified_payoffs(state:&AppState,history:&[Value])->Vec<Value>{
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

fn select_domain_for_state(state: &AppState, history: &[Value], objective: &str) -> String {
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

pub fn auto_write_allowed(state: &AppState, session_id: &str, tool: &str, args: &Value) -> bool {
    if !matches!(tool, "workspace_write" | "workspace_replace" | "workspace_replace_range")
        || !args["path"]
            .as_str()
            .is_some_and(crate::engine::rsi::safe_path)
    {
        return false;
    }
    let Some(session) = state.store.get("session", session_id) else {
        return false;
    };
    let Some(id) = session["rsiCampaignId"].as_str() else {
        return false;
    };
    let Some(parent) = state.store.get("job", id) else {
        return false;
    };
    if parent["kind"] != "rsi_evolve"
        || parent["status"] != "running"
        || parent["cancelRequested"] == true
        || parent["input"]["rsiNativeApproved"] != true
    {
        return false;
    }
    let Some(child) = session["rsiGenerationJobId"]
        .as_str()
        .and_then(|id| state.store.get("job", id))
    else {
        return false;
    };
    child["kind"] == "rsi_generation"
        && child["input"]["rsiCampaignId"] == id
        && child["status"] == "running"
        && child["cancelRequested"] != true
        && child["rsiWorkspace"]
            .as_str()
            .is_some_and(|path| Path::new(path) == state.config.workspace)
}

pub fn campaign_source(state: &AppState, input: &Value, source: &Path, base: &str) -> bool {
    let Some(id) = input["rsiCampaignId"].as_str() else {
        return false;
    };
    state.store.get("rsi_campaign", id).is_some_and(|record| {
        record["labDir"]
            .as_str()
            .is_some_and(|p| Path::new(p) == source)
            && record["headCommit"] == base
            && record["status"] == "running"
            && record["nativeApproved"] == true
    })
}

pub fn remaining_output_tokens(
    state: &AppState,
    session_id: &str,
    prompt_estimate: u64,
) -> Result<Option<u64>, String> {
    let Some(id) = state
        .store
        .get("session", session_id)
        .and_then(|s| s["rsiCampaignId"].as_str().map(str::to_string))
    else {
        return Ok(None);
    };
    let parent = state
        .store
        .get("job", &id)
        .ok_or("자동 개선 작업이 없습니다.")?;
    if parent["kind"] != "rsi_evolve"
        || parent["status"] != "running"
        || parent["cancelRequested"] == true
    {
        return Err("자동 개선이 중단되었습니다.".into());
    }
    let Some(budget) = parent["input"]["limits"]["tokenBudget"]
        .as_u64()
        .filter(|budget| *budget > 0)
    else {
        // The owner explicitly asked not to cap self-improvement tokens. We
        // still return None so the provider's own context/output limits apply.
        return Ok(None);
    };
    let prior = parent["evolution"]["activeBaseTokens"]
        .as_u64()
        .unwrap_or(0);
    let spent = state.store.session_usage(session_id)["totalTokens"]
        .as_u64()
        .unwrap_or(0);
    let available =
        budget.saturating_sub(prior.saturating_add(spent).saturating_add(prompt_estimate));
    if available < 256 {
        return Err("rsi_token_budget: 자동 개선의 토큰 예산이 부족해 다음 모델 호출을 중단했습니다.".into());
    }
    Ok(Some(available.min(8192)))
}

#[cfg(test)]
#[path = "../../tests/unit/engine_rsi_evolve_paired_descendant_target_tests.rs"]
mod paired_descendant_target_tests;

#[cfg(test)]
#[path = "../../tests/unit/engine_rsi_evolve_tests.rs"]
mod tests;

// Source template for the native domain-selection mutation. This file is
// compiled in differential tests; production uses it only after a real patch
// inserts it into the descendant's own RSI selector and A/B accepts it.
pub(super) fn fast_failure_domain(
    history: &[serde_json::Value],
    candidates: &[&str],
    objective_matches: impl Fn(&str) -> bool,
) -> String {
    #[derive(Clone, Copy, Default)]
    struct Stats {
        attempts: u64,
        accepted: u64,
        tokens: u64,
        verified: u64,
        gains: u64,
        verified_tokens: u64,
    }
    let mut stats = vec![Stats::default(); candidates.len()];
    for row in history {
        let Some(index) = candidates
            .iter()
            .position(|domain| row["domain"] == *domain)
        else {
            continue;
        };
        let item = &mut stats[index];
        item.attempts += 1;
        item.accepted += u64::from(row["accepted"] == true);
        item.tokens += row["tokens"].as_u64().unwrap_or(0);
        if row["evidenceVerified"] == true {
            item.verified += 1;
            item.gains += row["verifiedHoldoutGains"].as_u64().unwrap_or(0).min(64);
            item.verified_tokens += row["tokens"].as_u64().unwrap_or(0).max(1000);
        }
    }
    let mut best = (f64::NEG_INFINITY, candidates[0]);
    for (domain, item) in candidates.iter().zip(stats) {
        let attempts = item.attempts as f64;
        let token_cost = if attempts == 0.0 {
            0.0
        } else {
            (item.tokens as f64 / attempts) / 10_000.0
        };
        let count = attempts.max(1.0);
        let utility = item.accepted as f64 / (1.0 + token_cost * count) / count;
        let exploration = if attempts == 0.0 {
            3.0
        } else {
            (2.0 * ((history.len() as f64) + 1.0).ln() / attempts)
                .sqrt()
                .min(3.0)
        };
        let gain_efficiency = if item.verified_tokens > 0 {
            (item.gains as f64 * 10_000.0 / item.verified_tokens as f64).min(8.0)
        } else {
            0.0
        };
        let payoff = if item.verified > 0 {
            (item.gains as f64 / (item.verified as f64 + 1.0)).min(4.0) * 0.75
                + gain_efficiency.min(4.0) * 0.30
        } else {
            0.0
        };
        let objective_match = if objective_matches(domain) { 1.0 } else { 0.0 };
        let score = utility + payoff + exploration + objective_match * 2.0;
        if score > best.0 {
            best = (score, domain);
        }
    }
    best.1.to_string()
}
