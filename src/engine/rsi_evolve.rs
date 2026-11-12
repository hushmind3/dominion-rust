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

/// Explore every part of the product, then prefer strategies with measured
/// gains per unit cost. Failed and inconclusive attempts remain in the denominator.
/// The active `selfimprove.search` algorithm decides the ranking rule; the
/// default reproduces the original measured-cost UCB policy.
fn search_features(history: &[Value], domain: &str, objective: &str) -> BTreeMap<String, f64> {
    let rows = history
        .iter()
        .filter(|row| row["domain"] == domain)
        .collect::<Vec<_>>();
    let attempts = rows.len() as f64;
    let accepted = rows.iter().filter(|row| row["accepted"] == true).count() as f64;
    let regressions = rows
        .iter()
        .filter(|row| row["evaluation"]["verdict"] == "regressed")
        .count() as f64;
    let inconclusive = rows
        .iter()
        .filter(|row| row["evaluation"]["verdict"] == "inconclusive")
        .count() as f64;
    let tokens = rows
        .iter()
        .map(|row| row["tokens"].as_u64().unwrap_or(0))
        .sum::<u64>() as f64;
    let useful = rows.iter().filter(|row|row["accepted"]==true
        || row["newFindings"].as_u64().unwrap_or(0)>0).count() as f64;
    let verified=rows.iter().filter(|row|row["evidenceVerified"]==true)
        .collect::<Vec<_>>();
    let verified_outcomes=verified.len() as f64;
    let holdout_gains=verified.iter()
        .map(|row|row["verifiedHoldoutGains"].as_u64().unwrap_or(0).min(64))
        .sum::<u64>() as f64;
    let verified_tokens=verified.iter()
        .map(|row|row["tokens"].as_u64().unwrap_or(0).max(1000))
        .sum::<u64>() as f64;
    // Robust to single low-token anomalies; this is an observed payoff for
    // target selection, NOT causal attribution of algorithm quality.
    let gain_efficiency=if verified_tokens>0.0 {
        (holdout_gains*10_000.0/verified_tokens).min(8.0)
    } else {0.0};
    let success_rate = if attempts == 0.0 { 0.0 } else { accepted / attempts };
    let token_cost = if attempts == 0.0 {
        0.0
    } else {
        (tokens / attempts) / 10_000.0
    };
    let objective_match = if objective_matches(domain, objective) {
        1.0
    } else {
        0.0
    };
    let unresolved = if rows
        .last()
        .is_some_and(|row| row["candidateId"].is_null() && row["accepted"] != true)
    {
        1.0
    } else {
        0.0
    };
    let exploration = if attempts == 0.0 {
        3.0
    } else {
        (2.0 * ((history.len() as f64) + 1.0).ln() / attempts)
            .sqrt()
            .min(3.0)
    };
    BTreeMap::from([
        ("attempts".to_string(), attempts),
        ("accepted".to_string(), accepted),
        ("regressions".to_string(), regressions),
        ("inconclusive".to_string(), inconclusive),
        ("novelty".to_string(), if attempts == 0.0 { 1.0 } else { 0.0 }),
        ("successRate".to_string(), success_rate),
        ("usefulGenerations".to_string(), useful),
        ("barrenRate".to_string(), if attempts>0.0 {(attempts-useful)/attempts} else {0.0}),
        ("verifiedOutcomes".to_string(), verified_outcomes),
        ("holdoutGains".to_string(), holdout_gains),
        ("gainPer10kTokens".to_string(), gain_efficiency),
        ("tokenCost".to_string(), token_cost),
        ("objectiveMatch".to_string(), objective_match),
        ("unresolved".to_string(), unresolved),
        ("exploration".to_string(), exploration),
    ])
}

fn score_domain(algorithm: &Value, features: &BTreeMap<String, f64>) -> Option<f64> {
    let implementation = algorithm["implementation"]
        .as_str()
        .or_else(|| algorithm["variant"].as_str())
        .unwrap_or("failure-clusters");
    match implementation {
        "expression" => algorithm["parameters"]["expression"]
            .as_str()
            .and_then(|expression| {
                crate::engine::algorithms::eval_expression(expression, features).ok()
            }),
        "counterexample-first" => {
            let get = |name: &str| features.get(name).copied().unwrap_or(0.0);
            Some(
                get("regressions") * 3.0
                    + get("inconclusive") * 2.0
                    + get("novelty") * 1.5
                    + get("exploration") * 0.4
                    + get("objectiveMatch") * 1.0
                    - get("successRate") * 0.5
                    - get("tokenCost") * 0.2,
            )
        }
        _ => {
            let get = |name: &str| features.get(name).copied().unwrap_or(0.0);
            let attempts = get("attempts").max(1.0);
            let utility = get("accepted") / (1.0 + get("tokenCost") * attempts) / attempts;
            // The user's objective outranks exploration. A goal-specific
            // capability is selected before an unrelated unused domain.
            // Two equally accepted patches can differ substantially in actual
            // frozen holdout gain. Prefer the empirically productive domain
            // while keeping explicit owner goals and exploration intact.
            let measured_payoff=if get("verifiedOutcomes")>0.0 {
                (get("holdoutGains")/(get("verifiedOutcomes")+1.0)).min(4.0)*0.75
                    + get("gainPer10kTokens").min(4.0)*0.30
            }else{0.0};
            Some(utility + measured_payoff + get("exploration")
                + get("objectiveMatch") * 2.0)
        }
    }
}

pub(crate) fn candidate_domains(objective: &str) -> Vec<&'static str> {
    // Explicit capability requests are constraints, not small bonuses that
    // another domain can override through unrelated historical successes.
    // An empty objective is an intentional open-ended survey of all domains.
    let matching = DOMAINS.iter().copied()
        .filter(|domain| objective_matches(domain, objective))
        .collect::<Vec<_>>();
    if matching.is_empty() { DOMAINS.to_vec() } else { matching }
}

pub(crate) fn scored_domains(
    algorithm: &Value, history: &[Value], objective: &str,
) -> Option<Vec<(&'static str, f64)>> {
    candidate_domains(objective).into_iter().map(|domain| {
        score_domain(algorithm, &search_features(history, domain, objective))
            .filter(|score| score.is_finite())
            .map(|score| (domain, score))
    }).collect()
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

pub fn campaign_model_tokens(state:&AppState, id:&str)->u64 {
    let sessions=state.store.all("job").into_iter().filter(|job|
        job["kind"]=="rsi_generation" && job["input"]["rsiCampaignId"]==id)
        .filter_map(|job|job["sessionId"].as_str().map(str::to_string)).collect::<BTreeSet<_>>();
    sessions.iter().map(|id|state.store.session_usage(id)["totalTokens"].as_u64().unwrap_or(0)).sum()
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
    let total = campaign_model_tokens(state,&id);
    let spent = state.store.session_usage(session_id)["totalTokens"]
        .as_u64()
        .unwrap_or(0);
    // Preserve accounted historical usage when old ledgers no longer contain
    // their generation sessions. Never reset an already spent campaign budget.
    let total=total.max(parent["evolution"]["activeBaseTokens"].as_u64().unwrap_or(0).saturating_add(spent));
    let mut available = budget.saturating_sub(total.saturating_add(prompt_estimate));
    if let Some(cap)=state.store.get("session",session_id).and_then(|s|s["rsiEpisodeTokenLimit"].as_u64()) {
        available=available.min(cap.saturating_sub(spent.saturating_add(prompt_estimate)));
    }
    if available < 256 {
        return Err("rsi_token_budget: 자동 개선의 토큰 예산이 부족해 다음 모델 호출을 중단했습니다.".into());
    }
    Ok(Some(available.min(8192)))
}

pub(crate) async fn command(
    context: &JobContext,
    mut command: Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    if context
        .state
        .store
        .get("job", &context.job_id)
        .is_none_or(|job| job["status"] != "running" || job["cancelRequested"] == true)
    {
        return Err("사용자 요청으로 자동 개선이 중단되었습니다.".into());
    }
    let operation = tools::run_command(&mut command, limit);
    tokio::pin!(operation);
    loop {
        tokio::select! {
            result=&mut operation=>return result,
            _=tokio::time::sleep(Duration::from_millis(100))=>{
                if context.state.store.get("job",&context.job_id)
                    .is_none_or(|j|j["status"]!="running"||j["cancelRequested"]==true){
                    return Err("사용자 요청으로 자동 개선이 중단되었습니다.".into());
                }
                if let Some(record)=context.state.store.get("rsi_campaign",&context.job_id){
                    let active=chrono::Utc::now().signed_duration_since(
                        record["createdAt"].as_str().and_then(|v|chrono::DateTime::parse_from_rfc3339(v).ok())
                            .map(|v|v.with_timezone(&chrono::Utc)).unwrap_or_else(chrono::Utc::now)
                    ).num_milliseconds().max(0) as u64;
                    if record["phase"]!="waiting_for_choice"
                        && record["limits"]["timeBudgetSeconds"].as_u64().is_some_and(|budget|
                            active.saturating_sub(record["waitingMs"].as_u64().unwrap_or(0)) >= budget.saturating_mul(1000))
                    {
                        return Err("time_budget".into());
                    }
                    let active_tokens=record["activeGenerationJobId"].as_str()
                        .and_then(|id|context.state.store.get("job",id))
                        .and_then(|j|j["sessionId"].as_str().map(str::to_string))
                        .map(|s|context.state.store.session_usage(&s)["totalTokens"].as_u64().unwrap_or(0)).unwrap_or(0);
                    if record["phase"]=="diagnosing_and_editing"
                        && record["limits"]["tokenBudget"].as_u64().is_some_and(|budget|
                            record["activeBaseTokens"].as_u64().unwrap_or(record["tokens"].as_u64().unwrap_or(0)).saturating_add(active_tokens) >= budget)
                    {
                        return Err("token_budget".into());
                    }
                }
            }
        }
    }
}

async fn git(context: &JobContext, dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    let output = command(context, cmd, Duration::from_secs(30)).await?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1200)
            .collect());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn save(context: &JobContext, record: &Value) -> Result<(), String> {
    let saved = context
        .state
        .store
        .put("rsi_campaign", record)
        .ok_or("자동 개선 상태를 저장할 수 없습니다.")?;
    context.state.bus.record("rsi_campaign", &saved);
    context.checkpoint(json!({"evolution":saved}))
}

fn elapsed(record: &mut Value, started: Instant) {
    let active_ms = (started.elapsed().as_millis() as u64)
        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0));
    record["elapsedMs"] = json!(active_ms);
    let accepted = record["acceptedGenerations"].as_u64().unwrap_or(0) as f64;
    let tokens = record["tokens"].as_u64().unwrap_or(0) as f64;
    record["efficiency"] = json!({
        "verifiedGainsPerHour":accepted/(active_ms as f64/1000.0).max(1.0)*3600.0,
        "tokensPerAcceptedGeneration":if accepted>0.0{json!(tokens/accepted)}else{Value::Null},
        "accelerationProven":false
    });
}

/// Read-only snapshot of the user's checkout. Unrelated untracked research,
/// runtime data and secrets are excluded; users need not learn Git first.
fn lineage_fingerprint(commit: &str, tree: &str, diff: &[u8], untracked: &[(String, Vec<u8>)]) -> String {
    let mut fingerprint = Sha256::new();
    fingerprint.update(commit.trim().as_bytes());
    fingerprint.update([0]);
    fingerprint.update(tree.trim().as_bytes());
    fingerprint.update([0]);
    fingerprint.update(diff);
    for (name, contents) in untracked {
        fingerprint.update(name.as_bytes());
        fingerprint.update([0]);
        fingerprint.update(contents);
    }
    format!("{:x}", fingerprint.finalize())
}

/// A real lineage fingerprint hashes the source commit and tree. Older
/// campaigns hashed only `git diff HEAD`, which is empty for a clean checkout
/// and produced the SHA-256 of the empty string.
pub fn lineage_fingerprint_valid(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value != "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
}

async fn snapshot_source(
    context: &JobContext,
    source: &Path,
    lab: &Path,
) -> Result<(bool, String), String> {
    let commit = git(context, source, &["rev-parse", "HEAD"]).await?;
    let tree = git(context, source, &["rev-parse", "HEAD^{tree}"]).await?;
    let mut cmd = Command::new("git");
    cmd.current_dir(source)
        .args(["diff", "--binary", "--no-ext-diff", "HEAD"]);
    let output = command(context, cmd, Duration::from_secs(30)).await?;
    if !output.status.success() || output.stdout.len() >= 79_000 {
        return Err("원본 변경을 안전하게 복사하지 못했습니다.".into());
    }
    if !output.stdout.is_empty() {
        let file = lab
            .parent()
            .ok_or("실험 경로 없음")?
            .join(format!("snapshot-{}.patch", uuid::Uuid::new_v4()));
        std::fs::write(&file, &output.stdout).map_err(|e| e.to_string())?;
        let applied = git(
            context,
            lab,
            &["apply", file.to_str().ok_or("원본 복사 경로 오류")?],
        )
        .await;
        let _ = std::fs::remove_file(file);
        applied?;
    }
    let files = git(
        context,
        source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )
    .await?;
    let mut bytes = 0u64;
    let mut untracked: Vec<(String, Vec<u8>)> = Vec::new();
    for name in files.split('\0').filter(|name| !name.is_empty()) {
        let code = crate::engine::rsi::safe_path(name)
            || name
                .strip_prefix("scripts/")
                .is_some_and(|part| !part.contains('/') && !part.starts_with('.'));
        if !code || name.chars().any(char::is_control) {
            continue;
        }
        let path = source.join(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        bytes = bytes.saturating_add(metadata.len());
        if bytes > 8 * 1024 * 1024 {
            return Err("새 소스 파일의 복사 상한을 초과했습니다.".into());
        }
        let destination = lab.join(name);
        std::fs::create_dir_all(destination.parent().ok_or("원본 파일 경로 오류")?)
            .map_err(|e| e.to_string())?;
        let contents = std::fs::read(&path).map_err(|e| e.to_string())?;
        untracked.push((name.to_string(), contents.clone()));
        std::fs::write(destination, contents).map_err(|e| e.to_string())?;
    }
    let fingerprint = lineage_fingerprint(&commit, &tree, &output.stdout, &untracked);
    if git(context, lab, &["status", "--porcelain"])
        .await?
        .is_empty()
    {
        return Ok((false, fingerprint));
    }
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: snapshot existing source without changing the original checkout",
        ],
    )
    .await?;
    Ok((true, fingerprint))
}

pub fn choose(state: &AppState, id: &str, body: &Value) -> Result<Value, String> {
    let record = state
        .store
        .get("rsi_campaign", id)
        .ok_or("진행 중인 개선을 찾을 수 없습니다.")?;
    if record["status"] != "running"
        || record["phase"] != "waiting_for_choice"
        || state
            .store
            .get("job", id)
            .is_none_or(|job| job["status"] != "running" || job["cancelRequested"] == true)
    {
        return Err("현재 선택을 기다리는 비교가 없습니다.".into());
    }
    let comparison = &record["comparison"];
    let choice = body["choice"]
        .as_str()
        .filter(|v| matches!(*v, "baseline" | "candidate"))
        .ok_or("기존 버전 또는 개선 버전을 선택하세요.")?;
    if body["attempt"] != comparison["attempt"] {
        return Err("새 비교 결과가 도착했습니다. 최신 결과에서 선택하세요.".into());
    }
    if choice == "candidate" && comparison["canSelectCandidate"] != true {
        return Err("개선 효과와 회귀 검사를 통과한 버전만 선택할 수 있습니다.".into());
    }
    let decision = json!({"id":format!("{id}:{}",comparison["attempt"]),"campaignId":id,
        "attempt":comparison["attempt"],"evaluationId":comparison["evaluationId"],
        "choice":choice,"at":chrono::Utc::now().to_rfc3339()});
    if let Some(saved) = state.store.put_if_absent("rsi_choice", &decision) {
        return Ok(saved);
    }
    state
        .store
        .get("rsi_choice", decision["id"].as_str().unwrap())
        .filter(|saved| {
            saved["choice"] == choice && saved["evaluationId"] == comparison["evaluationId"]
        })
        .ok_or("이미 다른 버전을 선택했습니다.".into())
}

async fn select(context: &JobContext, record: &mut Value) -> Result<String, String> {
    if record["selectionPolicy"] == "automatic" {
        let choice = if record["comparison"]["canSelectCandidate"] == true {
            "candidate"
        } else {
            "baseline"
        };
        record["comparison"]["selected"] = json!(choice);
        return Ok(choice.into());
    }
    record["phase"] = json!("waiting_for_choice");
    save(context, record)?;
    let waiting = Instant::now();
    let key = format!("{}:{}", context.job_id, record["comparison"]["attempt"]);
    loop {
        if context
            .state
            .store
            .get("job", &context.job_id)
            .is_none_or(|j| j["status"] != "running" || j["cancelRequested"] == true)
        {
            return Err("사용자 요청으로 개선이 중단되었습니다.".into());
        }
        if let Some(decision) = context.state.store.get("rsi_choice", &key) {
            if decision["evaluationId"] != record["comparison"]["evaluationId"] {
                return Err("선택한 비교 기준이 다릅니다.".into());
            }
            record["waitingMs"] = json!(
                record["waitingMs"].as_u64().unwrap_or(0) + waiting.elapsed().as_millis() as u64
            );
            record["comparison"]["selected"] = decision["choice"].clone();
            record["phase"] = json!("selecting");
            save(context, record)?;
            return decision["choice"]
                .as_str()
                .map(str::to_string)
                .ok_or("선택 기록 손상".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

pub async fn run(context: &JobContext, job: &Value) -> Result<Value, String> {
    if job["input"]["rsiNativeApproved"] != true {
        return Err("자동 실험 실행 선택이 없습니다.".into());
    }
    if job["rsiEvolutionInFlight"] == true {
        return Err("needs_review: 중단된 실험 계보를 확인하세요.".into());
    }
    let input = &job["input"];
    let source = std::fs::canonicalize(input["rsiSourceDir"].as_str().ok_or("소스 경로 없음")?)
        .map_err(|e| e.to_string())?;
    let base = input["rsiBaseCommit"].as_str().ok_or("기준 커밋 없음")?;
    if git(context, &source, &["rev-parse", "HEAD"]).await? != base {
        return Err("원본 소스가 시작 시점과 다릅니다. 다시 시작하세요.".into());
    }
    if let Some(recorded_branch)=input["rsiSourceBranch"].as_str().filter(|value|!value.is_empty()) {
        let current_branch=git(context,&source,&["rev-parse","--abbrev-ref","HEAD"]).await?;
        if current_branch!=recorded_branch {
            return Err("자기개선 대기 중 원본 브랜치가 바뀌었습니다. 같은 브랜치에서 다시 시작하세요.".into());
        }
    }
    let limits = limits(&input["limits"])?;
    let parent = context.state.config.data_dir.join("rsi-labs");
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    let lab = parent.join(format!("lab-{}", uuid::Uuid::new_v4()));
    context.checkpoint(json!({"rsiEvolutionInFlight":true,"rsiPhase":"preparing_lab"}))?;
    git(
        context,
        &source,
        &[
            "worktree",
            "add",
            "--detach",
            lab.to_str().ok_or("실험 경로 오류")?,
            base,
        ],
    )
    .await?;
    let (snapshot, fingerprint) = snapshot_source(context, &source, &lab).await?;
    let mut lineage = json!({
        "selectedCommit": Value::Null,
        "sameSource": false,
        "sameBranch": false,
        "sameBase": false,
        "fingerprintValid": false,
        "fingerprintMatch": false,
        "reused": false
    });
    if let Some(selected) = context.state.store.get("rsi_current", "selected-engine") {
        let same_source = selected["sourceDir"].as_str() == source.to_str();
        let same_branch = selected["sourceBranch"] == input["rsiSourceBranch"]
            && input["rsiSourceBranch"].as_str().is_some_and(|s| !s.is_empty());
        let same_base = selected["sourceBaseCommit"].as_str() == Some(base);
        let selected_fingerprint = selected["sourceFingerprint"].as_str().unwrap_or("");
        let fingerprint_valid = lineage_fingerprint_valid(selected_fingerprint);
        let fingerprint_match = fingerprint_valid && selected_fingerprint == fingerprint;
        lineage = json!({
            "selectedCommit": selected["commit"],
            "sameSource": same_source,
            "sameBranch": same_branch,
            "sameBase": same_base,
            "fingerprintValid": fingerprint_valid,
            "fingerprintMatch": fingerprint_match,
            "reused": false
        });
        if same_source && same_branch && same_base && fingerprint_match {
            if let Some(commit) = selected["commit"]
                .as_str()
                .filter(|s| s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                git(context, &lab, &["reset", "--hard", commit]).await?;
                lineage["reused"] = json!(true);
            }
        }
    }
    let head = git(context, &lab, &["rev-parse", "HEAD"]).await?;
    #[cfg(unix)]
    if source.join("web/node_modules").is_dir()
        && lab.join("web").is_dir()
        && !lab.join("web/node_modules").exists()
    {
        std::os::unix::fs::symlink(
            source.join("web/node_modules"),
            lab.join("web/node_modules"),
        )
        .map_err(|e| e.to_string())?;
    }
    let mut accepted_streak_seed = 0u64;
    let mut max_streak_seed = 0u64;
    if lineage["reused"] == true {
        if let Some(prior_id) = input["rsiContinueFrom"].as_str() {
            if let Some(prior) = context.state.store.get("rsi_campaign", prior_id) {
                if prior["headCommit"] == head
                    && prior["sourceDir"] == json!(source)
                    && prior["sourceBranch"] == input["rsiSourceBranch"]
                    && prior["baseCommit"] == base
                    && prior["sourceFingerprint"] == fingerprint {
                    accepted_streak_seed = prior["acceptedStreak"].as_u64().unwrap_or(0);
                    max_streak_seed = prior["maxAcceptedStreak"].as_u64().unwrap_or(0);
                }
            }
        }
    }
    let mut record = json!({"id":context.job_id,"status":"running","phase":"building_generator",
        "sourceDir":source,"sourceBranch":input["rsiSourceBranch"],
        "labDir":lab,"baseCommit":base,"headCommit":head,"sourceSnapshot":snapshot,"sourceFingerprint":fingerprint,
        "objective":input["rsiObjective"],"providerId":input["providerId"],"model":input["model"],
        "continuedFrom":input["rsiContinueFrom"],"nextDecision":input["rsiPreviousDecision"],
        "nativeApproved":true,"systemEvolution":input["rsiSystemEvolution"],"limits":limits,"attempts":0,"acceptedGenerations":0,"tokens":0,
        "history":[],"activeGenerationJobId":null,"createdAt":chrono::Utc::now().to_rfc3339(),
        "selectionPolicy":input["selectionPolicy"],"waitingMs":0,
        "lineage":lineage,
        "acceptedStreak":accepted_streak_seed,"maxAcceptedStreak":max_streak_seed,"capabilityDemonstrated":false,
        "productionChanged":false,"accelerationProven":false});
    save(context, &record)?;
    let started = Instant::now();
    let result = run_loop(context, job, &mut record, started).await;
    if let Some(id) = record["activeGenerationJobId"].as_str().map(str::to_string) {
        if let Some(child) = context.state.store.get("job", &id) {
            if let Some(session) = child["sessionId"].as_str() {
                let usage = context.state.store.session_usage(session);
                record["tokens"] = json!(record["activeBaseTokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(usage["totalTokens"].as_u64().unwrap_or(0)));
                if child["status"] == "running" {
                    context.state.cancel_session(session);
                }
            }
            if child["status"] == "running" {
                context.state.store.patch_if("job",&id,"status",&json!("running"),
                    &json!({"status":"needs_review","cancelRequested":true,"error":"자동 개선 종료 시 중단된 세대 작업"}));
            }
        }
    }
    if input["rsiSystemEvolution"]==true {
        record["tokens"]=json!(campaign_model_tokens(&context.state,&context.job_id));
    }
    elapsed(&mut record, started);
    match result {
        Ok(reason) => {
            record["status"] = json!("completed");
            record["stopReason"] = json!(reason);
        }
        Err(error) if error == "time_budget" || error == "token_budget" => {
            record["status"] = json!("completed");
            record["stopReason"] = json!(error);
        }
        Err(error) => {
            let cancelled = context
                .state
                .store
                .get("job", &context.job_id)
                .is_none_or(|j| j["cancelRequested"] == true || j["status"] == "cancelled");
            record["status"] = json!(if cancelled {
                "cancelled"
            } else {
                "needs_review"
            });
            record["error"] = json!(error);
        }
    }
    record["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
    context
        .state
        .store
        .put("rsi_campaign", &record)
        .ok_or("최종 실험 기록 저장 실패")?;
    context.state.bus.record("rsi_campaign", &record);
    // Cancellation deliberately forbids resurrecting the parent job.
    if record["status"] != "cancelled" {
        context.checkpoint(json!({"rsiEvolutionInFlight":false,"evolution":record}))?;
    }
    Ok(json!({"status":record["status"],"evolution":record}))
}

/// A model-budget error can happen after the model has already written a
/// patch. If that patch was captured and passed independent checks, it is a
/// real candidate even when the worker reports needs_review.
fn verified_candidate_id(result: &Value) -> Option<&str> {
    result["rsi"]["candidateId"]
        .as_str()
        .filter(|_| result["rsi"]["checksPassed"] == true)
}

/// Stop repeating an already measured, unproductive patch against the exact
/// same source commit. A new base revision or a changed hypothesis/patch can
/// still be tested. This saves a full native paired build/evaluation cycle.
fn identical_failed_patch(
    state:&AppState,history:&[Value],candidate_id:&str,
)->Option<String>{
    let current=state.store.get("rsi_candidate",candidate_id)?;
    let patch=current["patchSha256"].as_str().filter(|s|!s.is_empty())?;
    let base=current["baseCommit"].as_str().filter(|s|!s.is_empty())?;
    for row in history.iter().rev() {
        if row["accepted"]==true {continue;}
        if !matches!(row["evaluation"]["verdict"].as_str(),
            Some("regressed"|"inconclusive"|"build_failed")) {continue;}
        let Some(old_id)=row["candidateId"].as_str() else {continue;};
        let Some(old)=state.store.get("rsi_candidate",old_id) else {continue;};
        if old["baseCommit"]==base && old["patchSha256"]==patch {
            return Some(old_id.to_string());
        }
    }
    None
}

/// Resume a pending candidate from a predecessor only when the new
/// campaign proves an identical source snapshot, branch, and inherited head.
fn matching_prior_campaign(state:&AppState,input:&Value,record:&Value)->Option<Value> {
    if record["lineage"]["reused"]!=true {return None;}
    let prior_id=input["rsiContinueFrom"].as_str()?;
    let prior=state.store.get("rsi_campaign",prior_id)?;
    let selected=record["lineage"]["selectedCommit"].as_str()?;
    if prior["headCommit"].as_str()!=Some(selected)
        || record["sourceDir"]!=prior["sourceDir"]
        || record["sourceBranch"]!=prior["sourceBranch"]
        || record["baseCommit"]!=prior["baseCommit"]
        || record["sourceFingerprint"]!=prior["sourceFingerprint"]
        || !lineage_fingerprint_valid(record["sourceFingerprint"].as_str().unwrap_or(""))
    {return None;}
    Some(prior)
}

/// Collect the exact files, ranges and searches earlier generations already
/// looked at. Without this, every generation starts from scratch and burns
/// tokens re-reading the same source.
fn research_digest(state: &AppState, input: &Value, record: &Value) -> String {
    let mut reads = BTreeSet::new();
    let mut searches = BTreeSet::new();
    let executions = state.store.all("execution");
    let operations = state.store.all("execution_operation");
    let mut generations = Vec::new();
    if let Some(prior) = matching_prior_campaign(state,input,record) {
        generations.extend(prior["history"].as_array().cloned().unwrap_or_default());
    }
    generations.extend(record["history"].as_array().cloned().unwrap_or_default());
    // Read offsets are source-revision specific. Reusing an old range after a
    // successfully adopted patch can skip the new implementation entirely.
    let revision=record["headCommit"].as_str().filter(|s|!s.is_empty());
    for generation in generations {
        if revision.is_some() && generation["generatorCommit"].as_str()!=revision {
            continue;
        }
        let Some(job_id) = generation["jobId"].as_str() else { continue };
        let Some(session_id) = state
            .store
            .get("job", job_id)
            .and_then(|job| job["sessionId"].as_str().map(str::to_string))
        else {
            continue;
        };
        for execution in &executions {
            if execution["sessionId"].as_str() != Some(session_id.as_str()) {
                continue;
            }
            let Some(run_id) = execution["id"].as_str() else { continue };
            for operation in &operations {
                if operation["runId"].as_str() != Some(run_id) {
                    continue;
                }
                match operation["tool"].as_str() {
                    Some("workspace_read_range") => {
                        let path = operation["arguments"]["path"].as_str().unwrap_or("");
                        if path.is_empty() {
                            continue;
                        }
                        let start = operation["arguments"]["startLine"].as_u64().unwrap_or(0);
                        let end = operation["arguments"]["endLine"].as_u64().unwrap_or(0);
                        reads.insert(format!("{path} {start}-{end}"));
                    }
                    Some("code_search") => {
                        if let Some(query) = operation["arguments"]["query"].as_str() {
                            searches.insert(query.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if reads.is_empty() && searches.is_empty() {
        return String::new();
    }
    let mut text = String::from("\n### 이미 확인한 범위 (다시 읽지 마세요)\n");
    for item in reads.iter().take(30) {
        text.push_str(&format!("- 읽음: {item}\n"));
    }
    for query in searches.iter().take(15) {
        text.push_str(&format!("- 검색함: \"{query}\"\n"));
    }
    text.push_str("위 파일·구간은 이전 세대에서 이미 확인했습니다. 같은 곳을 다시 읽지 말고, 필요한 정확한 블록만 확인한 뒤 바로 수정하세요.\n");
    text
}

/// The stored objective id must match the query actually used for source
/// selection. Autonomous campaigns rank by the selected capability domain,
/// not the generic system-generated instruction.
fn target_focus_objective(input:&Value)->&str {
    if input["rsiAutonomous"]==true {
        input["rsiTargetDomain"].as_str().unwrap_or("")
    } else {
        input["rsiObjective"].as_str().unwrap_or("")
    }
}

/// The domain score and the source target score are separate decisions.
/// A failed research-only generation only down-ranks its previous files; it
/// does not erase a verified source observation or count as an adopted patch.
fn choose_focus_files(
    state:&AppState, input:&Value, history:&[Value], policy:Option<&Value>,
) -> Vec<Value> {
    let Some(index_id)=input["rsiIndexId"].as_str() else {return Vec::new()};
    let domain=input["rsiTargetDomain"].as_str().unwrap_or("");
    // Autonomous RSI is not a learning-domain request just because its
    // generic instruction mentions knowledge or experience. Use the chosen
    // capability as the file query; explicit owner goals retain their text.
    let objective=target_focus_objective(input);
    let mut failures=BTreeMap::<String,u64>::new();
    let mut successes=BTreeMap::<String,u64>::new();
    let mut seen=BTreeSet::<String>::new();
    let mut consider=|generation:&Value,cross_campaign:bool|{
        let candidate=generation["candidateId"].as_str()
            .and_then(|id|state.store.get("rsi_candidate",id))
            .filter(|candidate|generation["accepted"]==true
                && candidate["status"]=="verified"
                && candidate["evaluation"]["metrics"]["eligible"]==true);
        let failed=generation["accepted"]!=true
            && generation["candidateId"].is_null()
            && generation["newFindings"].as_u64().unwrap_or(0)==0;
        if !failed && candidate.is_none() {return;}
        let id=generation["id"].as_str().unwrap_or("");
        if !id.is_empty() && !seen.insert(id.to_string()) {return;}
        for path in generation["focusFiles"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let earlier=generation["focusSelection"]["candidates"].as_array()
                .into_iter().flatten().find(|row|row["path"]==path);
            let pre_hash=earlier.and_then(|row|row["sourceHash"].as_str());
            let accepted_hash=generation["acceptedSourceHashes"][path].as_str();
            let current=crate::engine::code_index::indexed_source_hash(state,index_id,path);
            if cross_campaign && ![pre_hash,accepted_hash].into_iter().flatten()
                    .any(|hash|current.as_deref()==Some(hash)) {continue;}
            // A verified improvement belongs to the post-adoption source,
            // not the original file the model chose to inspect. A revert
            // must revoke positive feedback even if the old hash matches.
            let credited=candidate.as_ref().is_some_and(|candidate|
                candidate["paths"].as_array().is_some_and(|paths|
                    paths.iter().any(|item|item.as_str()==Some(path))))
                && accepted_hash.is_some() && current.as_deref()==accepted_hash;
            // Failed exploration is also revision-specific. A later patch
            // must not keep punishing a file whose content has changed.
            // Legacy local observations without a recorded hash can still
            // be used within the same campaign for backward compatibility.
            let failed_revision_matches=pre_hash
                .is_some_and(|hash|current.as_deref()==Some(hash))
                || (pre_hash.is_none() && !cross_campaign && current.is_some());
            if credited {
                *successes.entry(path.to_lowercase()).or_default()+=1;
            } else if failed && failed_revision_matches {
                *failures.entry(path.to_lowercase()).or_default()+=1;
            }
        }
    };
    for generation in history.iter().rev().take(8) {consider(generation,false);}
    // Reuse only the same repository partition and same owner objective.
    // Comparing source hashes prevents stale failures from poisoning a new
    // revision, while cross-campaign records survive process restarts.
    let partition=research_scope(input).partition_key;
    let objective_id=crate::engine::memory::stable_id(&[objective]);
    let mut prior=state.store.all("rsi_generation").into_iter()
        .filter(|generation|generation["knowledgePartition"]==partition &&
            generation["objectiveId"]==objective_id)
        .collect::<Vec<_>>();
    prior.sort_by(|a,b|b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    for generation in prior.iter().take(8) {consider(generation,true);}
    crate::engine::code_index::rank_focus_files_with_feedback(
        state,index_id,objective,domain,&failures,&successes,policy,2
    )
}

fn focus_file_context(state: &AppState, record: &Value, input: &Value) -> String {
    let Some(lab) = record["labDir"].as_str() else { return String::new() };
    let Some(index_id) = input["rsiIndexId"].as_str() else { return String::new() };
    let objective = input["rsiObjective"].as_str().unwrap_or("");
    let domain = input["rsiSelectedDomain"].as_str()
        .or_else(|| input["rsiDomain"].as_str()).unwrap_or("");
    let mut text = String::new();
    // Total injected source is capped; large files only contribute relevant spans.
    for path in input["rsiFocusFiles"].as_array().into_iter().flatten().filter_map(Value::as_str).take(2) {
        if !crate::engine::rsi::safe_path(path) { continue; }
        if let Some(excerpt) = crate::engine::code_index::bounded_focus_context(
            state, std::path::Path::new(lab), index_id, path, objective, domain
        ) {
            if text.chars().count() + excerpt.chars().count() > 16_000 { break; }
            text.push_str(&excerpt);
        }
    }
    text
}
/// A quality score is not an actionable repair target. Read bounded examples
/// from the exact independently evaluated patch, but only when the raw paired
/// receipt still matches that candidate's source and evidence hashes. These
/// observations guide the next proposal; they are never adoption evidence.
fn paired_behavioral_feedback(state:&AppState,candidate_id:&str)->Vec<String> {
    let Some(candidate)=state.store.get("rsi_candidate",candidate_id) else {
        return Vec::new();
    };
    if candidate["pairedComparisonComplete"]!=true {return Vec::new();}
    let Some(pair)=state.store.get("rsi_pair",&format!("rsi-pair-{candidate_id}")) else {
        return Vec::new();
    };
    if pair["candidateId"]!=candidate_id
        || pair["evidenceSha256"]!=candidate["evaluation"]["evidenceSha256"]
        || pair["report"]["patchSha256"]!=candidate["patchSha256"]
        || !pair["report"]["cases"].is_array() {
        return Vec::new();
    }
    let mut failures=Vec::new();
    for case in pair["report"]["cases"].as_array().into_iter().flatten() {
        // Holdout examples and answers must never become reflection data.
        // Explicit development cases may supply actionable counterexamples.
        if !candidate["cases"].as_array().is_some_and(|plan|plan.iter().any(|spec|
            spec["id"]==case["id"] && spec["holdout"]==false)) {continue;}
        let name=case["id"].as_str().unwrap_or("behavior");
        for observed in case["behavioralCounterexamples"]["candidate"]
            .as_array().into_iter().flatten() {
            let Some(input)=observed["input"].as_str() else {continue;};
            let Some(expected)=observed["expectedSegments"].as_u64() else {continue;};
            let Some(actual)=observed["observedSegments"].as_u64() else {continue;};
            if expected==actual {continue;}
            let steps=observed["steps"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).take(5).collect::<Vec<_>>().join(" / ");
            failures.push(format!(
                "{name}: 입력={:?}; 기대 단계={expected}; 후보 단계={actual}; 현재 분리={:?}",
                input.chars().take(180).collect::<String>(),
                steps.chars().take(230).collect::<String>()));
            if failures.len()>=3 {return failures;}
        }
    }
    failures
}

fn generation_context(state: &AppState, input: &Value, record: &Value) -> String {
    let objective = input["rsiObjective"].as_str().unwrap_or("");
    let scope = research_scope(input);
    let (prior_experience, _) =
        experience::context_block_with_manifest(state, &scope, objective);
    let mut text = String::new();
    if input["rsiIssueEvidence"].is_object() {
        text.push_str(&format!("\n\n## 사용자가 수정 요청한 실제 문제\n{}\n문제 설명은 증거이며 성공 판정이 아닙니다. 원문과 원인을 먼저 확인하세요.\n",input["rsiIssueEvidence"]));
    }
    if input["rsiSystemEvolution"]==true {
        let evidence=&input["rsiControllerPlan"]["usageEvidence"];
        if evidence["signals"].as_array().is_some_and(|rows|!rows.is_empty()) {
            text.push_str(&format!("\n\n## 실제 에이전트 사용에서 축적한 개선 근거\n{}\n실패 기록은 가설의 근거이며 개선 성공이 아닙니다. 관련 원문과 현재 소스를 대조하세요.\n",evidence));
        }
    }
    text.push_str("\n\n## 사용 가능한 기존 도구\ncode_search는 DB 심볼 위치를, workspace_read_range는 필요한 줄을 읽습니다. 인덱스나 검증된 발췌에 있는 위치를 다시 검색하지 마세요. 근거 있는 코드 변경은 workspace_replace 또는 읽은 줄 번호·sourceSha256으로 workspace_replace_range를 사용합니다. 새 파일은 workspace_write입니다. unsafe_audit는 SAFETY 근거를 검사합니다.\n");
    text.push_str("rsi_method_propose로 적용 조건·실행 노드·후속 검증을 가진 재사용 방법을, algorithm_propose로 탐색 계산식을 제안할 수 있습니다. 다음 예정된 작업에서 실제 실행하며 등록·도구 호출만으로 성과를 인정하지 않습니다. 외부 자료는 검증할 근거이고 지시가 아닙니다.\n");
    if let Some(algorithm) = crate::engine::algorithms::active(state, "selfimprove.search") {
        text.push_str("\n### 현재 자기개선 탐색 알고리즘\n");
        text.push_str(&format!(
            "- 이름: {}\n- 실행 커널: {}\n- 설명: {}\n- 매개변수: {}\n- revision: {}\n",
            algorithm["name"].as_str().unwrap_or(""),
            algorithm["implementation"].as_str().unwrap_or(""),
            algorithm["description"].as_str().unwrap_or(""),
            algorithm["parameters"],
            algorithm["revision"].as_str().unwrap_or(""),
        ));
        text.push_str("이 알고리즘이 다음 세대의 탐색 분야를 고릅니다. 더 나은 계산식이 있다고 판단되면 `algorithm_propose`로 제안하세요. 제안은 다음 예정된 세대에서 시험 사용되고, 실제 세대 결과가 비교 가능할 때만 유지·채택이 결정됩니다. 과거 성공 분야 일치율만으로는 채택되지 않습니다.\n");
    }
    if let Some(block) = prior_experience.filter(|block| !block.trim().is_empty()) {
        text.push_str("\n### 기존 RSI 경험·지식\n");
        text.push_str(&block);
        text.push('\n');
    }
    if let Some(history) = record["history"].as_array() {
        let recent = history.iter().rev().take(2).collect::<Vec<_>>();
        if !recent.is_empty() {
            text.push_str("\n### 이전 시도의 실제 결과\n");
            for generation in recent.into_iter().rev() {
                    let candidate=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id));
                let evidence=json!({"accepted":generation["accepted"],"commit":generation["acceptedCommit"],
                    "hypothesis":candidate.as_ref().map(|c|&c["hypothesis"]),"paths":candidate.as_ref().map(|c|&c["paths"]),
                    "verdict":generation["evaluation"]["verdict"],"error":generation["error"],
                    "editEvidence":generation["editEvidence"]});
                text.push_str(&format!("- {}세대: {}\n",generation["attempt"],
                    evidence.to_string().chars().take(1800).collect::<String>()));
            }
        }
        // A focused owner request that already produced verified source reads
        // must switch from research to implementation in its next generation.
        // A read-only result is valuable context, not the completed owner task.
        if input["rsiAutonomous"]!=true
            && input["rsiFocusFiles"].as_array().is_some_and(|files|!files.is_empty())
            && history.iter().rev().any(|row|row["candidateId"].is_null()
                && row["newFindings"].as_u64().unwrap_or(0)>0) {
            let files=input["rsiFocusFiles"].as_array().into_iter().flatten()
                .filter_map(Value::as_str).take(4).collect::<Vec<_>>().join(", ");
            text.push_str("\n### 연구에서 실행으로 전환\n");
            text.push_str(&format!(
                "- 목표가 명시된 파일: {files}. 기존 세대의 읽기 결과는 이미 제공됐습니다. 지금은 같은 연구를 반복할 차례가 아닙니다.\n"));
            text.push_str("- 검증된 원문에서 정확한 oldText 블록을 고르고 workspace_replace로 실제 소스 수정까지 완료하세요. 모르는 구간만 workspace_read_range로 짧게 보충하세요. 실행한 소스 변경이 없으면 개선 성과가 아닙니다.\n");
        }
        // Make failure feedback actionable rather than another inert memory:
        // the next model receives the previous hypothesis, its measured
        // verdict, and an explicit requirement to change the experiment.
        let selected=input["rsiTargetDomain"].as_str().unwrap_or("");
        // A newer read-only / compile-failed generation must not erase the
        // earlier compiler-passing candidate's concrete, unsolved holdout.
        // Without prioritizing measured behavioral failure, the next worker
        // keeps learning from the latest generic error instead of the actual
        // unchanged user task (e.g. planning quality 0.75 -> 0.75).
        let unsolved=|row:&&Value| row["accepted"]!=true
            && row["candidateId"].is_string()
            && row["evaluation"]["metrics"]["measurements"].as_array()
                .is_some_and(|cases|cases.iter().any(unresolved_quality_case));
        if let Some(failure)=history.iter().rev().find(|row|
                row["domain"]==selected && unsolved(row))
            .or_else(||history.iter().rev().find(unsolved))
            .or_else(||history.iter().rev().find(|row|
                row["domain"]==selected && row["accepted"]!=true))
            .or_else(||history.iter().rev().find(|row|
                row["accepted"]!=true && row["candidateId"].is_string()
                    && row["evaluation"]["metrics"]["metricGains"]==0)) {
            let error=failure["error"].as_str().unwrap_or("")
                .chars().take(300).collect::<String>();
            let verdict=failure["evaluation"]["verdict"].as_str().unwrap_or("unmeasured");
            let hypothesis=failure["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id))
                .and_then(|c|c["hypothesis"].as_str().map(str::to_string))
                .unwrap_or_default();
            text.push_str("\n### 실패를 반영한 다음 개선 실험\n");
            text.push_str(&format!("- 이전 동일 분야 실패: verdict={verdict}; error={}\n",
                if error.is_empty(){"실측된 진전 없음"}else{&error}));
            if !hypothesis.is_empty(){
                text.push_str(&format!("- 검증에 실패한 이전 가설: {}\n",
                    hypothesis.chars().take(280).collect::<String>()));
            }
            // The previous source bytes, not merely the name of a failed
            // experiment, are required to mutate a failed implementation.
            // Keep this bounded so short-context models can still edit.
            if let Some(patch)=failure["candidateId"].as_str()
                .and_then(|id|state.store.get("rsi_candidate",id))
                .and_then(|candidate|candidate["patch"].as_str().map(str::to_string)) {
                text.push_str("- 이전 모델이 실제 작성했지만 개선되지 않은 패치 (그대로 반복하지 말 것):\n");
                text.push_str(&patch.chars().take(1200).collect::<String>());
                text.push('\n');
            }
            // A compiled candidate with no holdout gain is a failed behavioral
            // hypothesis. Preserve its actual neutral quality measurements:
            // the next model must address the unchanged task, not celebrate
            // a successful build or repeat the same patch.
            let unchanged=failure["evaluation"]["metrics"]["measurements"].as_array()
                .into_iter().flatten()
                .filter(|case|case["holdout"]==true)
                .filter_map(|case|{
                    let metric=case["metrics"].as_array()?.iter().find(|metric|
                        metric["key"]=="quality"
                        && metric["improved"]==false
                        && metric["regressed"]==false
                        && metric["baseline"].as_f64().is_some_and(|v|v<1.0)
                        && metric["candidate"].as_f64()==metric["baseline"].as_f64())?;
                    Some(format!("{} ({}): baseline={}, candidate={}",
                        case["id"],case["domain"],metric["baseline"],metric["candidate"]))
                })
                .take(3).collect::<Vec<_>>();
            if !unchanged.is_empty() {
                text.push_str("- 동결된 실제 작업 품질이 개선되지 않은 항목: ");
                text.push_str(&unchanged.join("; "));
                text.push_str(". 같은 소스를 재수정하되 이전 패치를 되풀이하지 않고 해당 작업 결과를 바꾸는 구현을 작성하세요.\n");
            }
            if let Some(id)=failure["candidateId"].as_str() {
                let counterexamples=paired_behavioral_feedback(state,id);
                if !counterexamples.is_empty() {
                    text.push_str("- 실제 동결 평가에서 실패한 행동 (다음 소스 수정의 구체적 목표):\n");
                    for example in counterexamples {
                        text.push_str(&format!("  - {example}\n"));
                    }
                    text.push_str("- 점수만 다시 높인다고 주장하지 말고, 위 입력을 해결하는 행동 변경을 현재 계승 코드에서 구현하세요. 이 사례들은 이제 후손 학습용이며 새 입력 일반화 증거는 아닙니다.\n");
                }
            }
            let attempted=failure["editEvidence"]["attempts"].as_u64().unwrap_or(0);
            let verified=failure["editEvidence"]["verifiedWrites"].as_u64().unwrap_or(0);
            if attempted>0 {
                let candidate_state=if failure["candidateId"].is_null() {
                    "후보 0개"
                } else {
                    "후보는 생성됐으나 성능 개선 채택 실패"
                };
                text.push_str(&format!("- 실제 실행 도구 편집 시도 {attempted}회, 읽어 확인된 변경 {verified}회, {candidate_state}. 이번 세대는 도구 호출 자체가 아니라 실제 파일 diff와 독립 검증에 도달해야 합니다.\n"));
                for reason in failure["editEvidence"]["recentFailures"].as_array()
                    .into_iter().flatten().filter_map(Value::as_str).take(2) {
                    text.push_str(&format!("- 지난 편집 실패: {}\n",
                        reason.chars().take(260).collect::<String>()));
                }
            }
            text.push_str("- 이전 가설·변경을 그대로 반복하지 말고 실패 원인을 먼저 특정하세요. 수정 범위·가설·기대하는 작업 성능 지표를 바꾼 실험을 수행하세요. 조사 결과가 없다면 새 발견을 만들어내지 마세요.\n");
        }
        let verified=history_with_verified_payoffs(state,history);
        let measured=verified.iter().rev().filter(|row|row["evidenceVerified"]==true)
            .take(3).map(|row|json!({"domain":row["domain"],
                "verifiedHoldoutGains":row["verifiedHoldoutGains"],
                "tokens":row["tokens"],"acceptedCommit":row["acceptedCommit"]}))
            .collect::<Vec<_>>();
        if !measured.is_empty() {
            text.push_str("\n### 후속 세대에 계승할 검증된 성과\n");
            text.push_str(&json!(measured).to_string());
            text.push_str("\n이 성과는 검증된 과거 기록이며 새 전략의 인과적 우월성을 증명하지 않습니다. 효과가 있었던 지점을 재사용하거나 다른 분야에 전이하는 새 가설을 시험하세요.\n");
        }
    }
    text
}

fn research_scope(input:&Value)->MemoryScope {
    let source=input["rsiKnowledgeSource"].as_str().or_else(||input["rsiSourceDir"].as_str()).unwrap_or("");
    // The campaign input can still hold the user's symlinked path while the
    // child worktree stores the canonical path. Both must address the same
    // research partition or inheritance silently disappears.
    let canonical=std::fs::canonicalize(source).map(|path|path.to_string_lossy().to_string()).unwrap_or_else(|_|source.to_string());
    MemoryScope{session_id:"rsi-evolution".into(),agent_id:None,
        partition_key:format!("rsi:{}",crate::engine::memory::stable_id(&[&canonical]))}
}

/// Maps a user-selected focus file to the capability domain it belongs to.
/// This keeps work selection tied to the user's goal instead of static file
/// size, TODO counts or audit findings.
fn focus_domain_hint(path:&str)->&'static str {
    let path=path.to_lowercase();
    if path.contains("memory") { "memory" }
    else if path.contains("chat/")||path.contains("context")||path.contains("room") { "context" }
    else if path.contains("learning")||path.contains("experience") { "learning" }
    else if path.starts_with("web/src")||path.contains("/pages/") { "ui" }
    else if path.contains("rsi")||path.contains("algorithm") { "improver" }
    else { "tools" }
}

/// The objective used for capability selection. Focus files are appended as
/// capability words so the selected domain follows the user's actual target.
fn selection_objective(input:&Value)->String {
    // Autonomous campaigns contain a generic system-written recipe with
    // words like "knowledge"/"experience". They are NOT owner constraints:
    // using them as a domain selector permanently biases free exploration.
    // A broad free-survey objective is not a constraint. A measured
    // post-adoption goal, however, is an executable descendant assignment:
    // dropping it would let the old domain-selection score override the
    // independently measured improver -> transfer curriculum.
    let objective=if input["rsiAutonomous"]==true
        && input["rsiSourceScopeExpanded"]!=true {""}
        else {input["rsiObjective"].as_str().unwrap_or("")};
    // Engine-suggested focus files are observations, not owner constraints:
    // otherwise the first heuristic guess feeds back as a permanent objective.
    let hints=if input["rsiAutoFocus"]==true {Vec::new()} else {
        input["rsiFocusFiles"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(focus_domain_hint)
            .collect::<Vec<_>>()
    };
    if hints.is_empty() { objective.to_string() } else { format!("{objective} {}",hints.join(" ")) }
}

/// A learned method is an existing procedure DAG, not a name or a prompt.
/// It runs through the normal tool runtime (approvals, effects and budgets).
fn method_session_running(state: &AppState, session: &Value) -> bool {
    let Some(campaign)=session["rsiCampaignId"].as_str() else {return false};
    session["rsiRun"] == true
        && state.store.get("job",campaign).is_some_and(|job|job["kind"] == "rsi_evolve"
            && job["status"] == "running" && job["cancelRequested"] != true)
        && session["rsiGenerationJobId"].as_str().and_then(|id|state.store.get("job",id))
            .is_some_and(|job|job["kind"] == "rsi_generation" && job["input"]["rsiCampaignId"] == campaign
                && job["status"] == "running" && job["cancelRequested"] != true)
}

pub(crate) fn propose_method(state: &AppState, session_id: &str, body: &Value) -> Result<Value, String> {
    let mut session = state.store.get("session", session_id).ok_or("세션 없음")?;
    if !method_session_running(state,&session) {
        return Err("진행 중인 RSI 세대에서만 절차를 제안할 수 있습니다.".into());
    }
    let name = body["name"].as_str().filter(|s| !s.trim().is_empty() && s.chars().count() <= 100)
        .ok_or("100자 이내의 절차 이름이 필요합니다.")?;
    let domain = body["domain"].as_str().filter(|d| DOMAINS.contains(d)).ok_or("유효한 분야가 필요합니다.")?;
    let verification = crate::engine::procedure::normalize(&body["verification"]);
    if verification["type"] != "procedure"
        || verification["nodes"].as_array().is_none_or(Vec::is_empty)
        || verification["after"].as_array().is_none_or(Vec::is_empty)
        || verification["appliesTo"]["requestContains"].as_array().is_none_or(|terms|
            terms.is_empty() || terms.iter().any(|v| v.as_str().is_none_or(|s| s.trim().is_empty())))
    {
        return Err("적용할 목표(requestContains), 실행 노드, 검증 가능한 후속 조건이 필요합니다.".into());
    }
    for node in verification["nodes"].as_array().unwrap() {
        let tool = node["tool"].as_str().unwrap_or("");
        if !tools::rsi_allowed(tool) || matches!(tool, "rsi_method_propose" | "algorithm_propose") {
            return Err(format!("학습 절차에서 실행할 수 없는 도구: {tool}"));
        }
    }
    for condition in verification["after"].as_array().unwrap() {
        if (condition["kind"] == "file_contains"
            && condition["text"].as_str().is_none_or(str::is_empty))
            || (condition["kind"] == "file_equals" && !condition["text"].is_string())
        {
            return Err("파일 내용 검증에는 명시적인 기대 값이 필요합니다.".into());
        }
    }
    let revision = format!("{:x}", Sha256::digest(json!([domain, &verification]).to_string().as_bytes()));
    let method = json!({"id":format!("rsi-method-{revision}"),"revision":revision,"name":name,
        "domain":domain,"verification":verification,"status":"candidate",
        "assistanceMode":"harness_assisted","description":body["description"],
        "sourceGenerationJobId":session["rsiGenerationJobId"]});
    if !session["rsiMethodProposals"].is_array() { session["rsiMethodProposals"] = json!([]); }
    let proposals = session["rsiMethodProposals"].as_array_mut().unwrap();
    if !proposals.iter().any(|m| m["id"] == method["id"]) { proposals.push(method.clone()); }
    state.store.put("session", &session).ok_or("절차 제안 저장 실패")?;
    Ok(json!({"method":method,"next":"다음 예정된 개발에서 실제 도구로 실행하고 후속 조건과 개발 결과를 확인합니다. 등록은 성과가 아닙니다."}))
}

fn applicable_methods(state: &AppState, partition: &str, domain: &str, objective: &str) -> Vec<Value> {
    let mut methods = Vec::new();
    let records = state.store.all("experience").into_iter()
        .filter(|record|record["partitionKey"] == partition && record["status"] == "active")
        .collect::<Vec<_>>();
    // Re-proposing the identical program in another generation must not erase
    // its failed application history. A revised method has a new content hash.
    let blocked = records.iter().flat_map(|record|record["executableMethods"].as_array().into_iter().flatten())
        .filter(|m|matches!(m["status"].as_str(),Some("suspended" | "held")))
        .filter_map(|m|m["id"].as_str()).collect::<BTreeSet<_>>();
    for record in &records {
        for method in record["executableMethods"].as_array().into_iter().flatten() {
            if method["domain"] != domain
                || blocked.contains(method["id"].as_str().unwrap_or(""))
                || !matches!(method["status"].as_str(), Some("candidate" | "executable"))
                || !crate::engine::procedure::applies(method, objective, None, None) { continue; }
            let mut method = method.clone();
            method["rsiMethodExperienceId"] = record["id"].clone();
            methods.push(method);
        }
    }
    methods.sort_by(|a,b| b["successfulApplications"].as_u64().unwrap_or(0)
        .cmp(&a["successfulApplications"].as_u64().unwrap_or(0))
        .then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut ids = BTreeSet::new();
    methods.retain(|m| ids.insert(m["id"].as_str().unwrap_or("").to_string()));
    methods.truncate(3);
    methods
}

fn method_reuse_decision(state: &AppState, input: &Value, domain: &str) -> Value {
    let methods = applicable_methods(state, &research_scope(input).partition_key, domain,
        input["rsiObjective"].as_str().unwrap_or(""));
    json!({"action":if methods.is_empty(){"research_only"}else{"execute_methods"},
        "methodIds":methods.iter().map(|m|m["id"].clone()).collect::<Vec<_>>()})
}

pub(crate) fn configure_method_session(state: &AppState, input: &Value, session: &mut Value) {
    let domain = input["rsiDomain"].as_str().unwrap_or("");
    session["rsiResearchPartition"] = json!(research_scope(input).partition_key);
    session["rsiDomain"] = json!(domain);
    let mut decision = input["rsiDecisionInput"].clone();
    if !decision.is_object() {decision=json!({});}
    if decision["nextDomain"] != domain || !decision["reuseDecision"].is_object() {
        decision["reuseDecision"] = method_reuse_decision(state,input,domain);
    }
    session["rsiDecisionInput"] = decision;
}

/// Called by the normal chat runtime. Only the parent's concrete reuse choice
/// can inject a learned method; ordinary chats and other repositories cannot.
pub(crate) fn method_strategies(state: &AppState, session: &Value) -> Vec<Value> {
    if !method_session_running(state,session) || session["rsiDecisionInput"]["reuseDecision"]["action"] != "execute_methods" {
        return Vec::new();
    }
    let ids = session["rsiDecisionInput"]["reuseDecision"]["methodIds"].as_array().cloned().unwrap_or_default();
    let prior = state.store.all("execution_operation");
    applicable_methods(state, session["rsiResearchPartition"].as_str().unwrap_or(""),
        session["rsiDomain"].as_str().unwrap_or(""), session["rsiObjective"].as_str().unwrap_or(""))
        .into_iter().filter(|m| ids.contains(&m["id"]) && !prior.iter().any(|op|
            op["sessionId"] == session["id"] && op["origin"] == "procedure"
                && op["procedureStrategyId"] == m["id"]))
        .collect()
}

/// Evaluate the actual tool operations while the detached workspace still
/// exists. Neither a model assertion nor a `loaded` label is execution evidence.
pub(crate) fn collect_method_executions(state: &AppState, session_id: &str, workspace: &Path) {
    let Some(mut session) = state.store.get("session", session_id) else { return };
    let mut receipts = Vec::new();
    for execution in state.store.all("execution").into_iter().filter(|e| e["sessionId"] == session_id) {
        let Ok(mut operations) = state.store.execution_operations(execution["id"].as_str().unwrap_or("")) else {continue};
        crate::engine::procedure::sort_operations(&mut operations);
        for method in execution["strategyCandidates"].as_array().into_iter().flatten()
            .filter(|m| m["rsiMethodExperienceId"].is_string()) {
            let used = operations.iter().filter(|op|op["origin"] == "procedure"
                && op["procedureStrategyId"] == method["id"] && op["procedureRevision"] == method["revision"])
                .cloned().collect::<Vec<_>>();
            if used.is_empty() { continue; }
            let result = crate::engine::procedure::evaluate(method, &operations, workspace);
            receipts.push(json!({"methodId":method["id"],"revision":method["revision"],
                "experienceId":method["rsiMethodExperienceId"],"executionId":execution["id"],
                "operationIds":used.iter().map(|op|op["id"].clone()).collect::<Vec<_>>(),
                "result":result}));
        }
    }
    session["rsiMethodExecutions"] = json!(receipts);
    state.store.put("session", &session);
}

/// Exact source contribution by an inherited procedure, checked against
/// independently adopted bytes. Co-occurring reads or an execution claim do
/// not establish that the learned method produced the accepted source.
pub(crate) fn method_wrote_accepted_candidate(state:&AppState,generation:&Value,receipt:&Value,session_id:&str)->bool {
    if generation["accepted"]!=true {return false;}
    let Some(candidate)=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id)) else{return false;};
    if candidate["status"]!="verified" || !crate::engine::rsi::verified_pair_receipt(state,&candidate) {return false;}
    let ids=receipt["operationIds"].as_array().cloned().unwrap_or_default();
    state.store.execution_operations(receipt["executionId"].as_str().unwrap_or(""))
        .unwrap_or_default().iter().any(|operation| {
            let Some(path)=operation["arguments"]["path"].as_str() else{return false;};
            let Some(sha)=generation["acceptedSourceHashes"][path].as_str() else{return false;};
            ids.contains(&operation["id"]) && operation["sessionId"]==session_id
                && operation["origin"]=="procedure" && operation["procedureStrategyId"]==receipt["methodId"]
                && operation["procedureRevision"]==receipt["revision"] && operation["status"]=="completed"
                && matches!(operation["tool"].as_str(),Some("workspace_write"|"workspace_replace"|"workspace_replace_range"))
                && operation["result"]["written"]==true && operation["toolReceipt"]["verified"]==true
                && operation["toolReceipt"]["observedVersion"]==format!("sha256:{sha}")
                && candidate["paths"].as_array().is_some_and(|paths|paths.iter().any(|entry|entry==path))
        })
}

fn record_method_feedback(state: &AppState, generation: &Value, session_id: &str) -> Vec<Value> {
    let receipts = state.store.get("session", session_id)
        .and_then(|s|s["rsiMethodExecutions"].as_array().cloned()).unwrap_or_default();
    for receipt in &receipts {
        let Some(mut record) = receipt["experienceId"].as_str().and_then(|id|state.store.get("experience",id)) else {continue};
        let Some(method) = record["executableMethods"].as_array_mut().and_then(|methods|
            methods.iter_mut().find(|m|m["id"] == receipt["methodId"] && m["revision"] == receipt["revision"])) else {continue};
        // Recovered coordinator steps must not count a completed application twice.
        if method["applications"].as_array().is_some_and(|rows|rows.iter().any(|r|r["executionId"]==receipt["executionId"])) {continue;}
        let result = &receipt["result"];
        let unknown = result["unknown"] == true || result["status"] == "unknown"
            || result["verified"] != true;
        // One unknown condition must not hide a separately confirmed violation.
        let violated = result["verified"] == true && (result["status"] == "violated"
            || result["violations"].as_array().is_some_and(|rows| !rows.is_empty()));
        let ok = !unknown && !violated && result["followed"] == true;
        let progress = generation["accepted"] == true || generation["newFindings"].as_u64().unwrap_or(0)>0;
        method["status"] = json!(if violated {"suspended"} else if ok && progress {"executable"} else {"held"});
        if ok && generation["accepted"] == true {
            method["successfulApplications"] = json!(method["successfulApplications"].as_u64().unwrap_or(0)+1);
        }
        let application = json!({"jobId":generation["jobId"],"executionId":receipt["executionId"],
            "operationIds":receipt["operationIds"],
            "methodWroteAcceptedCandidate":ok && method_wrote_accepted_candidate(state,generation,receipt,session_id),
            "postconditionsPassed":if !ok && !violated {Value::Null}else{json!(ok)},
            "verificationVerdict":if violated {"violated"}else if ok {"passed"}else{"inconclusive"},
            "accepted":generation["accepted"],"newFindings":generation["newFindings"],
            "tokens":generation["tokens"],"elapsedMs":generation["elapsedMs"],
            "reuse":method["status"],"gain":"observed_association_not_causal_proof"});
        if !method["applications"].is_array() {method["applications"]=json!([]);}
        method["applications"].as_array_mut().unwrap().push(application);
        // Champion recovery synthesis is causal only to the failed procedure
        // OPERATION, not to a model-authored error summary. A revision can
        // acquire a new executable DAG candidate, but no quality credit is
        // awarded until a later run executes and independently evaluates it.
        let method_snapshot=method.clone();
        let operations=state.store.execution_operations(
            receipt["executionId"].as_str().unwrap_or("")
        ).unwrap_or_default();
        let ids=receipt["operationIds"].as_array().cloned().unwrap_or_default();
        let mut candidates=Vec::new();
        for operation in operations.iter().filter(|op|
            ids.contains(&op["id"]) && op["status"]=="failed"
                && op["origin"]=="procedure") {
            for proposed in crate::engine::rsi_repair_synthesis::propose(
                &method_snapshot, operation) {
                if !candidates.iter().any(|existing:&Value| existing["id"]==proposed["id"]) {
                    candidates.push(proposed);
                }
                if candidates.len()>=4 {break;}
            }
            if candidates.len()>=4 {break;}
        }
        let mut added=Vec::new();
        if let Some(methods)=record["executableMethods"].as_array_mut() {
            for candidate in candidates {
                if methods.iter().any(|method|method["id"]==candidate["id"]) {continue;}
                added.push(candidate["id"].clone());
                methods.push(candidate);
            }
        }
        if !added.is_empty() {
            state.store.audit("rsi.executable_recovery_synthesized",
                json!({"executionId":receipt["executionId"],
                    "parentMethodId":receipt["methodId"],"candidateIds":added,
                    "qualityProven":false}));
        }
        state.store.put("experience", &record);
    }
    receipts
}

pub(crate) fn preserve_failed_research(state:&AppState, source:&str) {
    let mut jobs=state.store.all("job").into_iter().filter(|job|
        job["kind"]=="rsi_generation" && job["input"]["rsiKnowledgeSource"]==source
        && (job["status"]!="completed" || job["result"]["rsi"]["candidateId"].is_null())
        && !matches!(job["status"].as_str(),Some("running"|"queued"))).take(12).collect::<Vec<_>>();
    jobs.reverse();
    for job in jobs {
        if let (Some(session),Some(id))=(job["sessionId"].as_str(),job["id"].as_str()) {
            let scope=research_scope(&job["input"]);
            let existing=format!("experience-rsi-{}",crate::engine::memory::stable_id(&[&scope.partition_key,id]));
            if state.store.get("experience",&existing).is_some_and(|e|e["failureBackfilled"]==true){continue;}
            if let Some(saved)=remember_research(state,&job["input"],&json!({"jobId":id,"accepted":false,"domain":job["rsiDomain"],
                "candidateId":job["result"]["rsi"]["candidateId"],"error":job["error"]}),session) {
                state.store.patch_if("experience",&saved,"id",&json!(saved),&json!({"failureBackfilled":true}));
            }
        }
    }
}

fn research_artifacts(state:&AppState,session:&str)->Vec<Value>{
    let mut operations=state.store.all("execution_operation");
    operations.sort_by(|a,b|a["opSeq"].as_u64().cmp(&b["opSeq"].as_u64()).then(a["id"].as_str().cmp(&b["id"].as_str())));
    let mut artifacts=Vec::new();
    for op in operations {
        if op["sessionId"]!=session {continue;}
        let result=&op["result"];let args=&op["arguments"];
        if op["status"]!="completed" || result["error"].is_string() || result["ok"]==false {
            let content=json!({"tool":op["tool"],"arguments":args,"error":op["error"],"result":result}).to_string();
            let hash=format!("{:x}",Sha256::digest(json!([&op["tool"],args,&op["error"],&result["error"],&op["status"]]).to_string().as_bytes()));
            artifacts.push(json!({"tool":"tool_failure","path":args["path"],"content":content.chars().take(4000).collect::<String>(),
                "verified":false,"operationId":op["id"],"hash":hash}));
            continue;
        }
        let tool=op["tool"].as_str().unwrap_or("");
        let content=match tool {
            "workspace_read_range"|"http_fetch"=>result["content"].as_str().or_else(||result["text"].as_str()).unwrap_or("").to_string(),
            "code_search" if result["matchCount"].as_u64().unwrap_or(0)>0=>result["matches"].to_string(),
            "web_search" if result["results"].as_array().is_some_and(|rows|!rows.is_empty())=>result["results"].to_string(),
            "rsi_method_propose" if result["method"]["verification"].is_object()=>result["method"]["verification"].to_string(),
            "algorithm_propose" if result["algorithm"]["parameters"].is_object()=>result["algorithm"]["parameters"].to_string(),
            _=>continue,
        };
        if content.trim().is_empty(){continue;}
        let text=content.chars().take(24_000).collect::<String>();
        let body=json!({"tool":tool,"path":args["path"],"url":args["url"],"query":args["query"],"truncated":content.chars().count()>24_000,
            "startLine":result["startLine"],"endLine":result["endLine"],"sourceSha256":result["sourceSha256"],"content":text});
        // Queries, timing and operation IDs are provenance, not new findings.
        let hash=format!("{:x}",Sha256::digest(json!([tool,&body["path"],&body["url"],&body["sourceSha256"],&body["content"]]).to_string().as_bytes()));
        if artifacts.iter().any(|item:&Value|item["hash"]==hash){continue;}
        let mut artifact=body;artifact["hash"]=json!(hash);artifact["operationId"]=op["id"].clone();artifacts.push(artifact);
    }
    // Keep a bounded set of actual findings, not just their call counters.
    if artifacts.len()>40 {artifacts.drain(..artifacts.len()-40);}
    artifacts
}

/// Persist concrete research even without a patch. Accepted source references
/// carry independent evaluation evidence; research is never labelled success.
pub(crate) fn remember_research(state:&AppState,input:&Value,generation:&Value,session:&str)->Option<String>{
    let mut artifacts=research_artifacts(state,session);
    if let Some(note)=generation["hypothesisMessageId"].as_str().and_then(|id|state.store.message(id))
        .filter(|message|message["session_id"]==session && message["role"]=="assistant") {
        if let Some(content)=note["content"].as_str().filter(|text|!text.trim().is_empty()) {
            let text=content.chars().take(1500).collect::<String>();
            artifacts.push(json!({"tool":"model_hypothesis","content":text,"verified":false,"sourceMessageId":note["id"],
                "hash":format!("{:x}",Sha256::digest(content.as_bytes()))}));
        }
    }
    // Interrupted reasoning can contain the next exact implementation plan.
    // Keep it as an unverified hypothesis, anchored to an observed source hash.
    if let Some(message)=state.store.messages(session).iter().rev().find(|m|
        m["role"]=="assistant" && m["reasoning"].as_str().is_some_and(|s|s.len()>100)).cloned() {
        let path=input["rsiFocusFiles"].as_array().and_then(|files|files.first()).cloned().unwrap_or(Value::Null);
        let source_hash=artifacts.iter().rev().find(|a|a["tool"]=="workspace_read_range" && a["path"]==path)
            .map(|a|a["sourceSha256"].clone()).unwrap_or(Value::Null);
        let content=message["reasoning"].as_str().unwrap_or("");
        artifacts.insert(0,json!({"tool":"model_hypothesis","path":path,"sourceSha256":source_hash,
            "content":content.chars().rev().take(12_000).collect::<String>().chars().rev().collect::<String>(),
            "verified":false,"sourceMessageId":message["id"],"hash":format!("{:x}",Sha256::digest(content.as_bytes()))}));
    }
    let candidate=generation["candidateId"].as_str().and_then(|id|state.store.get("rsi_candidate",id));
    let proposals = state.store.get("session",session)
        .and_then(|s|s["rsiMethodProposals"].as_array().cloned()).unwrap_or_default();
    if artifacts.is_empty() && candidate.is_none() && proposals.is_empty(){return None;}
    let scope=research_scope(input);let id=format!("experience-rsi-{}",crate::engine::memory::stable_id(&[&scope.partition_key,generation["jobId"].as_str()?]));
    let previous=state.store.get("experience",&id).unwrap_or(Value::Null);
    let mut methods=previous["executableMethods"].as_array().cloned().unwrap_or_default();
    for method in proposals {
        if !methods.iter().any(|m|m["id"]==method["id"]) {methods.push(method);}
    }
    let accepted=generation["accepted"]==true && candidate.as_ref().is_some_and(|c|c["status"]=="verified" && c["evaluation"]["metrics"]["eligible"]==true);
    let lesson=candidate.as_ref().and_then(|c|c["hypothesis"].as_str()).unwrap_or("검증 전 연구 자료. 출처와 현재 코드 버전을 확인하고 다음 구현에 활용하세요.");
    let mut applications=state.store.get("session",session).and_then(|s|s["rsiKnowledgeReuse"].as_array().cloned()).unwrap_or_default();
    let writes=state.store.all("execution_operation").into_iter().filter(|op|op["sessionId"]==session && op["status"]=="completed" && matches!(op["tool"].as_str(),Some("workspace_write"|"workspace_replace"|"workspace_replace_range"))).collect::<Vec<_>>();
    for receipt in &mut applications {
        let actions=writes.iter().filter(|op|op["arguments"]["path"]==receipt["path"]).map(|op|op["id"].clone()).collect::<Vec<_>>();
        receipt["status"]=json!(if actions.is_empty(){"provided"}else if accepted{"related_verified_change"}else{"related_change_unverified"});
        receipt["actionOperationIds"]=json!(actions);
    }
    if let Some(mut saved)=state.store.get("session",session){saved["rsiKnowledgeReuse"]=json!(applications);state.store.put("session",&saved);}
    let record=json!({"id":id,"scope":"shared","partitionKey":scope.partition_key,"sessionId":scope.session_id,
        "request":input["rsiObjective"],"signature":input["rsiObjective"],"lesson":lesson,"avoid":"출처 없는 주장·도구 사용 횟수만으로 능력이나 성과를 판정하지 말 것",
        "outcome":if accepted{"success"}else{"research"},"status":"active","origin":"rsi","uses":previous["uses"].as_u64().unwrap_or(0),"occurrences":1,
        "researchArtifacts":artifacts,"executableMethods":methods,
        "generationJobId":generation["jobId"],"domain":generation["domain"],
        "implementation_ref":if accepted {json!({"kind":"source_revision","commit":generation["acceptedCommit"],"paths":candidate.as_ref().map(|c|&c["paths"])})}else{Value::Null},
        "evidence":{"candidateId":generation["candidateId"],"evaluation":generation["evaluation"],"accepted":accepted,"failure":generation["error"],"captureError":generation["captureError"]},
        "knowledgeApplications":applications,"laterGain":"unmeasured",
        "createdAt":previous["createdAt"].as_str().map(str::to_string).unwrap_or_else(||chrono::Utc::now().to_rfc3339()),"lastSeenAt":chrono::Utc::now().to_rfc3339()});
    state.store.put("experience",&record)?;
    state.bus.record("experience",&record);Some(id)
}

/// Algorithms proposed by the current RSI generation through the
/// `algorithm_propose` tool. Only the stored records are returned; a proposal
/// is never treated as adopted before deterministic replay evaluation.
fn proposed_algorithms(state: &AppState, session: &str) -> Vec<Value> {
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
fn adaptive_search_expression(history:&[Value])->Option<String>{
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
fn recovery_search_expression(history:&[Value])->Option<String>{
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

fn queue_adaptive_search_trial(
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
fn queue_search_trial(
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
fn pending_trial_candidate(
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
fn comparable_trial_baseline(
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

fn trial_verdict(generation: &Value, baseline: &Value) -> (String, Value) {
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
fn record_trial_outcome(
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

/// Re-reads only the stored line range from the current source and formats it
/// exactly like `workspace_read_range`. It does not fabricate a model read.
fn numbered_source_range(text: &str, start: u64, end: u64) -> (String, u64, u64) {
    let lines = text.lines().collect::<Vec<_>>();
    let begin = (start.max(1) as usize).min(lines.len());
    let end = (end.max(start) as usize)
        .min(lines.len())
        .min(begin.saturating_add(400));
    let mut numbered = String::new();
    for (offset, line) in lines[begin.saturating_sub(1)..end].iter().enumerate() {
        numbered.push_str(&format!("{:>5} {}\n", begin + offset, line));
    }
    (numbered, begin as u64, end as u64)
}

fn unavailable_research_receipt(mut receipt: Value, reason: &str) -> Value {
    receipt["mode"] = json!("source_range_unavailable");
    receipt["status"] = json!("skipped");
    receipt["reason"] = json!(reason);
    receipt["sourceValidity"] = json!("source_unavailable");
    receipt["hypothesisValidity"] = json!("prior_hypothesis_unverified");
    receipt
}

/// Execute the learned inspection strategy before asking the model to discover
/// the same source again. Matching hashes are reused as-is. When the file has
/// changed, only the stored line range is re-read and refreshed; the research
/// record and its provenance are kept, and the refresh is labelled as an engine
/// revalidation rather than a model read.
pub(crate) fn reuse_research(state:&AppState,input:&Value,workspace:&Path,session:&str)->String{
    let scope=research_scope(input);let objective=input["rsiObjective"].as_str().unwrap_or("");
    let query=format!("{} {}",objective,input["rsiDomain"].as_str().unwrap_or(""));
    let records=experience::retrieve(state,&scope,&query,3);
    let root=match workspace.canonicalize(){Ok(root)=>root,Err(_)=>return String::new()};
    let mut rows=Vec::new();let mut receipts=Vec::new();let mut chars=0;
    let max_rows=if input["rsiSystemEvolution"]==true {8}else{3};
    let max_chars=if input["rsiSystemEvolution"]==true {48_000}else{16_000};
    let mut refreshes=BTreeMap::<String,Vec<Value>>::new();
    for record in records {
        let record_id=record["id"].as_str().unwrap_or("").to_string();
        for artifact in record["researchArtifacts"].as_array().into_iter().flatten(){
            if rows.len()>=max_rows {break;}
            let mut receipt=json!({"experienceId":record["id"],"artifactHash":artifact["hash"],"sourceOperationId":artifact["operationId"],"sourceMessageId":artifact["sourceMessageId"],"path":artifact["path"],"status":"provided"});
            let mut item=artifact.clone();
            match artifact["tool"].as_str(){
                Some("workspace_read_range")=>{
                    let Some(relative)=artifact["path"].as_str().filter(|p|crate::engine::rsi::safe_path(p)) else{
                        receipts.push(unavailable_research_receipt(receipt,"unsafe_path"));continue;
                    };
                    let Ok(path)=root.join(relative).canonicalize() else{
                        receipts.push(unavailable_research_receipt(receipt,"source_missing"));continue;
                    };
                    if !path.starts_with(&root){
                        receipts.push(unavailable_research_receipt(receipt,"outside_workspace"));continue;
                    }
                    let Ok(meta)=path.metadata() else{
                        receipts.push(unavailable_research_receipt(receipt,"source_missing"));continue;
                    };
                    if !meta.is_file()||meta.len()>1_500_000 {
                        receipts.push(unavailable_research_receipt(receipt,"source_too_large"));continue;
                    }
                    let Ok(bytes)=std::fs::read(path) else{
                        receipts.push(unavailable_research_receipt(receipt,"source_unreadable"));continue;
                    };
                    let digest=format!("{:x}",Sha256::digest(&bytes));
                    if artifact["sourceSha256"]==json!(digest) {
                        receipt["mode"]=json!("hash_verified_source_cache");
                        receipt["sourceValidity"]=json!("hash_verified_current_source");
                        receipt["hypothesisValidity"]=json!("prior_hypothesis_unverified");
                    } else {
                        let (Some(start),Some(end))=(artifact["startLine"].as_u64(),artifact["endLine"].as_u64()) else{
                            receipts.push(unavailable_research_receipt(receipt,"line_range_missing"));continue;
                        };
                        let Ok(text)=std::str::from_utf8(&bytes) else{
                            receipts.push(unavailable_research_receipt(receipt,"source_not_utf8"));continue;
                        };
                        let (content,begin,end)=numbered_source_range(text,start,end);
                        if content.is_empty() {
                            receipts.push(unavailable_research_receipt(receipt,"line_range_empty"));continue;
                        }
                        let truncated=content.chars().count()>2200;
                        let content=content.chars().take(2200).collect::<String>();
                        item["previousSourceSha256"]=artifact["sourceSha256"].clone();
                        item["sourceSha256"]=json!(digest);
                        item["content"]=json!(content);
                        item["startLine"]=json!(begin);
                        item["endLine"]=json!(end);
                        item["truncated"]=json!(truncated);
                        item["refreshedAt"]=json!(chrono::Utc::now().to_rfc3339());
                        item["refreshReason"]=json!("source_changed");
                        item["refreshKind"]=json!("line_range_revalidation");
                        item["refreshCount"]=json!(artifact["refreshCount"].as_u64().unwrap_or(0)+1);
                        // The refreshed lines are current source, not proof
                        // that the old hypothesis still means the same thing.
                        item["hypothesisValidity"]=json!("unverified_after_source_change");
                        receipt["mode"]=json!("refreshed_source_cache");
                        receipt["previousSourceSha256"]=artifact["sourceSha256"].clone();
                        receipt["sourceSha256"]=json!(digest);
                        receipt["refreshed"]=json!(true);
                        receipt["sourceValidity"]=json!("refreshed_range_current_source");
                        receipt["hypothesisValidity"]=json!("unverified_after_source_change");
                        refreshes.entry(record_id.clone()).or_default().push(item.clone());
                    }
                    // The durable excerpt is small, but its validated range is
                    // a reusable read plan. Rehydrate that bounded range from
                    // bytes already read for the hash check; do not ask a model
                    // to discover/read the missing tail again.
                    if item["truncated"]==true {
                        if let (Ok(text),Some(start),Some(end))=(std::str::from_utf8(&bytes),item["startLine"].as_u64(),item["endLine"].as_u64()) {
                            let (complete,begin,last)=numbered_source_range(text,start,end);
                            if !complete.is_empty() && complete.chars().count()<=24_000 {
                                item["content"]=json!(complete);item["startLine"]=json!(begin);item["endLine"]=json!(last);
                                item["truncated"]=json!(false);receipt["rangeHydrated"]=json!(true);
                                receipt["deliveredChars"]=json!(complete.chars().count());
                            }
                        }
                    }
                    receipt["entrypoint"]=json!("rsi_evolve::reuse_research");
                }
                Some("http_fetch"|"web_search")=>{
                    receipt["mode"]=json!("prior_external_research_unverified");
                    receipt["sourceValidity"]=json!("unverified_external_source");
                    receipt["hypothesisValidity"]=json!("prior_external_research_unverified");
                }
                Some("model_hypothesis"|"tool_failure")=>{
                    if let (Some(relative),Some(expected))=(artifact["path"].as_str(),artifact["sourceSha256"].as_str()) {
                        let current=root.join(relative).canonicalize().ok().filter(|p|p.starts_with(&root))
                            .and_then(|p|std::fs::read(p).ok()).map(|bytes|format!("{:x}",Sha256::digest(bytes)));
                        receipt["sourceValidity"]=json!(if current.as_deref()==Some(expected){"hash_verified_current_source"}else{"source_changed_hypothesis_unverified"});
                    }
                    receipt["mode"]=json!("prior_model_hypothesis_unverified");
                    receipt["hypothesisValidity"]=json!("prior_hypothesis_unverified");
                }
                _=>continue,
            }
            if item["path"].is_string() && item["truncated"]==false && rows.iter().any(|row:&Value|
                row["path"]==item["path"] && row["truncated"]==false
                    && row["startLine"].as_u64()<=item["startLine"].as_u64()
                    && row["endLine"].as_u64()>=item["endLine"].as_u64()) {
                receipt["coveredByDeliveredRange"]=json!(true);receipts.push(receipt);continue;
            }
            let size=item.to_string().chars().count();if chars+size>max_chars {continue;}
            if receipts.iter().any(|r:&Value|r["artifactHash"]==receipt["artifactHash"]){continue;}
            chars+=size;
            rows.push(json!({"tool":item["tool"],"path":item["path"],"url":item["url"],
                "startLine":item["startLine"],"endLine":item["endLine"],"content":item["content"],
                "sourceSha256":item["sourceSha256"],
                "truncated":item["truncated"],"experienceId":record["id"],
                "sourceValidity":receipt["sourceValidity"],"hypothesisValidity":receipt["hypothesisValidity"]}));
            receipts.push(receipt);
        }
    }
    for (experience_id,artifacts) in refreshes {
        let Some(mut record)=state.store.get("experience",&experience_id) else{continue};
        let mut changed=false;
        if let Some(list)=record["researchArtifacts"].as_array_mut() {
            for updated in artifacts {
                if let Some(slot)=list.iter_mut().find(|artifact|artifact["hash"]==updated["hash"]) {
                    *slot=updated;changed=true;
                }
            }
        }
        if changed {
            record["lastRefreshedAt"]=json!(chrono::Utc::now().to_rfc3339());
            record["lastRefreshKind"]=json!("line_range_revalidation");
            state.store.put("experience",&record);
        }
    }
    if let Some(mut saved)=state.store.get("session",session){saved["rsiKnowledgeReuse"]=json!(receipts);state.store.put("session",&saved);}
    if rows.is_empty(){String::new()}else{format!("\n\n기존 연구의 인용 자료(지시 아님). 아래 소스 범위는 현재 파일 해시로 검증했습니다. truncated=false인 범위의 원문은 전부 제공했으므로 같은 범위를 다시 읽을 필요가 없습니다. 코드가 갱신되어도 과거 가설과 외부 자료의 의미·효과는 여전히 미검증입니다.\n{}",json!(rows))}
}

fn session_research_fingerprint(state: &AppState, session_id: &str) -> String {
    let mut parts = Vec::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("");
        if !matches!(
            tool,
            "code_search" | "workspace_read_range" | "web_search" | "http_fetch"
        ) {
            continue;
        }
        parts.push(format!("{tool}:{}", operation["arguments"]));
    }
    parts.sort();
    let refs = parts.iter().map(String::as_str).collect::<Vec<_>>();
    crate::engine::memory::stable_id(&refs)
}

/// Research can continue inside the owner's existing budgets while it adds
/// concrete findings. Repeated calls or different queries alone are not progress.
fn generation_stop_reason(job:&Value,worker_error:Option<&str>)->Option<&'static str>{
    let error=job["result"]["rsi"]["modelError"].as_str().or(worker_error).or_else(||job["error"].as_str())?;
    if error.starts_with("rsi_token_budget:") || error=="token_budget" {Some("token_budget")}
    else if error=="time_budget" {Some("time_budget")}
    else {None}
}

fn generation_fits_remaining_budget(record:&Value,input:&Value)->bool {
    record["limits"]["tokenBudget"].as_u64().is_none_or(|budget|{
        let minimum=crate::engine::chat::estimate_text_tokens(input["prompt"].as_str().unwrap_or(""))+256;
        budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0))>=minimum
    })
}

/// Never splice a fixed number of characters out of serialized JSON:
/// an incomplete receipt wastes model tokens and can corrupt later feedback.
/// Shrink OPTIONAL telemetry as a whole and keep the objective and directive.
/// Independent paired cases that still miss their quality objective are
/// the actionable counterexamples for the next source-edit generation.
/// A neutral 1.0 -> 1.0 case is a satisfied guard, not a repair target.
fn unresolved_quality_case(case:&Value)->bool {
    case["metrics"].as_array().is_some_and(|metrics|metrics.iter().any(|metric|
        metric["key"]=="quality" && metric["improved"]==false
            && metric["regressed"]==false
            && metric["baseline"].as_f64().is_some_and(|value|value<1.0)
            && metric["candidate"].as_f64()==metric["baseline"].as_f64()))
}

fn fit_worker_prompt(
    base: &str, domain: &str, capability: &str, history: &[Value],
    remaining_tokens: Option<u64>,
) -> Result<(String, Value), String> {
    let directive = format!(
        "\n이번 탐색 분야: {domain}. 사용자 목표와 연결된 능력: {capability}. 전체 제품 기능·화면·외부 연결도 개선 대상입니다. 과거 성공·실패와 측정값을 참고하되 관측되지 않은 성능 향상을 주장하지 마세요. 이전 paired 사례의 baseline과 candidate가 같았다면 성공했다고 반복하지 말고, 그 사례를 바꿀 다른 소스 수정 가설을 실제로 구현하세요."
    );
    let full = history.iter().rev().take(8).map(|row| {
        let measured=row["evaluation"]["metrics"]["measurements"].as_array();
        let altered=measured.into_iter().flatten().filter(|case|case["metrics"].as_array()
            .is_some_and(|metrics|metrics.iter().any(|m|
                m["improved"]==true || m["regressed"]==true))).take(3).cloned();
        // Zero-gain is evidence too. Previously a successful compilation
        // followed by 13 unchanged paired cases was handed to the successor
        // as an EMPTY measurement set, causing blind repeat attempts.
        let mut cases=altered.collect::<Vec<_>>();
        // Prioritize a genuinely unsolved quality task before saturated
        // guards (e.g. 0.75 -> 0.75 before 1.0 -> 1.0).
        for case in measured.into_iter().flatten()
            .filter(|case|unresolved_quality_case(case))
            .chain(measured.into_iter().flatten().filter(|case|
                case["metrics"].as_array().is_some_and(|metrics|!metrics.is_empty()))) {
            if cases.len()>=3 {break;}
            if !cases.iter().any(|previous|previous["id"]==case["id"]) {
                cases.push(case.clone());
            }
        }
        json!({
            "domain":row["domain"],"accepted":row["accepted"],
            "tokens":row["tokens"],"elapsedMs":row["elapsedMs"],
            "verdict":row["evaluation"]["verdict"],"measurements":cases,
            "error":row["error"].as_str().map(|error|
                error.chars().take(240).collect::<String>()),
            // A descendant must know the real compiler failure from the
            // parent run, not merely that "no candidate" was adopted.
            "compilerDiagnostic":row["verification"]["checks"].as_array()
                .into_iter().flatten().find(|check|check["passed"]!=true)
                .and_then(|check|check["detail"].as_str())
                .map(|error|error.chars().take(780).collect::<String>()),
            "repairAttempts":row["verification"]["repairAttempts"]
        })
    }).collect::<Vec<_>>();
    let compact = history.iter().rev().take(2).map(|row|json!({
        "domain":row["domain"],"accepted":row["accepted"],
        "tokens":row["tokens"],"verdict":row["evaluation"]["verdict"],
        // Compact mode must still disclose at least one unchanged measured
        // case; otherwise low token budgets silently erase failure feedback.
        "pairedCase":row["evaluation"]["metrics"]["measurements"].as_array()
            .and_then(|cases|cases.iter().find(|case|unresolved_quality_case(case))
                .or_else(||cases.iter().find(|case|
                    case["metrics"].as_array().is_some_and(|metrics|!metrics.is_empty()))))
            .map(|case|json!({"id":case["id"],"domain":case["domain"],
                "metrics":case["metrics"]})),
        "error":row["error"].as_str().map(|error|
            error.chars().take(120).collect::<String>()),
        "compilerDiagnostic":row["verification"]["checks"].as_array()
            .into_iter().flatten().find(|check|check["passed"]!=true)
            .and_then(|check|check["detail"].as_str())
            .map(|error|error.chars().take(450).collect::<String>())
    })).collect::<Vec<_>>();
    for (mode, rows) in [("full",full),("compact",compact),("deferred",Vec::new())] {
        let mut prompt = format!("{base}{directive}");
        if !rows.is_empty() {
            prompt.push_str("\n이 실험 계보의 과거 결과: ");
            prompt.push_str(&json!(rows).to_string());
        } else if !history.is_empty() {
            prompt.push_str("\n과거 세대의 상세 지표는 작업 저장소에 보존되어 있습니다.");
        }
        let estimate = crate::engine::chat::estimate_text_tokens(&prompt);
        if remaining_tokens.is_none_or(|limit|estimate.saturating_add(256)<=limit) {
            return Ok((prompt,json!({
                "mode":mode,"feedbackRows":rows.len(),
                "availableRows":history.len(),"estimatedInputTokens":estimate,
                "ownerObjectivePreserved":true
            })));
        }
    }
    Err("rsi_token_budget: 목표와 핵심 지시문을 유지할 입력 예산이 부족합니다.".into())
}

/// An opted-in broad RSI campaign must not keep optimizing a now-satisfied
/// single-file fixture after a verified adoption. Choose the next challenge
/// from REAL frozen paired measurements and rescan the descendant's source.
/// Neither the new goal nor a quality gap counts as an adopted generation.
fn post_adoption_objective(input: &Value, history: &[Value]) -> Option<Value> {
    if input["rsiExpandAfterGain"] != true { return None; }
    let accepted = history.iter().rev().find(|row|
        row["accepted"] == true && row["evaluation"]["metrics"]["eligible"] == true)?;
    let residuals = accepted["evaluation"]["metrics"]["measurements"].as_array()
        .into_iter().flatten()
        .filter_map(|case| {
            let gaps = case["metrics"].as_array()?.iter()
                .filter(|metric|metric["key"] == "quality"
                    && metric["candidate"].as_f64().is_some_and(|value|value < 1.0)
                    && metric["improved"] != true)
                .map(|metric|json!({"metric":"quality",
                    "observed":metric["candidate"],"ceiling":1.0}))
                .collect::<Vec<_>>();
            if gaps.is_empty() {return None;}
            Some(json!({"caseId":case["id"],"domain":case["domain"],
                "gaps":gaps}))
        }).take(5).collect::<Vec<_>>();
    // Most fixed-quality oracles saturate at 1.0 after the first gain.
    // Rank real unoptimized cost measurements instead of blindly asking
    // the descendant to improve an already-satisfied accuracy fixture.
    let mut costs=Vec::<Value>::new();
    for case in accepted["evaluation"]["metrics"]["measurements"]
        .as_array().into_iter().flatten() {
        let frozen=input["rsiFrozenCases"].as_array().into_iter().flatten()
            .find(|row|row["id"]==case["id"]);
        for metric in case["metrics"].as_array().into_iter().flatten() {
            let key=metric["key"].as_str().unwrap_or("");
            if metric["direction"]!="lower" || metric["improved"]==true {continue;}
            let Some(observed)=metric["candidate"].as_f64()
                .filter(|v|v.is_finite() && *v>0.0) else {continue;};
            let Some(spec)=frozen.and_then(|row|row["metrics"].as_array())
                .and_then(|metrics|metrics.iter().find(|row|row["key"]==key))
                else {continue;};
            let relative=spec["minRelativeGain"].as_f64().unwrap_or(0.05);
            let absolute=spec["minAbsoluteGain"].as_f64().unwrap_or(0.0);
            let needed=(observed*relative).max(absolute);
            if needed<=0.0 {continue;}
            costs.push(json!({"caseId":case["id"],"domain":case["domain"],
                "metric":key,"observed":observed,
                "requiredReduction":needed,"qualityGuard":"no_regression"}));
        }
    }
    // Token/context reduction has a more stable, directly attributable
    // counter than noisy wall-clock timing. Within a metric, prioritize
    // the largest measured costs; the fixed evaluator still decides adoption.
    costs.sort_by(|a,b| {
        let rank=|row:&Value|if row["metric"]=="contextChars" {0}else{1};
        rank(a).cmp(&rank(b)).then_with(||
            b["observed"].as_f64().unwrap_or(0.0)
                .total_cmp(&a["observed"].as_f64().unwrap_or(0.0)))
    });
    costs.truncate(5);
    // An actually recursive demonstration needs an improved improver,
    // followed by a new independently measured capability. A first gain in
    // ordinary application logic does not satisfy this causal transfer.
    // Use paired meta evidence only to select the next target, NEVER to
    // count an unverified adoption or loosen the frozen evaluation.
    let meta_seen=history.iter().rev().take_while(|row|row["accepted"]==true)
        .any(|row|row["evaluation"]["metrics"]["eligible"]==true
            && row["evaluation"]["metrics"]["metaHoldoutGains"]
                .as_u64().unwrap_or(0)>0);
    // Once the improver itself has a measured gain, prioritize a second
    // domain with a known source transformation, if the native source
    // generator was opted into the campaign. Its benefit is still entirely
    // decided by the independent frozen evaluator.
    if meta_seen && input["rsiNativeInduction"]==true {
        costs.sort_by_key(|row| {
            if row["domain"]=="memory" && row["metric"]=="elapsedMs" {0}
            else if row["domain"]=="context" && row["metric"]=="contextChars" {1}
            else {2}
        });
    }
    let meta_cost=if meta_seen {None} else {
        costs.iter().find(|row|row["domain"]=="improver")
    };
    let objective = if let Some(target)=meta_cost {
        format!("먼저 개선기 자체의 탐색·선택 알고리즘을 더 효율적으로 발전시킨다. 독립 동결 기준의 메타 병목은 {} 이며 비용 감소 목표는 최소 {}이다. src/engine/rsi_evolve.rs 등 실제 개선기 소스의 반복 계산과 증거 처리 방식을 측정 가능한 새 실행 방식으로 수정한다. 독립 메타 holdout 승격 전에는 성과라고 보고하지 않는다. 이후에야 다른 능력으로 전이한다.",
            json!(target),target["requiredReduction"])
    } else if !residuals.is_empty() {
        format!("동결 평가의 미해결 정확도 결함 {} 중 하나를 실제 구현으로 개선하고 이전 세대의 채택 성과를 유지한다.",
            json!(residuals))
    } else if let Some(target)=costs.first() {
        format!("동결 A/B에서 다음 비용 병목을 계측했다: {}. 해당 도메인의 실제 소스를 개선하여 비용을 최소 {}만큼 줄이고 다른 품질·기능 회귀 없이 독립 재검증한다. 실패할 경우 다른 독립 지표를 탐색한다.",
            json!(target),target["requiredReduction"])
    } else {
        "기존 채택을 유지하면서 동결된 정확도·비용 지표에서 실제 신규 개선 기회를 발견하고 실행한다. 포화된 품질의 반복 수정은 피한다.".to_string()
    };
    Some(json!({
        "objective":objective,
        "metaTransferTarget":meta_cost.is_some(),
        "residuals":residuals,
        "costOpportunities":costs,
        "previousAcceptedCommit":accepted["acceptedCommit"],
        "prompt":format!(
            "DOMINION 연속 자기개선의 후손 세대입니다. 직전 채택 커밋은 {} 입니다. 이진 실행기와 작업 기준 코드는 이미 이 커밋에서 재빌드되었습니다. {} 현재 소스 인덱스로 다른 분야의 원인 파일을 찾고, 안전한 분리 작업공간에서 실제 동작/비용을 바꾸는 최소 구현을 작성하세요. 소스 조사로 끝내지 말고 검증 가능한 패치를 남기세요. 동일한 분해 버그의 재수정은 하지 마세요. 패치가 새로운 동결 평가 개선을 입증하기 전에는 성능 향상이라고 주장하지 마세요.",
            accepted["acceptedCommit"].as_str().unwrap_or(""),
            objective)
    }))
}

fn should_stop_no_progress(
    streak: u64, research: bool, changed: bool,
    pending_trial: bool, autonomous: bool,
) -> bool {
    // Explicit/manual campaigns keep their previous early-stop semantics.
    if !autonomous {return streak>=1 && (!research || !changed);}
    // A bounded autonomous campaign gets time to turn real, barren feedback
    // into a different strategy and run the pending trial. Never loop forever.
    if streak>=6 {return true;}
    if pending_trial {return false;}
    if research && changed {return false;}
    streak>=3
}

fn new_research_findings(state:&AppState,input:&Value,job_id:&str,hashes:&[Value],history:&[Value])->usize{
    let mut known=history.iter().flat_map(|g|g["knowledgeArtifactHashes"].as_array().into_iter().flatten()).filter_map(Value::as_str).map(str::to_string).collect::<BTreeSet<_>>();
    for experience in state.store.all("experience").into_iter().filter(|e|
        e["status"]=="active" && e["generationJobId"]!=job_id
            && e["partitionKey"]==research_scope(input).partition_key) {
        known.extend(experience["researchArtifacts"].as_array().into_iter().flatten().filter_map(|a|a["hash"].as_str().map(str::to_string)));
    }
    hashes.iter().filter(|hash|hash.as_str().is_some_and(|hash|!known.contains(hash))).count()
}

fn session_tool_usage(state: &AppState, session_id: &str) -> Vec<Value> {
    let mut counts = BTreeMap::<String, u64>::new();
    for operation in state.store.all("execution_operation") {
        if operation["sessionId"].as_str() != Some(session_id)
            || operation["status"].as_str() != Some("completed")
        {
            continue;
        }
        let tool = operation["tool"].as_str().unwrap_or("unknown").to_string();
        *counts.entry(tool).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(tool, calls)| json!({"tool": tool, "calls": calls}))
        .collect()
}

/// Turn a real, compiler-passing but neutral descendant into an editable
/// starting point for its successor. Rejection remains rejection: this never
/// changes the accepted HEAD or promotes an unmeasured patch.
/// A neutral inherited patch changes the disposable checkout *after* the
/// coordinator has indexed the pristine base. Source excerpts and old read
/// ranges therefore refer to the WRONG bytes for the descendant. Let the
/// descendant obtain a fresh source SHA from its actual inherited worktree.
/// Keep paired failure evidence, but never reuse stale source as edit context.
fn generation_prompt_variants(
    original: &str, inherited: &str, digest: &str, focus: &str,
    neutral_id: Option<&str>,
) -> Vec<(&'static str, String, bool, bool)> {
    if let Some(id) = neutral_id {
        let note = format!(
            "\n\n### 실제 중립 소스 후보의 후손 교정\n후보 {id}의 컴파일 통과·독립 품질 무개선 패치가 이 세대 작업공간에 이미 적용됩니다. 채택된 패치는 아닙니다. 기존 기준 소스 발췌와 과거 읽기 범위의 줄 번호/SHA는 이 작업공간과 다를 수 있습니다. 먼저 workspace_read_range로 현재 함수 원문과 sourceSha256을 새로 읽으세요. 수정할 것은 이전에 실패한 행동이며, 부분적인 문법 수정이나 동일 패치 재제출이 아닙니다. 이전 후보를 보존한 채 실질적으로 다른 동작을 구현하고 검증하세요."
        );
        return vec![
            ("neutral_fresh_source", format!("{original}\n\n{inherited}{note}"), true, true),
            ("neutral_compact_fresh_source", format!("{original}{note}"), true, true),
        ];
    }
    vec![
        ("full", format!("{original}\n\n{inherited}{digest}{focus}"), false, false),
        ("source_on_demand", format!("{original}\n\n{inherited}{digest}"), true, false),
        ("research_on_demand", format!("{original}\n\n{inherited}"), true, true),
    ]
}

/// Development-only observations from the ACTUALLY RUNNING revision. No
/// replacement source, expected patch or confirmation examples are supplied.
/// The same probe is rerun in every rebuilt descendant before choosing a task.
pub fn diagnose_capabilities() -> Value {
    let cases = [
        ("파일을 만들고 이름을 알려줘", 2usize),
        ("문장을 고치고 결과를 보여줘", 2),
        ("캐시를 지우고 다시 시작해", 2),
    ];
    let observations=cases.iter().map(|(input,expected)| {
        let result=crate::engine::decompose::decompose(input,
            &json!({"implementation":"numbered-first","parameters":{"kernel":"numbered-first"}}));
        let observed=result["subtasks"].as_array().map_or(0,Vec::len);
        json!({"input":input,"expectedSegments":expected,"observedSegments":observed,
            "passed":observed==*expected})
    }).collect::<Vec<_>>();
    let failures=observations.iter().filter(|row|row["passed"]==false).count();
    json!({"version":1,"generatorCommit":option_env!("DOMINION_BUILD_COMMIT"),
        "domain":"tools","focusFiles":["src/engine/decompose.rs"],
        "failed":failures,"observations":observations})
}

/// Shared coordinator transition. Downstream evaluation executes this exact
/// transition too, so a candidate may improve either target binding or the
/// worker's interpretation of it. No evaluator-specific substitute selector.
fn bind_inherited_target(input:&mut Value, next:&Value) {
    if let Some(domain)=next["nextDomain"].as_str().filter(|domain|DOMAINS.contains(domain)) {
        input["rsiSelectedDomain"]=json!(domain);
        input["rsiDecisionInput"]=next.clone();
    }else {
        input["rsiSelectedDomain"]=Value::Null;
        input["rsiDecisionInput"]=Value::Null;
    }
}

async fn run_loop(
    context: &JobContext,
    job: &Value,
    record: &mut Value,
    started: Instant,
) -> Result<String, String> {
    let input = &job["input"];
    let lab = PathBuf::from(record["labDir"].as_str().ok_or("실험 경로 없음")?);
    let target = context
        .state
        .config
        .data_dir
        .join("rsi-build")
        .join("generator");
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let target = std::fs::canonicalize(target).map_err(|e| e.to_string())?;
    let binary = target
        .join("release")
        .join(format!("dominion-rust{}", std::env::consts::EXE_SUFFIX));
    let mut built_commit = String::new();
    loop {
        let remaining = record["limits"]["timeBudgetSeconds"]
            .as_u64()
            .map(|budget| {
                budget.saturating_sub(
                    started
                        .elapsed()
                        .as_secs()
                        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0) / 1000),
                )
            })
            .unwrap_or(u64::MAX);
        if remaining == 0 {
            return Ok("time_budget".into());
        }
        if record["attempts"].as_u64().unwrap_or(0)
            >= record["limits"]["maxAttempts"].as_u64().unwrap_or(10)
        {
            return Ok("attempt_limit".into());
        }
        if !generation_fits_remaining_budget(record,input) {
            return Ok("token_budget".into());
        }
        let head = record["headCommit"]
            .as_str()
            .ok_or("세대 커밋 없음")?
            .to_string();
        if built_commit != head {
            record["phase"] = json!("building_generator");
            save(context, record)?;
            let mut build = Command::new(rsi_worktree::cargo_executable());
            build
                .current_dir(&lab)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "--bin",
                    "dominion-rust",
                    "--target-dir",
                ])
                .arg(&target)
                .env("DOMINION_RSI_BUILD_COMMIT", &head);
            let build_timeout = if remaining == u64::MAX {
                3600
            } else {
                remaining.min(600)
            };
            let output = command(context, build, Duration::from_secs(build_timeout)).await?;
            if !output.status.success() {
                return Err(format!(
                    "차세대 개선기 빌드 실패: {}",
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                ));
            }
            let mut identity = Command::new(&binary);
            identity.arg("--rsi-worker-identity");
            let output = command(context, identity, Duration::from_secs(10)).await?;
            if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != head {
                return Err("실제 개선기 바이너리와 세대 커밋이 다릅니다.".into());
            }
            record["generatorBinarySha256"] = json!(format!(
                "{:x}",
                Sha256::digest(std::fs::read(&binary).map_err(|e| e.to_string())?)
            ));
            record["actualGeneratorCommit"] = json!(head);
            built_commit = head.clone();
        }
        let attempt = record["attempts"].as_u64().unwrap_or(0) + 1;
        let mut child_input = input.clone();
        child_input["rsiSourceDir"] = json!(lab);
        child_input["rsiKnowledgeSource"]=record["sourceDir"].clone();
        child_input["rsiBaseCommit"] = json!(head);
        child_input["rsiCampaignId"] = json!(context.job_id);
        child_input["rsiGeneratorCommit"] = json!(head);
        // Give the next worker the REAL previous rejected source bytes as its
        // detached starting point. Re-running on the pristine baseline loses
        // the prior implementation and repeatedly produces a neutral patch.
        // The original head remains the immutable A/B baseline.
        let prior_history=record["history"].as_array().cloned().unwrap_or_default();
        let expanded=post_adoption_objective(input,&prior_history);
        if let Some(next)=&expanded {
            child_input["rsiFocusFiles"]=json!([]);
            child_input["rsiAutonomous"]=json!(true);
            child_input["rsiObjective"]=next["objective"].clone();
            child_input["prompt"]=next["prompt"].clone();
            child_input["rsiResidualEvidence"]=next["residuals"].clone();
            child_input["rsiCostOpportunities"]=next["costOpportunities"].clone();
            child_input["rsiMetaTransferTarget"]=next["metaTransferTarget"].clone();
            child_input["rsiSourceScopeExpanded"]=json!(true);
            child_input["rsiSelectedDomain"]=if next["metaTransferTarget"]==true {
                json!("improver")
            } else {
                next["costOpportunities"].as_array().and_then(|costs|costs.first())
                    .map(|row|row["domain"].clone())
                    .or_else(||next["residuals"].as_array()
                        .and_then(|rows|rows.first())
                        .map(|row|row["domain"].clone()))
                    .unwrap_or(Value::Null)
            };
        }
        // Diagnose through the rebuilt executable, never through the old server's
        // implementation. Fixed-model source improvement remains model-authored.
        if input["rsiAutonomous"]==true && input["rsiNativeInduction"]!=true && input["rsiSystemEvolution"]!=true {
            let mut probe=Command::new(&binary);probe.arg("--rsi-diagnose");
            let output=command(context,probe,Duration::from_secs(10)).await?;
            if !output.status.success() { return Err("선택 버전의 능력 진단 실행 실패".into()); }
            let diagnosis:Value=serde_json::from_slice(&output.stdout).map_err(|e|e.to_string())?;
            if diagnosis["generatorCommit"]!=head {return Err("진단 실행기 버전 불일치".into());}
            record["diagnosis"]=diagnosis.clone();
            if diagnosis["failed"].as_u64().unwrap_or(0)>0 {
                child_input["rsiDiagnostic"]=diagnosis.clone();
                child_input["rsiSelectedDomain"]=diagnosis["domain"].clone();
                child_input["rsiFocusFiles"]=diagnosis["focusFiles"].clone();
                child_input["rsiObjective"]=json!("실제 작업 분해 진단에서 실패한 순차 동작 인식을 개선한다. 기존 단일 명사와 기존 분해 동작을 보존하고 예문 전용 분기를 만들지 않는다.");
                child_input["prompt"]=json!(format!(
                    "DOMINION 실제 소스 자기개선입니다. 현재 실행 버전에서 관측한 개발 진단: {diagnosis}\n실패 원인을 소스에서 찾아 실제 코드를 수정하세요. 예문을 하드코딩하거나 평가기·테스트·진단 코드를 수정하지 마세요. 확인 평가는 별도로 동결되어 있습니다. workspace_read_range로 해당 구현과 해시를 확인한 뒤 workspace_replace_range로 근거 있는 최소 변경을 작성하세요. 빌드와 독립 비교는 실행기가 수행합니다."));
            }
        }
        // The coordinator is not the archive-selection brain during whole-
        // system evolution. The compiled successor chooses its own stepping
        // stone from the same independently sealed candidate evidence.
        let archive=if input["rsiSystemEvolution"]==true {Value::Null}
            else {crate::engine::rsi_archive::select(
                &context.state, &prior_history, &head, &child_input["rsiFrozenCases"])};
        child_input["rsiInheritedCandidateId"]=archive["candidateId"].clone();
        child_input["rsiArchiveSelection"]=archive;
        let objective = input["rsiObjective"].as_str().unwrap_or("");
        let mut allowed_campaigns = vec![context.job_id.to_string()];
        if let Some(prior)=matching_prior_campaign(&context.state,input,record) {
            if let Some(id)=prior["id"].as_str() { allowed_campaigns.push(id.to_string()); }
        }
        // An old queued candidate may have been replaced or claimed by a
        // sibling while the coordinator was rebuilding the next worker.
        let trial_algorithm = record["nextTrialAlgorithmId"].as_str()
            .filter(|id| !id.is_empty())
            .and_then(|id| context.state.store.get("algorithm",id))
            .filter(|algorithm| algorithm["rsiTrial"]["campaignId"]
                .as_str().is_some_and(|id|allowed_campaigns.iter().any(|owned|owned==id)))
            .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string))
            .or_else(|| pending_trial_candidate(&context.state,objective,&allowed_campaigns)
                .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string)));
        child_input["rsiAllowedTrialCampaigns"]=json!(allowed_campaigns);
        if let Some(id) = trial_algorithm {
            child_input["rsiTrialAlgorithmId"] = json!(id);
            record["nextTrialAlgorithmId"] = json!(id);
        } else {
            record["nextTrialAlgorithmId"] = Value::Null;
        }
        bind_inherited_target(&mut child_input,&record["nextDecision"]);
        // After an independently accepted generation, the next task is
        // selected from the *measured paired residuals*, not from a stale
        // parent_next_decision computed for the former source revision.
        // Reassert after the legacy nextDecision assignment above.
        if let Some(next)=&expanded {
            let measured_domain=if next["metaTransferTarget"]==true {
                Some("improver")
            }else {
                next["costOpportunities"].as_array().and_then(|rows|rows.first())
                    .and_then(|row|row["domain"].as_str())
                    .or_else(||next["residuals"].as_array()
                        .and_then(|rows|rows.first())
                        .and_then(|row|row["domain"].as_str()))
            };
            if let Some(domain)=measured_domain.filter(|d|DOMAINS.contains(d)) {
                child_input["rsiSelectedDomain"]=json!(domain);
                child_input["rsiDecisionInput"]=json!({
                    "basis":"independently_measured_post_adoption_target",
                    "previousAcceptedCommit":next["previousAcceptedCommit"],
                    "targetDomain":domain,
                    "metaTransferTarget":next["metaTransferTarget"]
                });
            }
        }
        if input["rsiSystemEvolution"]==true {
            // A system-evolution campaign must not re-pin every generation to
            // its original improver file. The rebuilt descendant selects
            // subsequent targets; the coordinator only preserves measured
            // post-adoption constraints and the owner's explicit file choice.
            let first_system_generation=attempt==1
                && !prior_history.iter().any(|row|row["accepted"]==true);
            let owner_focus=input["rsiFocusFiles"].as_array()
                .is_some_and(|paths|!paths.is_empty());
            let evidence=if first_system_generation {
                crate::engine::rsi_meta::development_evidence(&context.state)
            } else {None};
            if first_system_generation && !owner_focus && input["rsiAutonomous"]==true {
                child_input["rsiSelectedDomain"]=json!("improver");
                child_input["rsiDecisionInput"]=json!({
                    "basis":"initial_system_evolution","targetDomain":"improver"
                });
                child_input["rsiObjective"]=json!("DOMINION 자체 개선기의 알고리즘·하네스·탐색·학습을 실제 소스로 개선하여 같은 모델과 예산에서 후속 코드 개선 성과를 높인다.");
            } else if expanded.is_none() && !paired_descendant_target(&child_input) {
                // Never inherit a stale parent decision after an adoption.
                child_input["rsiSelectedDomain"]=Value::Null;
                child_input["rsiDecisionInput"]=Value::Null;
            }
            if !owner_focus {
                child_input["rsiFocusFiles"]=json!([]);
            }
            child_input["rsiSystemEvidence"]=json!(evidence);
            child_input["prompt"]=json!(format!(
                "DOMINION 재귀적 시스템 자기개선 작업입니다. 선택한 API 또는 로컬 모델을 이용하여 알고리즘, 후보 생성·선택, 실행 하네스, 문맥·기억·학습·도구 코드를 직접 개선할 수 있습니다. 전체 소스가 탐색 범위이며, 검증된 후손은 다음 세대의 개선 계획도 생성해야 합니다.\n사용자가 지정한 목표: {}\n현재 세대의 개발 증거(현재 소스에서도 원인을 확인할 것): {}\n수정된 개선기를 실제 재빌드하고 이전 개선기와 동일 모델·예산의 후속 소스 수정 성과를 독립 평가합니다. 제품 버그 하나의 수정으로 이 과제를 대체하지 마세요. 평가기 변경이나 무검증 성공 주장은 허용되지 않습니다. 실제 소스 변경을 검증 가능한 근거로 남기세요.",
                child_input["rsiObjective"],child_input["rsiSystemEvidence"]));
        }
        child_input["rsiIndexId"]=crate::engine::code_index::refresh(&context.state,&lab)?["id"].clone();
        let inherited_candidates=context.state.store.all("experience").into_iter()
            .filter(|item|item["status"]=="active" && item["partitionKey"]==research_scope(&child_input).partition_key
                && item["outcome"]=="success" && item["evidence"]["accepted"]==true)
            .collect::<Vec<_>>();
        let mut inherited_implementations=Vec::new();
        for item in inherited_candidates {
            let Some(commit)=item["implementation_ref"]["commit"].as_str() else {continue};
            // Earlier accepted capabilities remain in descendant binaries.
            if git(context,&lab,&["merge-base","--is-ancestor",commit,&head]).await.is_ok() {
                inherited_implementations.push(json!({"experienceId":item["id"],"implementation_ref":item["implementation_ref"],
                    "status":"available_in_binary","generatorCommit":head,"binarySha256":record["generatorBinarySha256"]}));
            }
            if inherited_implementations.len()>=4 {break;}
        }
        child_input["rsiInheritedImplementations"]=json!(inherited_implementations);
        child_input["rsiAvailableTools"] =
            json!(["code_search", "workspace_read_range", "workspace_replace_range", "workspace_replace", "workspace_write", "web_search", "http_fetch", "algorithm_propose", "unsafe_audit", "rsi_method_propose"]);
        let history=record["history"].as_array().cloned().unwrap_or_default();
        let target_objective=selection_objective(&child_input);
        let continuation=history.last()
            .filter(|_|expanded.is_none())
            .filter(|row|row["candidateId"].is_null()
                && row["newFindings"].as_u64().unwrap_or(0)>0)
            .and_then(|row|row["domain"].as_str())
            .filter(|domain|DOMAINS.contains(domain)).map(str::to_string);
        // A pending exploration formula must not hijack the first
        // measured post-adoption meta/transfer target. Keep the trial in
        // durable state; only defer applying it to this descendant.
        let trial=child_input["rsiTrialAlgorithmId"].as_str()
            .filter(|_|expanded.is_none())
            .filter(|_|child_input["rsiAllowedTrialCampaigns"].is_array())
            .map(str::to_string);
        let suggested=child_input["rsiSelectedDomain"].as_str().map(str::to_string);
        let (target_domain, target_selection, target_strategy)=select_generation_strategy(
            &context.state,&history,&target_objective,continuation,trial,suggested,&head);
        child_input["rsiTargetDomain"]=json!(target_domain);
        child_input["rsiTargetPlan"]=json!({
            "domain":target_domain,"selection":target_selection,
            "strategy":target_strategy,"generatorCommit":head
        });
        if input["rsiSystemEvolution"]==true
            && input["rsiFocusFiles"].as_array().is_none_or(|files|files.is_empty()) {
            // Only the rebuilt descendant ranks system-evolution files.
            // The old resident coordinator must not pre-commit a stale guess.
            child_input["rsiFocusFiles"]=json!([]);
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"pending_rebuilt_descendant_controller"
            });
        } else if child_input["rsiFocusFiles"]
            .as_array().is_none_or(|files|files.is_empty()) {
            let policy=target_strategy["id"].as_str()
                .and_then(|id|context.state.store.get("algorithm",id))
                .or_else(||crate::engine::algorithms::active(&context.state,"selfimprove.search"));
            let ranked=choose_focus_files(&context.state,&child_input,&history,policy.as_ref());
            let selected=ranked.iter().filter_map(|row|row["path"].as_str())
                .map(str::to_string).collect::<Vec<_>>();
            child_input["rsiFocusFiles"]=json!(selected);
            child_input["rsiAutoFocus"]=json!(true);
            child_input["rsiFocusSelection"]=json!({
                "source":"indexed_evidence","candidates":ranked,
                "domain":target_domain,
                "policyRevision":policy.as_ref().map(|row|row["revision"].clone()),
                "sourceIndexId":child_input["rsiIndexId"]
            });
        } else {
            child_input["rsiAutoFocus"]=json!(child_input["rsiDiagnostic"].is_object() || input["rsiSystemEvolution"]==true);
            child_input["rsiFocusSelection"]=json!({"source":if child_input["rsiDiagnostic"].is_object(){"runtime_diagnosis"}else{"user_selected"}});
        }
        // The accepted *rebuilt* generator, not the original resident server,
        // plans the next system-evolution task. The resident coordinator owns
        // budgets, frozen evaluation and promotion; these fields are mutable
        // runtime policy and must be allowed to evolve with source revisions.
        if input["rsiSystemEvolution"]==true {
            record["phase"]=json!("planning_next_generator");
            save(context,record)?;
            let plan_dir=context.state.config.data_dir.join("rsi-controller-plans");
            std::fs::create_dir_all(&plan_dir).map_err(|e|e.to_string())?;
            let plan_path=plan_dir.join(format!("{}-{attempt}.json",context.job_id));
            let snapshot=json!({
                "generatorCommit":head,"attempt":attempt,"input":child_input
            });
            std::fs::write(&plan_path,serde_json::to_vec(&snapshot).map_err(|e|e.to_string())?)
                .map_err(|e|e.to_string())?;
            let mut planner=Command::new(&binary);
            planner.arg("--rsi-plan-generation").arg(&plan_path)
                .current_dir(&lab)
                .env("DOMINION_DATA",&context.state.config.data_dir)
                .env("DOMINION_WORKSPACE",&lab)
                .env_remove("DOMINION_DESKTOP_PARENT");
            let output=command(context,planner,Duration::from_secs(30)).await;
            let _=std::fs::remove_file(&plan_path);
            let output=output?;
            if !output.status.success() {
                return Err(format!("재빌드된 개선기의 다음 세대 계획 실패: {}",
                    String::from_utf8_lossy(&output.stderr)));
            }
            let plan:Value=serde_json::from_slice(&output.stdout)
                .map_err(|e|format!("후손 계획 출력 형식 오류: {e}"))?;
            if plan["source"]!="compiled_descendant_system_controller_v2"
                || plan["generatorCommit"]!=head
                || plan["historyCount"].as_u64()!=Some(history.len() as u64) {
                return Err("후손 계획과 실행 소스·세대 계보가 다릅니다.".into());
            }
            if plan["systemBlueprint"]["generatorCommit"]!=head
                || plan["systemBlueprint"]["historyCount"].as_u64()!=Some(history.len() as u64)
                || plan["systemBlueprint"]["architecture"].as_array().is_none_or(|items|
                    items.is_empty() || items.len()>64 || items.iter().any(|item|
                        item["role"].as_str().is_none_or(|role|role.is_empty() || role.len()>64)))
                || !plan["systemBlueprint"]["sourceTreeSha256"].as_str()
                    .is_some_and(|hash|hash.len()==64 && hash.bytes().all(|byte|byte.is_ascii_hexdigit()))
                || !plan["improvementDirective"].is_object()
                || plan["lineageCapsule"]["source"]!="descendant_curated_lineage_v1"
                || plan["lineageCapsule"]["historyCount"].as_u64()!=Some(history.len() as u64)
                || plan["systemBlueprint"]["lineageSha256"]!=plan["lineageCapsule"]["sha256"]
                || !plan["lineageCapsule"]["sha256"].as_str().is_some_and(|hash|
                    hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())) {
                return Err("후손 시스템 자기모형·실패 학습·계보가 일치하지 않습니다.".into());
            }
            crate::engine::rsi_system::validate_plan_evidence(&context.state,&lab,&head,
                child_input["rsiIndexId"].as_str().unwrap_or(""),&plan)?;
            if !crate::engine::rsi_meta_controller::valid_policy(
                &plan["metaPolicy"],history.len())
                || (plan["architectureTarget"].is_object()
                    && plan["metaPolicy"]["target"]!=plan["architectureTarget"]) {
                return Err("후손의 수정 가능한 메타 개선기 계획이 일치하지 않습니다.".into());
            }
            if !crate::engine::rsi_interaction::valid_graph(&plan["interactionGraph"])
                || plan["metaPolicy"]["interactionGraphSha256"]!=plan["interactionGraph"]["sha256"]
                || (plan["architectureTarget"].is_object()
                    && !crate::engine::rsi_interaction::valid_portfolio(
                        &plan["coDesignPortfolio"],&plan["interactionGraph"],
                        plan["architectureFrontier"].as_array()
                            .ok_or("후손의 전체 소스 후보가 없습니다.")?))
                || (plan["architectureTarget"].is_null()
                    && !plan["coDesignPortfolio"].is_null()) {
                return Err("후손의 상호 지능 개선 인과 가설·복합 설계가 일치하지 않습니다.".into());
            }
            if plan["successorStage"].is_object() {
                let stage=&plan["successorStage"];
                if stage["source"]!="inherited_model_authored_improvement_program_v1"
                    || plan["architectureTarget"]["inheritedProgramStage"]!=true
                    || stage["path"]!=plan["architectureTarget"]["path"]
                    || stage["sourceSha256"]!=plan["architectureTarget"]["sourceSha256"]
                    || stage["role"]!=plan["architectureTarget"]["role"] {
                    return Err("후손의 개선 프로그램 단계가 선택한 실제 소스와 다릅니다.".into());
                }
            }
            let frozen=crate::engine::rsi_profile::from_input(&child_input)?;
            crate::engine::rsi_system::validate_peer_references(
                &context.state,&plan["peerLineages"],frozen["sha256"].as_str().unwrap_or(""))?;
            let domain=plan["domain"].as_str()
                .filter(|d|DOMAINS.contains(d)).ok_or("후손이 유효하지 않은 개선 영역을 선택했습니다.")?;
            let focus=plan["focusFiles"].as_array()
                .ok_or("후손 계획에 소스 후보 목록이 없습니다.")?;
            if focus.len()>8 || focus.iter().any(|file|file.as_str().is_none_or(|p|
                !crate::engine::rsi::safe_path(p)
                    || !lab.join(p).is_file()
                    || crate::engine::code_index::indexed_source_hash(
                        &context.state,child_input["rsiIndexId"].as_str().unwrap_or(""),p).is_none())) {
                return Err("후손이 허용되지 않은 소스 파일을 선택했습니다.".into());
            }
            child_input["rsiSelectedDomain"]=json!(domain);
            child_input["rsiTargetDomain"]=json!(domain);
            child_input["rsiTargetPlan"]=json!({
                "domain":domain,"selection":plan["selection"],
                "strategy":plan["strategy"],"generatorCommit":head
            });
            let explicit_owner_focus=input["rsiFocusFiles"].as_array()
                .is_some_and(|files|!files.is_empty());
            if !explicit_owner_focus {
                child_input["rsiFocusFiles"]=json!(focus);
                child_input["rsiAutoFocus"]=json!(true);
            }
            child_input["rsiFocusSelection"]=json!({
                "source":if explicit_owner_focus {"owner_selected"} else {"rebuilt_descendant_controller"},
                "domain":domain,"candidates":plan["ranked"],
                "sourceIndexId":child_input["rsiIndexId"],
                "generatorCommit":head
            });
            child_input["rsiArchitectureFrontier"]=plan["architectureFrontier"].clone();
            child_input["rsiArchitectureTarget"]=plan["architectureTarget"].clone();
            child_input["rsiSystemBlueprint"]=plan["systemBlueprint"].clone();
            child_input["rsiImprovementDirective"]=plan["improvementDirective"].clone();
            child_input["rsiLineageCapsule"]=plan["lineageCapsule"].clone();
            child_input["rsiPeerLineages"]=plan["peerLineages"].clone();
            child_input["rsiComparativeMode"]=plan["comparativeMode"].clone();
            child_input["rsiMetaPolicy"]=plan["metaPolicy"].clone();
            child_input["rsiSuccessorStage"]=plan["successorStage"].clone();
            child_input["rsiInteractionGraph"]=plan["interactionGraph"].clone();
            child_input["rsiCoDesignPortfolio"]=plan["coDesignPortfolio"].clone();
            if !plan["archiveSelection"].is_null() {
                let id=plan["archiveSelection"]["candidateId"].as_str()
                    .ok_or("후손 보관소 후보 ID가 없습니다.")?;
                let candidate=context.state.store.get("rsi_candidate",id)
                    .ok_or("후손 보관소 후보가 현재 기록에 없습니다.")?;
                if candidate["patchSha256"]!=plan["archiveSelection"]["patchSha256"]
                    || candidate["baseCommit"]!=head
                    || candidate["pairedComparisonComplete"]!=true {
                    return Err("후손 보관소 선택의 실제 패치·출처가 맞지 않습니다.".into());
                }
            }
            child_input["rsiInheritedCandidateId"]=plan["archiveSelection"]["candidateId"].clone();
            child_input["rsiArchiveSelection"]=plan["archiveSelection"].clone();
            child_input["rsiEditPolicy"]=plan["editPolicy"].clone();
            child_input["rsiControllerPlan"]=plan.clone();
            context.state.store.audit("rsi.descendant_controller_plan",json!({
                "campaignId":context.job_id,"attempt":attempt,
                "generatorCommit":head,"domain":domain,
                "focusFiles":focus,"selection":plan["selection"]
            }));
        }
        // This is the system's learned *execution method*, independently of
        // which model is used to write the candidate patch.
        if input["rsiSystemEvolution"]!=true {
            child_input["rsiEditPolicy"]=learned_edit_method(&context.state,&child_input,&history);
        }
        context.state.store.audit("rsi.edit_method_selected",json!({
            "campaignId":context.job_id,"attempt":attempt,
            "generatorCommit":head,"policy":child_input["rsiEditPolicy"]
        }));
        // The resident coordinator sends only immutable owner constraints.
        // The rebuilt descendant gathers and composes all system-design,
        // research, source and failure context from its own compiled code.
        let inherited = if input["rsiSystemEvolution"]==true {String::new()}
            else {generation_context(&context.state, &child_input, record)};
        let digest = if input["rsiSystemEvolution"]==true {String::new()}
            else {research_digest(&context.state, input, record)};
        let focus = if input["rsiSystemEvolution"]==true {String::new()}
            else {focus_file_context(&context.state, record, &child_input)};
        let original = if input["rsiSystemEvolution"]==true {
            input["rsiObjective"].as_str().unwrap_or("").to_string()
        } else {child_input["prompt"].as_str().unwrap_or("").to_string()};
        // Preserve the unmixed prompt parts so an evolved descendant worker
        // can replan without inheriting a stale source excerpt or a stale
        // coordinator-selected file as apparent evidence.
        child_input["rsiPromptBase"]=json!(original);
        child_input["rsiPromptEvidence"]=json!(inherited);
        child_input["rsiPromptResearch"]=json!(digest);
        child_input["rsiSourceAdvanced"]=json!(head != record["baseCommit"].as_str().unwrap_or(""));
        let model_bound = child_input["model"].as_str().is_some_and(|m| !m.is_empty())
            && child_input["providerId"].as_str().is_some_and(|p| !p.is_empty());
        // Re-evaluate the ACTUAL complete parent prompt before any paid model
        // call. A large saved research ledger must not silently consume the
        // budget and leave the next generation unable to start.
        let variants = generation_prompt_variants(
            &original, &inherited, &digest, &focus,
            child_input["rsiInheritedCandidateId"].as_str(),
        );
        let Some((mode, composed, defer_focus, defer_research)) = variants.into_iter()
            .find(|(_, text, _, _)| {
                !model_bound || generation_fits_remaining_budget(
                    record, &json!({"prompt":text}))
            }) else { return Ok("token_budget".into()); };
        child_input["prompt"] = json!(composed);
        child_input["rsiContextPlan"] = json!({
            "mode":mode,"sourceExcerptDeferred":defer_focus && !focus.is_empty(),
            "researchReadLedgerDeferred":defer_research && !digest.is_empty(),
            "evidencePreserved":true
        });
        child_input["rsiRemainingCampaignTokens"] = record["limits"]["tokenBudget"]
            .as_u64()
            .map(|budget| budget.saturating_sub(record["tokens"].as_u64().unwrap_or(0)))
            .map_or(Value::Null, |tokens| json!(tokens));
        // Account only for source context that was ACTUALLY delivered.
        child_input["rsiFocusContextChars"] = json!(if defer_focus {0} else {focus.chars().count()});
        child_input["rsiPromptContextChars"] = json!(child_input["prompt"]
            .as_str().map(|prompt| prompt.chars().count()).unwrap_or(0));
        let child = context
            .state
            .store
            .put(
                "job",
                &json!({"kind":"rsi_generation","status":"running",
            "input":child_input,"queuedAt":chrono::Utc::now().to_rfc3339(),"attempts":1}),
            )
            .ok_or("세대 작업 저장 실패")?;
        let child_id = child["id"].as_str().ok_or("세대 작업 ID 없음")?;
        record["attempts"] = json!(attempt);
        record["activeGenerationJobId"] = json!(child_id);
        record["activeBaseTokens"] = record["tokens"].clone();
        record["phase"] = json!("diagnosing_and_editing");
        save(context, record)?;
        let mut worker = Command::new(&binary);
        worker
            .arg("--rsi-worker-job")
            .arg(child_id)
            .current_dir(&lab)
            .env(
                "DOMINION_DATA",
                std::fs::canonicalize(&context.state.config.data_dir).map_err(|e| e.to_string())?,
            )
            .env("DOMINION_WORKSPACE", &lab)
            .env_remove("DOMINION_DESKTOP_PARENT");
        let worker_started = Instant::now();
        let remaining = record["limits"]["timeBudgetSeconds"]
            .as_u64()
            .map(|budget| {
                budget.saturating_sub(
                    started
                        .elapsed()
                        .as_secs()
                        .saturating_sub(record["waitingMs"].as_u64().unwrap_or(0) / 1000),
                )
            })
            .unwrap_or(u64::MAX);
        let worker_timeout = if remaining == u64::MAX {
            7 * 24 * 3600
        } else {
            remaining.max(1)
        };
        let output = command(context, worker, Duration::from_secs(worker_timeout)).await;
        let current = context
            .state
            .store
            .get("job", child_id)
            .ok_or("세대 작업 사라짐")?;
        let session = current["sessionId"].as_str();
        let stopped=generation_stop_reason(&current,output.as_ref().err().map(String::as_str));
        let tools_used = session
            .map(|session_id| session_tool_usage(&context.state, session_id))
            .unwrap_or_default();
        let research_fingerprint = session
            .map(|session_id| session_research_fingerprint(&context.state, session_id))
            .unwrap_or_default();
        let capabilities_used = tools_used
            .iter()
            .filter(|row| {
                matches!(
                    row["tool"].as_str(),
                    Some("code_search" | "workspace_read_range" | "workspace_replace" | "workspace_replace_range")
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        // A reused session must never attribute an older worker's stage
        // transitions to this descendant. Match the worker and exact policy.
        let mut program_stages = context.state.store.all("rsi_program_phase")
            .into_iter().filter(|row|
                row["generationJobId"].as_str()==Some(child_id)
                && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
            .collect::<Vec<_>>();
        program_stages.sort_by(|left,right|
            left["recordedAt"].as_str().unwrap_or("").cmp(
                right["recordedAt"].as_str().unwrap_or("")));
        if program_stages.len()>48 {
            program_stages.drain(..program_stages.len()-48);
        }
        // The next generator needs actual ordered tool outcomes, not merely
        // a model-written description. Keep only bounded, non-secret fields.
        let mut tool_timeline = session.map(|id| context.state.store.all("execution_operation")
            .into_iter().filter(|op|op["sessionId"].as_str()==Some(id)
                && op["origin"]=="model")
            .map(|op| json!({
                "runId":op["runId"],"opSeq":op["opSeq"],
                "startedAt":op["startedAt"],
                "tool":op["tool"],"status":op["status"],
                "path":op["arguments"]["path"],
                "result":{"written":op["result"]["written"],
                    "postSha256":op["result"]["postSha256"]},
                "toolReceipt":{"verified":op["toolReceipt"]["verified"]},
            })).collect::<Vec<_>>()).unwrap_or_default();
        tool_timeline.sort_by(|a,b|a["startedAt"].as_str().unwrap_or("")
            .cmp(b["startedAt"].as_str().unwrap_or(""))
            .then_with(||a["opSeq"].as_u64().unwrap_or(0)
                .cmp(&b["opSeq"].as_u64().unwrap_or(0))));
        if tool_timeline.len()>64 {tool_timeline.drain(..tool_timeline.len()-64);}
        let mut rule_executions=context.state.store.all("rsi_rule_execution")
            .into_iter().filter(|row|
                row["generationJobId"].as_str()==Some(child_id)
                && row["policyRevision"]==current["input"]["rsiEditPolicy"]["revision"])
            .collect::<Vec<_>>();
        rule_executions.sort_by(|a,b|a["recordedAt"].as_str().unwrap_or("")
            .cmp(b["recordedAt"].as_str().unwrap_or("")));
        if rule_executions.len()>32 {rule_executions.drain(..rule_executions.len()-32);}
        let usage = session
            .map(|s| context.state.store.session_usage(s))
            .unwrap_or(Value::Null);
        let tokens = usage["totalTokens"].as_u64().unwrap_or(0);
        record["tokensEstimated"] = json!(
            record["tokensEstimated"] == true || usage["estimatedCalls"].as_u64().unwrap_or(0) > 0
        );
        record["tokens"] = json!(record["tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(tokens));
        let mut generation = json!({"id":format!("{}-{attempt}",context.job_id),"campaignId":context.job_id,
            "attempt":attempt,"generatorCommit":head,"jobId":child_id,"tokens":tokens,
            "tokenUsage":{"promptTokens":usage["promptTokens"],"completionTokens":usage["completionTokens"],
                "calls":usage["calls"],"estimatedCalls":usage["estimatedCalls"]},
            "sourceContextChars":current["input"]["rsiFocusContextChars"],
            "totalPromptChars":current["input"]["rsiPromptContextChars"],
            "focusFiles":current["input"]["rsiFocusFiles"],
            "scopeExpanded":current["input"]["rsiSourceScopeExpanded"],
            "residualEvidence":current["input"]["rsiResidualEvidence"],
            "costOpportunities":current["input"]["rsiCostOpportunities"],
            "metaTransferTarget":current["input"]["rsiMetaTransferTarget"],
            "focusSelection":current["input"]["rsiFocusSelection"],
            "targetReplan":current["rsiTargetReplan"],
            "knowledgePartition":research_scope(&current["input"]).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&[
                target_focus_objective(&current["input"])]),
            "generatorBinarySha256":record["generatorBinarySha256"],"generatorModel":input["model"],"generatorProviderId":input["providerId"],
            "domainSelection":current["rsiDomainSelection"],"searchStrategy":current["rsiSearchStrategy"],
            "decisionInput":current["rsiDecisionInput"],"selectedDomainInput":current["input"]["rsiSelectedDomain"]});
        let generation_details = json!({
            "researchDelivered":current["rsiResearchDelivered"],"workerMarker":current["result"]["rsi"]["fixturePhase"],
            "boundaryResult":current["rsiBoundaryResult"],
            "elapsedMs":worker_started.elapsed().as_millis() as u64,"domain":current["rsiDomain"],
            "toolsUsed":tools_used,"builtInToolsUsed":capabilities_used,
            "inheritedImplementations":current["input"]["rsiInheritedImplementations"],
            "systemEvolution":input["rsiSystemEvolution"],
            "evaluationProfileSha256":input["rsiEvaluationProfile"]["sha256"],
            "architectureTarget":current["input"]["rsiArchitectureTarget"],
            "systemBlueprint":current["input"]["rsiSystemBlueprint"],
            "improvementDirective":current["input"]["rsiImprovementDirective"],
            "lineageSha256":current["input"]["rsiLineageCapsule"]["sha256"],
            "lineageVerified":current["rsiLineageVerified"],
            "comparativeMode":current["input"]["rsiComparativeMode"],
            "successorStage":current["input"]["rsiSuccessorStage"],
            "interactionGraphSha256":current["input"]["rsiInteractionGraph"]["sha256"],
            "coDesignPortfolio":current["input"]["rsiCoDesignPortfolio"],
            "peerReferenceCandidateIds":current["input"]["rsiPeerLineages"]["references"]
                .as_array().map(|rows|rows.iter().filter_map(|row|
                    row["candidateId"].as_str()).take(4).collect::<Vec<_>>()).unwrap_or_default(),
            "controllerPlan":current["input"]["rsiControllerPlan"],
            "systemBlueprintVerified":current["rsiSystemBlueprintVerified"],
            "methodProposals":session.and_then(|id|context.state.store.get("session",id))
                .map(|row|row["rsiMethodProposals"].clone()).unwrap_or(Value::Null),
            "systemDesign":session.and_then(|id|context.state.store.get("session",id))
                .map(|row|row["rsiSystemDesign"].clone()).unwrap_or(Value::Null),
            "hypothesisMessageId":current["result"]["rsi"]["modelMessageId"],
            "researchFingerprint":research_fingerprint,
            "editEvidence":current["result"]["rsi"]["editEvidence"],
            "editPolicy":current["input"]["rsiEditPolicy"],
            "programStages":program_stages,
            "editMethodReward":Value::Null,
            "diffBytes":current["result"]["rsi"]["diffBytes"],
            "captureError":current["result"]["rsi"]["captureError"],
            "repairFailure":current["result"]["repairError"],
            "nativeMutation":current["result"]["nativeMutation"],
            "candidateOrigin":current["result"]["rsi"]["candidateOrigin"],
            "compilerRepairReceipts":current["result"]["compilerRepairReceipts"],
            "accepted":false,"createdAt":chrono::Utc::now().to_rfc3339()});
        generation.as_object_mut().expect("generation object")
            .extend(generation_details.as_object().expect("generation details object").clone());
        generation["requestedModel"]=input["model"].clone();
        // The meta-policy was chosen by the compiled generator BEFORE it
        // observed this generation's result. Keep it separate from measured
        // feedback to avoid look-ahead and let later descendants learn.
        generation["metaPolicy"]=current["input"]["rsiMetaPolicy"].clone();
        generation["metaPolicyVerified"]=current["rsiMetaPolicyVerified"].clone();
        if generation["candidateOrigin"]=="native_inductive_source_mutation" {
            generation["generatorModel"]=json!(format!("dominion-native:{}",
                generation["nativeMutation"]["operator"].as_str().unwrap_or("unknown")));
            generation["generatorProviderId"]=json!("dominion-native");
        }
        // Keep the generation JSON macro below Rust's recursive expansion
        // ceiling and attach bounded evidence only after building the core
        // generation receipt.
        generation["toolTimeline"]=json!(tool_timeline);
        generation["ruleExecutions"]=json!(rule_executions);
        // A successful native write can still produce uncompilable Rust.
        // Keep the *real compiler diagnostic* and the pre-repair history
        // separate from independently eligible improvements. This makes
        // next-generation repair synthesis learn WHY a patch failed.
        let check_diagnostics=current["result"]["rsi"]["checks"].as_array()
            .into_iter().flatten().take(4).map(|check|json!({
                "label":check["label"],"passed":check["passed"],
                "exitCode":check["exitCode"],
                "detail":check["stderr"].as_str()
                    .or_else(||check["error"].as_str()).unwrap_or("")
                    .chars().take(1600).collect::<String>()
            })).collect::<Vec<_>>();
        let prior_diagnostics=current["result"]["previousAttempts"].as_array()
            .into_iter().flatten().take(3).map(|attempt|{
                let first=attempt["result"]["checks"].as_array()
                    .into_iter().flatten().find(|c|c["passed"]!=true);
                json!({
                    "attempt":attempt["attempt"],
                    "checksPassed":attempt["result"]["checksPassed"],
                    "error":first.and_then(|c|c["stderr"].as_str()
                        .or_else(||c["error"].as_str()))
                        .unwrap_or("").chars().take(1200).collect::<String>()
                })
            }).collect::<Vec<_>>();
        generation["inheritedCandidateId"]=current["input"]["rsiInheritedCandidateId"].clone();
        generation["archiveSelection"]=current["input"]["rsiArchiveSelection"].clone();
        generation["inheritedPatchChanged"]=current["result"]["rsi"]["inheritedPatchChanged"].clone();
        generation["verification"]=json!({
            "rawCandidateId":current["result"]["rsi"]["candidateId"],
            "checksPassed":current["result"]["rsi"]["checksPassed"],
            "reason":current["result"]["reason"],
            "checks":check_diagnostics,
            "repairAttempts":current["result"]["repairAttempts"],
            "repairHistory":prior_diagnostics,
        });
        let proposed_candidate = verified_candidate_id(&current["result"]);
        let previous_failed = proposed_candidate.and_then(|id|
            identical_failed_patch(&context.state,
                record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]),id));
        // A byte-identical failed patch from the identical base does not
        // deserve another full expensive A/B run.
        let candidate_id = proposed_candidate.filter(|_|previous_failed.is_none());
        let waiting_before = record["waitingMs"].as_u64().unwrap_or(0);
        if let Some(id) = candidate_id {
            record["phase"] = json!("measuring");
            save(context, record)?;
            generation["candidateId"] = json!(id);
            let compared = rsi_worktree::compare_at(
                context,
                id,
                &lab,
                Path::new(
                    record["sourceDir"]
                        .as_str()
                        .ok_or("고정 평가기 경로 없음")?,
                ),
            )
            .await;
            match compared {
                Ok(evaluation) => {
                    generation["evaluation"] = evaluation;
                    let candidate = context
                        .state
                        .store
                        .get("rsi_candidate", id)
                        .ok_or("후보 소실")?;
                    let design=&generation["systemDesign"];
                    let paths=candidate["paths"].as_array().cloned().unwrap_or_default();
                    let design_complete=input["rsiSystemEvolution"]!=true || (
                        candidate["origin"]=="model_driven_detached_worktree"
                        && design["source"]=="model_authored_system_design"
                        && design["generatorCommit"]==head
                        && design["generationJobId"]==child_id
                        && generation["systemBlueprintVerified"]==true
                        && crate::engine::rsi_system::design_implemented(design,&paths));
                    generation["designImplemented"]=json!(design_complete);
                    generation["pairedRevalidated"]=json!(crate::engine::rsi::verified_pair_receipt(&context.state,&candidate));
                    let eligible = candidate["status"] == "verified"
                        && candidate["evaluation"]["metrics"]["eligible"] == true
                        && generation["pairedRevalidated"]==true && design_complete;
                    let winner = if eligible {
                        "candidate"
                    } else if candidate["status"] == "rejected" {
                        "baseline"
                    } else {
                        "inconclusive"
                    };
                    record["comparison"] = json!({"attempt":attempt,"candidateId":id,"winner":winner,
                        "metrics":candidate["evaluation"]["metrics"],"baselineCommit":head,
                        "evaluationId":candidate["evaluation"]["id"],"canSelectCandidate":eligible});
                    let choice = select(context, record).await?;
                    generation["selection"] = json!(choice);
                    if eligible && choice == "candidate" {
                        let commit = adopt(context, &lab, &candidate, &head).await?;
                        let mut accepted_hashes=serde_json::Map::new();
                        for path in candidate["paths"].as_array().into_iter().flatten()
                            .filter_map(Value::as_str) {
                            if !crate::engine::rsi::safe_path(path) {continue;}
                            if let Ok(bytes)=std::fs::read(lab.join(path)) {
                                accepted_hashes.insert(path.to_string(),
                                    json!(format!("{:x}",Sha256::digest(&bytes))));
                            }
                        }
                        generation["acceptedSourceHashes"]=Value::Object(accepted_hashes);
                        generation["accepted"] = json!(true);
                        generation["acceptedCommit"] = json!(commit);
                        record["headCommit"] = json!(commit);
                        record["acceptedGenerations"] =
                            json!(record["acceptedGenerations"].as_u64().unwrap_or(0) + 1);
                        let mut saved = candidate;
                        saved["experimentAdoption"] = json!({"campaignId":context.job_id,
                            "commit":commit,"generatorCommit":head,"productionDeployed":false});
                        context
                            .state
                            .store
                            .put("rsi_candidate", &saved)
                            .ok_or("채택 기록 저장 실패")?;
                        context.state.store.put("rsi_current",&json!({"id":"selected-engine","commit":commit,
                            "sourceDir":record["sourceDir"],"sourceBranch":record["sourceBranch"],
                            "sourceBaseCommit":record["baseCommit"],
                            "sourceFingerprint":record["sourceFingerprint"],"campaignId":context.job_id,
                            "candidateId":id,"selectedAt":chrono::Utc::now().to_rfc3339()})).ok_or("선택한 개선기 저장 실패")?;
                    }
                }
                Err(error) => generation["error"] = json!(error),
            }
        } else {
            let worker_error = output.as_ref().err().map(String::as_str);
            let child = &current["result"]["rsi"];
            if let Some(previous)=previous_failed.as_deref() {
                generation["candidateId"]=json!(proposed_candidate);
                generation["evaluation"]=json!({"verdict":"duplicate_unproductive_patch",
                    "duplicateOf":previous});
                generation["repeatedEvaluationAvoided"]=json!(true);
                generation["error"]=json!(format!(
                    "동일 기준 커밋에서 이미 개선 효과가 없던 패치를 다시 제안했습니다: {previous}. 새로운 가설과 다른 코드 변경이 필요합니다."));
            } else {
                generation["error"] = json!(worker_error
                    .or_else(|| current["result"]["repairError"].as_str())
                    .or_else(|| child["modelError"].as_str())
                    .or_else(|| child["captureError"].as_str())
                    .or_else(|| current["error"].as_str())
                    .unwrap_or("유효한 변경이나 검사 통과 없음"));
            }
        }
        if input["rsiSystemEvolution"]==true {
            record["tokens"]=json!(campaign_model_tokens(&context.state,&context.job_id));
            generation["metaEvaluation"]=candidate_id.and_then(|id|
                context.state.store.get("rsi_candidate",id)).map(|c|c["metaEvaluation"].clone()).unwrap_or(Value::Null);
        }
        generation["elapsedMs"] = json!((worker_started.elapsed().as_millis() as u64)
            .saturating_sub(
                record["waitingMs"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(waiting_before)
            ));
        let artifacts=session.map(|s|research_artifacts(&context.state,s)).unwrap_or_default();
        let hashes=artifacts.iter().map(|a|a["hash"].clone()).collect::<Vec<_>>();
        let new_findings=new_research_findings(&context.state,&current["input"],child_id,
            &hashes,record["history"].as_array().map(Vec::as_slice).unwrap_or(&[]));
        let verified_outcomes=history_with_verified_payoffs(
            &context.state,&[generation.clone()]);
        generation["editMethodReward"]=json!(verified_outcomes.first()
            .map(edit_method_reward).unwrap_or(0.0));
        generation["knowledgeArtifactHashes"]=json!(hashes);
        generation["newFindings"]=json!(new_findings);
        generation["stopReason"]=json!(stopped);
        if candidate_id.is_none() && new_findings>0 && output.is_ok() && stopped.is_none()
            && current["result"]["rsi"]["modelError"].is_null() {
            generation["error"]=Value::Null;
        }
        if let Some(session)=session {
            generation["methodExecutions"]=json!(record_method_feedback(&context.state,&generation,session));
        }
        generation["knowledgeId"]=json!(remember_research(&context.state,&current["input"],&generation,session.unwrap_or("")));
        if let Some(session)=session {
            generation["knowledgeReuse"]=context.state.store.get("session",session).map(|s|s["rsiKnowledgeReuse"].clone()).unwrap_or(Value::Null);
        }
        let prior_history = record["history"].as_array().cloned().unwrap_or_default();
        if let Some(session)=session {
            // Queue identity and next-generation lookup must use one target
            // objective. Autonomous system prompts are not owner constraints.
            let trial_objective=selection_objective(input);
            let proposals = proposed_algorithms(&context.state, session);
            let active = crate::engine::algorithms::active(&context.state, "selfimprove.search")
                .unwrap_or_else(|| json!({"id":Value::Null,"revision":Value::Null,"implementation":"failure-clusters","parameters":{}}));
            let mut queued = Vec::new();
            let had_pending_trial=record["nextTrialAlgorithmId"].is_string();
            for candidate in proposals
                .iter()
                .filter(|candidate| candidate["kind"] == "selfimprove.search")
            {
                let report = queue_search_trial(
                    &context.state,
                    candidate,
                    &active,
                    &prior_history,
                    &trial_objective,
                    &context.job_id,
                    attempt,
                );
                if report["status"] == "pending_trial" {
                    if let Some(id) = report["candidateId"].as_str() {
                        queued.push(id.to_string());
                    }
                }
            }
            if !queued.is_empty() {
                generation["searchTrialsQueued"] = json!(queued);
            }
            // Autonomous RSI must be capable of evolving its own search
            // policy even when a small model never calls algorithm_propose.
            // No new generation or paid model invocation is created here.
            if queued.is_empty() && !had_pending_trial
                && input["rsiAutonomous"]==true
                && generation["domainSelection"]!="algorithm_trial" {
                let mut complete=prior_history.clone();
                complete.push(generation.clone());
                if let Some(report)=queue_adaptive_search_trial(
                    &context.state,&complete,&active,
                    &trial_objective,
                    &context.job_id,attempt,
                ){
                    if let Some(id)=report["candidateId"].as_str() {
                        queued.push(id.to_string());
                        generation["autoEvolvedSearch"]=json!({
                            "candidateId":id,
                            "hypothesis":report["candidateRevision"],
                            "source":report["hypothesisSource"],
                            "status":"pending_future_real_trial_not_improvement_proof"
                        });
                    }
                }
            }
            record["nextTrialAlgorithmId"] = Value::Null;
            if generation["domainSelection"].as_str() == Some("algorithm_trial") {
                if let Some(id) = generation["searchStrategy"]["id"].as_str() {
                    let outcome =
                        record_trial_outcome(&context.state, id, &generation, &prior_history);
                    // A research continuation did not run the formula. Keep the
                    // candidate pending for the next already scheduled generation.
                    if outcome["status"] == "not_used" {
                        record["nextTrialAlgorithmId"] = json!(id);
                    } else if outcome["status"] == "pending_second_trial" {
                        record["nextTrialAlgorithmId"] = json!(id);
                    }
                    generation["trialOutcome"] = outcome;
                }
            }
            if record["nextTrialAlgorithmId"].is_null() {
                if let Some(id) = queued.into_iter().next() {
                    record["nextTrialAlgorithmId"] = json!(id);
                }
            }
        }
        if input["rsiSystemEvolution"]==true {
            generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
                &generation["metaPolicy"],&generation);
            context.state.store.audit("rsi.meta_controller_feedback",json!({
                "campaignId":context.job_id,"generationJobId":generation["jobId"],
                "generatorCommit":head,
                "metaPolicyRevision":generation["metaPolicy"]["revision"],
                "independentObservation":generation["metaLearning"]["independentObservation"],
                "brierError":generation["metaLearning"]["brierError"]
            }));
        }
        let generation_accepted = generation["accepted"] == true;
        // A real, independently adopted NATIVE source mutation can succeed
        // even when a previous model turn exhausted its own output budget.
        // Do not discard that measured descendant merely because the model
        // ended badly. The coordinator's aggregate token/time budgets are
        // enforced again before launching any later generation.
        let stopped = if generation_accepted
            && generation["nativeMutation"]["origin"]=="native_inductive_source_mutation"
            && stopped==Some("token_budget") {
            None
        } else {stopped};
        let accepted_streak = if generation_accepted {
            record["acceptedStreak"].as_u64().unwrap_or(0) + 1
        } else {
            // Research-only or failed generations interrupt a consecutive
            // adoption chain, even when no evaluation was produced.
            0
        };
        record["acceptedStreak"] = json!(accepted_streak);
        let max_streak = record["maxAcceptedStreak"]
            .as_u64()
            .unwrap_or(0)
            .max(accepted_streak);
        record["maxAcceptedStreak"] = json!(max_streak);
        // Three consecutive adopted generations are the documented threshold
        // for calling the capability demonstrated, not a single lucky patch.
        let mut evidence_record=record.clone();
        evidence_record["history"].as_array_mut()
            .ok_or("세대 이력 손상")?.push(generation.clone());
        if input["rsiSystemEvolution"]==true {
            let history=evidence_record["history"].as_array().cloned().unwrap_or_default();
            record["systemEvidence"]=crate::engine::rsi_system::handoff_evidence(&context.state,&history);
            record["capabilityDemonstrated"]=record["systemEvidence"]["demonstrated"].clone();
        }else{
            record["capabilityDemonstrated"] = json!(max_streak >= 3
                && crate::engine::rsi::campaign_capability_proven(&context.state,&evidence_record));
        }
        let generation_knowledge = generation["knowledgeId"].clone();
        let generation_tokens = generation["tokens"].clone();
        let generation_elapsed = generation["elapsedMs"].clone();
        let generation_verdict = generation["evaluation"]["verdict"].clone();
        // Workers read this durable table, not the coordinator's in-memory
        // history. Publish only after research, method feedback and trial
        // outcomes are complete so the next worker can actually continue.
        // A real completion timestamp is also necessary to evolve the method
        // from the latest measured outcome across restarted campaigns.
        generation["createdAt"] = json!(chrono::Utc::now().to_rfc3339());
        context.state.store.put("rsi_generation", &generation)
            .ok_or("완성된 세대의 연구·실행 결과 저장 실패")?;
        record["history"]
            .as_array_mut()
            .ok_or("세대 목록 손상")?
            .push(generation);
        // New findings, not a write count or query spelling, justify continuing
        // a research-only generation within the configured campaign budgets.
        let no_edit = candidate_id.is_none()
            && current["result"]["rsi"]["diffBytes"].as_u64().unwrap_or(0) == 0;
        let research = !artifacts.is_empty();
        let changed = new_findings>0;
        let streak = if no_edit {
            record["noProgressStreak"].as_u64().unwrap_or(0) + 1
        } else {
            0
        };
        record["noProgressStreak"] = json!(streak);
        record["activeGenerationJobId"] = Value::Null;
        let decision_objective = selection_objective(input);
        let decision_history = record["history"].as_array().cloned().unwrap_or_default();
        let decision_basis = if let Some(reason)=stopped {
            reason
        } else if previous_failed.is_some() {
            "duplicate_unproductive_patch"
        } else if generation_accepted {
            "accepted_implementation"
        } else if candidate_id.is_none()
            && record["history"].as_array().and_then(|rows|rows.last())
                .and_then(|row|row["editEvidence"]["attempts"].as_u64())
                .unwrap_or(0)>0 {
            "edit_attempt_without_candidate"
        } else if new_findings > 0 {
            "research_findings"
        } else if no_edit {
            "no_progress"
        } else {
            "implementation_not_adopted"
        };
        let next_domain = select_domain_for_state(&context.state, &decision_history, &decision_objective);
        let reuse = method_reuse_decision(&context.state, input, &next_domain);
        record["nextDecision"] = json!({
            "basis": decision_basis,
            "fromGeneration": attempt,
            "nextDomain": next_domain,
            "knowledgeId": generation_knowledge,
            "headCommit": record["headCommit"],
            "trialAlgorithmId": record["nextTrialAlgorithmId"],
            "observed": {
                "accepted": generation_accepted,
                "tokens": generation_tokens,
                "elapsedMs": generation_elapsed,
                "verdict": generation_verdict,
                "editEvidence":record["history"].as_array()
                    .and_then(|rows|rows.last()).map(|row|row["editEvidence"].clone()),
            },
            "reuseDecision": reuse,
        });
        elapsed(record, started);
        save(context, record)?;
        if proof_goal_reached(&context.state,input,record) {
            return Ok("capability_demonstrated".into());
        }
        if let Some(reason)=stopped {return Ok(reason.into());}
        if let Some(error)=generation_model_error(&current) {
            // A late provider failure after source bytes were written,
            // compiled and independently paired-evaluated is not a reason
            // to terminate the lineage before its next measured mutation.
            // An unmeasured error still stops this campaign.
            let paired=record["history"].as_array()
                .and_then(|rows|rows.last())
                .is_some_and(|last|last["evaluation"]["source"]
                    =="server_invoked_local_paired_git_worktrees");
            if !paired {return Err(error.to_string());}
        }
        if no_edit && should_stop_no_progress(streak, research, changed,
            record["nextTrialAlgorithmId"].is_string(),
            input["rsiAutonomous"]==true) {
            return Ok("no_progress".into());
        }
    }
}

fn proof_goal_reached(state:&AppState,input:&Value,record:&Value)->bool {
    input["rsiStopAfterProof"]==true && if input["rsiSystemEvolution"]==true {
        let history=record["history"].as_array().cloned().unwrap_or_default();
        crate::engine::rsi_system::handoff_evidence(state,&history)["demonstrated"]==true
    }else{
        record["maxAcceptedStreak"].as_u64().unwrap_or(0)>=3
            && crate::engine::rsi::campaign_capability_proven(state,record)
    }
}

/// Empty strings are not model errors; a research-only generation sets the
/// field to "" and must not abort the whole campaign.
fn generation_model_error(current: &Value) -> Option<&str> {
    current["result"]["rsi"]["modelError"]
        .as_str()
        .filter(|error| !error.trim().is_empty())
}

pub(crate) async fn adopt(
    context: &JobContext,
    lab: &Path,
    candidate: &Value,
    base: &str,
) -> Result<String, String> {
    if candidate["status"] != "verified"
        || candidate["evaluation"]["metrics"]["eligible"] != true
        || candidate["evaluation"]["source"] != "server_invoked_local_paired_git_worktrees"
        || candidate["baseCommit"] != base
    {
        return Err("독립 평가를 통과한 현재 세대 후보만 실험 계보에 채택합니다.".into());
    }
    if git(context, lab, &["rev-parse", "HEAD"]).await? != base
        || !git(context, lab, &["status", "--porcelain"])
            .await?
            .is_empty()
    {
        return Err("실험 기준 소스가 변경됐습니다.".into());
    }
    let patch = candidate["patch"].as_str().ok_or("패치 없음")?;
    if format!("{:x}", Sha256::digest(patch.as_bytes()))
        != candidate["patchSha256"].as_str().unwrap_or("")
    {
        return Err("채택 패치 해시 불일치".into());
    }
    let file = lab
        .parent()
        .ok_or("실험 경로 오류")?
        .join(format!("{}.patch", uuid::Uuid::new_v4()));
    std::fs::write(&file, patch).map_err(|e| e.to_string())?;
    let applied = git(
        context,
        lab,
        &["apply", file.to_str().ok_or("패치 경로 오류")?],
    )
    .await;
    let _ = std::fs::remove_file(file);
    applied?;
    git(context, lab, &["add", "--all"]).await?;
    git(
        context,
        lab,
        &[
            "-c",
            "user.name=DOMINION experiment",
            "-c",
            "user.email=rsi@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "commit",
            "--no-verify",
            "-m",
            "RSI: adopt independently measured experimental candidate",
        ],
    )
    .await?;
    git(context, lab, &["rev-parse", "HEAD"]).await
}

/// The coordinator's guess is auditable; a rebuilt descendant's policy
/// may select a different domain and must then recompute its source evidence.
fn generation_target_plan(domain:&str,selection:&str,strategy:&Value,commit:&str)->Value {
    json!({"domain":domain,"selection":selection,"strategy":strategy,"generatorCommit":commit})
}

fn reconcile_descendant_target(
    state:&AppState,job:&mut Value,history:&[Value],
    domain:&str,selection:&str,strategy:&Value,commit:&str
)->Result<bool,String>{
    let proposed=generation_target_plan(domain,selection,strategy,commit);
    let old=job["input"]["rsiTargetPlan"].clone();
    if old==proposed {return Ok(false);}
    if !old.is_object() {return Err("차세대 대상 재계획의 기존 근거가 없습니다.".into());}
    let base=job["input"]["rsiPromptBase"].as_str()
        .ok_or("차세대 재계획에 필요한 원본 지시문이 없습니다.")?.to_string();
    let inherited=job["input"]["rsiPromptEvidence"].as_str().unwrap_or("").to_string();
    let research=job["input"]["rsiPromptResearch"].as_str().unwrap_or("").to_string();
    job["input"]["rsiTargetDomain"]=json!(domain);
    if job["input"]["rsiAutoFocus"]==true {
        let path=job["input"]["rsiSourceDir"].as_str()
            .ok_or("차세대 소스 경로 없음")?;
        let source=PathBuf::from(path);
        let index=crate::engine::code_index::refresh(state,&source)?;
        job["input"]["rsiIndexId"]=index["id"].clone();
        let policy=strategy["id"].as_str()
            .and_then(|id|state.store.get("algorithm",id))
            .or_else(||crate::engine::algorithms::active(state,"selfimprove.search"));
        let ranked=choose_focus_files(state,&job["input"],history,policy.as_ref());
        let selected=ranked.iter().filter_map(|row|row["path"].as_str())
            .map(str::to_string).collect::<Vec<_>>();
        job["input"]["rsiFocusFiles"]=json!(selected);
        job["input"]["rsiFocusSelection"]=json!({
            "source":"descendant_indexed_evidence","domain":domain,
            "candidates":ranked,
            "policyRevision":policy.as_ref().map(|a|a["revision"].clone()),
            "sourceIndexId":index["id"]
        });
    }
    let path=job["input"]["rsiSourceDir"].as_str()
        .ok_or("차세대 소스 경로 없음")?;
    let focus=focus_file_context(state,&json!({"labDir":path}),&job["input"]);
    let bound=job["input"]["model"].as_str().is_some_and(|s|!s.is_empty())
        && job["input"]["providerId"].as_str().is_some_and(|s|!s.is_empty());
    let variants=[
        ("full",format!("{base}\n\n{inherited}{research}{focus}"),false,false),
        ("source_on_demand",format!("{base}\n\n{inherited}{research}"),true,false),
        ("research_on_demand",format!("{base}\n\n{inherited}"),true,true),
    ];
    let remaining=job["input"]["rsiRemainingCampaignTokens"].as_u64();
    let Some((mode,prompt,source_deferred,research_deferred))=variants.into_iter()
        .find(|(_,prompt,_,_)| !bound || remaining.is_none_or(|available|
            crate::engine::chat::estimate_text_tokens(prompt).saturating_add(256)<=available))
    else {return Err("rsi_token_budget: 차세대 재계획 입력 예산 부족".into())};
    job["input"]["prompt"]=json!(prompt);
    job["input"]["rsiPromptContextChars"]=json!(prompt.chars().count());
    job["input"]["rsiFocusContextChars"]=json!(
        if source_deferred {0}else{focus.chars().count()});
    job["input"]["rsiContextPlan"]=json!({
        "mode":mode,"sourceExcerptDeferred":source_deferred && !focus.is_empty(),
        "researchReadLedgerDeferred":research_deferred && !research.is_empty(),
        "evidencePreserved":true
    });
    job["input"]["rsiTargetPlan"]=proposed.clone();
    if job["input"]["rsiDecisionInput"].is_object() {
        job["input"]["rsiDecisionInput"]["nextDomain"]=json!(domain);
        job["input"]["rsiDecisionInput"]["decisionSource"]=json!("descendant_worker");
    }
    let receipt=json!({
        "coordinatorPlan":old,"descendantPlan":proposed,
        "sourceIndexId":job["input"]["rsiIndexId"],
        "reason":"descendant_authoritative_selection"
    });
    job["rsiTargetReplan"]=receipt.clone();
    job["input"]["rsiTargetReplan"]=receipt;
    Ok(true)
}

fn select_generation_strategy(
    state: &AppState,
    history: &[Value],
    objective: &str,
    continuation: Option<String>,
    trial_id: Option<String>,
    selected_domain: Option<String>,
    generator_commit: &str,
) -> (String, &'static str, Value) {
    // Use the same candidate-scoped raw evaluation evidence when the rebuilt
    // child worker chooses a domain. Do not let stale claim fields steer it.
    let history_enriched=history_with_verified_payoffs(state,history);
    let history=&history_enriched;
    let active = crate::engine::algorithms::active(state, "selfimprove.search");
    // Research is inherited in the prompt even when a policy trial takes
    // priority. Otherwise an endlessly productive reading loop starves the
    // executable candidate forever.
    let pending_trial=trial_id.as_deref()
        .and_then(|id|state.store.get("algorithm",id))
        .is_some_and(|algorithm|matches!(
            algorithm["rsiTrial"]["status"].as_str(),
            Some("pending_trial"|"pending_second_trial")
        ));
    if let Some(domain) = continuation.filter(|_|!pending_trial) {
        return (domain, "research_continuation", Value::Null);
    }
    if let Some(id) = trial_id {
        if let Some(algorithm) = state.store.get("algorithm", &id).filter(|algorithm| {
            algorithm["kind"] == "selfimprove.search"
                && algorithm["source"] == "rsi-generated"
                && algorithm["rsiTrial"]["status"] != "trial_rejected"
        }) {
            if scored_domains(&algorithm,history,objective).is_none() {
                let baseline=active.unwrap_or_else(||json!({
                    "implementation":"failure-clusters","parameters":{}
                }));
                let mut strategy=search_strategy(&baseline,"active");
                strategy["trialFallback"]=json!("candidate_expression_invalid_for_runtime_features");
                return (select_domain(&baseline,history,objective),"algorithm_active",strategy);
            }
            // Pin the candidate's target to the domain used by its previous
            // active-policy control. Recomputing the adaptive score *after*
            // control generation can switch domains and make the controlled
            // experiment permanently incomparable.
            let pinned=history.iter().rev().find(|row|{
                row["generatorCommit"]==generator_commit
                    && row["domainSelection"]=="algorithm_active"
                    && row["searchStrategy"]["controlForCandidateId"].as_str()==Some(id.as_str())
                    && row["searchStrategy"]["controlForCandidateRevision"]==algorithm["revision"]
                    && row["domain"].as_str().is_some_and(|d|
                        candidate_domains(objective).contains(&d))
            }).and_then(|row|row["domain"].as_str());
            let domain=pinned.map(str::to_string)
                .unwrap_or_else(||select_domain(&algorithm, history, objective));
            let generation = json!({"domain":domain,"generatorCommit":generator_commit});
            if comparable_trial_baseline(history, &generation, &algorithm).is_some() {
                return (domain, "algorithm_trial", search_strategy(&algorithm, "trial"));
            }
            // A barren control cannot supply paired quality evidence, even
            // after repeated attempts. After two real no-patch controls in the
            // candidate's chosen domain, run the candidate ONCE to explore a
            // different path. Its trial remains inconclusive for promotion
            // unless a separately measured active control exists.
            let barren_controls=history.iter().filter(|row|{
                row["generatorCommit"]==generator_commit
                    && row["domain"]==domain
                    && row["domainSelection"]=="algorithm_active"
                    && row["candidateId"].is_null()
                    && row["accepted"]==false
                    && row["jobId"].as_str().is_some_and(|id|!id.is_empty())
            }).count();
            if barren_controls>=2
                && algorithm["rsiTrial"]["trials"].as_array().is_none_or(Vec::is_empty) {
                let mut strategy=search_strategy(&algorithm,"trial");
                strategy["trialFallback"]=json!("exploratory_after_barren_controls");
                strategy["evidenceLevel"]=json!("unpaired_exploration_never_promotion");
                return (domain,"algorithm_trial",strategy);
            }
            // Bootstrap a comparable control in the same domain. Different
            // domains cannot prove improved targeting.
            let active = active
                .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
            let mut selected = search_strategy(&active, "active");
            selected["trialFallback"] = json!("unused_comparable_baseline_missing");
            selected["controlForCandidateId"] = algorithm["id"].clone();
            selected["controlForCandidateRevision"] = algorithm["revision"].clone();
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

/// A target derived from independent frozen paired outcomes is scoped to
/// this exact adopted revision. A descendant can synthesize its OWN edit
/// policy, but its speculative search selector must not silently overwrite
/// the measured objective before testing the next candidate.
fn paired_descendant_target(input:&Value)->bool {
    input["rsiSourceScopeExpanded"]==true
        && input["rsiDecisionInput"]["basis"]==
            "independently_measured_post_adoption_target"
        && input["rsiSelectedDomain"]==input["rsiDecisionInput"]["targetDomain"]
        && input["rsiSelectedDomain"].as_str()
            .is_some_and(|domain|DOMAINS.contains(&domain))
}

#[cfg(test)]
mod paired_descendant_target_tests {
    use super::*;
    #[test]
    fn immutable_paired_target_outvotes_unmeasured_descendant_exploration() {
        let evidence=json!({"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"improver",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"improver"}});
        assert!(paired_descendant_target(&evidence));
        let mut mismatched=evidence.clone();
        mismatched["rsiSelectedDomain"]=json!("memory");
        assert!(!paired_descendant_target(&mismatched));
        let mut speculative=evidence;
        speculative["rsiDecisionInput"]["basis"]=json!("parent_next_decision");
        assert!(!paired_descendant_target(&speculative));
        let only_system=json!({"rsiSystemEvolution":true,"rsiSelectedDomain":"improver"});
        assert!(!paired_descendant_target(&only_system),
            "system mode cannot masquerade as independently measured target");
    }
}

/// Ask the exact rebuilt descendant to plan its next improvement. This
/// command is deliberately read-only and never starts a server or a model.
fn system_evolution_target_hint(input:&Value, attempt:u64)->Option<String> {
    if paired_descendant_target(input) {
        input["rsiSelectedDomain"].as_str().map(str::to_string)
    } else if input["rsiAutonomous"]!=true && input["rsiAutoFocus"]==false
        && input["rsiFocusFiles"].as_array().is_some_and(|rows|!rows.is_empty()) {
        // A real user-specified source task is an immutable task constraint
        // regardless of whether it comes from an interactive UI or a
        // downstream A/B. The compiled planner still selects its algorithms.
        input["rsiSelectedDomain"].as_str()
            .filter(|domain|DOMAINS.contains(domain)).map(str::to_string)
    } else if attempt==1 && input["rsiAutonomous"]==true {
        Some("improver".into())
    } else {None}
}

pub fn plan_generation_from_file(path:&str)->Result<(),String> {
    let snapshot:Value=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?)
        .map_err(|e|e.to_string())?;
    let commit=snapshot["generatorCommit"].as_str().ok_or("계획 생성기 버전이 없습니다.")?;
    if option_env!("DOMINION_BUILD_COMMIT")!=Some(commit) {
        return Err("계획 생성기 바이너리와 소스 커밋이 다릅니다.".into());
    }
    let input=&snapshot["input"];
    if input["rsiSystemEvolution"]!=true {
        return Err("전체 시스템 진화 작업만 후손 계획을 호출할 수 있습니다.".into());
    }
    let campaign=input["rsiCampaignId"].as_str().ok_or("계획 캠페인 ID가 없습니다.")?;
    let config=crate::config::Config::from_env();
    let store=crate::db::Store::open(&config.db_path()).map_err(|e|e.to_string())?;
    let vault=crate::vault::Vault::open(&config.key_path());
    let state=AppState::new(config,store,vault);
    let mut history=state.store.all("rsi_generation").into_iter()
        .filter(|row|row["campaignId"]==campaign).collect::<Vec<_>>();
    history.sort_by_key(|row|row["attempt"].as_u64().unwrap_or(0));
    let objective=selection_objective(input);
    let source=input["rsiGeneratorSourceDir"].as_str()
        .or_else(||input["rsiSourceDir"].as_str())
        .ok_or("후손 시스템 소스 위치가 없습니다.")?;
    let system_blueprint=crate::engine::rsi_system::blueprint(Path::new(source),commit,&history);
    let mut frontier=crate::engine::rsi_system::architecture_frontier(&state,&system_blueprint,&history,
        input["rsiIndexId"].as_str().unwrap_or(""),Path::new(input["rsiSourceDir"].as_str().unwrap_or(source)));
    let inherited_stage=crate::engine::rsi_system::inherited_stage(&history,&frontier);
    if let Some(stage)=inherited_stage.as_ref() {
        // Descendant meta-learning decides between a newly discovered goal
        // and an inherited executable policy. Previously adopted stages
        // compete with novel alternatives on the SAME source-backed frontier.
        for row in &mut frontier {
            if row["role"]==stage["role"] && row["domain"]==stage["domain"]
                && row["path"]==stage["path"]
                && row["sourceSha256"]==stage["sourceSha256"] {
                row["inheritedProgramStage"]=json!(true);
                row["learningPolicy"]=stage["learningPolicy"].clone();
                row["toolPolicy"]=stage["toolPolicy"].clone();
                row["executionPolicy"]=stage["executionPolicy"].clone();
            }
        }
    }
    let explicit=input["rsiAutonomous"]!=true && input["rsiAutoFocus"]==false
        && input["rsiFocusFiles"].as_array().is_some_and(|paths|!paths.is_empty());
    let inherited=if input["rsiAutonomous"]==true {
        crate::engine::rsi_system::inherited_domain(&history)
    }else{None};
    let allowed=candidate_domains(&objective);
    let lineage=crate::engine::rsi_system::lineage_capsule(&history);
    let influence=crate::engine::rsi_interaction::influence_graph(&history,&frontier);
    let profile=crate::engine::rsi_profile::from_input(input)?;
    let peers=crate::engine::rsi_system::peer_lineage_references(
        &state,campaign,profile["sha256"].as_str().unwrap_or(""),
        input["rsiIndexId"].as_str().unwrap_or(""),None);
    // Crucial: the *compiled descendant*, rather than the resident server,
    // scores possible meta-improvement targets and proposes its next experiment.
    let meta_policy=crate::engine::rsi_meta_controller::synthesize(
        &history,&frontier,&allowed,inherited.as_deref(),&lineage,&peers,&influence);
    let architecture_target=if explicit || paired_descendant_target(input) {
        Value::Null
    }else if meta_policy["target"].is_object() {
        meta_policy["target"].clone()
    }else{
        frontier.iter().find(|row|row["domain"].as_str().is_some_and(|domain|
            allowed.contains(&domain) && inherited.as_deref().is_none_or(|pinned|pinned==domain)))
            .cloned().unwrap_or(Value::Null)
    };
    // A whole-system successor can design interlocking improvements in
    // memory, harness, learning and meta-search in the SAME generation.
    // Explicit owner-assigned targets remain a higher-priority constraint.
    let coupled_domains=if input["rsiAutonomous"]==true && !explicit
        && !paired_descendant_target(input) {
        DOMAINS.to_vec()
    }else{allowed.clone()};
    let co_design=if architecture_target.is_object() {
        crate::engine::rsi_interaction::coupled_portfolio(
            &architecture_target,&frontier,&influence,&coupled_domains)
    }else{Value::Null};
    let successor_stage=if architecture_target["inheritedProgramStage"]==true
        && inherited_stage.as_ref().is_some_and(|stage|
            stage["role"]==architecture_target["role"]
            && stage["path"]==architecture_target["path"]
            && stage["sourceSha256"]==architecture_target["sourceSha256"]) {
        inherited_stage.clone().unwrap_or(Value::Null)
    }else{Value::Null};
    let successor_hint=if explicit || paired_descendant_target(input) {
        system_evolution_target_hint(input,snapshot["attempt"].as_u64().unwrap_or(0))
    }else{architecture_target["domain"].as_str().map(str::to_string).or(inherited)};
    let trial=if paired_descendant_target(input) {None} else {
        input["rsiTrialAlgorithmId"].as_str().map(str::to_string)
    };
    let (domain,selection,strategy)=select_generation_strategy(
        &state,&history,&objective,None,trial,successor_hint,commit);
    let mut source_input=input.clone();
    source_input["rsiTargetDomain"]=json!(domain);
    source_input["rsiSelectedDomain"]=json!(domain);
    let policy=strategy["id"].as_str()
        .and_then(|id|state.store.get("algorithm",id))
        .or_else(||crate::engine::algorithms::active(&state,"selfimprove.search"));
    let ranked=choose_focus_files(&state,&source_input,&history,policy.as_ref());
    let explicit_paths=input["rsiFocusFiles"].as_array()
        .filter(|paths|input["rsiAutonomous"]!=true
            && input["rsiAutoFocus"]==false && !paths.is_empty());
    let mut focus=if let Some(paths)=explicit_paths {
        paths.iter().filter_map(Value::as_str)
            .filter(|path|crate::engine::rsi::safe_path(path))
            .take(8).collect::<Vec<_>>()
    }else{
        ranked.iter().filter_map(|row|row["path"].as_str())
            .filter(|path|crate::engine::rsi::safe_path(path))
            .take(8).collect::<Vec<_>>()
    };
    if !explicit && architecture_target["domain"]==domain {
        let mut chosen=co_design["focus"].as_array().into_iter().flatten()
            .filter_map(|row|row["path"].as_str()).collect::<Vec<_>>();
        chosen.reverse();
        for path in chosen {
            focus.retain(|current|*current!=path);
            focus.insert(0,path);
        }
        focus.truncate(8);
    }
    let directive=crate::engine::rsi_system::improvement_directive(&history);
    let comparative_mode=if peers["references"].as_array()
        .is_some_and(|items|!items.is_empty()) {"compare_independent_peer_lineage"}
        else if lineage["failureCounterexamples"].as_array()
            .is_some_and(|items|items.len()>=2) {"multi_trajectory_reflection"}
        else {"local_source_redesign"};
    let archive=if input["rsiAutonomous"]==true && input["rsiAutoFocus"]==true {
        crate::engine::rsi_archive::select(
            &state,&history,commit,&input["rsiFrozenCases"])
    }else{Value::Null};
    let edit_policy=learned_edit_method(&state,input,&history);
    println!("{}",json!({
        "source":"compiled_descendant_system_controller_v2",
        "generatorCommit":commit,"historyCount":history.len(),
        "domain":domain,"selection":selection,"strategy":strategy,
        "focusFiles":focus,"ranked":ranked,
        "architectureFrontier":frontier,"architectureTarget":architecture_target,
        "usageEvidence":crate::engine::rsi_system::usage_evidence(&state),
        "systemBlueprint":system_blueprint,
        "improvementDirective":directive,
        "lineageCapsule":lineage,
        "peerLineages":peers,"comparativeMode":comparative_mode,
        "metaPolicy":meta_policy,"successorStage":successor_stage,
        "interactionGraph":influence,"coDesignPortfolio":co_design,
        "archiveSelection":archive,
        "editPolicy":edit_policy
    }));
    Ok(())
}

#[cfg(test)]
mod system_evolution_handoff_tests {
    use super::*;
    #[test]
    fn measured_descendant_targets_survive_but_stale_parent_hints_do_not() {
        let first=json!({"rsiSystemEvolution":true,"rsiAutonomous":true,"rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&first,1),Some("improver".into()));
        assert_eq!(system_evolution_target_hint(&first,2),None);
        let explicit=json!({"rsiSystemEvolution":true,"rsiAutonomous":false,
            "rsiObjective":"Improve memory","rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&explicit,1),None,
            "owner-defined objectives must outrank the default improver seed");
        let focused=json!({"rsiSystemEvolution":true,"rsiAutonomous":false,
            "rsiAutoFocus":false,"rsiFocusFiles":["src/engine/memory.rs"],
            "rsiSelectedDomain":"memory"});
        assert_eq!(system_evolution_target_hint(&focused,1),Some("memory".into()),
            "a successor planner must honor assigned source tasks as well");
        let measured=json!({"rsiSystemEvolution":true,"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"context",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"context"}});
        assert_eq!(system_evolution_target_hint(&measured,2),Some("context".into()));
    }
}

/// The production coordinator may pass an approved controller receipt, but
/// MUST NOT recompute and replace the compiled descendant's design. Verify the
/// exact generation and immutable source identity before any paid operation.
fn validated_descendant_controller(
    input:&Value,history:&[Value],commit:&str,
)->Result<Option<(String,&'static str,Value)>,String>{
    if input["rsiSystemEvolution"]!=true {return Ok(None);}
    // A separately assigned downstream coding task has a pinned target and
    // a distinct, source-identified compiled generator. There is no automatic
    // next-generation plan to inherit in this mode; the real descendant
    // worker still runs the ordinary model, harness and editing policy.
    // Normal self-evolution campaigns must always provide a controller plan.
    if input["rsiControllerPlan"].is_null()
        && input["rsiGeneratorSourceDir"].as_str().is_some_and(|p|!p.is_empty())
        && input["rsiFocusFiles"].as_array().is_some_and(|p|!p.is_empty())
        && input["rsiSystemBlueprint"]["generatorCommit"]==commit
        && input["rsiSystemBlueprint"]["source"]=="compiled_descendant_system_blueprint_v1"
        && input["rsiImprovementDirective"].is_object()
        && input["rsiAutoFocus"]==false {
        return Ok(None);
    }
    let plan=&input["rsiControllerPlan"];
    if plan["source"]!="compiled_descendant_system_controller_v2"
        || plan["generatorCommit"]!=commit
        || plan["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["generatorCommit"]!=commit
        || plan["systemBlueprint"]["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["source"]!="compiled_descendant_system_blueprint_v1"
        || !plan["improvementDirective"].is_object()
        || !plan["editPolicy"].is_object()
        || plan["lineageCapsule"]["source"]!="descendant_curated_lineage_v1"
        || plan["lineageCapsule"]["historyCount"].as_u64()!=Some(history.len() as u64)
        || plan["systemBlueprint"]["lineageSha256"]!=plan["lineageCapsule"]["sha256"]
        || plan["peerLineages"]["source"]!="cross_campaign_comparative_archive_v1"
        || !crate::engine::rsi_meta_controller::valid_policy(&plan["metaPolicy"],history.len())
        || !crate::engine::rsi_interaction::valid_graph(&plan["interactionGraph"])
        || plan["metaPolicy"]["interactionGraphSha256"]!=plan["interactionGraph"]["sha256"]
        || (plan["coDesignPortfolio"].is_object()
            && plan["architectureFrontier"].as_array().is_none_or(|frontier|
                !crate::engine::rsi_interaction::valid_portfolio(
                    &plan["coDesignPortfolio"],&plan["interactionGraph"],frontier)))
        || (plan["successorStage"].is_object()
            && (plan["successorStage"]["source"]!="inherited_model_authored_improvement_program_v1"
                || plan["architectureTarget"]["path"]!=plan["successorStage"]["path"]
                || plan["architectureTarget"]["sourceSha256"]!=plan["successorStage"]["sourceSha256"]))
        || (plan["architectureTarget"].is_object()
            && plan["metaPolicy"]["target"]!=plan["architectureTarget"])
        || !matches!(plan["comparativeMode"].as_str(),
            Some("local_source_redesign"|"multi_trajectory_reflection"|"compare_independent_peer_lineage"))
    {
        return Err("후손 제어기의 출처·실행 이력·방법이 일치하지 않습니다.".into());
    }
    let domain=plan["domain"].as_str()
        .filter(|d|DOMAINS.contains(d))
        .ok_or("후손 제어기가 유효하지 않은 분야를 지정했습니다.")?;
    let selection=match plan["selection"].as_str() {
        Some("algorithm_active")=>"algorithm_active",
        Some("algorithm_trial")=>"algorithm_trial",
        Some("research_continuation")=>"research_continuation",
        _=>return Err("후손 제어기의 탐색 방법이 유효하지 않습니다.".into())
    };
    if input["rsiSelectedDomain"]!=domain
        || input["rsiTargetDomain"]!=domain
        || input["rsiTargetPlan"]["domain"]!=domain
        || input["rsiTargetPlan"]["generatorCommit"]!=commit
        || input["rsiTargetPlan"]["selection"]!=plan["selection"]
        || input["rsiTargetPlan"]["strategy"]!=plan["strategy"]
        || input["rsiEditPolicy"]!=plan["editPolicy"]
        || input["rsiSystemBlueprint"]!=plan["systemBlueprint"]
        || input["rsiImprovementDirective"]!=plan["improvementDirective"]
        || input["rsiLineageCapsule"]!=plan["lineageCapsule"]
        || input["rsiPeerLineages"]!=plan["peerLineages"]
        || input["rsiComparativeMode"]!=plan["comparativeMode"]
        || input["rsiMetaPolicy"]!=plan["metaPolicy"]
        || input["rsiSuccessorStage"]!=plan["successorStage"]
        || input["rsiInteractionGraph"]!=plan["interactionGraph"]
        || input["rsiCoDesignPortfolio"]!=plan["coDesignPortfolio"]
    {
        return Err("후손 계획과 실행 worker의 목표·전략·자기모형이 다릅니다.".into());
    }
    if input["rsiFocusSelection"]["source"]!="owner_selected"
        && input["rsiFocusFiles"]!=plan["focusFiles"] {
        return Err("후손이 선택한 소스가 worker 실행 전에 변경됐습니다.".into());
    }
    if paired_descendant_target(input) && input["rsiDecisionInput"]["targetDomain"]!=domain {
        return Err("검증된 세대 간 목표를 후손 제어기가 무시했습니다.".into());
    }
    Ok(Some((domain.to_string(),selection,plan["strategy"].clone())))
}

#[cfg(test)]
mod descendant_controller_contract_tests {
    use super::*;
    #[test]
    fn successor_plan_is_authoritative_and_rejects_parent_override() {
        let commit="a".repeat(40);
        let lineage=json!({"source":"descendant_curated_lineage_v1",
            "historyCount":1,"sha256":"f".repeat(64)});
        let meta=crate::engine::rsi_meta_controller::synthesize(
            &[json!({"attempt":1})],&[json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceBytes":100})],
            &["learning"],None,&json!({}),&json!({}));
        let plan=json!({
            "source":"compiled_descendant_system_controller_v2",
            "generatorCommit":commit,"historyCount":1,"domain":"learning",
            "selection":"algorithm_active",
            "strategy":{"id":"descendant-search","revision":"r2"},
            "focusFiles":["src/engine/learning.rs"],
            "systemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                "generatorCommit":commit,"historyCount":1,"lineageSha256":lineage["sha256"]},
            "lineageCapsule":lineage,
            "peerLineages":{"source":"cross_campaign_comparative_archive_v1",
                "profileSha256":"f".repeat(64),"references":[],"automaticPatchReuse":false},
            "comparativeMode":"local_source_redesign",
            "metaPolicy":meta,"successorStage":null,
            "improvementDirective":{"mode":"change_approach"},
            "editPolicy":{"revision":"descendant-method-r2"}
        });
        let input=json!({
            "rsiSystemEvolution":true,"rsiGeneratorCommit":commit,
            "rsiSelectedDomain":"learning","rsiTargetDomain":"learning",
            "rsiTargetPlan":{"domain":"learning","selection":"algorithm_active",
                "strategy":plan["strategy"],"generatorCommit":commit},
            "rsiFocusFiles":plan["focusFiles"],
            "rsiFocusSelection":{"source":"rebuilt_descendant_controller"},
            "rsiEditPolicy":plan["editPolicy"],
            "rsiSystemBlueprint":plan["systemBlueprint"],
            "rsiImprovementDirective":plan["improvementDirective"],
            "rsiLineageCapsule":plan["lineageCapsule"],
            "rsiPeerLineages":plan["peerLineages"],
            "rsiComparativeMode":plan["comparativeMode"],
            "rsiMetaPolicy":plan["metaPolicy"],
            "rsiSuccessorStage":plan["successorStage"],
            "rsiControllerPlan":plan
        });
        let history=vec![json!({"attempt":1})];
        let validated=validated_descendant_controller(&input,&history,&commit)
            .unwrap().unwrap();
        assert_eq!(validated.0,"learning");
        assert_eq!(validated.2["id"],"descendant-search");
        let mut hijacked=input.clone();
        hijacked["rsiSelectedDomain"]=json!("improver");
        assert!(validated_descendant_controller(&hijacked,&history,&commit).is_err());
        let mut stale=input.clone();
        stale["rsiControllerPlan"]["generatorCommit"]=json!("old");
        assert!(validated_descendant_controller(&stale,&history,&commit).is_err());
        let manual=json!({"rsiSystemEvolution":true,"rsiAutoFocus":false,
            "rsiGeneratorSourceDir":"/pinned/source","rsiFocusFiles":["src/engine/memory.rs"],
            "rsiSystemBlueprint":{"source":"compiled_descendant_system_blueprint_v1",
                "generatorCommit":commit},
            "rsiImprovementDirective":{"mode":"discover"}});
        assert!(validated_descendant_controller(&manual,&history,&commit)
            .unwrap().is_none(),"assigned downstream coding uses the normal compiled worker");
        let mut unsafe_manual=manual;
        unsafe_manual["rsiGeneratorSourceDir"]=Value::Null;
        assert!(validated_descendant_controller(&unsafe_manual,&history,&commit).is_err(),
            "unplanned self-evolution cannot bypass the compiled controller");
        let mut lost_lineage=input.clone();
        lost_lineage["rsiLineageCapsule"]["sha256"]=json!("a".repeat(64));
        assert!(validated_descendant_controller(&lost_lineage,&history,&commit).is_err(),
            "the resident coordinator cannot exchange the descendant's inherited experience");
        let mut wrong_method=input;
        wrong_method["rsiEditPolicy"]["revision"]=json!("parent");
        assert!(validated_descendant_controller(&wrong_method,&history,&commit).is_err());
    }
}

/// Executes only one durable generation, without starting a server, scheduler,
/// migrations or recovering another process's jobs in the shared ledger.
pub async fn worker(job_id: &str) -> Result<(), String> {
    let config = crate::config::Config::from_env();
    let store = crate::db::Store::open(&config.db_path()).map_err(|e| e.to_string())?;
    let vault = crate::vault::Vault::open(&config.key_path());
    let state = AppState::new(config, store, vault);
    let mut job = state
        .store
        .get("job", job_id)
        .ok_or("세대 작업을 찾을 수 없습니다.")?;
    if job["kind"] != "rsi_generation"
        || job["status"] != "running"
        || job["cancelRequested"] == true
        || job["input"]["rsiGeneratorCommit"].as_str() != option_env!("DOMINION_BUILD_COMMIT")
    {
        return Err("세대 작업 또는 개선기 소스 revision이 일치하지 않습니다.".into());
    }
    let campaign = job["input"]["rsiCampaignId"]
        .as_str()
        .ok_or("자동 개선 ID 없음")?.to_string();
    if state.store.get("job", &campaign).is_none_or(|p| {
        p["kind"] != "rsi_evolve" || p["status"] != "running" || p["cancelRequested"] == true
    }) {
        return Err("자동 개선이 중단되었습니다.".into());
    }
    let mut history = state
        .store
        .all("rsi_generation")
        .into_iter()
        .filter(|g| g["campaignId"] == campaign)
        .collect::<Vec<_>>();
    history.sort_by_key(|g|g["attempt"].as_u64().unwrap_or(0));
    // Downstream A/B episodes exercise the normal generation path.
    // An evaluator-specific input branch would be a shortcut the candidate
    // could optimize instead of improving the actual agent runtime.
    let objective = selection_objective(&job["input"]);
    // System evolution is a search mode, not a measured holdout constraint.
    // Only a genuine paired target or live failed diagnosis may pin a domain.
    let paired_target=paired_descendant_target(&job["input"])
        || job["input"]["rsiDiagnostic"]["failed"].as_u64().unwrap_or(0)>0;
    let latest=history.iter().max_by_key(|g|g["attempt"].as_u64().unwrap_or(0));
    let continuation = latest.filter(|_|!paired_target)
        .filter(|g|g["candidateId"].is_null() && g["newFindings"].as_u64().unwrap_or(0)>0)
        .and_then(|g|g["domain"].as_str()).map(str::to_string);
    let trial_id = job["input"]["rsiTrialAlgorithmId"].as_str()
        .filter(|_|!paired_target)
        .filter(|value| !value.is_empty())
        .and_then(|id|state.store.get("algorithm",id))
        .filter(|algorithm|algorithm["rsiTrial"]["campaignId"].as_str()
            .is_some_and(|owner|job["input"]["rsiAllowedTrialCampaigns"]
                .as_array().is_some_and(|ids|ids.iter().any(|id|id.as_str()==Some(owner)))))
        .and_then(|algorithm|algorithm["id"].as_str().map(str::to_string));
    let selected_domain = job["input"]["rsiSelectedDomain"]
        .as_str()
        .filter(|domain| DOMAINS.contains(domain))
        .map(str::to_string);
    let commit=job["input"]["rsiGeneratorCommit"].as_str().unwrap_or("").to_string();
    let delegated=validated_descendant_controller(&job["input"],&history,&commit)?;
    let (mut domain, mut domain_selection, mut strategy)=if let Some(plan)=delegated {
        plan
    }else{
        select_generation_strategy(
            &state,&history,&objective,continuation.clone(),trial_id.clone(),
            selected_domain.clone(),&commit)
    };
    // An adopted binary can revise its own targeting logic. Keep the original
    // parent decision unless the recompiled generator reaches a different
    // conclusion on the same durable history and objective.
    if job["input"]["rsiSystemEvolution"]!=true
        && job["input"]["rsiSourceAdvanced"]==true
        && job["input"]["rsiAutoFocus"]==true && continuation.is_none()
        && selected_domain.is_some() && !paired_target {
        let fresh=select_generation_strategy(&state,&history,&objective,
            None,trial_id,None,&commit);
        if fresh.0!=domain {
            (domain,domain_selection,strategy)=fresh;
            strategy["decisionSource"]=json!("descendant_algorithm");
        }
    }
    if paired_target {
        state.store.audit("rsi.independent_target_preserved",json!({
            "jobId":job_id,"generatorCommit":commit,
            "targetDomain":domain,"source":"frozen_paired_post_adoption"
        }));
    }
    if job["input"]["rsiSystemEvolution"]!=true {
    if let Err(error)=reconcile_descendant_target(&state,&mut job,&history,
        &domain,domain_selection,&strategy,&commit) {
        if error.starts_with("rsi_token_budget:") {
            job["status"]=json!("failed");
            job["error"]=json!(error);
            job["result"]=json!({"rsi":{"modelError":error}});
            state.store.put("job",&job).ok_or("세대 예산 중단 기록 실패")?;
        }
        return Err(error);
    }
    }
    // The coordinator that queued this child may be the OLD generator. A
    // recompiled descendant must actually use its own revised policy
    // synthesis, not inherit an obsolete parent-selected edit method.
    let mut descendant_method=if job["input"]["rsiSystemEvolution"]==true {
        job["input"]["rsiControllerPlan"]["editPolicy"].clone()
    }else{learned_edit_method(&state,&job["input"],&history)};
    descendant_method["selectedByGeneratorCommit"]=json!(commit);
    job["input"]["rsiEditPolicy"]=descendant_method.clone();
    job["rsiEditPolicyDecision"]=json!({
        "producer":"running_descendant_binary",
        "generatorCommit":commit,
        "methodRevision":descendant_method["revision"],
        "observations":descendant_method["observations"],
    });
    state.store.audit("rsi.descendant_edit_method_selected",json!({
        "jobId":job_id,"generatorCommit":commit,"method":descendant_method,
    }));
    job["rsiDomain"] = json!(domain);
    job["rsiDomainSelection"] = json!(domain_selection);
    job["rsiSearchStrategy"] = strategy.clone();
    job["rsiSelectionObjective"] = json!(objective);
    job["rsiDecisionInput"] = job["input"]["rsiDecisionInput"].clone();
    job["input"]["rsiDomainSelection"] = json!(domain_selection);
    job["input"]["rsiSearchStrategy"] = strategy;
    let focus = job["input"]["rsiFocusFiles"].as_array().into_iter().flatten()
        .filter_map(Value::as_str).take(8).collect::<Vec<_>>().join(", ");
    let capability = if focus.is_empty() { domain.clone() } else { format!("{domain} · {focus}") };
    job["rsiDomain"] = json!(domain);
    job["input"]["rsiDomain"]=json!(domain);
    let descendant_prompt=if job["input"]["rsiSystemEvolution"]==true {
        let source=job["input"]["rsiSourceDir"].as_str()
            .ok_or("후손 소스 폴더가 없습니다.")?.to_string();
        // The agent's compiled source is normally the same checkout that it
        // edits. In a downstream A/B episode both generators edit IDENTICAL
        // subject bytes, while their own executable source revisions differ.
        // Never mistake the common subject for the candidate's intelligence.
        let generator_source=job["input"]["rsiGeneratorSourceDir"]
            .as_str().unwrap_or(&source).to_string();
        let blueprint=crate::engine::rsi_system::blueprint(
            Path::new(&generator_source),&commit,&history);
        if blueprint!=job["input"]["rsiSystemBlueprint"] {
            return Err("후손 소스 자기모형이 컴파일된 계획과 일치하지 않습니다.".into());
        }
        let expected=crate::engine::rsi_system::improvement_directive(&history);
        if job["input"]["rsiImprovementDirective"]!=expected {
            return Err("후손 실패 학습 지시와 실제 세대 기록이 다릅니다.".into());
        }
        let lineage=crate::engine::rsi_system::lineage_capsule(&history);
        if job["input"]["rsiControllerPlan"].is_object()
            && job["input"]["rsiLineageCapsule"]!=lineage {
            return Err("후손의 성공·실패 학습 계보가 현재 기록과 다릅니다.".into());
        }
        let pinned=job["input"]["rsiControllerPlan"].is_object();
        if pinned {
            let profile=crate::engine::rsi_profile::from_input(&job["input"])?;
            crate::engine::rsi_system::validate_peer_references(
                &state,&job["input"]["rsiPeerLineages"],
                profile["sha256"].as_str().unwrap_or(""))?;
        }
        job["rsiLineageVerified"]=json!(pinned);
        job["rsiMetaPolicyVerified"]=json!(!pinned
            || crate::engine::rsi_meta_controller::valid_policy(
                &job["input"]["rsiMetaPolicy"],history.len()));
        if pinned && (!crate::engine::rsi_interaction::valid_graph(
            &job["input"]["rsiInteractionGraph"])
            || (job["input"]["rsiCoDesignPortfolio"].is_object()
                && job["input"]["rsiControllerPlan"]["architectureFrontier"]
                    .as_array().is_none_or(|frontier|
                    !crate::engine::rsi_interaction::valid_portfolio(
                        &job["input"]["rsiCoDesignPortfolio"],
                        &job["input"]["rsiInteractionGraph"],frontier)))) {
            return Err("실제 후손 작업기의 복합 지능 설계 출처가 변경됐습니다.".into());
        }
        job["rsiSystemBlueprintVerified"]=json!(true);
        job["rsiImprovementDirectiveVerified"]=json!(true);
        state.store.audit("rsi.descendant_system_runtime",json!({
            "jobId":job_id,"generatorCommit":commit,
            "source":"compiled_successor_planning_and_context",
            "architectureComponents":blueprint["architecture"].as_array().map(Vec::len),
            "improvementDirective":expected["mode"]
        }));
        let campaign_record=state.store.get("rsi_campaign",&campaign)
            .unwrap_or(Value::Null);
        // Recompute the context from the NEW generator's code. A previous
        // resident server can neither select the prompt nor re-inject stale
        // source excerpts as purported evidence.
        let observed=generation_context(&state,&job["input"],&campaign_record);
        let research=research_digest(&state,&job["input"],&campaign_record);
        // Neutral archived source is inherited AFTER the campaign checkout.
        // A pre-inheritance excerpt belongs to the wrong bytes and SHA.
        // Give the model a fresh-read instruction instead of stale source.
        let focus=if job["input"]["rsiInheritedCandidateId"].is_string() {
            "중립 후속 후보가 별도 작업공간에 적용됩니다. 이전 소스 발췌와 해시는 변경 전 버전입니다. workspace_read_range로 현재 원문과 sourceSha256을 확인한 뒤 편집하세요.".to_string()
        }else{
            focus_file_context(&state,&json!({"labDir":source}),&job["input"])
        };
        let capsule=|text:&str,limit:usize|text.chars().take(limit).collect::<String>();
        let owner=state.store.get("job",&campaign)
            .and_then(|parent|parent["input"]["rsiObjective"].as_str()
                .map(str::to_string)).unwrap_or_default();
        let assigned=job["input"]["rsiObjective"].as_str().unwrap_or(&owner);
        let curated=serde_json::to_string(&lineage).unwrap_or_default();
        let peer_evidence=serde_json::to_string(&job["input"]["rsiPeerLineages"])
            .unwrap_or_default();
        let meta_instructions=if job["input"]["rsiMetaPolicy"].is_object() {
            crate::engine::rsi_meta_controller::instruction(&job["input"]["rsiMetaPolicy"])
        }else{String::new()};
        let coupled_instructions=if job["input"]["rsiCoDesignPortfolio"].is_object() {
            let source=job["input"]["rsiCoDesignPortfolio"]["focus"].as_array()
                .into_iter().flatten().take(3)
                .map(|row|format!("role={} path={} sha={}",
                    row["role"],row["path"],row["sourceSha256"]))
                .collect::<Vec<_>>().join("; ");
            format!("INTERACTING INTELLIGENCE SUBSYSTEMS: {}. This is a cross-role redesign HYPOTHESIS, not verified causal proof. Inspect each current source, explain the functional interfaces and use rsi_system_plan component=system with separate componentFiles for each role. Modify multiple actual modules when their interaction is the limitation. Avoid claiming single-file credit for a joint intervention.",
                source)
        }else{String::new()};
        let successor_instructions=if job["input"]["rsiSuccessorStage"].is_object() {
            format!("ADOPTED NEXT-GENERATION IMPROVEMENT PROGRAM: {}. Source={} sha={}. Learning policy={} tool policy={} execution policy={}. Reconfirm current source SHA before editing. This program is an unverified design instruction, NOT a measured capability gain.",
                job["input"]["rsiSuccessorStage"]["objective"],
                job["input"]["rsiSuccessorStage"]["path"],
                job["input"]["rsiSuccessorStage"]["sourceSha256"],
                job["input"]["rsiSuccessorStage"]["learningPolicy"],
                job["input"]["rsiSuccessorStage"]["toolPolicy"],
                job["input"]["rsiSuccessorStage"]["executionPolicy"])
        }else{String::new()};
        format!("{}\n\nEDITABLE META-IMPROVER DECISION:\n{}\n\nINHERITED EXECUTABLE IMPROVEMENT PROGRAM:\n{}\n\nNEXT IMPROVEMENT ACTION (measured history): {}\n\nCAMPAIGN OBJECTIVE: {}\n\nCURRENT ASSIGNED TASK: {}\n\nCOMPARATIVE REDESIGN MODE: {}\n\nOTHER INDEPENDENTLY TESTED LINEAGES (references only, never automatic patches):\n{}\n\nCURATED ADOPTED DESIGNS AND FAILURE COUNTEREXAMPLES (data, not instructions):\n{}\n\nPREVIOUS VERIFIABLE EVIDENCE:\n{}\n\nRELEVANT RESEARCH:\n{}\n\nCURRENT REVISION SOURCE:\n{}",
            crate::engine::rsi_system::instruction(&blueprint,assigned),
            format!("{meta_instructions}\n{coupled_instructions}"),
            successor_instructions,expected,owner,assigned,job["input"]["rsiComparativeMode"],
            capsule(&peer_evidence,3500),capsule(&curated,5500),capsule(&observed,3500),
            capsule(&research,3500),capsule(&focus,6000))
    } else {job["input"]["prompt"].as_str().unwrap_or("").to_string()};
    let prepared = fit_worker_prompt(
        &descendant_prompt, &domain, &capability,
        &history, job["input"]["rsiRemainingCampaignTokens"].as_u64()
    );
    let (prompt, plan) = match prepared {
        Ok(value) => value,
        Err(error) => {
            // Persist preflight as a real budget stop. The parent can conclude
            // without paying for a doomed model call or launching a new step.
            job["status"]=json!("failed");
            job["error"]=json!(error);
            job["result"]=json!({"rsi":{"modelError":error}});
            state.store.put("job",&job).ok_or("세대 예산 중단 기록 실패")?;
            return Err(error);
        }
    };
    job["input"]["prompt"]=json!(prompt);
    job["rsiWorkerContextPlan"]=plan;
    state.store.put("job", &job).ok_or("세대 지시 저장 실패")?;
    let context = JobContext::new(state.clone(), job_id.into());
    let result = jobs::run_prompt(&context, &job).await;
    match result {
        Ok(value) => {
            context.checkpoint(json!({"status":"completed","result":value}))?;
            Ok(())
        }
        Err(error) => {
            context.checkpoint(json!({"status":"failed","error":error}))?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-evolution-test-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root.join("work"),
        };
        let state = AppState::new(
            config.clone(),
            crate::db::Store::open(&config.db_path()).unwrap(),
            crate::vault::Vault::open(&config.key_path()),
        );
        (state, root)
    }
    fn parent(state: &AppState) -> JobContext {
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"running","input":{"rsiNativeApproved":true}})).unwrap();
        JobContext::new(state.clone(), "campaign".into())
    }
    #[test]
    fn verified_adoption_opens_a_new_source_goal_only_when_opted_in() {
        let root=json!({"rsiExpandAfterGain":true,"rsiObjective":"fix one file"});
        let history=vec![json!({
            "accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"measurements":[
                {"id":"planning-quality","domain":"planning","metrics":[
                    {"key":"quality","candidate":1.0,"improved":true}]},
                {"id":"memory-quality","domain":"memory","metrics":[
                    {"key":"quality","candidate":0.7,"improved":false}]}
            ]}}
        })];
        let chosen=post_adoption_objective(&root,&history).unwrap();
        assert!(chosen["objective"].as_str().unwrap().contains("memory-quality"));
        assert!(!chosen["objective"].as_str().unwrap().contains("planning-quality"));
        assert_eq!(chosen["residuals"].as_array().unwrap().len(),1);
        assert!(chosen["prompt"].as_str().unwrap().contains(&"a".repeat(40)));
        let mut disabled=root.clone();
        disabled["rsiExpandAfterGain"]=json!(false);
        assert!(post_adoption_objective(&disabled,&history).is_none());
        let forged=vec![json!({"accepted":true,"acceptedCommit":"b".repeat(40),
            "evaluation":{"metrics":{"eligible":false}}})];
        assert!(post_adoption_objective(&root,&forged).is_none());
    }

    #[test]
    fn saturated_quality_switches_to_measured_context_cost_before_noisy_latency() {
        let root=json!({"rsiExpandAfterGain":true,"rsiFrozenCases":[
            {"id":"group-chat-input-cost","metrics":[{"key":"contextChars",
                "minRelativeGain":0.05,"minAbsoluteGain":50.0}]},
            {"id":"memory-quality","metrics":[{"key":"elapsedMs",
                "minRelativeGain":0.20,"minAbsoluteGain":1.0}]}
        ]});
        let history=vec![json!({"accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":1,"measurements":[
                {"id":"memory-quality","domain":"memory","metrics":[
                    {"key":"quality","candidate":1.0,"improved":false},
                    {"key":"elapsedMs","direction":"lower","candidate":230.0,"improved":false}]},
                {"id":"group-chat-input-cost","domain":"context","metrics":[
                    {"key":"quality","candidate":1.0,"improved":false},
                    {"key":"contextChars","direction":"lower","candidate":77000.0,"improved":false}]}
            ]}}})];
        let next=post_adoption_objective(&root,&history).unwrap();
        assert_eq!(next["costOpportunities"][0]["caseId"],"group-chat-input-cost");
        assert_eq!(next["costOpportunities"][0]["requiredReduction"],3850.0);
        assert!(next["objective"].as_str().unwrap().contains("contextChars"));
        assert!(next["residuals"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_nonmeta_first_adoption_learns_to_improve_the_improver_before_transfer() {
        let input=json!({"rsiExpandAfterGain":true,"rsiFrozenCases":[
            {"id":"search-quality","metrics":[{"key":"elapsedMs",
                "minRelativeGain":0.20,"minAbsoluteGain":1.0}]},
            {"id":"group-chat-input-cost","metrics":[{"key":"contextChars",
                "minRelativeGain":0.05,"minAbsoluteGain":50.0}]}
        ]});
        let history=vec![json!({"accepted":true,"acceptedCommit":"a".repeat(40),
            "evaluation":{"metrics":{"eligible":true,"metaHoldoutGains":0,
                "measurements":[
                    {"id":"search-quality","domain":"improver","metrics":[
                        {"key":"elapsedMs","direction":"lower",
                         "candidate":200.0,"improved":false}]},
                    {"id":"group-chat-input-cost","domain":"context","metrics":[
                        {"key":"contextChars","direction":"lower",
                         "candidate":77000.0,"improved":false}]}
                ]}}})];
        let target=post_adoption_objective(&input,&history).unwrap();
        assert_eq!(target["metaTransferTarget"],true);
        assert!(target["objective"].as_str().unwrap().contains("search-quality"));
        assert!(target["objective"].as_str().unwrap().contains("개선기"));
        let mut after=history.clone();
        after[0]["evaluation"]["metrics"]["metaHoldoutGains"]=json!(1);
        let transfer=post_adoption_objective(&input,&after).unwrap();
        assert_eq!(transfer["metaTransferTarget"],false);
        assert!(transfer["objective"].as_str().unwrap().contains("group-chat-input-cost"));
    }

    #[test]
    fn paired_behavioral_counterexamples_guide_descendants_without_forged_receipts(){
        let (state,root)=state();
        let a="a".repeat(64);
        let b="b".repeat(64);
        let mut candidate=json!({"id":"candidate-1","patchSha256":a,
            "cases":[{"id":"planning-development","holdout":false}],
            "pairedComparisonComplete":true,
            "evaluation":{"evidenceSha256":b}});
        state.store.put("rsi_candidate",&candidate).unwrap();
        let pair=json!({"id":"rsi-pair-candidate-1","candidateId":"candidate-1",
            "evidenceSha256":b,"report":{"patchSha256":a,
                "cases":[{"id":"planning-development",
                    "behavioralCounterexamples":{"candidate":[
                        {"input":"설정을 읽고 결과를 저장한 다음 검증해",
                         "expectedSegments":3,"observedSegments":2,
                         "steps":["설정을 읽고 결과를 저장한","검증해"]}
                    ]}}]}});
        state.store.put("rsi_pair",&pair).unwrap();
        let cases=paired_behavioral_feedback(&state,"candidate-1");
        assert_eq!(cases.len(),1);
        assert!(cases[0].contains("기대 단계=3"));
        assert!(cases[0].contains("후보 단계=2"));
        assert!(cases[0].contains("설정을 읽고"));
        candidate["cases"][0]["holdout"]=json!(true);
        state.store.put("rsi_candidate",&candidate).unwrap();
        assert!(paired_behavioral_feedback(&state,"candidate-1").is_empty(),
            "even authentic evaluator output must not expose a holdout answer");
        let mut stale=pair;
        stale["evidenceSha256"]=json!("0".repeat(64));
        state.store.put("rsi_pair",&stale).unwrap();
        assert!(paired_behavioral_feedback(&state,"candidate-1").is_empty(),
            "a different or stale paired report must never teach a descendant");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_budget_stops_are_not_no_progress_or_another_generation(){
        let worker=json!({"status":"completed","result":{"status":"needs_review","reason":"model_error",
            "rsi":{"modelError":"rsi_token_budget: remaining context cannot fit"}}});
        assert_eq!(generation_stop_reason(&worker,None),Some("token_budget"));
        assert_eq!(generation_stop_reason(&json!({}),Some("time_budget")),Some("time_budget"));
        assert_eq!(generation_stop_reason(&json!({"result":{"rsi":{"modelError":"authentication failed"}}}),None),None);
        assert_eq!(generation_stop_reason(&json!({"result":{"rsi":{"diffBytes":0}}}),None),None);
        let record=json!({"limits":{"tokenBudget":30000},"tokens":27446});
        assert!(!generation_fits_remaining_budget(&record,&json!({"prompt":"한".repeat(3000)})),"do not build or launch a worker when even its existing prompt cannot fit");
        assert!(generation_fits_remaining_budget(&json!({"limits":{}}),&json!({"prompt":"한".repeat(3000)})));
    }

    #[test]
    fn method_only_research_survives_worker_presave_and_is_novel_only_once(){
        let (state,root)=state();let input=json!({"rsiSourceDir":root,"rsiObjective":"memory lookup"});
        state.store.put("execution_operation",&json!({"id":"proposal-op","sessionId":"s","tool":"rsi_method_propose","status":"completed",
            "result":{"method":{"verification":{"type":"procedure","nodes":[{"id":"read","tool":"code_search","arguments":{"query":"memory"}}],"after":[{"kind":"tool_succeeded","tool":"code_search"}]}}}}));
        let artifacts=research_artifacts(&state,"s");assert_eq!(artifacts.len(),1);
        state.store.put("experience",&json!({"id":"current","partitionKey":research_scope(&input).partition_key,
            "status":"active","generationJobId":"current-worker","researchArtifacts":artifacts}));
        let hashes=artifacts.iter().map(|a|a["hash"].clone()).collect::<Vec<_>>();
        assert_eq!(new_research_findings(&state,&input,"current-worker",&hashes,&[]),1,"worker pre-save must not erase its own progress");
        assert!(!should_stop_no_progress(1, true, true, false, false));
        assert_eq!(new_research_findings(&state,&input,"next-worker",&hashes,&[]),0,"re-proposing the same program is not new research");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn capability_usage_is_read_from_real_operation_records() {
        let (state, root) = state();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op1","sessionId":"s1","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op2","sessionId":"s1","tool":"workspace_read_range","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op3","sessionId":"s2","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op4","sessionId":"s1","tool":"workspace_read","status":"failed"}),
            )
            .unwrap();
        let usage = session_tool_usage(&state, "s1");
        assert_eq!(usage.len(), 2, "{usage:?}");
        assert!(usage.iter().any(|row| row["tool"] == "code_search"));
        assert!(usage
            .iter()
            .any(|row| row["tool"] == "workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn research_digest_lists_ranges_already_read() {
        let (state, root) = state();
        state
            .store
            .put("job", &json!({"id":"digest-job","sessionId":"digest-session"}));
        state
            .store
            .put("execution", &json!({"id":"digest-run","sessionId":"digest-session"}));
        state.store.put(
            "execution_operation",
            &json!({"id":"digest-op","runId":"digest-run","tool":"workspace_read_range","status":"completed",
                "arguments":{"path":"src/engine/memory.rs","startLine":10,"endLine":20}}),
        );
        let record = json!({"history":[{"jobId":"digest-job"}]});
        let text = research_digest(&state, &json!({}), &record);
        assert!(text.contains("src/engine/memory.rs 10-20"));
        assert!(text.contains("다시 읽지 마세요"));
        assert_eq!(research_digest(&state, &json!({}), &json!({"history":[]})), "");
        // An arbitrary continuation label must NOT import sibling evidence.
        let head="a".repeat(40);
        let fingerprint="f".repeat(64);
        state.store.put("rsi_campaign",&json!({
            "id":"digest-prior","headCommit":head,"sourceDir":"source-a",
            "sourceBranch":"dominion-sub3","baseCommit":"base",
            "sourceFingerprint":fingerprint,
            "history":[{"jobId":"digest-job","generatorCommit":head}]
        }));
        let requested=json!({"rsiContinueFrom":"digest-prior"});
        assert_eq!(research_digest(&state,&requested,&json!({"history":[]})),"",
            "goal matching alone must never import another campaign's reads");
        let verified=json!({
            "sourceDir":"source-a","sourceBranch":"dominion-sub3",
            "baseCommit":"base","sourceFingerprint":fingerprint,
            "headCommit":head,
            "lineage":{"reused":true,"selectedCommit":head},
            "history":[]
        });
        let continued=research_digest(&state,&requested,&verified);
        assert!(continued.contains("src/engine/memory.rs 10-20"));
        let mut updated=verified;
        updated["headCommit"]=json!("b".repeat(40));
        assert_eq!(research_digest(&state,&requested,&updated),"",
            "source offsets from earlier revisions must be re-read");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn descendant_replans_its_own_source_and_drops_stale_parent_excerpt() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/memory.rs"),
            "fn memory_static() { println!(\"unrelated\"); }\n").unwrap();
        std::fs::write(root.join("src/context.rs"),
            "fn context_worker() { println!(\"revised\"); }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let mut job=json!({"input":{
            "rsiSourceDir":root,"rsiKnowledgeSource":root,
            "rsiIndexId":index["id"],"rsiAutoFocus":true,
            "rsiTargetDomain":"memory",
            "rsiFocusFiles":["src/memory.rs"],
            "rsiTargetPlan":{"domain":"memory","selection":"algorithm_active",
                "strategy":{"implementation":"failure-clusters"},"generatorCommit":"before"},
            "rsiObjective":"","rsiPromptBase":"OWNER BASE",
            "rsiPromptEvidence":"\nVERIFIED PRIOR EVIDENCE",
            "rsiPromptResearch":"\nRESEARCH RECEIPT",
            "rsiRemainingCampaignTokens":5000,
            "model":"test-model","providerId":"test-provider",
            "rsiDecisionInput":{"nextDomain":"memory"}
        }});
        let revised=json!({"implementation":"failure-clusters"});
        assert!(reconcile_descendant_target(&state,&mut job,&[],"context",
            "algorithm_active",&revised,"after").unwrap());
        assert_eq!(job["input"]["rsiTargetDomain"],"context");
        assert_eq!(job["input"]["rsiFocusFiles"][0],"src/context.rs");
        assert_eq!(job["input"]["rsiFocusSelection"]["source"],"descendant_indexed_evidence");
        let new_prompt=job["input"]["prompt"].as_str().unwrap();
        assert!(new_prompt.starts_with("OWNER BASE"));
        assert!(new_prompt.contains("VERIFIED PRIOR EVIDENCE"));
        assert!(new_prompt.contains("context_worker"));
        assert!(!new_prompt.contains("memory_static"),
            "a wrong parent-selected source excerpt must not survive child replanning");
        assert_eq!(job["input"]["rsiDecisionInput"]["nextDomain"],"context");
        assert_eq!(job["rsiTargetReplan"]["reason"],"descendant_authoritative_selection");
        assert!(!reconcile_descendant_target(&state,&mut job,&[],"context",
            "algorithm_active",&revised,"after").unwrap(),
            "unchanged descendant plans must not reselect sources or append prompt text");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_success_becomes_revision_scoped_target_learning_not_a_patch_counter() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let beta=root.join("src/beta.rs");
        let original="fn search_dispatch() { println!(\"search\"); }\n";
        let improved="fn search_dispatch() { println!(\"search improved\"); }\n";
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),original).unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/alpha.rs");
        let selected_beta=crate::engine::code_index::rank_focus_files(
            &state,index["id"].as_str().unwrap(),"search dispatch","tools",
            &BTreeMap::new(),None,2).into_iter()
            .find(|v|v["path"]=="src/beta.rs").unwrap();
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let accepted_hash=crate::engine::code_index::indexed_source_hash(
            &state,index["id"].as_str().unwrap(),"src/beta.rs").unwrap();
        assert_ne!(selected_beta["sourceHash"],accepted_hash);
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"verified",
            "paths":["src/beta.rs"],
            "evaluation":{"metrics":{"eligible":true}}
        })).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"prior-success","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":true,"candidateId":"verified-patch","newFindings":0,
            "focusFiles":["src/beta.rs"],"focusSelection":{"candidates":[selected_beta.clone()]},
            "acceptedSourceHashes":{"src/beta.rs":accepted_hash},
            "createdAt":"2026-11-10T00:00:00Z"
        })).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/beta.rs");
        assert_eq!(learned[0]["evidence"]["priorProductive"],1.0);
        std::fs::write(&beta,original).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let reverted=choose_focus_files(&state,&input,&[],None);
        assert_eq!(reverted[0]["path"],"src/alpha.rs",
            "reverting to the pre-patch version must not earn a success reward");
        assert_eq!(reverted.iter().find(|row|row["path"]=="src/beta.rs").unwrap()
            ["evidence"]["priorProductive"],0.0);
        std::fs::write(&beta,"fn search_dispatch() { println!(\"new revision\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let stale=choose_focus_files(&state,&input,&[],None);
        assert_eq!(stale[0]["evidence"]["priorProductive"],0.0);
        // A falsely declared acceptance must not contribute feedback.
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"rejected",
            "paths":["src/beta.rs"],"evaluation":{"metrics":{"eligible":false}}
        })).unwrap();
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let unverified=choose_focus_files(&state,&input,&[],None);
        assert_eq!(unverified[0]["path"],"src/alpha.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cross_campaign_target_failures_are_reused_only_at_same_source_revision() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let first=root.join("src/search_first.rs");
        let second=root.join("src/search_second.rs");
        std::fs::write(&first,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        std::fs::write(&second,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/search_first.rs");
        let prior=json!({"id":"persisted-prior","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":false,"candidateId":null,"newFindings":0,
            "focusFiles":["src/search_first.rs"],
            "focusSelection":{"candidates":initial},
            "createdAt":"2026-11-10T00:00:00Z"});
        state.store.put("rsi_generation",&prior).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/search_second.rs",
            "the later campaign should not re-spend its tokens on the same unproductive target");
        // Same-length edit is a different revision, so old failure evidence
        // can no longer drive the new source choice.
        std::fs::write(&first,"fn search_dispatch() { /* scan bravo */ }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let refreshed=choose_focus_files(&state,&input,&[],None);
        assert_eq!(refreshed[0]["path"],"src/search_first.rs",
            "stale failure evidence must not penalize changed code");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn fallback_source_selector_cannot_falsely_promote_a_generated_target_formula() {
        let (state,root)=state();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"invalid-runtime-target",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration",
                "targetExpression":"1 / pathEvidence"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        let generation=json!({"jobId":"actual-trial","attempt":2,
            "domainSelection":"algorithm_trial","domain":"memory",
            "focusSelection":{"source":"indexed_evidence",
                "policyRevision":candidate["revision"],
                "candidates":[{"scoring":"corpus-evidence",
                    "policyError":"division_by_zero"}]},
            "accepted":true,"tokens":1000,"elapsedMs":100,
            "evaluation":{"verdict":"improved"}});
        let outcome=record_trial_outcome(&state,id,&generation,&[]);
        assert_eq!(outcome["reason"],"target_formula_not_executed");
        let saved=state.store.get("algorithm",id).unwrap();
        assert_eq!(saved["rsiTrial"]["status"],"trial_inconclusive");
        assert_ne!(saved["status"],"active");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn indexed_focus_finds_opaque_sources_and_model_can_explore_new_targets() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/opaque.rs"),
            "fn dispatch() { let x = \"bm25\"; println!(\"{x}\"); }\n").unwrap();
        std::fs::write(root.join("src/engine/neutral.rs"),
            "fn unrelated() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiTargetDomain":"memory",
            "rsiObjective":"bm25","rsiKnowledgeSource":root});
        let autonomous=json!({"rsiAutonomous":true,"rsiTargetDomain":"tools",
            "rsiObjective":"knowledge experience generic default"});
        assert_eq!(target_focus_objective(&autonomous),"tools");
        assert_eq!(target_focus_objective(&input),"bm25");
        let ranked=choose_focus_files(&state,&input,&[],None);
        assert_eq!(ranked[0]["path"],"src/engine/opaque.rs");
        let explorer=json!({"revision":"new-source-space",
            "parameters":{"targetExpression":"novelty - sourceCost"}});
        let unknown=json!({"rsiIndexId":index["id"],"rsiTargetDomain":"",
            "rsiObjective":"uniqueunmatchedobjective","rsiKnowledgeSource":root});
        assert!(choose_focus_files(&state,&unknown,&[],None).is_empty());
        assert_eq!(choose_focus_files(&state,&unknown,&[],Some(&explorer)).len(),2,
            "a model-authored policy can search beyond fixed filenames and keywords");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn focus_file_context_inlines_the_target() {
        let (state, root) = state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/x.rs"), "fn a() {}\n").unwrap();
        let meta = crate::engine::code_index::refresh(&state, &root).unwrap();
        let text = focus_file_context(&state, &json!({"labDir":root}), &json!({"rsiFocusFiles":["src/engine/x.rs"],"rsiIndexId":meta["id"],"rsiObjective":"a"}));
        assert!(text.contains("fn a() {}"));
        assert!(text.contains("대상 파일 전체"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generation_context_reuses_implementation_evidence_not_tool_counters() {
        let (state, root) = state();
        let input = json!({"rsiObjective":"코드 탐색 능력을 다음 세대에 계승"});
        state.store.put("rsi_candidate",&json!({"id":"candidate","hypothesis":"검증된 검색 전략과 적용 조건","paths":["src/engine/memory.rs"]}));
        let record = json!({"history":[{
            "attempt":1,"accepted":true,"acceptedCommit":"accepted-source-revision","candidateId":"candidate",
            "toolsUsed":[{"tool":"code_search","calls":2},{"tool":"workspace_read_range","calls":1}]
        }]});
        let text = generation_context(&state, &input, &record);
        assert!(text.contains("code_search"), "{text}");
        assert!(text.contains("workspace_read_range"), "{text}");
        assert!(text.contains("1세대"), "{text}");
        assert!(text.contains("검증된 검색 전략과 적용 조건"), "{text}");
        assert!(text.contains("src/engine/memory.rs"), "{text}");
        assert!(text.contains("accepted-source-revision"), "{text}");
        assert!(!text.contains("\"calls\""),"tool frequency is not an inherited implementation: {text}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn next_generation_inherits_unchanged_holdout_quality_and_failed_edit_evidence() {
        let (state,root)=state();
        let input=json!({"rsiObjective":"plan better","rsiTargetDomain":"planning"});
        let record=json!({"history":[{
            "attempt":2,"domain":"planning","accepted":false,"candidateId":"candidate-failed",
            "evaluation":{"verdict":"inconclusive","metrics":{"measurements":[
                {"id":"planning-quality","domain":"planning","holdout":true,
                 "metrics":[{"key":"quality","baseline":0.75,"candidate":0.75,
                    "improved":false,"regressed":false}]}
            ]}},
            "editEvidence":{"attempts":3,"verifiedWrites":1,
                "recentFailures":["workspace_replace_range: unchanged"]},
            "error":"no quality gain"
        }]});
        let text=generation_context(&state,&input,&record);
        assert!(text.contains("planning-quality"),"{text}");
        assert!(text.contains("0.75"),"{text}");
        assert!(text.contains("편집 시도 3회"),"{text}");
        assert!(text.contains("workspace_replace_range: unchanged"),"{text}");
        assert!(!text.contains("개선 확인"),"{text}");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn edit_method_selection_learns_from_real_executions_not_model_prose() {
        let baseline=synthesize_edit_method(&[]);
        assert_eq!(baseline["method"],"balanced");
        let actual=json!({"jobId":"j1","tokens":50000,
            "editPolicy":{"method":"balanced"},"candidateId":null,
            "editEvidence":{"verifiedWrites":0},"accepted":false});
        let next=synthesize_edit_method(&[actual.clone()]);
        assert_ne!(next["method"],"balanced",
            "an unproductive measured attempt must explore a different executable method");
        let fake=json!({"jobId":"j2","tokens":0,
            "editPolicy":{"method":"focused-range"},"accepted":true,
            "evaluation":{"metrics":{"eligible":true}}});
        assert_eq!(synthesize_edit_method(&[actual,fake])["method"],next["method"],
            "unexecuted success claims cannot update the policy");
    }

    #[test]
    fn repeated_verified_success_can_outweigh_exploration_of_failed_methods() {
        let mut generations=Vec::new();
        for (method,checkpoint,tool,reward) in [
            ("balanced",3,"workspace_replace_range",true),
            ("generated-early",2,"workspace_replace_range",false),
            ("generated-exact",4,"workspace_replace",false)
        ] {
            for i in 0..8 {
                generations.push(json!({
                    "jobId":format!("{method}-{i}"),"tokens":10000,
                    "editPolicy":{"method":method,"checkpointAfter":checkpoint,
                        "preferredTool":tool,"repairLimit":2},
                    "accepted":reward,"evidenceVerified":reward,
                    "candidateId":if reward {json!(format!("candidate-{i}"))} else {Value::Null},
                    "evaluation":{"metrics":{"eligible":reward}}
                }));
            }
        }
        assert_eq!(synthesize_edit_method(&generations)["method"],"balanced",
            "verified improvement must preserve the winning composed controller");
    }

    #[test]
    fn edit_method_reward_requires_measured_quality_for_full_credit() {
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":true,
            "candidateOrigin":"native_inductive_source_mutation","tokens":9000,
            "evaluation":{"metrics":{"eligible":true}}})),0.0,
            "native mutations cannot credit an unused model editor, even after model calls");
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}}})),1.0);
        assert_eq!(edit_method_reward(&json!({"accepted":true,"evidenceVerified":false,
            "evaluation":{"metrics":{"eligible":true}}})),0.0,
            "unverified summaries cannot earn a successful improvement reward");
        assert_eq!(edit_method_reward(&json!({"candidateId":"c",
            "evaluation":{"verdict":"inconclusive","metrics":{"improvedHoldouts":0}}})),0.0);
        assert_eq!(edit_method_reward(&json!({"candidateId":"c",
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "verdict":"inconclusive","metrics":{"improvedHoldouts":0}}})),0.10);
        assert_eq!(edit_method_reward(&json!({"accepted":false,"candidateId":null})),0.0);
        let invented=json!({"accepted":true,"evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"rev-1","rules":[{"id":"rule-1"}]}});
        assert_eq!(edit_method_reward(&invented),0.0,
            "a policy that did not execute cannot claim the improvement");
        let mut real=invented;
        real["ruleExecutions"]=json!([{"ruleId":"rule-1","policyRevision":"rev-1",
            "runId":"run-g1","toolCalls":1,"opSeqBefore":1,
            "allow":["workspace_replace_range"]}]);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule receipt without downstream tool execution is not causal evidence");
        real["toolTimeline"]=json!([
            {"runId":"run-g1","opSeq":1,"tool":"workspace_read_range","status":"completed"},
            {"runId":"run-g1","opSeq":2,"tool":"workspace_replace_range","status":"completed",
                "path":"src/engine/test.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}
        ]);
        real["acceptedSourceHashes"]=json!({"src/engine/test.rs":"a".repeat(64)});
        assert_eq!(edit_method_reward(&real),1.0);
        real["ruleExecutions"][0]["opSeqBefore"]=json!(3);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule activated after the patch must never inherit its reward");
        real["ruleExecutions"][0]["opSeqBefore"]=json!(1);
        real["ruleExecutions"][0]["allow"]=json!(["code_search"]);
        assert_eq!(edit_method_reward(&real),0.0,
            "a rule whose advertised action did not run is uncredited");
    }

    #[test]
    fn proof_goal_does_not_stop_on_unverified_streak_counters() {
        let (state,root)=state();
        assert!(!proof_goal_reached(&state,&json!({"rsiStopAfterProof":true}),
            &json!({"id":"forged","maxAcceptedStreak":3,"capabilityDemonstrated":true,"history":[]})));
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_verified_patch_is_a_candidate_even_when_the_model_hit_the_budget() {
        let result = json!({
            "status": "needs_review",
            "reason": "model_error",
            "rsi": {
                "candidateId": "rsi-test",
                "checksPassed": true,
                "modelError": "토큰 예산(250000)을 초과했습니다."
            }
        });
        assert_eq!(verified_candidate_id(&result), Some("rsi-test"));
        assert_eq!(
            verified_candidate_id(&json!({"rsi":{"candidateId":"rsi-test","checksPassed":false}})),
            None
        );
    }

    #[test]
    fn research_continues_for_new_findings_not_for_new_query_spellings() {
        assert!(!should_stop_no_progress(1, true, true, false, false));
        assert!(should_stop_no_progress(1, true, false, false, false));
        assert!(should_stop_no_progress(1, false, true, false, false));
        assert!(!should_stop_no_progress(2, true, true, false, false));
        assert!(!should_stop_no_progress(3, true, true, false, false));
        assert!(should_stop_no_progress(3, true, false, false, false));
    }

    #[test]
    fn truncated_research_rehydrates_its_complete_verified_range_without_reading_the_file_into_the_prompt(){
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let mut source=(0..80).map(|i|format!("// source line {i} with enough content to exceed a short excerpt\n")).collect::<String>();
        source.push_str("fn COMPLETE_RANGE_TAIL() {}\n");
        source.push_str("fn OUTSIDE_REQUESTED_RANGE() {}\n");
        std::fs::write(root.join("src/reuse.rs"),&source).unwrap();
        let input=json!({"rsiSourceDir":root,"rsiObjective":"memory complete source"});
        state.store.put("session",&json!({"id":"next"}));
        state.store.put("experience",&json!({"id":"bounded-research","scope":"shared","status":"active",
            "partitionKey":research_scope(&input).partition_key,"request":"memory complete source","signature":"memory complete source",
            "lesson":"memory complete source","outcome":"research","confidence":0.9,"researchArtifacts":[{
                "tool":"workspace_read_range","hash":"artifact","path":"src/reuse.rs","startLine":1,"endLine":81,
                "sourceSha256":format!("{:x}",Sha256::digest(source.as_bytes())),"truncated":true,
                "content":source.chars().take(2200).collect::<String>()}]}));
        let context=reuse_research(&state,&input,&root,"next");
        assert!(context.contains("COMPLETE_RANGE_TAIL"));assert!(!context.contains("OUTSIDE_REQUESTED_RANGE"));
        let receipt=&state.store.get("session","next").unwrap()["rsiKnowledgeReuse"][0];
        assert_eq!(receipt["rangeHydrated"],true);assert_eq!(receipt["mode"],"hash_verified_source_cache");
        assert_eq!(state.store.get("experience","bounded-research").unwrap()["researchArtifacts"][0]["content"].as_str().unwrap().chars().count(),2200);
        let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn research_survives_restart_and_executes_hash_checked_reuse_in_the_next_session() {
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let source="pub fn remember() { /* REUSABLE_RESEARCH_SOURCE */ }\n";
        std::fs::write(root.join("src/reuse.rs"),source).unwrap();
        state.store.put("session",&json!({"id":"research-1","rsiRun":true}));
        let args=json!({"path":"src/reuse.rs","startLine":1,"endLine":1});
        let ctx=tools::ToolContext{state:&state,store:&state.store,workspace:&root,session_id:"research-1",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let observed=tools::execute(&ctx,"workspace_read_range",&args,"run-1").await.unwrap();
        let old_hash=observed["sourceSha256"].clone();
        state.store.put("execution_operation",&json!({"id":"source-observation","sessionId":"research-1","tool":"workspace_read_range","arguments":args,"result":observed,"status":"completed"}));
        let input=json!({"rsiSourceDir":root,"rsiObjective":"장기 기억 검색 개선"});
        let first=json!({"jobId":"generation-1","accepted":false,"domain":"memory"});
        let id=remember_research(&state,&input,&first,"research-1").unwrap();
        assert_eq!(state.store.get("experience",&id).unwrap()["outcome"],"research");
        let cfg=state.config.clone();drop(state);
        let state=AppState::new(cfg.clone(),crate::db::Store::open(&cfg.db_path()).unwrap(),crate::vault::Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"research-2","rsiRun":true}));
        let context=reuse_research(&state,&input,&root,"research-2");
        assert!(context.contains("REUSABLE_RESEARCH_SOURCE"));
        let receipt=state.store.get("session","research-2").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(receipt["experienceId"],id);assert_eq!(receipt["sourceOperationId"],"source-observation");
        assert_eq!(receipt["mode"],"hash_verified_source_cache");
        assert_eq!(receipt["sourceValidity"],"hash_verified_current_source");
        assert_eq!(receipt["hypothesisValidity"],"prior_hypothesis_unverified");
        assert_eq!(receipt["entrypoint"],"rsi_evolve::reuse_research");
        // A real next-session mutation is linked to its source provenance.
        // Persisted research from generation 1 becomes a line-scoped, model-sized
        // source edit in generation 2. The old source is not retransmitted.
        let change=json!({"path":"src/reuse.rs","startLine":1,"endLine":1,
            "newText":"pub fn remember() { /* IMPROVED_RESEARCH_SOURCE */ }",
            "expectedSha256":old_hash.clone()});
        let ctx=tools::ToolContext{state:&state,store:&state.store,workspace:&root,session_id:"research-2",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let written=tools::execute(&ctx,"workspace_replace_range",&change,"run-2").await.unwrap();
        assert_eq!(written["replaced"],true,"{written}");
        assert_eq!(std::fs::read_to_string(root.join("src/reuse.rs")).unwrap(),
            "pub fn remember() { /* IMPROVED_RESEARCH_SOURCE */ }\n");
        state.store.put("execution_operation",&json!({"id":"real-next-change","sessionId":"research-2","tool":"workspace_replace_range","arguments":change,"result":written,"status":"completed"}));
        state.store.put("rsi_candidate",&json!({"id":"c","status":"verified","hypothesis":"Apply the observed implementation","paths":["src/reuse.rs"],"evaluation":{"metrics":{"eligible":true}}}));
        let second=json!({"jobId":"generation-2","accepted":true,"candidateId":"c","acceptedCommit":"a".repeat(40),"domain":"memory"});
        let next_id=remember_research(&state,&input,&second,"research-2").unwrap();
        let saved=state.store.get("experience",&next_id).unwrap();
        assert_eq!(saved["implementation_ref"]["commit"],"a".repeat(40));
        assert_eq!(saved["knowledgeApplications"][0]["actionOperationIds"][0],"real-next-change");
        assert_eq!(saved["knowledgeApplications"][0]["status"],"related_verified_change");
        assert_eq!(saved["laterGain"],"unmeasured","related actions alone cannot prove a causal gain");
        state.store.put("session",&json!({"id":"research-3"}));
        let refreshed=reuse_research(&state,&input,&root,"research-3");
        assert!(refreshed.contains("IMPROVED_RESEARCH_SOURCE"),
            "a changed source range must be refreshed, not discarded: {refreshed}");
        let refresh_receipt=state.store.get("session","research-3").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(refresh_receipt["mode"],"refreshed_source_cache");
        assert_eq!(refresh_receipt["sourceValidity"],"refreshed_range_current_source");
        assert_eq!(refresh_receipt["hypothesisValidity"],"unverified_after_source_change");
        assert_eq!(refresh_receipt["experienceId"],id);
        assert_eq!(refresh_receipt["refreshed"],true);
        let refreshed_record=state.store.get("experience",&id).unwrap();
        assert_eq!(refreshed_record["researchArtifacts"][0]["refreshKind"],"line_range_revalidation");
        assert_eq!(refreshed_record["researchArtifacts"][0]["previousSourceSha256"],old_hash);
        assert_ne!(refreshed_record["researchArtifacts"][0]["sourceSha256"],old_hash);
        let foreign=json!({"rsiSourceDir":"/different/project","rsiObjective":"장기 기억 검색 개선"});
        assert!(reuse_research(&state,&foreign,&root,"research-3").is_empty());
        drop(state);let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_source_keeps_research_record_and_reports_unavailable_range() {
        let (state, root) = state();
        let workspace = root.join("work");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        let file = workspace.join("src/reuse.rs");
        std::fs::write(&file, "old source\n").unwrap();
        let old_hash = format!("{:x}", Sha256::digest(std::fs::read(&file).unwrap()));
        let input = json!({"rsiSourceDir":workspace,"rsiObjective":"memory research refresh"});
        let scope = research_scope(&input);
        state.store.put("experience", &json!({
            "id":"experience-missing","scope":"shared","partitionKey":scope.partition_key,
            "signature":"memory research refresh","request":"memory research refresh",
            "lesson":"test range refresh","outcome":"research","status":"active","occurrences":1,
            "researchArtifacts":[{
                "tool":"workspace_read_range","path":"src/reuse.rs","startLine":1,"endLine":1,
                "sourceSha256":old_hash,"content":"    1 old source\n","hash":"artifact-missing",
                "operationId":"op-1"
            }]
        })).unwrap();
        state.store.put("session", &json!({"id":"s-missing"})).unwrap();
        std::fs::remove_file(&file).unwrap();
        let prompt = reuse_research(&state, &input, &workspace, "s-missing");
        assert!(prompt.is_empty(), "missing source must not be invented: {prompt}");
        let receipt = state.store.get("session", "s-missing").unwrap()["rsiKnowledgeReuse"][0].clone();
        assert_eq!(receipt["mode"], "source_range_unavailable");
        assert_eq!(receipt["reason"], "source_missing");
        assert_eq!(receipt["sourceValidity"], "source_unavailable");
        assert_eq!(receipt["hypothesisValidity"], "prior_hypothesis_unverified");
        assert!(
            state.store.get("experience", "experience-missing").is_some(),
            "the research record survives an unavailable source"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_tools_and_unfinished_plans_survive_and_reuse_the_source_hash() {
        let (state,root)=state();std::fs::create_dir_all(root.join("src")).unwrap();
        let source="fn reusable() {}\n";let hash=format!("{:x}",Sha256::digest(source.as_bytes()));
        std::fs::write(root.join("src/reuse.rs"),source).unwrap();
        state.store.put("session",&json!({"id":"before"}));
        state.store.put("session",&json!({"id":"after"}));
        state.store.write_message(&json!({"session_id":"before","role":"assistant","content":"",
            "reasoning":"UNFINISHED_IMPLEMENTATION_PLAN ".repeat(10)})).unwrap();
        state.store.put("execution_operation",&json!({"id":"read","sessionId":"before","opSeq":1,
            "tool":"workspace_read_range","status":"completed","arguments":{"path":"src/reuse.rs"},
            "result":{"content":source,"sourceSha256":hash,"startLine":1,"endLine":1}}));
        state.store.put("execution_operation",&json!({"id":"failed","sessionId":"before","opSeq":2,
            "tool":"workspace_replace_range","status":"failed","arguments":{"path":"src/reuse.rs"},
            "result":{"error":"STALE_SOURCE_EDIT_REJECTED"}}));
        let input=json!({"rsiSourceDir":root,"rsiObjective":"memory reuse","rsiFocusFiles":["src/reuse.rs"],"rsiSystemEvolution":true});
        let id=remember_research(&state,&input,&json!({"jobId":"failed-job","domain":"memory","accepted":false,"error":"tool limit"}),"before").unwrap();
        let saved=state.store.get("experience",&id).unwrap();
        assert_eq!(saved["evidence"]["accepted"],false);
        assert_eq!(saved["evidence"]["failure"],"tool limit");
        let prompt=reuse_research(&state,&input,&root,"after");
        assert!(prompt.contains("UNFINISHED_IMPLEMENTATION_PLAN"));
        assert!(prompt.contains("STALE_SOURCE_EDIT_REJECTED"));
        assert!(prompt.contains(&hash),"source cache must deliver its hash for a grounded edit");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn empty_searches_and_changed_queries_do_not_manufacture_findings() {
        let (state,root)=state();
        state.store.put("execution_operation",&json!({"id":"empty","sessionId":"s","tool":"code_search","arguments":{"query":"anything"},"result":{"matchCount":0,"matches":[]},"status":"completed"}));
        assert!(research_artifacts(&state,"s").is_empty());
        let result=json!({"matchCount":1,"matches":[{"path":"src/reuse.rs","name":"remember","line":1}]});
        for (id,query) in [("found-1","remember"),("found-2","fn remember")] {
            state.store.put("execution_operation",&json!({"id":id,"sessionId":"s","tool":"code_search","arguments":{"query":query},"result":result,"status":"completed"}));
        }
        assert_eq!(research_artifacts(&state,"s").len(),1);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn resource_limits_reject_unbounded_or_malformed_requests() {
        assert!(limits(&json!({})).unwrap()["tokenBudget"].is_null());
        assert!(limits(&json!({})).unwrap()["timeBudgetSeconds"].is_null());
        assert!(limits(&json!({"tokenBudget":0,"timeBudgetSeconds":0})).unwrap()["tokenBudget"].is_null());
        for body in [
            json!({"maxAttempts":0}),
            json!({"timeBudgetSeconds":999999}),
            json!({"tokenBudget":"unlimited"}),
            json!({"tokenBudget":999}),
        ] {
            assert!(limits(&body).is_err());
        }
    }
    #[tokio::test]
    async fn manual_selection_waits_for_owner_and_is_revision_bound() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record = json!({"id":"campaign","status":"running","selectionPolicy":"manual","waitingMs":0,
            "comparison":{"attempt":2,"evaluationId":"evaluation-2","canSelectCandidate":true}});
        let owner = async {
            loop {
                if state
                    .store
                    .get("rsi_campaign", "campaign")
                    .is_some_and(|r| r["phase"] == "waiting_for_choice")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":1})
            )
            .is_err());
            let chosen = choose(
                &state,
                "campaign",
                &json!({"choice":"candidate","attempt":2}),
            )
            .unwrap();
            assert_eq!(
                choose(
                    &state,
                    "campaign",
                    &json!({"choice":"candidate","attempt":2})
                )
                .unwrap(),
                chosen
            );
            assert!(choose(
                &state,
                "campaign",
                &json!({"choice":"baseline","attempt":2})
            )
            .is_err());
        };
        let (selected, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(select(&context, &mut record), owner)
        })
        .await
        .unwrap();
        assert_eq!(selected.unwrap(), "candidate");
        assert_eq!(record["comparison"]["selected"], "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[tokio::test]
    async fn automatic_choice_keeps_the_baseline_when_superiority_is_unproven() {
        let (state, root) = state();
        let context = parent(&state);
        let mut record =
            json!({"selectionPolicy":"automatic","comparison":{"canSelectCandidate":false}});
        assert_eq!(select(&context, &mut record).await.unwrap(), "baseline");
        record["comparison"]["canSelectCandidate"] = json!(true);
        assert_eq!(select(&context, &mut record).await.unwrap(), "candidate");
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn automatic_write_permission_is_scoped_to_the_active_private_worktree() {
        let (state, root) = state();
        let _context = parent(&state);
        state.store.put(
            "job",
            &json!({"id":"generation","kind":"rsi_generation","status":"running",
            "input":{"rsiCampaignId":"campaign"},"rsiWorkspace":state.config.workspace}),
        );
        state.store.put("session",&json!({"id":"session","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"generation"}));
        let args = json!({"path":"src/engine/algorithm.rs"});
        assert!(auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        assert!(auto_write_allowed(
            &state,
            "session",
            "workspace_replace_range",
            &args
        ), "RSI line edit must not pause at the owner approval gate");
        assert!(!auto_write_allowed(&state, "session", "shell_exec", &args));
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &json!({"path":".git/config"})
        ));
        let mut outside = state.clone();
        outside.config.workspace = root.join("outside");
        assert!(!auto_write_allowed(
            &outside,
            "session",
            "workspace_write",
            &args
        ));
        state.store.patch_if(
            "job",
            "campaign",
            "status",
            &json!("running"),
            &json!({"cancelRequested":true}),
        );
        assert!(!auto_write_allowed(
            &state,
            "session",
            "workspace_write",
            &args
        ));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn restart_never_blindly_replays_a_partial_evolution() {
        let (state, root) = state();
        let context = parent(&state);
        context
            .checkpoint(json!({"rsiEvolutionInFlight":true}))
            .unwrap();
        jobs::recover(&state);
        assert_eq!(
            state.store.get("job", "campaign").unwrap()["status"],
            "needs_review"
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn exhausted_usage_prevents_a_new_model_call_and_caps_the_remaining_output() {
        let (state, root) = state();
        let context = parent(&state);
        context.checkpoint(json!({"input":{"limits":{"tokenBudget":2000}},"evolution":{"activeBaseTokens":500}})).unwrap();
        state
            .store
            .put("session", &json!({"id":"s","rsiCampaignId":"campaign"}));
        assert_eq!(
            remaining_output_tokens(&state, "s", 1000).unwrap(),
            Some(500)
        );
        assert!(remaining_output_tokens(&state, "s", 1400).is_err());
        assert_eq!(
            remaining_output_tokens(&state, "ordinary", 99999).unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn downstream_episodes_share_the_campaign_budget_and_keep_their_own_cap() {
        let (state,root)=state();let context=parent(&state);
        context.checkpoint(json!({"input":{"limits":{"tokenBudget":2000}}})).unwrap();
        for (id,tokens) in [("a",1100),("b",300)] {
            state.store.put("session",&json!({"id":id,"rsiCampaignId":"campaign"}));
            state.store.put("job",&json!({"id":id,"kind":"rsi_generation","sessionId":id,
                "input":{"rsiCampaignId":"campaign","rsiMetaEpisode":true}}));
            state.store.audit("model.usage",json!({"sessionId":id,"totalTokens":tokens}));
        }
        assert_eq!(remaining_output_tokens(&state,"a",100).unwrap(),Some(500));
        state.store.put("session",&json!({"id":"b","rsiCampaignId":"campaign","rsiEpisodeTokenLimit":500}));
        assert!(remaining_output_tokens(&state,"b",100).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn native_paired_runner_finds_a_quality_gain_when_every_guard_already_passes() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(
            source.join("Cargo.toml"),
            "[package]\nname=\"rsi-test-subject\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        let before = "pub fn accepts(n:i32)->bool { n >= 0 }\nfn main(){}\n";
        let after = "pub fn accepts(n:i32)->bool { n > 0 }\nfn main(){}\n";
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "baseline",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), after).unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&source).args(["diff", "HEAD"]);
        let patch = command(&context, cmd, Duration::from_secs(5))
            .await
            .unwrap()
            .stdout;
        std::fs::write(source.join("src/main.rs"), before).unwrap();
        let fixtures = root.join("frozen");
        std::fs::create_dir_all(&fixtures).unwrap();
        let oracle = fixtures.join("oracle.rs");
        let mut code = String::new();
        for (index, value) in [-4, -3, -2, -1, 1, 2, 3].into_iter().enumerate() {
            code.push_str(&format!(
                "#[test] fn case_{index}(){{assert_eq!(crate::accepts({value}),{value}>0);}}\n"
            ));
        }
        code.push_str("#[test] fn case_7(){let quality=(-4..=4).filter(|n|crate::accepts(*n)==(*n>0)).count() as f64/9.0;println!(\"DOMINION_RSI_METRIC {{\\\"quality\\\":{quality}}}\");}\n");
        std::fs::write(&oracle, &code).unwrap();
        let cases=(0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i==7{"quality"}else{"guard"},"holdout":i==0||i==7,
            "filter":format!("rsi_frozen_oracle::case_{i}"),"metrics":if i==7{json!([{"key":"quality","direction":"higher","minRelativeGain":0.05}])}else{json!([])}})).collect::<Vec<_>>();
        let plan = json!({"oracleVersion":"independent-boundary-v1","cases":cases,"oracleFixture":oracle,
            "oracleSha256":format!("{:x}",Sha256::digest(code.as_bytes()))});
        let patch_file = fixtures.join("candidate.patch");
        std::fs::write(&patch_file, &patch).unwrap();
        let plan_file = fixtures.join("plan.json");
        std::fs::write(&plan_file, plan.to_string()).unwrap();
        std::fs::write(
            fixtures.join("rsi-lab.mjs"),
            include_str!("../../scripts/rsi-lab.mjs"),
        )
        .unwrap();
        std::fs::write(
            fixtures.join("rsi-measure.mjs"),
            include_str!("../../scripts/rsi-measure.mjs"),
        )
        .unwrap();
        let report_file = fixtures.join("report.json");
        let mut cmd = Command::new(rsi_worktree::node_executable());
        cmd.current_dir(&source)
            .arg(fixtures.join("rsi-lab.mjs"))
            .arg(&base)
            .arg(&patch_file)
            .arg(&plan_file)
            .arg(&report_file);
        let output = command(&context, cmd, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_str(&std::fs::read_to_string(&report_file).unwrap()).unwrap();
        assert_eq!(report["baselineBuildOk"], true);
        assert_eq!(report["candidateBuildOk"], true);
        assert!(
            report["cases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["baselinePass"] == true && r["candidatePass"] == true),
            "{report}"
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["candidate"], 1.0);
        assert!(
            report["cases"][7]["metrics"]["quality"]["baseline"]
                .as_f64()
                .unwrap()
                < 1.0
        );
        assert_eq!(report["cases"][7]["metrics"]["quality"]["samples"], 6);
        assert_eq!(std::fs::read_to_string(&oracle).unwrap(), code);
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn snapshot_and_adoption_preserve_the_users_checkout_and_commit() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/main.rs"), "base\n").unwrap();
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "base",
            ],
        )
        .await
        .unwrap();
        let original = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        std::fs::write(source.join("src/main.rs"), "owner edit\n").unwrap();
        std::fs::write(source.join("src/new.rs"), "new code\n").unwrap();
        std::fs::write(source.join("notes.txt"), "owner research\n").unwrap();
        std::fs::create_dir_all(source.join("scripts/research")).unwrap();
        std::fs::write(
            source.join("scripts/research/probe.py"),
            "owner research script\n",
        )
        .unwrap();
        std::fs::write(
            source.join("scripts/new-build.mjs"),
            "// new build helper\n",
        )
        .unwrap();
        let lab = root.join("lab");
        git(
            &context,
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                lab.to_str().unwrap(),
                &original,
            ],
        )
        .await
        .unwrap();
        let (snapshot, fingerprint) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(snapshot);
        assert_eq!(fingerprint.len(), 64);
        assert!(!lab.join("notes.txt").exists());
        assert!(lab.join("src/new.rs").exists());
        assert!(!lab.join("scripts/research/probe.py").exists());
        assert!(lab.join("scripts/new-build.mjs").exists());
        let base = git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap();
        std::fs::write(lab.join("src/main.rs"), "measured improvement\n").unwrap();
        let mut cmd = Command::new("git");
        cmd.current_dir(&lab).args(["diff", "HEAD"]);
        let patch = String::from_utf8(
            command(&context, cmd, Duration::from_secs(5))
                .await
                .unwrap()
                .stdout,
        )
        .unwrap();
        std::fs::write(lab.join("src/main.rs"), "owner edit\n").unwrap();
        let mut candidate = json!({"status":"verified","baseCommit":base,"patch":patch,
            "patchSha256":format!("{:x}",Sha256::digest(patch.as_bytes())),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees","metrics":{"eligible":false}}});
        assert!(adopt(&context, &lab, &candidate, &base).await.is_err());
        candidate["evaluation"]["metrics"]["eligible"] = json!(true);
        let accepted = adopt(&context, &lab, &candidate, &base).await.unwrap();
        assert_ne!(accepted, base);
        assert_eq!(
            git(&context, &lab, &["rev-parse", "HEAD"]).await.unwrap(),
            accepted
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_to_string(source.join("src/main.rs")).unwrap(),
            "owner edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "owner research\n"
        );
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn accepted_source_is_rebuilt_and_really_executes_the_next_generation() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(
            source.join("src/main.rs"),
            include_str!("../../tests/fixtures/rsi-generation-worker.rs"),
        )
        .unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source)
            .args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30))
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(
            &context,
            &source,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "controlled worker",
            ],
        )
        .await
        .unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"])
            .await
            .unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context
            .checkpoint(
                json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "limits":{"maxAttempts":2,"tokenBudget":1000,"timeBudgetSeconds":240}}}),
            )
            .unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(240), run(&context, &job))
            .await
            .unwrap()
            .unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 2);
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        assert_eq!(evolution["history"][0]["domain"], "original_executable");
        assert_eq!(
            evolution["history"][1]["domain"], "improved_executable",
            "a label change alone must not count as recursion"
        );
        assert_eq!(
            evolution["history"][1]["generatorCommit"],
            evolution["history"][0]["acceptedCommit"]
        );
        assert_ne!(
            evolution["history"][1]["generatorBinarySha256"],
            evolution["history"][0]["generatorBinarySha256"]
        );
        let inherited=&evolution["history"][1]["inheritedImplementations"][0];
        assert_eq!(inherited["status"],"available_in_binary");
        assert_eq!(inherited["implementation_ref"]["commit"],evolution["history"][0]["acceptedCommit"]);
        assert_eq!(inherited["binarySha256"],evolution["history"][1]["generatorBinarySha256"]);
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            evolution["headCommit"]
        );
        assert_eq!(
            git(&context, &source, &["rev-parse", "HEAD"])
                .await
                .unwrap(),
            base
        );
        assert!(std::fs::read_to_string(source.join("src/main.rs"))
            .unwrap()
            .contains("n >= 0"));
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(
            &context,
            &source,
            &["worktree", "remove", "--force", lab.to_str().unwrap()],
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn goal_capability_loop_inherits_research_adopts_and_applies_next_decision() {
        let (state, root) = state();
        let context = parent(&state);
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"dominion-rust\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[dependencies]\nserde_json=\"1\"\nsha2=\"0.10\"\nrusqlite={version=\"0.32\",features=[\"bundled\"]}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(source.join("src/main.rs"), include_str!("../../tests/fixtures/rsi-generation-worker.rs")).unwrap();
        std::fs::write(source.join("build.rs"),r#"fn main(){let out=std::process::Command::new("git").args(["rev-parse","HEAD"]).output().unwrap();println!("cargo:rustc-env=FIXTURE_COMMIT={}",String::from_utf8_lossy(&out.stdout).trim());println!("cargo:rerun-if-env-changed=DOMINION_RSI_BUILD_COMMIT");}"#).unwrap();
        let mut lock = Command::new("cargo");
        lock.current_dir(&source).args(["generate-lockfile", "--offline"]);
        let output = command(&context, lock, Duration::from_secs(30)).await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        git(&context, &source, &["init", "-q"]).await.unwrap();
        git(&context, &source, &["add", "."]).await.unwrap();
        git(&context, &source, &["-c","user.name=Test","-c","user.email=test@localhost","commit","-qm","controlled worker"]).await.unwrap();
        let base = git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap();
        git(&context,&source,&["checkout","-b","dominion-sub3"]).await.unwrap();
        let cases=["zero_boundary","negative_one","negative_two","negative_three","positive_one","positive_two","positive_three","positive_four"]
            .into_iter().enumerate().map(|(i,name)|json!({"id":name,"domain":if i%2==0{"boundary"}else{"retention"},"holdout":i<2,"filter":format!("cases::{name}")})).collect::<Vec<_>>();
        context.checkpoint(json!({"input":{"rsiNativeApproved":true,"rsiSourceDir":source,"rsiBaseCommit":base,
            "selectionPolicy":"automatic","rsiFrozenCases":cases,
            "rsiSourceBranch":"dominion-sub3",
            "rsiObjective":"memory capability improvement",
            "rsiFocusFiles":["src/engine/memory.rs"],
            "rsiFixtureMode":"research_then_adopt",
            "limits":{"maxAttempts":4,"tokenBudget":1000,"timeBudgetSeconds":300}}})).unwrap();
        let job = state.store.get("job", "campaign").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(300), run(&context, &job)).await.unwrap().unwrap();
        let evolution = &result["evolution"];
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(evolution["attempts"], 4, "{result}");
        assert_eq!(evolution["acceptedGenerations"], 1, "{result}");
        let history = evolution["history"].as_array().unwrap();
        assert_eq!(history.len(), 4);
        assert!(history[0]["candidateId"].is_null());
        assert!(history[0]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[0]["knowledgeId"].as_str().is_some());
        assert_eq!(history[1]["researchDelivered"], true, "next generation did not receive stored research");
        assert_eq!(history[1]["accepted"], true, "{result}");
        assert_eq!(history[1]["decisionInput"]["nextDomain"], "memory",
            "the user goal did not drive the next capability selection");
        let accepted_commit = history[1]["acceptedCommit"].as_str().unwrap();
        assert_eq!(history[2]["generatorCommit"], accepted_commit);
        assert_ne!(history[2]["generatorBinarySha256"], history[1]["generatorBinarySha256"]);
        assert_eq!(history[2]["workerMarker"], "improved_execution");
        assert_eq!(history[2]["boundaryResult"], false, "the rebuilt function must actually execute");
        assert!(history[2]["candidateId"].is_string());
        assert_eq!(history[2]["accepted"], false, "a comment-only candidate must not be adopted");
        assert!(history[2]["newFindings"].as_u64().unwrap() > 0);
        assert!(history[2]["knowledgeId"].as_str().is_some());
        assert_ne!(history[2]["knowledgeId"], history[0]["knowledgeId"]);
        let inherited = &history[2]["inheritedImplementations"][0];
        assert_eq!(inherited["status"], "available_in_binary");
        assert_eq!(inherited["implementation_ref"]["commit"], accepted_commit);
        assert_eq!(history[2]["decisionInput"]["basis"], "accepted_implementation");
        assert_eq!(history[3]["decisionInput"]["basis"], "research_findings");
        assert_eq!(history[3]["decisionInput"]["observed"]["accepted"], false);
        assert!(history[3]["decisionInput"]["observed"]["tokens"].is_number());
        assert!(history[3]["decisionInput"]["observed"]["elapsedMs"].is_number());
        assert_eq!(history[3]["domain"], history[3]["decisionInput"]["nextDomain"]);
        assert_eq!(history[3]["searchStrategy"]["decisionSource"], "parent_next_decision",
            "the fourth generation did not execute the parent's next decision");
        assert_eq!(history[3]["workerMarker"], "decision_use");
        assert_eq!(evolution["nextDecision"]["basis"], "no_progress");
        assert_eq!(evolution["nextDecision"]["fromGeneration"], 4);
        assert_eq!(
            state.store.get("rsi_current", "selected-engine").unwrap()["commit"],
            accepted_commit
        );
        assert_eq!(git(&context, &source, &["rev-parse", "HEAD"]).await.unwrap(), base);
        for row in history {
            let stored=state.store.get("rsi_generation",row["id"].as_str().unwrap()).unwrap();
            for field in ["newFindings","knowledgeId","knowledgeArtifactHashes","methodExecutions","accepted"] {
                assert_eq!(stored[field],row[field],"next workers must read the completed {field}");
            }
        }
        // The same accepted chain can reach the real test branch through an
        // explicit product action. Untracked research and main are preserved.
        git(&context,&source,&["update-ref","refs/heads/main",&base]).await.unwrap();
        git(&context,&source,&["checkout","main"]).await.unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_err(),
            "main must never be an application target");
        assert_eq!(git(&context,&source,&["rev-parse","HEAD"]).await.unwrap(),base);
        git(&context,&source,&["checkout","dominion-sub3"]).await.unwrap();
        std::fs::write(source.join("my-research.txt"),"keep my unrelated research").unwrap();
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source}));
        context.checkpoint(json!({"status":"completed"})).unwrap();
        let original=state.store.all("rsi_candidate").into_iter()
            .find(|c|c["experimentAdoption"]["commit"]==accepted_commit).unwrap();
        let mut tampered=original.clone();tampered["patch"]=json!("unverified replacement");
        state.store.put("rsi_candidate",&tampered);
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_err());
        state.store.put("rsi_candidate",&original);
        let mut stopped=state.store.get("rsi_campaign","campaign").unwrap();
        stopped["status"]=json!("cancelled");stopped["acceptedGenerations"]=json!(0);
        state.store.put("rsi_campaign",&stopped);
        // A later stopped campaign may carry an earlier verified selection.
        // No new model generation is needed merely to publish that selection.
        let applied=rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.unwrap();
        assert_eq!(applied["commit"],accepted_commit);
        assert!(rsi_worktree::apply_campaign_to_sub(&state,"campaign",accepted_commit).await.is_ok(),"retry must not apply twice");
        assert_eq!(std::fs::read_to_string(source.join("my-research.txt")).unwrap(),"keep my unrelated research");
        let rev=|reference:&str|String::from_utf8(std::process::Command::new("git").current_dir(&source)
            .args(["rev-parse",reference]).output().unwrap().stdout).unwrap().trim().to_string();
        assert_eq!(rev("HEAD"),accepted_commit);assert_eq!(rev("main"),base);
        context.checkpoint(json!({"status":"running"})).unwrap();
        let lab = PathBuf::from(evolution["labDir"].as_str().unwrap());
        git(&context, &source, &["worktree", "remove", "--force", lab.to_str().unwrap()]).await.unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    fn generation(domain: &str, accepted: bool) -> Value {
        json!({
            "domain": domain,
            "accepted": accepted,
            "tokens": 10_000,
            "candidateId": if accepted { json!("candidate") } else { Value::Null },
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn active_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_active",
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn trial_generation(
        job: &str,
        attempt: u64,
        domain: &str,
        accepted: bool,
        commit: &str,
        tokens: u64,
        elapsed: u64,
        candidate: &str,
    ) -> Value {
        json!({
            "jobId": job, "attempt": attempt, "domain": domain, "accepted": accepted,
            "generatorCommit": commit, "domainSelection": "algorithm_trial",
            "searchStrategy": {"id": candidate, "mode": "trial"},
            "tokens": tokens, "elapsedMs": elapsed,
            "evaluation": {"verdict": if accepted { "improved" } else { "inconclusive" }}
        })
    }

    fn search_expression(name: &str, expression: &str) -> Value {
        json!({
            "id": format!("algorithm-{name}"),
            "kind": "selfimprove.search",
            "name": name,
            "variant": name,
            "implementation": "expression",
            "revision": format!("revision-{name}"),
            "parameters": {"expression": expression}
        })
    }

    #[test]
    fn empty_model_error_does_not_abort_a_campaign() {
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":""}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{"modelError":"  "}}})), None);
        assert_eq!(generation_model_error(&json!({"result":{"rsi":{}}})), None);
        assert_eq!(
            generation_model_error(&json!({"result":{"rsi":{"modelError":"rsi_token_budget: stop"}}})),
            Some("rsi_token_budget: stop")
        );
    }

    #[tokio::test]
    async fn source_snapshot_fingerprint_tracks_the_commit() {
        let (state, root) = state();
        state.store.put("job", &json!({"id":"snapshot-test","status":"running","cancelRequested":false}));
        let source = root.join("source-repo");
        std::fs::create_dir_all(&source).unwrap();
        let run_git = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&source)
                .args(["-c","user.name=test","-c","user.email=test@localhost","-c","commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap()
        };
        assert!(run_git(&["init","-q"]).status.success());
        std::fs::write(source.join("value.txt"), "one").unwrap();
        assert!(run_git(&["add","."]).status.success());
        assert!(run_git(&["commit","-q","-m","one"]).status.success());
        let lab = root.join("lab");
        std::fs::create_dir_all(&lab).unwrap();
        std::process::Command::new("git").current_dir(&lab).args(["init","-q"]).output().unwrap();
        let context = crate::engine::jobs::JobContext::new(state.clone(), "snapshot-test".into());
        let (_, first) = snapshot_source(&context, &source, &lab).await.unwrap();
        std::fs::write(source.join("value.txt"), "two").unwrap();
        assert!(run_git(&["commit","-qam","two"]).status.success());
        let (_, second) = snapshot_source(&context, &source, &lab).await.unwrap();
        assert!(lineage_fingerprint_valid(&first), "a clean checkout must not hash to the empty digest");
        assert_ne!(first, second, "a new commit must change the lineage fingerprint");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lineage_fingerprint_identifies_the_source_commit() {
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let first = lineage_fingerprint("a".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        let second = lineage_fingerprint("c".repeat(40).as_str(), "b".repeat(40).as_str(), b"", &[]);
        assert_eq!(first.len(), 64);
        assert_ne!(first, empty_hash, "a clean checkout still has a commit and tree");
        assert_ne!(first, second, "different commits must not share a fingerprint");
        assert!(lineage_fingerprint_valid(&first));
        assert!(!lineage_fingerprint_valid(empty_hash));
        assert!(!lineage_fingerprint_valid(""));
    }

    #[test]
    fn autonomous_objective_is_open_ended_while_explicit_goals_are_respected() {
        let generated=json!({
            "rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다",
            "rsiFocusFiles":[]
        });
        assert_eq!(selection_objective(&generated),"");
        assert_eq!(candidate_domains(&selection_objective(&generated)),DOMAINS);
        let explicit=json!({
            "rsiAutonomous":false,
            "rsiObjective":"자기개선 알고리즘 검색을 개선",
            "rsiFocusFiles":[]
        });
        let eligible=candidate_domains(&selection_objective(&explicit));
        assert!(eligible.contains(&"improver"));
        assert!(!eligible.contains(&"ui"));
        assert!(!eligible.contains(&"memory"));
        let latest=json!({"rsiAutonomous":true,"rsiFocusFiles":["src/engine/memory.rs"]});
        assert!(selection_objective(&latest).contains("memory"),
            "explicit focus files retain their intended capability even for open objectives");
        let automatically_focused=json!({
            "rsiAutonomous":true,"rsiAutoFocus":true,
            "rsiObjective":"지식과 경험을 활용해 개선",
            "rsiFocusFiles":["src/engine/memory.rs"]
        });
        assert_eq!(selection_objective(&automatically_focused),"",
            "automatic file discovery must never turn into an owner constraint");
    }

    #[test]
    fn invalid_generated_search_expression_cannot_redirect_an_explicit_goal() {
        let candidate=json!({"implementation":"expression","parameters":{
            "expression":"unknownRuntimeFeature + 1"
        }});
        assert_eq!(select_domain(&candidate,&[],"브라우저 도구 연결"),"tools");
        assert_eq!(select_domain(&candidate,&[],"자기개선 알고리즘"),"improver");
        let history=vec![json!({"domain":"memory","accepted":true,"tokens":1})];
        assert_eq!(select_domain(&candidate,&history,"화면 버튼 개선"),"ui");
    }

    #[test]
    fn worker_feedback_is_valid_json_and_compacts_before_token_overflow() {
        let prior=vec![json!({
            "domain":"memory","accepted":false,"tokens":20_000,
            "error":"too much telemetry ".repeat(2000),
            "evaluation":{"verdict":"inconclusive","metrics":{
                "measurements":[{"id":"quality","metrics":[{"improved":true,"regressed":false}]}]
            }}
        })];
        let (minimal,_) = fit_worker_prompt("owner objective","memory","memory",&[],None).unwrap();
        let max_tokens=crate::engine::chat::estimate_text_tokens(&minimal)+256+64;
        let (prompt,plan)=fit_worker_prompt("owner objective","memory","memory",&prior,
            Some(max_tokens)).unwrap();
        assert!(prompt.starts_with("owner objective"));
        assert_ne!(plan["mode"],"full");
        assert!(crate::engine::chat::estimate_text_tokens(&prompt)+256<=max_tokens);
        assert!(fit_worker_prompt("owner objective","memory","memory",&prior,Some(1))
            .unwrap_err().starts_with("rsi_token_budget:"));
        let (unbounded,_) = fit_worker_prompt("owner objective","memory","memory",&prior,None).unwrap();
        let json_text=unbounded.split("이 실험 계보의 과거 결과: ").nth(1).unwrap();
        assert!(serde_json::from_str::<Value>(json_text).unwrap().is_array(),
            "feedback must be complete, parsable JSON rather than truncated bytes");
    }

    #[test]
    fn korean_goals_select_the_requested_capability_without_focus_files() {
        let algorithm=json!({"implementation":"failure-clusters"});
        for (goal,domain) in [
            ("오래된 사실을 기억하고 회상해","memory"),
            ("대화 토큰 비용을 줄여","context"),
            ("외부 지식을 학습해","learning"),
            ("브라우저 검색 도구를 연결해","tools"),
            ("버튼과 화면을 단순하게 해","ui"),
            ("자기개선 알고리즘을 발전시켜","improver"),
        ] {assert_eq!(select_domain(&algorithm,&[],goal),domain,"{goal}");}
        assert!(!objective_matches("ui","build the service"));
    }

    #[tokio::test]
    async fn learned_method_uses_real_tools_and_failed_postconditions_change_reuse() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        // This local provider only substitutes model decisions. The production
        // runtime must read the research, register a DAG, bind results, execute
        // tools, verify effects and update the following decision itself.
        async fn provider(Json(body):Json<Value>)->axum::response::Response {
            let messages=body["messages"].as_array().unwrap();
            let last=messages.last().unwrap();
            let request=messages.iter().rev().find(|m|m["role"]=="user")
                .and_then(|m|m["content"].as_str()).unwrap_or("");
            let mut call=None;
            if request.contains("학습 조사") {
                if last["role"]=="tool" {
                    let result:Value=serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
                    if let Some(content)=result["content"].as_str() {
                        let (_,source)=content.trim_start().split_once(' ').unwrap();
                        let knowledge:Value=serde_json::from_str(source.trim()).unwrap();
                        call=Some(("rsi_method_propose",json!({"name":"연구에서 합성한 자료 처리","domain":"context",
                            "description":knowledge["principle"],
                            "verification":{"type":"procedure","appliesTo":{"requestContains":["대화"]},
                                "nodes":[
                                    {"id":"source","tool":"workspace_read_range","arguments":{"path":knowledge["input"],"startLine":1,"endLine":2}},
                                    {"id":"derive","tool":"workspace_write","arguments":{"path":knowledge["output"],"content":"$source.content"}}
                                ],"edges":[{"from":"source","to":"derive"}],
                                "after":[{"kind":"file_contains","path":knowledge["output"],"text":knowledge["needle"]}]}})));
                    }
                } else {
                    call=Some(("workspace_read_range",json!({"path":"src/recipe.json","startLine":1,"endLine":1})));
                }
            }
            let (delta,finish)=if let Some((name,args))=call {
                (json!({"tool_calls":[{"index":0,"id":format!("call-{name}"),"type":"function",
                    "function":{"name":name,"arguments":args.to_string()}}]}),"tool_calls")
            }else{(json!({"content":"작업 완료"}),"stop")};
            let events=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]}),
                json!({"choices":[{"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],events).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {axum::serve(listener,Router::new().route("/chat/completions",post(provider))).await.unwrap()});
        let (state,root)=state();
        let _parent=parent(&state);
        std::fs::create_dir_all(state.config.workspace.join("src")).unwrap();
        let marker=format!("learned-{}",uuid::Uuid::new_v4());
        std::fs::write(state.config.workspace.join("src/input.txt"),&marker).unwrap();
        std::fs::write(state.config.workspace.join("src/recipe.json"),json!({
            "input":"src/input.txt","output":"src/derived.txt","needle":marker,
            "principle":"읽은 결과를 다음 도구의 입력으로 연결한다"}).to_string()).unwrap();
        state.store.put("provider",&json!({"id":"local-test","kind":"openai","baseUrl":format!("http://{address}"),
            "model":"test","toolMode":"native","enabled":true}));
        let input=json!({"rsiObjective":"대화 자료 처리 능력","rsiDomain":"context",
            "rsiKnowledgeSource":state.config.workspace,"rsiSourceDir":state.config.workspace});
        for (index,session_id) in ["learn","use","counterexample","after-failure"].into_iter().enumerate() {
            state.store.put("job",&json!({"id":session_id,"kind":"rsi_generation","status":"running",
                "rsiWorkspace":state.config.workspace,"input":{"rsiCampaignId":"campaign"}}));
            let mut session=json!({"id":session_id,"providerId":"local-test","rsiRun":true,
                "rsiObjective":input["rsiObjective"],"rsiCampaignId":"campaign","rsiGenerationJobId":session_id});
            configure_method_session(&state,&input,&mut session);
            state.store.put("session",&session);
            if index==2 {std::fs::write(state.config.workspace.join("src/input.txt"),"changed premise").unwrap();}
            let request=crate::engine::chat::TurnRequest::from_value(&json!({"sessionId":session_id,
                "input":if index==0{"대화 방법을 학습 조사해"}else{"대화 자료를 처리해"},
                "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false})).unwrap();
            tokio::time::timeout(Duration::from_secs(10),crate::engine::run_quiet(&state,&request))
                .await.unwrap().unwrap();
            collect_method_executions(&state,session_id,&state.config.workspace);
            let generation=json!({"jobId":session_id,"accepted":false,"domain":"context","newFindings":1,"tokens":20,"elapsedMs":1});
            if index==0 {
                let id=remember_research(&state,&input,&generation,session_id).expect("research and executable proposal must persist");
                let record=state.store.get("experience",&id).unwrap();
                assert_eq!(record["executableMethods"].as_array().unwrap().len(),1);
                assert!(!record["researchArtifacts"].as_array().unwrap().is_empty());
                assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
            }else {
                let receipts=record_method_feedback(&state,&generation,session_id);
                if index<3 {
                    assert_eq!(receipts.len(),1,"actual engine procedure invocation missing");
                    assert_eq!(receipts[0]["operationIds"].as_array().unwrap().len(),2);
                    assert_eq!(receipts[0]["result"]["followed"],index==1,"{receipts:?}");
                }else{assert!(receipts.is_empty(),"failed method must not run again");}
                if index==1 {
                    assert!(std::fs::read_to_string(state.config.workspace.join("src/derived.txt")).unwrap().contains(&marker));
                    assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"execute_methods");
                    // A different repository and an explicit no-reuse decision cannot inject it.
                    let mut probe=session.clone();probe["id"]=json!("not-yet-executed");
                    assert_eq!(method_strategies(&state,&probe).len(),1);
                    let mut foreign=probe.clone();foreign["rsiResearchPartition"]=json!("other");
                    assert!(method_strategies(&state,&foreign).is_empty());
                    foreign=probe.clone();foreign["rsiDecisionInput"]["reuseDecision"]["action"]=json!("research_only");
                    assert!(method_strategies(&state,&foreign).is_empty());
                }else{assert_eq!(method_reuse_decision(&state,&input,"context")["action"],"research_only");}
            }
        }
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_method_unknown_verification_is_held_not_failed() {
        let (state,root)=state();
        let workspace=&state.config.workspace;
        std::fs::create_dir_all(workspace.join("unreadable.txt")).unwrap();
        std::fs::write(workspace.join("known.txt"),"ok").unwrap();
        let evaluate=|after:Value| crate::engine::procedure::evaluate(
            &json!({"id":"method","verification":{"type":"procedure","after":after}}),&[],workspace);
        let unknown=evaluate(json!([{"kind":"file_equals","path":"unreadable.txt","text":"ok"}]));
        assert_eq!(unknown["verified"],true);
        assert_eq!(unknown["unknown"],true);
        let mut status_only=unknown.clone();status_only["unknown"]=Value::Null;
        let violated=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        let mixed=evaluate(json!([
            {"kind":"file_equals","path":"unreadable.txt","text":"ok"},
            {"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        assert_eq!(mixed["unknown"],true);
        let passed=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"ok"}]));
        for (index,(result,status,verdict,postcondition)) in [
            (unknown,"held","inconclusive",Value::Null),
            (status_only,"held","inconclusive",Value::Null),
            (violated,"suspended","violated",json!(false)),
            (mixed,"suspended","violated",json!(false)),
            (passed,"executable","passed",json!(true)),
        ].into_iter().enumerate() {
            let id=format!("experience-{index}");
            state.store.put("experience",&json!({"id":id,"executableMethods":[
                {"id":"method","revision":"r1","status":"candidate","successfulApplications":0}]}));
            let session=format!("s-{index}");
            state.store.put("session",&json!({"id":session,"rsiMethodExecutions":[{
                "methodId":"method","revision":"r1","experienceId":id,
                "executionId":format!("exec-{index}"),"result":result}]}));
            let generation=json!({"jobId":"child","accepted":true,"newFindings":1});
            record_method_feedback(&state,&generation,&session);
            record_method_feedback(&state,&generation,&session);
            let record=state.store.get("experience",&id).unwrap();
            let method=&record["executableMethods"][0];
            assert_eq!(method["status"],status);
            assert_eq!(method["applications"].as_array().unwrap().len(),1);
            assert_eq!(method["applications"][0]["verificationVerdict"],verdict);
            assert_eq!(method["applications"][0]["postconditionsPassed"],postcondition);
            assert_eq!(method["successfulApplications"],usize::from(verdict=="passed"));
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_method_proposals_require_running_rsi_and_executable_verification() {
        let (state,root)=state();
        let _context=parent(&state);
        state.store.put("session",&json!({"id":"s","rsiRun":true,"rsiCampaignId":"campaign","rsiGenerationJobId":"child"}));
        state.store.put("job",&json!({"id":"child","kind":"rsi_generation","status":"running","input":{"rsiCampaignId":"campaign"}}));
        let valid=json!({"name":"읽기 절차","domain":"context","verification":{
            "type":"procedure","appliesTo":{"requestContains":["대화"]},
            "nodes":[{"id":"read","tool":"workspace_read_range","arguments":{"path":"src/a.rs","startLine":1}}],
            "after":[{"kind":"tool_called","tool":"workspace_read_range","path":"src/a.rs"}]}});
        assert!(propose_method(&state,"missing",&valid).is_err());
        let mut invalid=valid.clone();
        invalid["verification"]["nodes"][0]["tool"]=json!("shell_exec");
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["nodes"][0]["tool"]=json!("rsi_method_propose");
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["after"]=json!([]);
        assert!(propose_method(&state,"s",&invalid).is_err());
        invalid=valid.clone();invalid["verification"]["after"]=json!([{"kind":"file_contains","path":"src/a.rs"}]);
        assert!(propose_method(&state,"s",&invalid).is_err());
        let first=propose_method(&state,"s",&valid).unwrap();
        assert_eq!(first["method"]["status"],"candidate");
        propose_method(&state,"s",&valid).unwrap();
        assert_eq!(state.store.get("session","s").unwrap()["rsiMethodProposals"].as_array().unwrap().len(),1);
        state.store.put("job",&json!({"id":"campaign","kind":"rsi_evolve","status":"cancelled"}));
        assert!(propose_method(&state,"s",&valid).is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn autonomous_trial_queue_identity_matches_descendant_lookup_objective() {
        let input=json!({"rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 자기개선"});
        assert_eq!(selection_objective(&input),"");
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"aligned-goal",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"exploration + objectiveMatch"}})).unwrap();
        let objective=selection_objective(&input);
        let report=queue_search_trial(&state,&candidate,&active,&[],&objective,"campaign",1);
        assert_eq!(report["status"],"pending_trial");
        let pending=pending_trial_candidate(&state,&objective,&["campaign".into()]);
        assert_eq!(pending.unwrap()["id"],candidate["id"],
            "a policy queued by an autonomous campaign must be selectable by its child");
        let raw_prompt=input["rsiObjective"].as_str().unwrap();
        assert!(pending_trial_candidate(&state,raw_prompt,&["campaign".into()]).is_none(),
            "generic system wording must not replace the canonical selection objective");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn observed_quality_gain_autonomously_queues_a_real_policy_trial() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let baseline=active["id"].clone();
        let base="a".repeat(40);
        let accepted_commit="b".repeat(40);
        let plan=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "domain":"tools","holdout":n<3,"metrics":[]}))
            .collect::<Vec<_>>();
        let measurements=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "baselinePass":n!=0,"candidatePass":true}))
            .collect::<Vec<_>>();
        let metrics=crate::engine::rsi::evaluate_cases(&plan,&measurements,"harness").unwrap();
        state.store.put("rsi_candidate",&json!({
            "id":"measured","target":"harness","status":"verified","cases":plan,
            "baseCommit":base,"patchSha256":"p".repeat(64),
            "oracleVersion":"frozen-v1","experimentAdoption":{"commit":accepted_commit},
            "evaluation":{"metrics":metrics,"verdict":metrics["verdict"],
                "source":"server_invoked_local_paired_git_worktrees","evidenceSha256":"e".repeat(64)}
        })).unwrap();
        state.store.put("rsi_pair",&json!({
            "id":"rsi-pair-measured","candidateId":"measured",
            "runner":"local_gitrepo_not_os_sandbox","evidenceSha256":"e".repeat(64),
            "report":{"baseCommit":base,"patchSha256":"p".repeat(64),
                "oracleVersion":"frozen-v1","reportSha256":"e".repeat(64),
                "baselineBuildOk":true,"candidateBuildOk":true,"cases":measurements},
            "metrics":metrics,"verdict":metrics["verdict"]
        })).unwrap();
        let history=vec![
            json!({"id":"1","domain":"memory","accepted":false,"newFindings":0,"tokens":5000}),
            json!({"id":"2","domain":"tools","accepted":true,"tokens":7000,
                "candidateId":"measured","generatorCommit":base,
                "acceptedCommit":accepted_commit}),
            json!({"id":"3","domain":"memory","accepted":false,"newFindings":0,"tokens":14000}),
            json!({"id":"4","domain":"tools","accepted":false,"newFindings":1,"tokens":18000})
        ];
        let queued=queue_adaptive_search_trial(&state,&history,&active,
            "","campaign",4).expect("verified outcomes must generate a trialable policy");
        assert_eq!(queued["status"],"pending_trial");
        let id=queued["candidateId"].as_str().unwrap();
        let new_policy=state.store.get("algorithm",id).unwrap();
        assert_eq!(new_policy["source"],"rsi-generated");
        assert_eq!(new_policy["rsiTrial"]["campaignId"],"campaign");
        assert_eq!(new_policy["rsiTrial"]["status"],"pending_trial");
        assert!(new_policy["parameters"]["expression"].as_str().unwrap().contains("holdoutGains"));
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            baseline,"automatic proposal must not activate without later real trials");
        let old=state.store.get("rsi_pair","rsi-pair-measured").unwrap();
        let mut tampered=old.clone();
        tampered["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&tampered).unwrap();
        let revalidated=history_with_verified_payoffs(&state,&history);
        assert_eq!(revalidated[1]["evidenceVerified"],false);
        assert!(adaptive_search_expression(&revalidated).is_none(),
            "mutable outcome claims cannot bootstrap an evolved policy");
        let no_proof=queue_adaptive_search_trial(&state,&history,&active,
            "","fresh-campaign",4);
        assert!(no_proof.is_none(),"forged raw report must not generate an improved policy");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn self_improver_evolves_an_executable_search_formula_from_measured_history() {
        let rows=vec![
            json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":5000}),
            json!({"domain":"tools","accepted":true,"evidenceVerified":true,
                "verifiedHoldoutGains":2,"tokens":6000}),
            json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":9000}),
            json!({"domain":"tools","accepted":false,"newFindings":1,"tokens":11000})
        ];
        assert!(adaptive_search_expression(&rows[..3]).is_none());
        let expression=adaptive_search_expression(&rows).unwrap();
        let candidate=json!({"implementation":"expression","parameters":{"expression":expression}});
        assert!(scored_domains(&candidate,&rows,"").is_some(),
            "self-generated policy must run in the ordinary search algorithm kernel");
        let mut later=rows.clone();
        later.push(json!({"domain":"memory","accepted":false,"newFindings":0,"tokens":40000}));
        let revised=adaptive_search_expression(&later).unwrap();
        assert_ne!(expression,revised,"the formula itself must evolve with observed outcomes");
        let no_proof=rows.iter().map(|row|{
            let mut row=row.clone();
            row["evidenceVerified"]=json!(false);
            row
        }).collect::<Vec<_>>();
        assert!(adaptive_search_expression(&no_proof).is_none(),
            "claimed acceptance without independently verified payoff is insufficient");
    }

    #[test]
    fn repeating_a_failed_patch_is_skipped_but_a_new_base_or_patch_can_retry() {
        let (state,root)=state();
        let base="a".repeat(40);
        let digest="d".repeat(64);
        state.store.put("rsi_candidate",&json!({"id":"old","baseCommit":base,
            "patchSha256":digest})).unwrap();
        state.store.put("rsi_candidate",&json!({"id":"new","baseCommit":base,
            "patchSha256":digest})).unwrap();
        let mut history=vec![json!({"candidateId":"old","accepted":false,
            "evaluation":{"verdict":"regressed"}})];
        assert_eq!(identical_failed_patch(&state,&history,"new"),Some("old".into()));
        history[0]["accepted"]=json!(true);
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        history[0]["accepted"]=json!(false);
        history[0]["evaluation"]["verdict"]=json!("unmeasured");
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        history[0]["evaluation"]["verdict"]=json!("inconclusive");
        let mut changed=state.store.get("rsi_candidate","new").unwrap();
        changed["baseCommit"]=json!("b".repeat(40));
        state.store.put("rsi_candidate",&changed).unwrap();
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        changed["baseCommit"]=json!(base);
        changed["patchSha256"]=json!("e".repeat(64));
        state.store.put("rsi_candidate",&changed).unwrap();
        assert!(identical_failed_patch(&state,&history,"new").is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_authored_search_formulas_receive_every_advertised_runtime_feature() {
        let history=vec![
            json!({"domain":"tools","accepted":false,"newFindings":3,"tokens":1000}),
            json!({"domain":"tools","accepted":false,"newFindings":0,"tokens":2000})
        ];
        let features=search_features(&history,"tools","tools");
        for key in crate::engine::algorithms::SELFIMPROVE_FEATURES {
            assert!(features.contains_key(*key),"advertised feature missing: {key}");
        }
        assert_eq!(features["usefulGenerations"],1.0);
        assert_eq!(features["barrenRate"],0.5);
        let candidate=json!({"implementation":"expression","parameters":{
            "expression":"usefulGenerations + verifiedOutcomes + holdoutGains + gainPer10kTokens - barrenRate"
        }});
        assert!(score_domain(&candidate,&features).is_some(),
            "a registered model-authored formula must not silently fall back at runtime");
    }

    #[test]
    fn original_paired_holdout_gains_change_the_next_improvement_target() {
        let (state,root)=state();
        let policy=json!({"implementation":"failure-clusters"});
        let base="a".repeat(40);
        let accepted_commit="b".repeat(40);
        let plan=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "domain":"tools","holdout":n<3,"metrics":[]}))
            .collect::<Vec<_>>();
        let raw=(0..8).map(|n|json!({"id":format!("case-{n}"),
            "baselinePass":n!=0,"candidatePass":true}))
            .collect::<Vec<_>>();
        let metrics=crate::engine::rsi::evaluate_cases(&plan,&raw,"harness").unwrap();
        assert_eq!(metrics["eligible"],true);
        let report=json!({"baseCommit":base,"patchSha256":"p".repeat(64),
            "oracleVersion":"frozen-v1","reportSha256":"e".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,"cases":raw});
        let candidate=json!({"id":"tools-verified","status":"verified",
            "baseCommit":base,"patchSha256":"p".repeat(64),"oracleVersion":"frozen-v1",
            "target":"harness","cases":plan,
            "experimentAdoption":{"commit":accepted_commit},
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "evidenceSha256":"e".repeat(64),"metrics":metrics,"verdict":metrics["verdict"]}});
        state.store.put("rsi_candidate",&candidate).unwrap();
        state.store.put("rsi_pair",&json!({"id":"rsi-pair-tools-verified",
            "candidateId":"tools-verified","runner":"local_gitrepo_not_os_sandbox",
            "evidenceSha256":"e".repeat(64),"report":report,
            "metrics":metrics,"verdict":metrics["verdict"]})).unwrap();
        // The older accepted-count heuristic sees both domains as equal
        // and chooses the first domain, not the one with the actual gain.
        let history=vec![
            json!({"domain":"memory","candidateId":"memory-unproven",
                "accepted":true,"tokens":10000,"generatorCommit":base,
                "acceptedCommit":accepted_commit}),
            json!({"domain":"tools","candidateId":"tools-verified",
                "accepted":true,"tokens":10000,"generatorCommit":base,
                "acceptedCommit":accepted_commit})
        ];
        assert_eq!(select_domain(&policy,&history,"memory tools"),"memory");
        let verified=history_with_verified_payoffs(&state,&history);
        assert_eq!(verified[0]["evidenceVerified"],false);
        assert_eq!(verified[1]["verifiedHoldoutGains"],1);
        assert_eq!(select_domain(&policy,&verified,"memory tools"),"tools",
            "replayed verified outcome must guide the next actual domain selection");
        assert_eq!(select_domain(&policy,&verified,"memory"),"memory",
            "the user's explicit objective stays authoritative");
        // A forged generation summary without a matching raw paired receipt
        // must not earn meta-learning credit.
        let mut forged=history.clone();
        forged[0]["evidenceVerified"]=json!(true);
        forged[0]["verifiedHoldoutGains"]=json!(64);
        let rechecked=history_with_verified_payoffs(&state,&forged);
        assert_eq!(rechecked[0]["verifiedHoldoutGains"],0);
        let mut tampered=state.store.get("rsi_pair","rsi-pair-tools-verified").unwrap();
        tampered["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&tampered).unwrap();
        assert_eq!(select_domain(&policy,&history_with_verified_payoffs(&state,&history),
            "memory tools"),"memory","altered raw evidence cannot steer the search policy");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_generation_changes_the_next_model_experiment_instructions() {
        let (state,root)=state();
        state.store.put("rsi_candidate",&json!({"id":"failed-patch",
            "hypothesis":"과거 가설을 반복해도 성능이 향상될 것이다"}));
        let record=json!({"history":[{"attempt":1,"domain":"memory",
            "accepted":false,"candidateId":"failed-patch",
            "evaluation":{"verdict":"regressed"},"error":"baseline 대비 품질 하락"}]});
        let prompt=generation_context(&state,&json!({
            "rsiTargetDomain":"memory","rsiObjective":"기억 성능 개선"}),&record);
        assert!(prompt.contains("실패를 반영한 다음 개선 실험"),"{prompt}");
        assert!(prompt.contains("과거 가설을 반복해도 성능이 향상될 것이다"),"{prompt}");
        assert!(prompt.contains("수정 범위·가설·기대하는 작업 성능 지표를 바꾼 실험"),"{prompt}");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_selection_follows_the_user_goal_before_unrelated_domains() {
        let baseline = json!({"implementation":"failure-clusters","parameters":{}});
        assert_eq!(select_domain(&baseline, &[], "improver"), "improver");
        assert_eq!(select_domain(&baseline, &[], "memory"), "memory");
        assert_eq!(select_domain(&baseline, &[], "unrelated capability"), "memory");
    }

    #[test]
    fn replay_reports_match_rate_only_and_never_contains_promotion() {
        let baseline = json!({"id":"baseline","revision":"b","implementation":"failure-clusters","parameters":{}});
        let candidate = search_expression("accepted-first", "accepted * 10 + novelty * 0.1");
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let report = evaluate_search_algorithm(&candidate, &baseline, &history, "unrelated");
        assert_eq!(report["metric"], "past_successful_domain_match_rate");
        assert_eq!(report["samples"], 2);
        assert_eq!(report["candidateMatches"], 2);
        assert!(report["candidateMatchRate"].as_f64().unwrap()
            > report["activeMatchRate"].as_f64().unwrap());
        assert!(report.get("promote").is_none());
        assert!(report.get("recommendation").is_none());

        let sparse = &history[..2];
        let report = evaluate_search_algorithm(&candidate, &baseline, sparse, "context");
        assert_eq!(report["samples"], 1);
        assert_eq!(report["evaluable"], false);
    }

    #[test]
    fn replay_match_queues_a_trial_without_activating_it() {
        let (state, root) = state();
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let queued = queue_search_trial(
            &state,
            &candidate,
            &active_before,
            &history,
            "context",
            "campaign",
            1,
        );
        assert_eq!(queued["status"], "pending_trial");
        assert_eq!(queued["replay"]["samples"], 2);
        let saved = state.store.get("algorithm", candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["status"], "pending_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"],
            "replay match must not replace the global strategy"
        );
        assert!(state.store.all("algorithm_eval").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn generation_selection_distinguishes_continuation_trial_and_active() {
        let (state, root) = state();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        crate::engine::algorithms::activate(&state, candidate["id"].as_str().unwrap()).unwrap();
        let id = candidate["id"].as_str().unwrap();

        let (domain, mode, strategy) = select_generation_strategy(
            &state,
            &[],
            "context",
            Some("memory".to_string()),
            Some(id.to_string()),
            None,
            "commit-1",
        );
        assert_eq!(domain, "memory");
        assert_eq!(mode, "research_continuation");
        assert!(strategy.is_null(), "continuation did not run the formula");

        let (_, mode, strategy) =
            select_generation_strategy(&state, &[], "context", None, Some(id.to_string()), None, "commit-1");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["trialFallback"], "unused_comparable_baseline_missing");

        let baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let (_, mode, _) = select_generation_strategy(
            &state, &[baseline.clone()], "context", None, Some(id.to_string()), None, "commit-2");
        assert_eq!(mode, "algorithm_active", "another generator's result is not a control");
        let (domain, mode, strategy) = select_generation_strategy(
            &state, &[baseline], "context", None, Some(id.to_string()), None, "commit-1");
        assert_eq!(domain, "context");
        assert_eq!(mode, "algorithm_trial");
        assert_eq!(strategy["id"], id);
        assert_eq!(strategy["mode"], "trial");

        let (domain, mode, strategy) = select_generation_strategy(
            &state, &[], "context", None, None, Some("learning".to_string()), "commit-1");
        assert_eq!(domain, "learning");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["decisionSource"], "parent_next_decision");

        let (_, mode, strategy) =
            select_generation_strategy(&state, &[], "context", None, None, None, "commit-1");
        assert_eq!(mode, "algorithm_active");
        assert_eq!(strategy["mode"], "active");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trial_verdict_requires_success_before_supporting_cost_savings() {
        let mut baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let mut trial = trial_generation("trial", 2, "context", false, "commit-1", 5_000, 500, "candidate");
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive",
            "cheaper non-results are not successful improvement");
        baseline["error"] = json!("failed");
        trial["error"] = json!("failed");
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive");
        // Even an inconsistent accepted flag cannot turn execution errors into support.
        baseline["accepted"] = json!(true);
        trial["accepted"] = json!(true);
        assert_eq!(trial_verdict(&trial, &baseline).0, "inconclusive");
        baseline["error"] = Value::Null;
        assert_eq!(trial_verdict(&trial, &baseline).0, "rejected");
        trial["error"] = Value::Null;
        assert_eq!(trial_verdict(&trial, &baseline).0, "supported");
        trial["evaluation"]["verdict"] = json!("regressed");
        assert_eq!(trial_verdict(&trial, &baseline).0, "rejected");
    }

    #[test]
    fn trial_outcome_needs_a_comparable_baseline_and_two_supported_trials() {
        let (state2, root2) = state();
        let (state, root) = state();
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let id = candidate["id"].as_str().unwrap();
        queue_search_trial(
            &state,
            &candidate,
            &active_before,
            &[],
            "context",
            "campaign",
            1,
        );

        let baseline = active_generation("base", 1, "context", false, "commit-1", 10_000, 1_000);
        let first = trial_generation("trial-1", 2, "context", true, "commit-1", 9_000, 900, id);
        let outcome = record_trial_outcome(&state, id, &first, &[baseline.clone()]);
        assert_eq!(outcome["verdict"], "supported");
        assert_eq!(outcome["status"], "pending_second_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"],
            "one trial must not activate the candidate"
        );

        let repeated = record_trial_outcome(&state, id, &first, &[baseline.clone()]);
        assert_eq!(repeated["alreadyRecorded"], true);
        assert_eq!(repeated["status"], "pending_second_trial");
        let (_, mode, strategy) = select_generation_strategy(
            &state, &[baseline.clone(), first.clone()], "context", None, Some(id.into()), None, "commit-1");
        assert_eq!(mode, "algorithm_active", "a second trial must not reuse the first control");
        assert_eq!(strategy["trialFallback"], "unused_comparable_baseline_missing");
        let saved = state.store.get("algorithm", id).unwrap();
        assert_eq!(saved["rsiTrial"]["trials"].as_array().unwrap().len(), 1);
        assert!(comparable_trial_baseline(&[baseline.clone()], &first, &saved).is_none());

        let baseline2 = active_generation("base-2", 3, "context", false, "commit-1", 10_000, 1_000);
        let (_, mode, _) = select_generation_strategy(
            &state, &[baseline.clone(), first.clone(), baseline2.clone()], "context", None, Some(id.into()), None, "commit-1");
        assert_eq!(mode, "algorithm_trial", "a fresh control permits the pending trial");
        let second = trial_generation("trial-2", 4, "context", true, "commit-1", 9_000, 900, id);
        let outcome = record_trial_outcome(&state, id, &second, &[baseline.clone(), first.clone(), baseline2]);
        assert_eq!(outcome["verdict"], "supported");
        assert_eq!(outcome["status"], "adopted");
        let saved = state.store.get("algorithm", id).unwrap();
        assert_eq!(saved["rsiTrial"]["trials"].as_array().unwrap().len(), 2);
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            id
        );

        // A trial with no active-strategy baseline under the same commit and
        // domain stays inconclusive and never activates.
        let active2 = crate::engine::algorithms::active(&state2, "selfimprove.search").unwrap();
        let candidate2 = crate::engine::algorithms::register(
            &state2,
            &json!({
                "kind":"selfimprove.search","name":"objective-first-2",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        let id2 = candidate2["id"].as_str().unwrap();
        queue_search_trial(&state2, &candidate2, &active2, &[], "context", "campaign", 1);
        let unmatched = trial_generation("trial-x", 2, "context", true, "commit-9", 1, 1, id2);
        let outcome = record_trial_outcome(&state2, id2, &unmatched, &[]);
        assert_eq!(outcome["status"], "inconclusive");
        let repeated = record_trial_outcome(&state2, id2, &unmatched, &[]);
        assert_eq!(repeated["alreadyRecorded"], true);
        assert_eq!(state2.store.get("algorithm", id2).unwrap()["rsiTrial"]["trials"].as_array().unwrap().len(), 1);
        assert_eq!(
            crate::engine::algorithms::active(&state2, "selfimprove.search").unwrap()["id"],
            active2["id"]
        );

        // A research continuation is not a trial even when a candidate was queued.
        let mut continuation = trial_generation("trial-c", 2, "context", true, "commit-1", 1, 1, id);
        continuation["domainSelection"] = json!("research_continuation");
        let outcome = record_trial_outcome(&state2, id2, &continuation, &[]);
        assert_eq!(outcome["status"], "not_used");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(root2);
    }

    #[test]
    fn proposed_algorithm_is_queued_not_activated() {
        let (state, root) = state();
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true,"rsiCampaignId":"campaign"}))
            .unwrap();
        state
            .store
            .put("job", &json!({"id":"campaign","kind":"rsi_evolve","status":"running"}))
            .unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search",
                "name":"objective-first",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op","sessionId":"s","tool":"algorithm_propose","status":"completed",
                    "result":{"algorithmId":candidate["id"]}}),
            )
            .unwrap();
        let proposals = proposed_algorithms(&state, "s");
        assert_eq!(proposals.len(), 1);
        let active_before = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let history = vec![
            generation("context", true),
            generation("context", true),
            generation("context", true),
        ];
        let queued = queue_search_trial(
            &state,
            &proposals[0],
            &active_before,
            &history,
            "context",
            "campaign",
            1,
        );
        assert_eq!(queued["status"], "pending_trial");
        let saved = state.store.get("algorithm", candidate["id"].as_str().unwrap()).unwrap();
        assert_eq!(saved["rsiTrial"]["status"], "pending_trial");
        assert_eq!(
            crate::engine::algorithms::active(&state, "selfimprove.search").unwrap()["id"],
            active_before["id"]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_trial_control_uses_candidate_domain_then_unlocks_real_trial() {
        let (state,root)=state();
        let history=vec![generation("tools",false)];
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"sub3-prefer-attempted",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"attempts * 100 + objectiveMatch"}
        })).unwrap();
        let id=candidate["id"].as_str().unwrap();
        assert_eq!(select_domain(&candidate,&history,"unrelated goal"),"tools");
        assert_ne!(select_domain(&active,&history,"unrelated goal"),"tools");
        let (domain,mode,strategy)=select_generation_strategy(
            &state,&history,"unrelated goal",None,Some(id.into()),None,"commit-one");
        assert_eq!(domain,"tools");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(strategy["controlForCandidateId"],id);
        let mut updated=history;
        updated.push(active_generation("control",2,"tools",false,"commit-one",1000,100));
        let (domain,mode,_)=select_generation_strategy(
            &state,&updated,"unrelated goal",None,Some(id.into()),None,"commit-one");
        assert_eq!(domain,"tools");
        assert_eq!(mode,"algorithm_trial");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn unresolved_trial_cannot_be_claimed_by_sibling_campaign() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let first=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"first-owned",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + novelty"}
        })).unwrap();
        let second=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"second-owned",
            "implementation":"expression","source":"rsi-generated",
            "parameters":{"expression":"objectiveMatch + exploration"}
        })).unwrap();
        assert_eq!(queue_search_trial(&state,&first,&active,&[],"context","campaign-a",1)["status"],"pending_trial");
        assert_eq!(queue_search_trial(&state,&second,&active,&[],"context","campaign-b",1)["status"],"pending_trial");
        assert_eq!(pending_trial_candidate(&state,"context",&["campaign-a".into()]).unwrap()["id"],first["id"]);
        assert_eq!(pending_trial_candidate(&state,"context",&["campaign-b".into()]).unwrap()["id"],second["id"]);
        assert!(pending_trial_candidate(&state,"context",&["unknown".into()]).is_none());
        // Requeue the old in-memory object: ownership must be checked against
        // the latest stored copy, not overwritten by a stale proposal.
        let duplicate=queue_search_trial(&state,&first,&active,&[],"context","campaign-b",2);
        assert_eq!(duplicate["status"],"owned_by_other_campaign");
        assert_eq!(duplicate["ownerCampaignId"],"campaign-a");
        assert_eq!(state.store.get("algorithm",first["id"].as_str().unwrap()).unwrap()
            ["rsiTrial"]["campaignId"],"campaign-a");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn pending_trial_candidate_survives_for_a_later_scheduled_generation() {
        let (state, root) = state();
        let active = crate::engine::algorithms::active(&state, "selfimprove.search").unwrap();
        let candidate = crate::engine::algorithms::register(
            &state,
            &json!({
                "kind":"selfimprove.search","name":"objective-first-pending",
                "implementation":"expression",
                "parameters":{"expression":"objectiveMatch * 10 + novelty"},
                "source":"rsi-generated"
            }),
        )
        .unwrap();
        queue_search_trial(&state, &candidate, &active, &[], "context", "campaign", 1);
        assert_eq!(
            pending_trial_candidate(&state, "context", &["campaign".into()]).unwrap()["id"],
            candidate["id"],
            "a pending candidate is reused by a later scheduled generation"
        );
        assert!(
            pending_trial_candidate(&state, "a different objective", &["campaign".into()]).is_none(),
            "a pending candidate is scoped to its objective"
        );
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn autonomous_barren_generations_execute_their_evolved_policy_without_false_promotion() {
        let (state,root)=state();
        let active=crate::engine::algorithms::active(&state,"selfimprove.search").unwrap();
        let active_id=active["id"].clone();
        let history=(0..3).map(|n|json!({
            "id":format!("generation-{n}"),
            "jobId":format!("job-{n}"),"attempt":n+1,
            "domain":"tools","accepted":false,"candidateId":Value::Null,
            "newFindings":0,"tokens":5000+n*1000,
            "domainSelection":"algorithm_active","generatorCommit":"commit-a"
        })).collect::<Vec<_>>();
        assert!(recovery_search_expression(&history[..2]).is_none());
        let report=queue_adaptive_search_trial(&state,&history,&active,"","campaign",3)
            .expect("three real, barren generations must propose an executable alternative");
        assert_eq!(report["hypothesisSource"],"observed_unproductive_generations");
        let id=report["candidateId"].as_str().unwrap();
        let candidate=state.store.get("algorithm",id).unwrap();
        assert!(scored_domains(&candidate,&history,"").is_some());
        assert_eq!(candidate["rsiTrial"]["status"],"pending_trial");
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            active_id,"a recovery hypothesis is not improvement evidence");
        // Even when research yielded an apparent continuation, the scheduled
        // candidate must get control measurements, then an actual trial.
        let (domain,mode,strategy)=select_generation_strategy(
            &state,&history,"",Some("memory".into()),Some(id.into()),None,"commit-b");
        assert_eq!(mode,"algorithm_active");
        assert_eq!(strategy["controlForCandidateId"],id);
        let mut later=history.clone();
        later.push(json!({"jobId":"control-1","domain":domain,
            "domainSelection":"algorithm_active","generatorCommit":"commit-b",
            "searchStrategy":strategy,
            "candidateId":Value::Null,"accepted":false,"tokens":3000}));
        later.push(json!({"jobId":"control-2","domain":domain,
            "domainSelection":"algorithm_active","generatorCommit":"commit-b",
            "candidateId":Value::Null,"accepted":false,"tokens":4000}));
        let (trial_domain,trial_mode,trial_strategy)=select_generation_strategy(
            &state,&later,"",Some("memory".into()),Some(id.into()),None,"commit-b");
        assert_eq!(trial_domain,domain);
        assert_eq!(trial_mode,"algorithm_trial");
        assert_eq!(trial_strategy["id"],id);
        assert_eq!(trial_strategy["trialFallback"],"exploratory_after_barren_controls");
        let trial=trial_generation("trial-1",6,&domain,false,"commit-b",1000,100,id);
        let outcome=record_trial_outcome(&state,id,&trial,&later);
        assert_eq!(outcome["status"],"inconclusive",
            "a real exploratory run with no paired outcome is not adoption");
        assert_eq!(crate::engine::algorithms::active(&state,"selfimprove.search").unwrap()["id"],
            active_id);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn neutral_descendant_reloads_current_source_instead_of_stale_base_excerpt() {
        let variants=generation_prompt_variants(
            "Improve behavior", "Paired quality unchanged",
            "STALE_READ_RANGE=10-20", "STALE_BASE_SHA=abc", Some("neutral-id"));
        assert_eq!(variants.len(),2);
        for (mode, prompt, defer_focus, defer_research) in variants {
            assert!(mode.starts_with("neutral_"));
            assert!(defer_focus && defer_research);
            assert!(prompt.contains("neutral-id"));
            assert!(prompt.contains("workspace_read_range"));
            assert!(!prompt.contains("STALE_READ_RANGE"));
            assert!(!prompt.contains("STALE_BASE_SHA"));
            assert!(prompt.contains("실질적으로 다른 동작"),
                "child prompt must demand behavioral correction");
        }
    }

    #[test]
    fn ordinary_generation_still_has_full_and_deferred_source_context() {
        let variants=generation_prompt_variants(
            "Improve behavior", "measured", "research", "CURRENT_SOURCE", None);
        assert_eq!(variants.len(),3);
        assert_eq!(variants[0].0,"full");
        assert!(variants[0].1.contains("CURRENT_SOURCE"));
        assert!(!variants[0].2 && !variants[0].3);
        assert!(variants[2].2 && variants[2].3);
    }

    #[test]
    fn autonomous_recovery_budget_is_bounded_but_does_not_stop_before_trial() {
        assert!(should_stop_no_progress(1,false,false,false,false));
        assert!(!should_stop_no_progress(1,false,false,false,true));
        assert!(!should_stop_no_progress(2,false,false,false,true));
        assert!(should_stop_no_progress(3,false,false,false,true));
        assert!(!should_stop_no_progress(4,false,false,true,true));
        assert!(!should_stop_no_progress(5,false,false,true,true));
        assert!(should_stop_no_progress(6,false,false,true,true));
        assert!(!should_stop_no_progress(2,true,true,false,true));
    }

}

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
