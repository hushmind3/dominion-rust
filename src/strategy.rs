//! Evidence-grounded, non-weight strategy transfer across RSI and agent runs.
//! Persisted Run/Improvement records remain the source of truth: no duplicate
//! index can falsely promote a candidate that was never adopted.
use crate::{
    domain::{Run, RunStatus},
};
use serde::Serialize;
use std::{cmp::Ordering, collections::HashSet};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct StrategyEvidence {
    pub source_id: Uuid,
    pub source: String,
    pub repository: String,
    pub goal: String,
    pub principle: String,
    pub when_to_use: String,
    pub failure_mode: String,
    pub files: Vec<String>,
    pub outcome: String,
    pub baseline_score: Option<f64>,
    pub resulting_score: Option<f64>,
    pub attempts: usize,
    pub cost_krw: f64,
    pub updated_at_ms: u64,
}

fn bounded(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn words(text: &str) -> HashSet<String> {
    text.to_lowercase().split(|c:char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| s.chars().count() >= 3 && s.chars().count() <= 36)
        .filter(|s| !matches!(*s,
            "from" | "with" | "have" | "into" | "this" | "that" | "agent" |
            "code" | "improve" | "please" | "source" | "file" | "files" |
            "수정하고" | "개선하고" | "만들어라" | "프로젝트"))
        .take(35).map(str::to_owned).collect()
}

fn relevant(query: &str, candidate: &StrategyEvidence) -> u32 {
    let needed = words(query);
    if needed.is_empty() { return 0; }
    let title = candidate.goal.to_lowercase();
    let method = candidate.principle.to_lowercase();
    let files = candidate.files.join(" ").to_lowercase();
    needed.iter().map(|term| {
        if title.contains(term) {5}
        else if method.contains(term) {3}
        else if files.contains(term) {2}
        else if candidate.when_to_use.to_lowercase().contains(term) {1}
        else {0}
    }).sum()
}

pub(crate) fn from_run(run: &Run) -> Option<StrategyEvidence> {
    if !run.strategy_learning || run.status != RunStatus::Completed { return None; }
    let repository=run.repository.as_ref()?;
    let verified=run.events.iter().any(|e|e.kind=="verification"
        && crate::verification::strict_pass(&e.message));
    let receipts=run.events.iter().filter(|e|e.kind=="tool_result").count();
    if !verified || receipts==0 { return None; }
    let method=run.plan.as_deref().unwrap_or("");
    if method.trim().is_empty() { return None; }
    Some(StrategyEvidence {
        source_id:run.id, source:"agent".into(),repository:repository.clone(),
        goal:bounded(&run.instruction,400),principle:bounded(method,420),
        when_to_use:"Previously completed with independently verified tool receipts".into(),
        failure_mode:String::new(),files:Vec::new(),outcome:"verified".into(),
        baseline_score:None,resulting_score:None,attempts:run.steps,
        cost_krw:run.cost_krw,updated_at_ms:run.updated_at_ms
    })
}

/// Rank the compact proof records, without cloning every historical agent
/// transcript on each model turn. Never combine unrelated repositories.
pub fn rank(
    evidence: Vec<StrategyEvidence>, repository:&str, query:&str, max:usize,
) -> Vec<StrategyEvidence> {
    if repository.trim().is_empty() || repository=="global" || max==0 {return Vec::new();}
    let mut ranked=evidence.into_iter()
        .filter(|e|e.repository==repository)
        .filter_map(|e|{
            let overlap=relevant(query,&e);
            if overlap==0 {None} else {
                let outcome_bonus=match e.outcome.as_str(){
                    "adopted"=>3.0,
                    "verified"=>1.0,
                    _=>-2.0,
                };
                let measured_gain=match (e.baseline_score,e.resulting_score) {
                    (Some(a),Some(b)) if b>a => (b-a).ln_1p().min(3.0),
                    _=>0.0,
                };
                Some((overlap as f64 + outcome_bonus + measured_gain,e))
            }
        }).collect::<Vec<_>>();
    ranked.sort_by(|a,b|b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal)
        .then_with(||b.1.updated_at_ms.cmp(&a.1.updated_at_ms)));
    let mut out=Vec::new();
    let mut warnings=0usize;
    for (_,e) in ranked {
        if e.outcome=="rejected" {
            if warnings>=1 {continue;}
            warnings+=1;
        }
        out.push(e);
        if out.len()>=max.min(6) {break;}
    }
    out
}

/// Bounded lower-trust contextual evidence; never a system instruction.
pub fn guidance(records:&[StrategyEvidence]) -> String {
    records.iter().map(|e|format!(
        "[{}:{}] {}. Observed={} attempts={} score={:?}->{:?}; when={}; caution={}",
        e.source,e.source_id,bounded(&e.principle,250),e.outcome,
        e.attempts,e.baseline_score,e.resulting_score,
        bounded(&e.when_to_use,150),bounded(&e.failure_mode,170)
    )).collect::<Vec<_>>().join("\n").chars().take(1800).collect()
}

