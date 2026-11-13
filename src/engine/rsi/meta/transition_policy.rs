//! Typed, bounded experiment-policy predicates owned by the editable RSI
//! meta engine. An adopted descendant may rewrite this predicate vocabulary,
//! thresholds, and selection algorithm without changing the frozen evaluator.
//! The compiler consumes measured episode fields; it never assigns success.
use serde::{Deserialize,Serialize};
use serde_json::Value;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="snake_case")]
enum Metric {
    ImprovedHoldouts,
    MetaHoldoutGains,
    Regressions,
    MeasuredHoldouts,
    MethodControlWins,
    MethodControlLosses,
    ActualToolFailures,
    ModelTokens,
}
#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="snake_case")]
enum Comparison { GreaterThan, AtLeast, LessThan, AtMost, Equal }

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Clause {
    metric:Metric,
    comparison:Comparison,
    threshold:f64,
}
/// A model-authored combination of independent, replayable observations.
/// No fixed generation count or A->B->A->B stage list is embedded here.
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Guard {
    mode:GuardMode,
    clauses:Vec<Clause>,
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="snake_case")]
enum GuardMode { All, Any }

pub(crate) fn valid(guard:&Guard)->bool {
    (1..=8).contains(&guard.clauses.len())
        && guard.clauses.iter().all(|c|
            c.threshold.is_finite() && c.threshold>=0.0
                && c.threshold<=2_000_000.0)
}
fn value(g:&Value,metric:&Metric)->Option<f64> {
    let raw=match metric {
        Metric::ImprovedHoldouts=>&g["evaluation"]["metrics"]["improvedHoldouts"],
        Metric::MetaHoldoutGains=>&g["evaluation"]["metrics"]["metaHoldoutGains"],
        Metric::Regressions=>&g["evaluation"]["metrics"]["regressions"],
        Metric::MeasuredHoldouts=>&g["evaluation"]["metrics"]["measuredHoldouts"],
        Metric::MethodControlWins=>&g["methodAblation"]["treatmentOnlyWins"],
        Metric::MethodControlLosses=>&g["methodAblation"]["controlOnlyWins"],
        Metric::ActualToolFailures=>&g["toolUsage"]["failed"],
        Metric::ModelTokens=>&g["tokenUsage"]["totalTokens"],
    };
    raw.as_u64().map(|n|n as f64)
        .or_else(||raw.as_f64().filter(|n|n.is_finite() && *n>=0.0))
}
/// Predicates only act on independently measured generations. A missing
/// metric is NOT equivalent to zero; it cannot make a `less_than` guard true.
pub(crate) fn evaluate(guard:&Guard,g:&Value)->bool {
    if !valid(guard)
        || (g["pairedRevalidated"]!=true && g["pairedMeasured"]!=true)
        || !g["evaluation"]["metrics"].is_object()
        || g["candidateId"].as_str().is_none_or(str::is_empty) {
        return false;
    }
    let match_clause=|c:&Clause| {
        let Some(observed)=value(g,&c.metric) else{return false;};
        match c.comparison {
            Comparison::GreaterThan=>observed>c.threshold,
            Comparison::AtLeast=>observed>=c.threshold,
            Comparison::LessThan=>observed<c.threshold,
            Comparison::AtMost=>observed<=c.threshold,
            Comparison::Equal=>(observed-c.threshold).abs()<1e-9,
        }
    };
    match &guard.mode {
        GuardMode::All=>guard.clauses.iter().all(match_clause),
        GuardMode::Any=>guard.clauses.iter().any(match_clause),
    }
}
