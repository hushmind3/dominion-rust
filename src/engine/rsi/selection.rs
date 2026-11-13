//! Measured search algorithm and capability selection.
//! Compiled RSI child module; external APIs are re-exported by rsi_evolve.
use super::*;

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
    let useful=rows.iter().filter(|row|row["accepted"]==true ||
        row["candidateId"].is_string() || row["newFindings"].as_u64().unwrap_or(0)>0)
        .count() as f64;
    let barren_rate=if attempts==0.0 {0.0}else{1.0-useful/attempts};
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
        ("usefulGenerations".to_string(),useful),
        ("barrenRate".to_string(),barren_rate),
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
            // Posterior success probability remains defined with no trials.
            // Expensive, barren research lowers priority; real accepted code
            // changes matter more than merely producing new text findings.
            // Domain ranking can still be replaced by a model-authored formula.
            let tries=get("attempts");
            let posterior=(get("accepted")+0.5)/(tries+2.0);
            let useful=get("usefulGenerations")/(tries+1.0);
            let cost=get("tokenCost").max(0.0);
            let penalty=get("barrenRate")*cost/(1.0+cost);
            Some(3.0*posterior/(1.0+cost)+0.25*useful
                +get("exploration")-penalty
                +get("objectiveMatch")*2.0)
        }
    }
}

pub(super) fn eligible_domains(objective: &str) -> Vec<&'static str> {
    // An explicit owner objective is a constraint; a failed candidate formula
    // must not silently send a tools goal to the first declared domain.
    let matching=DOMAINS.iter().copied().filter(|domain|objective_matches(domain,objective))
        .collect::<Vec<_>>();
    if matching.is_empty() {DOMAINS.to_vec()} else {matching}
}

pub(super) fn valid_domain_scores(algorithm:&Value,history:&[Value],objective:&str)
    ->Option<Vec<(&'static str,f64)>> {
    eligible_domains(objective).into_iter().map(|domain|{
        let features=search_features(history,domain,objective);
        score_domain(algorithm,&features).filter(|score|score.is_finite())
            .map(|score|(domain,score))
    }).collect()
}

pub(crate) fn select_domain(algorithm: &Value, history: &[Value], objective: &str) -> String {
    let scores=valid_domain_scores(algorithm,history,objective)
        .or_else(||valid_domain_scores(&json!({"implementation":"failure-clusters"}),
            history,objective)).unwrap_or_default();
    // Strict comparison preserves declared domain order as a stable tie break.
    let mut best=(f64::NEG_INFINITY,eligible_domains(objective)[0]);
    for (domain,score) in scores {
        if score>best.0 {best=(score,domain);}
    }
    best.1.to_string()
}

pub(super) fn search_strategy(algorithm: &Value, mode: &str) -> Value {
    json!({
        "id": algorithm["id"],
        "revision": algorithm["revision"],
        "name": algorithm["name"],
        "implementation": algorithm["implementation"],
        "mode": mode,
    })
}

pub(super) fn select_domain_for_state(state: &AppState, history: &[Value], objective: &str) -> String {
    let algorithm = crate::engine::algorithms::active(state, "selfimprove.search")
        .unwrap_or_else(|| json!({"implementation":"failure-clusters","parameters":{}}));
    select_domain(&algorithm, history, objective)
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

