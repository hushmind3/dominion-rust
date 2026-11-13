//! Mutable domain targeting and measured-cost search policy of the
//! recompiled RSI descendant. The immutable supervisor facade only routes
//! calls; accepted versions may evolve these source-backed strategies.
use super::{DOMAINS};
use serde_json::{json,Value};
use std::collections::BTreeMap;

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


/// Explore every part of the product, then prefer strategies with measured
/// gains per unit cost. Failed and inconclusive attempts remain in the denominator.
/// The active `selfimprove.search` algorithm decides the ranking rule; the
/// default reproduces the original measured-cost UCB policy.
pub(super) fn search_features(history: &[Value], domain: &str, objective: &str) -> BTreeMap<String, f64> {
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

pub(super) fn score_domain(algorithm: &Value, features: &BTreeMap<String, f64>) -> Option<f64> {
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


// Source template for the native domain-selection mutation. This file is
// compiled in differential tests; production uses it only after a real patch
// inserts it into the descendant's own RSI selector and A/B accepts it.
pub(crate) fn fast_failure_domain(
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
