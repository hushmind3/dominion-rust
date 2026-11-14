//! Compiled, bounded economy policy for room turns.
//! Algorithm store selects the versioned parameters; these functions execute
//! those parameters from the immutable roomSnapshot attached to a durable job.
use serde_json::Value;

const MAX_SPEAKERS: usize = 32;

pub(super) fn active(room:&Value) -> bool {
    room["economyMode"]=="adaptive" && room["routingMode"]=="discussion"
}

pub(super) fn all_requested(input:&str)->bool {
    let text=input.to_lowercase();
    ["다들","다 같이","다같이","모두","모든","전원","전부",
     "각자","나머지","나머지도","아홉","9명","인사",
     "everyone","all agents"].iter().any(|word|text.contains(word))
}

fn parameter(room:&Value,key:&str,fallback:usize)->usize {
    room["economyKernel"]["parameters"][key].as_u64()
        .map(|value| value.clamp(1,MAX_SPEAKERS as u64) as usize)
        .unwrap_or(fallback)
}

pub(super) fn target_speakers(room:&Value,deep:bool,chars:usize)->usize {
    let (field,default)=if deep {("deepSpeakers",3)}
        else if chars<=60 {("casualSpeakers",1)}
        else {("mediumSpeakers",2)};
    parameter(room,field,default)
}

/// A room exchange's metering origin is immutable across safe job resumes.
/// Without this persisted baseline, a crash/requeue can grant a fresh full
/// budget to the same discussion. Zero means no explicit token ceiling.
pub(super) fn metering_baseline(input:&Value, observed_total:u64)->u64 {
    input["usageBaselineTokens"].as_u64().unwrap_or(observed_total)
}

pub(super) fn budget_spent(token_budget:u64,baseline:u64,observed_total:u64)->bool {
    token_budget>0 && observed_total.saturating_sub(baseline)>=token_budget
}

/// Subtract the immutable first-call receipt from current session totals.
/// Missing baseline fields remain null rather than being advertised as zero.
/// Cache totals are provider-reported evidence, not an actual KRW invoice.
pub(super) fn observed_delta(baseline:&Value,current:&Value)->Value {
    let mut out=serde_json::Map::new();
    for key in [
        "calls", "promptTokens", "completionTokens", "totalTokens",
        "estimatedCalls", "cachedPromptTokens", "cacheEligiblePromptTokens",
        "cacheReportedCalls",
    ] {
        let delta=baseline[key].as_u64().and_then(|start|{
            current[key].as_u64().map(|observed|observed.saturating_sub(start))
        });
        out.insert(key.into(),delta.map(|value|serde_json::json!(value)).unwrap_or(Value::Null));
    }
    out.insert("scope".into(),Value::String("one-room-exchange".into()));
    out.insert("cacheCoverage".into(),Value::String("reported-only".into()));
    Value::Object(out)
}

/// The owner's explicit roster/round request, @mentions and "all" messages
/// must not be silently turned into a cheap one-speaker exchange. The mode
/// is intentionally opt-in per room with new rooms defaulting to adaptive.
pub(super) fn effective_rounds(
    room:&Value, input:&str, owner_selected_speakers:bool,
    requested_rounds:usize, deep:bool,
)->usize {
    if !active(room)||owner_selected_speakers||all_requested(input) {
        return requested_rounds;
    }
    if !deep {return 1;}
    parameter(room,"deepRounds",2).min(requested_rounds)
}

#[cfg(test)]
#[path = "../../../tests/unit/engine_rooms_economy_tests.rs"]
mod tests;
