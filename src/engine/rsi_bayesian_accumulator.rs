//! Native fused Bayesian domain statistics used in an independently built
//! descendant only after a real source patch and frozen paired evaluation.
#[derive(Clone, Copy, Default)]
struct DomainStats {
    attempts: u64,
    accepted: u64,
    useful: u64,
    tokens: u64,
}

/// Exactly matches rsi_evolve's original failure-clusters domain score,
/// including unseen-domain exploration, explicit objective and strict ties.
/// Memory and improver history are inspected together in one traversal.
pub(super) fn fast_sub1_bayesian_domain(
    history: &[serde_json::Value],
    candidates: &[&str],
    objective_matches: impl Fn(&str)->bool,
) -> String {
    if candidates.is_empty() {return String::new();}
    let mut stats=vec![DomainStats::default();candidates.len()];
    for row in history {
        let Some(index)=candidates.iter().position(|domain|row["domain"]==*domain)
            else {continue;};
        let current=&mut stats[index];
        current.attempts+=1;
        current.accepted+=u64::from(row["accepted"]==true);
        current.useful+=u64::from(row["accepted"]==true
            ||row["candidateId"].is_string()
            ||row["newFindings"].as_u64().unwrap_or(0)>0);
        current.tokens+=row["tokens"].as_u64().unwrap_or(0);
    }
    let mut best=(f64::NEG_INFINITY,candidates[0]);
    for (domain,s) in candidates.iter().zip(stats.into_iter()) {
        let attempts=s.attempts as f64;
        let accepted=s.accepted as f64;
        let useful=s.useful as f64;
        let cost=if attempts==0.0 {0.0}
            else {(s.tokens as f64/attempts)/10_000.0};
        let posterior=(accepted+0.5)/(attempts+2.0);
        let successful=useful/(attempts+1.0);
        let barren=if attempts==0.0 {0.0}
            else {1.0-useful/attempts};
        let penalty=barren*cost/(1.0+cost);
        let exploration=if attempts==0.0 {3.0}
            else {(2.0*((history.len() as f64)+1.0).ln()/attempts)
                .sqrt().min(3.0)};
        let objective=if objective_matches(domain) {1.0} else {0.0};
        let score=3.0*posterior/(1.0+cost)+0.25*successful
            +exploration-penalty+objective*2.0;
        if score>best.0 {best=(score,domain);}
    }
    best.1.to_string()
}
