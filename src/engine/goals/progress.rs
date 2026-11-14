//! Deterministic per-turn progress for autonomous goals.
//! A model's self-reported progress is never a stall-reset signal.
//! Only cleared Rust acceptance gaps or NEW successful effect receipts count.
use crate::engine::verification;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TurnProgress {
    pub new_actions: usize,
    pub cleared_checks: usize,
    pub tool_errors: usize,
}
impl TurnProgress {
    pub fn advanced(self)->bool{
        self.cleared_checks>0 || (self.new_actions>0 && self.tool_errors==0)
    }
}
fn action_signatures(events:&[Value])->HashSet<String>{
    events.iter().filter(|event|verification::is_action_evidence(event))
        .filter_map(|event|{
            let args=event.get("arguments").or_else(||event.get("args"))?;
            Some(format!("{}:{}",verification::tool_name(event),args))
        }).collect()
}

/// The parent harness owns the signal; a claimed success in assistant text
/// or a second identical command does not advance the goal. Independent
/// acceptance checks can still demonstrate progress by shrinking the gap set.
pub(super) fn measure(
    evidence_before:&[Value],evidence_after:&[Value],
    gaps_before:&[String],gaps_after:&[String],
    tool_errors:usize,
)->TurnProgress{
    let before=action_signatures(evidence_before);
    let after=action_signatures(evidence_after);
    let new_actions=after.difference(&before).count();
    let remaining=gaps_after.iter().collect::<HashSet<_>>();
    let cleared_checks=gaps_before.iter().filter(|gap|!remaining.contains(gap)).count();
    TurnProgress{new_actions,cleared_checks,tool_errors}
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn write(path:&str)->Value{
        json!({"tool":"workspace_write","arguments":{"path":path,"content":"payload"},
            "result":{"success":true}})
    }
    #[test]
    fn fresh_successful_write_resets_stall_but_repeat_does_not(){
        let first=write("a.txt");
        let before=vec![first.clone()];
        let second=write("b.txt");
        let advanced=measure(&before,&[first.clone(),second],&[],&[],0);
        assert_eq!(advanced.new_actions,1);
        assert!(advanced.advanced());
        let repeated=measure(&before,&[first.clone(),first],&[],&[],0);
        assert_eq!(repeated.new_actions,0);
        assert!(!repeated.advanced());
    }
    #[test]
    fn failed_tools_and_assistant_claims_are_not_progress(){
        let attempted=json!({"tool":"workspace_write","arguments":{"path":"a.txt"},
            "result":{"isError":true,"success":false}});
        let status=measure(&[],&[attempted],&["missing".into()],&["missing".into()],1);
        assert!(!status.advanced());
        let mixed=measure(&[],&[write("a.txt")],&[],&[],1);
        assert_eq!(mixed.new_actions,1);
        assert!(!mixed.advanced());
    }
    #[test]
    fn cleared_deterministic_predicates_override_noisy_reviewer_and_tools(){
        let old=vec!["file test missing".to_string(),"empty result".into()];
        let remaining=vec!["empty result".to_string()];
        let result=measure(&[],&[],&old,&remaining,1);
        assert_eq!(result.cleared_checks,1);
        assert!(result.advanced());
    }
}
