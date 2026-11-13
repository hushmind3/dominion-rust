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
