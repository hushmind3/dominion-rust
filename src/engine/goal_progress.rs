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
    advanced_with_policy(before,after,prior_checks,current_checks,false)
}

/// For an action goal, another successful search/read is useful context but
/// not evidence that the goal moved closer to completion. Only a proven
/// action or an independently closed acceptance gap resets the stall counter.
/// Research-only goals still advance through genuinely novel observations.
pub(crate) fn advanced_with_policy(before:&[Value],after:&[Value],
    prior_checks:&[String],current_checks:&[String],require_action:bool)->bool {
    let open_before:HashSet<&str>=prior_checks.iter().map(String::as_str).collect();
    let open_after:HashSet<&str>=current_checks.iter().map(String::as_str).collect();
    if open_before.difference(&open_after).next().is_some(){return true;}
    let seen:HashSet<String>=before.iter().map(fingerprint).collect();
    after.iter().any(|event|!seen.contains(&fingerprint(event))
        && crate::engine::verification::successful_evidence(event)
        && (!require_action || crate::engine::verification::is_action_evidence(event)))
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
    #[test]
    fn read_only_churn_does_not_prevent_action_replanning() {
        let first=json!({"tool":"code_search","arguments":{"query":"A"},
            "result":{"matches":["src/a.rs"]}});
        let second=json!({"tool":"code_search","arguments":{"query":"B"},
            "result":{"matches":["src/b.rs"]}});
        assert!(!advanced_with_policy(&[first.clone()],&[first.clone(),second.clone()],
            &["file_changed:src/a.rs".into()],&["file_changed:src/a.rs".into()],true));
        // A research task may count a genuinely new source observation.
        assert!(advanced_with_policy(&[first.clone()],&[first,second],&[],&[],false));
    }

    #[test]
    fn verified_action_or_closed_gap_is_progress() {
        let before=json!({"tool":"workspace_read","result":{"content":"old"}});
        let edit=json!({"tool":"workspace_replace_range",
            "arguments":{"path":"src/a.rs"},"result":{"written":true}});
        assert!(advanced_with_policy(&[before.clone()],&[before,edit],
            &["file_changed:src/a.rs".into()],&["file_changed:src/a.rs".into()],true));
        assert!(advanced_with_policy(&[],&[],&["file_changed:src/a.rs".into()],
            &[],true));
    }

    #[test]
    fn failed_or_unknown_action_is_never_progress() {
        let failed=json!({"tool":"shell_exec",
            "arguments":{"command":"make"},
            "result":{"exitCode":1}});
        assert!(!advanced_with_policy(&[],&[failed],&[],&[],true));
        let unknown=json!({"tool":"workspace_write",
            "result":{"error":"permission denied"}});
        assert!(!advanced_with_policy(&[],&[unknown],&[],&[],true));
    }

}
