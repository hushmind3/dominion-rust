//! Verification is evidence-driven. Model praise is not an execution receipt.
use crate::domain::Run;
use serde_json::Value;

/// A strict answer tag avoids accepting "PASS, probably..." or "PASS but..."
/// in place of explicit verification.
pub fn strict_pass(answer:&str)->bool{
    answer.trim()=="PASS"
}

/// Extract actual command/tool outcomes from the durable timeline, preserving
/// negative receipts. The verifier sees these separately from model narration.
pub fn tool_receipts(run:&Run)->Vec<Value>{
    run.events.iter().filter(|event|matches!(event.kind.as_str(),
        "tool_result"|"tool_error"|"tool_reconciled"|"approval_required"|"cancel_pending_reconciliation"))
        .rev().take(80).rev()
        .map(|event|serde_json::json!({
            "kind":event.kind,"message":event.message,"detail":event.detail,"seq":event.seq
        })).collect()
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::{CreateRun,Run};
    use serde_json::json;
    #[test]
    fn pass_requires_exact_evidence_verdict(){
        assert!(strict_pass("PASS"));
        assert!(strict_pass(" PASS\n"));
        assert!(!strict_pass("PASS, but tests were not run"));
        assert!(!strict_pass("PASS maybe"));
        assert!(!strict_pass("REVISE: tests failed"));
        assert!(!strict_pass("PASS\nREVISE"));
    }
    #[test]
    fn tool_receipts_cannot_be_invented_from_model_text(){
        let mut run=Run::new(CreateRun{instruction:"test".into(),budget_krw:None,max_steps:None},5).unwrap();
        run.messages.push(json!({"role":"assistant","content":"I passed all tests"}));
        assert!(tool_receipts(&run).is_empty());
        run.event("tool_error","shell: failed",Some(json!({"exit":1})));
        let receipts=tool_receipts(&run);
        assert_eq!(receipts.len(),1);
        assert_eq!(receipts[0]["kind"],"tool_error");
    }
}
