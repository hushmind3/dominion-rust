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
#[cfg(test)]
mod tests{
    use super::*;use serde_json::json;
    fn write(path:&str)->Value{
        json!({"tool":"workspace_write","arguments":{"path":path,"content":"source"},
            "result":{"written":true},"runId":"r1","status":"completed","opSeq":1})
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
    fn failed_tools_and_text_claims_do_not_reset_stalls(){
        let bad=json!({"tool":"workspace_write","arguments":{"path":"a.rs"},
            "result":{"success":false},"status":"failed"});
        let p=measure(&[],&[bad],&["missing".into()],&["missing".into()],1);
        assert_eq!(p.new_actions,0);assert_eq!(p.cleared_checks,0);
        assert!(!p.advanced());
    }
}
