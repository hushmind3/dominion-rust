//! Rust-owned progress decisions for autonomous goal turns.
//! A reviewer/assistant's natural-language claim is never evidence.
//! Only newly observed successful tool actions or cleared deterministic
//! acceptance gaps reset the stalled-turn counter.
use crate::engine::verification;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) struct TurnProgress {
    pub new_actions:usize,
    pub cleared_checks:usize,
    pub tool_errors:usize,
}
impl TurnProgress {
    pub fn advanced(self)->bool {
        self.cleared_checks>0 || (self.new_actions>0 && self.tool_errors==0)
    }
}

fn action_keys(events:&[Value])->HashSet<String>{
    events.iter().filter(|event|verification::is_action_evidence(event))
        .filter_map(|event|{
            // An argument-free success is not a reproducible change receipt.
            // Result text alone (often assistant prose) never counts.
            let args=event.get("arguments").or_else(||event.get("args"))?;
            Some(format!("{}:{}",verification::tool_name(event),args))
        }).collect()
}

pub(super) fn measure(
    before:&[Value],after:&[Value],
    gaps_before:&[String],gaps_after:&[String],
    tool_errors:usize,
)->TurnProgress {
    let prior=action_keys(before);
    let current=action_keys(after);
    let new_actions=current.difference(&prior).count();
    let remaining=gaps_after.iter().map(String::as_str).collect::<HashSet<_>>();
    let cleared_checks=gaps_before.iter().map(String::as_str).collect::<HashSet<_>>()
        .difference(&remaining).count();
    TurnProgress{new_actions,cleared_checks,tool_errors}
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn actual_write(path:&str)->Value {
        json!({"tool":"workspace_write",
            "arguments":{"path":path,"content":"payload"},
            "result":{"written":true,"success":true}})
    }
    #[test]
    fn new_real_write_is_progress_but_identical_repeat_is_not() {
        let a=actual_write("a.txt");
        let b=actual_write("b.txt");
        let advanced=measure(&[a.clone()],&[a.clone(),b],&[],&[],0);
        assert_eq!(advanced.new_actions,1);
        assert!(advanced.advanced());
        let repeated=measure(&[a.clone()],&[a.clone(),a],&[],&[],0);
        assert!(!repeated.advanced());
    }
    #[test]
    fn read_only_failed_writes_and_prose_cannot_reset_stalls() {
        let read=json!({"tool":"workspace_read","arguments":{"path":"a.txt"},
            "result":{"success":true,"content":"hello"}});
        let lied=json!({"tool":"workspace_write","arguments":{"path":"a.txt"},
            "result":{"written":false,"success":true}});
        let failed=json!({"tool":"workspace_write","arguments":{"path":"a.txt"},
            "result":{"written":true,"success":false}});
        let outcome=measure(&[],&[read,lied,failed],&[],&[],1);
        assert_eq!(outcome.new_actions,0);
        assert!(!outcome.advanced());
    }
    #[test]
    fn tool_error_prevents_receipt_only_progress_but_acceptance_is_authoritative() {
        let receipt=actual_write("a.txt");
        let mixed=measure(&[],&[receipt],&[],&[],1);
        assert_eq!(mixed.new_actions,1);
        assert!(!mixed.advanced());
        let verified=measure(&[],&[],&["file missing".into(),"no tool".into()],
            &["no tool".into()],1);
        assert_eq!(verified.cleared_checks,1);
        assert!(verified.advanced());
    }
}
