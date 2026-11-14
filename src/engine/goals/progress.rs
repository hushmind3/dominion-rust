//! Deterministic, non-model advancement signal for multi-step autonomous goals.
//! Replanning happens after successive turns WITHOUT actual progress, not
//! simply every N iterations even when files/tests are improving.
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
    pub fn advanced(self)->bool{
        self.cleared_checks>0 || (self.new_actions>0&&self.tool_errors==0)
    }
}
fn signatures(receipts:&[Value])->HashSet<String>{
    receipts.iter().filter(|receipt|verification::is_action_evidence(receipt))
        .filter_map(|receipt|{
            let args=receipt.get("arguments").or_else(||receipt.get("args"))?;
            Some(format!("{}:{args}",verification::tool_name(receipt)))
        }).collect()
}

/// A new side effect on an unrelated path is not evidence that the user's
/// requested artifact improved. Exact criteria are more authoritative than
/// "a tool returned success". Generic goals without predicates keep the
/// previous action novelty heuristic.
fn relevant_to_checks(receipt:&Value,checks:&Value)->bool{
    let Some(items)=checks.as_array() else {return true;};
    if items.is_empty(){return true;}
    let tool=verification::tool_name(receipt);
    let args=receipt.get("arguments").or_else(||receipt.get("args"))
        .unwrap_or(&Value::Null);
    items.iter().any(|check|{
        match check["type"].as_str().unwrap_or("") {
            "file_exists"|"file_absent"|"file_changed"|"file_equals"
            |"file_contains"|"file_replace_exact" => {
                let expected=check["path"].as_str().unwrap_or("").replace('\\',"/");
                let actual=args["path"].as_str().unwrap_or("").replace('\\',"/");
                !expected.is_empty() && expected==actual
                    && matches!(tool,"workspace_write"|"file_write"|
                        "workspace_replace"|"workspace_replace_range"|
                        "workspace_replace_function")
            }
            "command_succeeded" => verification::command_matches(
                receipt,check["command"].as_str().unwrap_or("")
            ),
            "tool_succeeded" => tool==check["tool"].as_str().unwrap_or(""),
            _=>false,
        }
    })
}

/// Objective-grounded progress for the live goal controller. Unrelated
/// successful writes remain in evidence, but do not reset its stall/replan
/// counter or justify a paid reviewer call.
pub(super) fn measure_objective(
    before:&[Value],after:&[Value],gaps_before:&[String],gaps_after:&[String],
    tool_errors:usize,checks:&Value,
)->TurnProgress{
    let original=signatures(before);
    let new=signatures(&after.iter()
        .filter(|receipt|relevant_to_checks(receipt,checks))
        .cloned().collect::<Vec<_>>());
    let remaining=gaps_after.iter().map(String::as_str).collect::<HashSet<_>>();
    TurnProgress{
        new_actions:new.difference(&original).count(),
        cleared_checks:gaps_before.iter().filter(|v|!remaining.contains(v.as_str())).count(),
        tool_errors,
    }
}

/// Compare independent acceptance checks from BEFORE and AFTER this tool
/// iteration; a model's self-reported success is not a measurable advance.
pub(super) fn measure(
    before:&[Value],after:&[Value],gaps_before:&[String],gaps_after:&[String],
    tool_errors:usize
)->TurnProgress{
    let original=signatures(before);
    let now=signatures(after);
    let remaining=gaps_after.iter().map(String::as_str).collect::<HashSet<_>>();
    TurnProgress{
        new_actions:now.difference(&original).count(),
        cleared_checks:gaps_before.iter().filter(|v|!remaining.contains(v.as_str())).count(),
        tool_errors,
    }
}
/// An independent reviewer cannot turn unchanged disk state or missing
/// tool receipts into a completed action. Save the model call for progress,
/// testable completion, or an actual new operation that needs interpretation.
pub(super) fn reviewer_needed(gaps:&[String],progress:TurnProgress)->bool{
    gaps.is_empty() || progress.new_actions>0 || progress.cleared_checks>0
}
#[cfg(test)]
mod tests{
    use super::*;use serde_json::json;
    fn write(path:&str)->Value{
        json!({"tool":"workspace_write","arguments":{"path":path,"content":"source"},
            "result":{"written":true},"runId":"r1","status":"completed","opSeq":1})
    }
    #[test]
    fn unrelated_successful_writes_do_not_hide_a_stalled_objective() {
        let criteria=serde_json::json!([{"type":"file_contains",
            "path":"src/expected.rs","contains":"implemented"}]);
        let unrelated=write("examples/unrelated.txt");
        let relevant=write("src/expected.rs");
        let unrelated_progress=measure_objective(&[],&[unrelated],&["missing".into()],
            &["missing".into()],0,&criteria);
        assert_eq!(unrelated_progress.new_actions,0);
        assert!(!unrelated_progress.advanced());
        assert!(!reviewer_needed(&["missing".into()],unrelated_progress));
        let advanced=measure_objective(&[],&[relevant],&["missing".into()],
            &["missing".into()],0,&criteria);
        assert!(advanced.advanced());
        // A generic goal without machine-checkable paths still counts tools.
        assert!(measure_objective(&[],&[write("something.txt")],
            &[],&[],0,&serde_json::json!([])).advanced());
    }
    #[test]
    fn command_and_tool_acceptance_require_matching_action_receipts() {
        let shell=serde_json::json!({"tool":"shell_exec",
            "arguments":{"command":"cargo test"},
            "result":{"exitCode":0},"status":"completed"});
        let expected=serde_json::json!([{"type":"command_succeeded","command":"cargo test"}]);
        assert_eq!(measure_objective(&[],&[shell.clone()],&[],&[],0,&expected).new_actions,1);
        assert_eq!(measure_objective(&[],&[shell],&[],&[],0,
            &serde_json::json!([{"type":"command_succeeded","command":"cargo clippy"}])).new_actions,0);
    }
    #[test]
    fn progress_requires_new_successful_action_or_cleared_check(){
        let one=write("a.rs");
        let next=write("b.rs");
        assert!(measure(&[one.clone()],&[one.clone(),next],&[],&[],0).advanced());
        assert!(!measure(&[one.clone()],&[one.clone(),one.clone()],&[],&[],0).advanced());
        assert!(!measure(&[],&[one],&[],&[],1).advanced());
        assert!(measure(&[],&[],&["missing A".into(),"missing B".into()],
            &["missing B".into()],1).advanced());
    }
    #[test]
    fn unchanged_objective_state_skips_only_wasteful_model_review(){
        let unchanged=TurnProgress{new_actions:0,cleared_checks:0,tool_errors:1};
        let changed=TurnProgress{new_actions:1,cleared_checks:0,tool_errors:0};
        assert!(!reviewer_needed(&["file not modified".into()],unchanged));
        assert!(reviewer_needed(&["file not modified".into()],changed));
        assert!(reviewer_needed(&[],unchanged),
            "A passing deterministic gate may still need independent model review");
    }
    #[test]
    fn failed_tools_and_text_claims_do_not_reset_stalls(){
        let bad=json!({"tool":"workspace_write","arguments":{"path":"a.rs"},
            "result":{"success":false},"status":"failed"});
        let p=measure(&[],&[bad],&["missing".into()],&["missing".into()],1);
        assert_eq!(p.new_actions,0);assert_eq!(p.cleared_checks,0);
        assert!(!p.advanced());
    }
}
