//! Evidence-linked progress for durable autonomous goals.
//! Repeated tool results or a model claiming "progress" are not new evidence.
//! Only a previously unseen successful tool observation or a closed
//! deterministic acceptance gap resets the bounded replanning stall counter.
use serde_json::{json,Value};
use std::collections::HashSet;

fn fingerprint(event:&Value)->String {
    // Call IDs and timestamps outside the result are transport identity.
    // They must not turn the same observation into false progress.
    json!({
        "tool":event["tool"],
        "arguments":event["arguments"],
        "result":event["result"]
    }).to_string()
}

pub(crate) fn advanced(before:&[Value],after:&[Value],
    prior_checks:&[String],current_checks:&[String])->bool {
    let open_before:HashSet<&str>=prior_checks.iter().map(String::as_str).collect();
    let open_after:HashSet<&str>=current_checks.iter().map(String::as_str).collect();
    if open_before.difference(&open_after).next().is_some(){return true;}
    let seen:HashSet<String>=before.iter().map(fingerprint).collect();
    after.iter().any(|event|!seen.contains(&fingerprint(event)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn repetition_does_not_reset_failure_detection(){
        let one=json!({"tool":"code_search","callId":"a",
            "arguments":{"query":"needle"},"result":{"paths":["src/main.rs"]}});
        let mut same=one.clone();
        same["callId"]=json!("b");
        assert!(!advanced(&[one.clone()],&[one,same],&[],&[]));
    }
    #[test]
    fn new_observation_or_dispatched_check_is_progress(){
        let one=json!({"tool":"code_search","result":{"paths":["a.rs"]}});
        let two=json!({"tool":"code_search","result":{"paths":["b.rs"]}});
        assert!(advanced(&[one.clone()],&[one,two],&[],&[]));
        assert!(advanced(&[],&[],&["file_changed:a".into()],&[]));
    }
}
