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
    let remaining=gaps_after.iter().map(String::as_str).collect::<HashSet<_>>();
    let cleared_checks=gaps_before.iter().filter(|gap|!remaining.contains(gap.as_str())).count();
    TurnProgress{new_actions,cleared_checks,tool_errors}
}

/// Material progress is derived from the SHA of actual owner-workspace files,
/// not from a new tool-call name, path argument or assistant assertion. Rewriting
/// the same bytes to a different path only counts if it actually created an
/// observed artifact there.
pub(super) fn observed_artifact_changes(before:&[Value],after:&[Value])->usize{
    let hashes=before.iter().filter_map(|row|{
        let path=row["path"].as_str()?;
        let sha=row["sha256"].as_str()?;
        (row["grade"]=="observed_file_bytes"&&sha.len()==64)
            .then(||(path.to_string(),sha.to_string()))
    }).collect::<std::collections::HashMap<_,_>>();
    after.iter().filter(|row|{
        let Some(path)=row["path"].as_str()else{return false;};
        let Some(sha)=row["sha256"].as_str()else{return false;};
        row["grade"]=="observed_file_bytes"&&sha.len()==64
            &&sha.bytes().all(|byte|byte.is_ascii_hexdigit())
            &&hashes.get(path).is_none_or(|previous|previous!=sha)
    }).count()
}

/// Strict coding tasks cannot reset the stall counter by issuing different
/// calls that produce no changed file bytes and clear no acceptance checks.
/// All other goal families retain the existing action-based progress policy.
pub(super) fn materially_advanced(
    turn:TurnProgress,require_tests:bool,artifact_changes:usize
)->bool{
    if require_tests{
        turn.cleared_checks>0 || artifact_changes>0
    }else{
        turn.advanced()
    }
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
    fn strict_coding_progress_requires_real_artifact_change_or_cleared_check(){
        let sha="a".repeat(64);
        let row=json!({"path":"src/main.rs","sha256":sha,"grade":"observed_file_bytes"});
        let baseline=vec![row.clone()];
        let unchanged=vec![row];
        assert_eq!(observed_artifact_changes(&baseline,&unchanged),0);
        let renamed=json!({"path":"src/main.rs","sha256":"b".repeat(64),
            "grade":"observed_file_bytes"});
        assert_eq!(observed_artifact_changes(&baseline,&[renamed]),1);
        let fake=json!({"path":"other.rs","sha256":"c".repeat(64),
            "grade":"model_claim"});
        assert_eq!(observed_artifact_changes(&baseline,&[fake]),0);
        let apparent=TurnProgress{new_actions:3,cleared_checks:0,tool_errors:0};
        assert!(apparent.advanced());
        assert!(!materially_advanced(apparent,true,0));
        assert!(materially_advanced(apparent,true,1));
        let cleared=TurnProgress{new_actions:0,cleared_checks:1,tool_errors:1};
        assert!(materially_advanced(cleared,true,0));
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
