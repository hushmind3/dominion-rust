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
