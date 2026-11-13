//! Opt-in, evidence-conservative sparse debate.
//! Stops only literal near-duplicate contributions, never guesses semantic consensus.
//! The full participant roster and manual selection remain authoritative.
use serde_json::Value;
use std::collections::BTreeSet;

fn grams(value: &str) -> BTreeSet<String> {
    let clean: Vec<char> = value.to_lowercase().chars()
        .filter(|ch| ch.is_alphanumeric()).collect();
    clean.windows(3).map(|window|window.iter().collect()).collect()
}

fn redundant(a: &str,b: &str) -> bool {
    if a.chars().count() < 70 || b.chars().count() < 70 {return false;}
    let left=grams(a);
    let right=grams(b);
    let intersection=left.intersection(&right).count();
    let union=left.union(&right).count();
    union >= 40 && intersection * 100 >= union * 82
}

pub(super) fn should_finish(room: &Value, owner_text: &str,
    explicit_speakers: bool, outputs: &[Value], conflict: &Value) -> bool {
    if room["sparseDebate"] != true || room["adaptiveSpeakers"] != true
        || explicit_speakers || room["maxTurns"].as_u64().unwrap_or(0)>0
        || outputs.len()<2 || conflict["conflict"]==true
    {return false;}
    let request=owner_text.to_lowercase();
    if ["모두","전원","전부","각자","다들","everyone","all agents"].iter()
        .any(|marker|request.contains(marker))
        || owner_text.contains('@')
    {return false;}
    let a=&outputs[outputs.len()-2];
    let b=&outputs[outputs.len()-1];
    if a["agentId"]==b["agentId"]
        || a["toolCalls"].as_array().is_some_and(|items|!items.is_empty())
        || b["toolCalls"].as_array().is_some_and(|items|!items.is_empty())
    {return false;}
    redundant(a["content"].as_str().unwrap_or(""),
        b["content"].as_str().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn never_ends_explicit_or_disagreeing_rooms() {
        let text="An extensively detailed conclusion with reasons, tradeoffs and an actionable explanation for the current request.";
        let data=vec![json!({"agentId":"a","content":text}),json!({"agentId":"b","content":text})];
        let room=json!({"sparseDebate":true,"adaptiveSpeakers":true,"maxTurns":0});
        assert!(!should_finish(&room,"모두 설명",false,&data,&json!({"conflict":false})));
        assert!(!should_finish(&room,"설명",true,&data,&json!({"conflict":false})));
        assert!(!should_finish(&room,"설명",false,&data,&json!({"conflict":true})));
        assert!(should_finish(&room,"설명",false,&data,&json!({"conflict":false})));
    }
}
