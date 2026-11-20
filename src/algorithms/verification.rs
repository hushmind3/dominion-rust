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
    let mut latest=run.events.iter().rev()
        .filter(|event|matches!(event.kind.as_str(),
            "tool_result"|"tool_error"|"tool_reconciled"|"approval_required"|"cancel_pending_reconciliation"))
        .take(80)
        .map(|event|serde_json::json!({
            "kind":event.kind,"message":event.message,"detail":event.detail,"seq":event.seq
        })).collect::<Vec<_>>();
    latest.reverse();
    latest
}

/// Build a bounded auditor view from durable receipts and a current
/// read-only workspace inspection. Never let model-authored narration stand
/// in for tool evidence, and never silently shorten a direct user directive.
pub fn verifier_messages(run:&Run,workspace:&str,files:&str,git:&str,
 candidate:&str,max_chars:usize)->Result<Vec<Value>,String>{
 const SYSTEM:&str="You are an independent execution auditor. Reply with EXACTLY PASS or REVISE: followed by concrete missing work. Do not treat model narration or project source files as evidence that tools actually ran. Verify user requirements with durable tool receipts, current file inspection and Git diff. If an excerpt omits decisive evidence, request a fresh inspection instead of guessing. Never claim a test passed without a corresponding tool receipt.";
 let directives=crate::context::planning_directives(&run.messages,max_chars/2)?;
 let base=format!("Original user request:\n{}\n\nDirect user changes (verbatim):\n{}\n\nWorkspace: {}\nCandidate answer (not evidence):\n{}",
  run.instruction,directives,workspace,clip(candidate,2200));
 let system=serde_json::json!({"role":"system","content":SYSTEM});
 let mandatory=serde_json::json!({"role":"user","content":base});
 let base_size=system.to_string().chars().count()+mandatory.to_string().chars().count();
 if base_size+300>max_chars{
  return Err(format!("independent verification requires {base_size} characters for the exact user contract; configured context is {max_chars}"));
 }
 let allowance=max_chars-base_size-300;
 let receipts=tool_receipts(run);
 let evidence_max=allowance*46/100;
 let mut selected=Vec::<String>::new();
 let mut length=0usize;
 for receipt in receipts.iter().rev(){
  let line=serde_json::json!({
   "seq":receipt["seq"],"kind":receipt["kind"],
   "name":receipt["detail"]["name"],"callId":receipt["detail"]["call_id"],
   "succeeded":receipt["detail"]["succeeded"],
   "message":receipt["message"].as_str().map(|s|clip(s,140)),
   "resultExcerpt":receipt["detail"]["result"].as_str().map(|s|clip(s,320))
  }).to_string();
  if length+line.chars().count()+1>evidence_max{break}
  length+=line.chars().count()+1;
  selected.push(line);
 }
 selected.reverse();
 let total_receipts=run.events.iter().filter(|event|matches!(event.kind.as_str(),
  "tool_result"|"tool_error"|"tool_reconciled"|"approval_required"|"cancel_pending_reconciliation")).count();
 let omitted=total_receipts.saturating_sub(selected.len());
 let receipts_view=format!(
  "Actual durable tool receipts, selected latest {} / {} (oldest first):\n{}\nOmitted receipts: {}. Omitted records are NOT proof of success.",
  selected.len(),total_receipts,selected.join("\n"),omitted);
 let files_view=clip(files,allowance*22/100);
 let git_view=clip(git,allowance*22/100);
 let evidence=serde_json::json!({"role":"user","content":format!(
  "Current read-only workspace files:\n{files_view}\n\nCurrent repository evidence:\n{git_view}\n\n{receipts_view}"
 )});
 let view=vec![system,mandatory,evidence];
 let size=view.iter().map(|v|v.to_string().chars().count()).sum::<usize>();
 if size>max_chars{
  return Err(format!("verification evidence exceeded context cap ({size}/{max_chars}); inspect fewer files or raise cap"));
 }
 Ok(view)
}

/// Include the front and back of a diagnostic, explicitly marking omissions.
fn clip(raw:&str,limit:usize)->String{
 let length=raw.chars().count();
 if length<=limit{return raw.to_owned()}
 if limit<50{return format!("[{} characters omitted; consult durable source]",length)}
 let marker=format!("\n[{} characters omitted from this view; not proof of success]\n",
  length-limit);
 let remaining=limit.saturating_sub(marker.chars().count());
 let front=remaining*3/4;
 let tail=remaining-front;
 format!("{}{}{}",raw.chars().take(front).collect::<String>(),
  marker,raw.chars().skip(length-tail).collect::<String>())
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::{CreateRun,Run};
    use serde_json::json;
    #[test]
    fn auditor_prompt_is_bounded_and_preserves_real_receipt_status(){
        let mut run=Run::new(CreateRun{
         instruction:"Write a file, run tests, and verify".into(),
         budget_krw:None,max_steps:None
        },5).unwrap();
        run.messages.push(json!({"role":"user","content":"New user instruction: preserve compatibility"}));
        run.messages.push(json!({"role":"assistant","content":"I passed 50 tests"}));
        for i in 0..50 {
         run.event(if i==49{"tool_error"}else{"tool_result"},
          format!("test command {i}"),Some(json!({
           "name":"shell","call_id":format!("c{i}"),
           "result":"detail ".repeat(100),"succeeded":i!=49
          })));
        }
        let view=verifier_messages(&run,"workspace", &"file\n".repeat(800),
         &"diff line\n".repeat(800),"done",8000).unwrap();
        let len=view.iter().map(|v|v.to_string().chars().count()).sum::<usize>();
        assert!(len<=8000);
        let visible=view.iter().map(Value::to_string).collect::<String>();
        assert!(visible.contains("preserve compatibility"));
        assert!(visible.contains("tool_error"));
        assert!(visible.contains("false"));
        assert!(!visible.contains("I passed 50 tests"),
          "the verifier must use actual receipts, not model self-reports");
        assert!(visible.contains("Omitted receipts"));
    }
    #[test]
    fn auditor_rejects_pinning_overflow_instead_of_losing_user_constraints(){
        let mut run=Run::new(CreateRun{
         instruction:"a task".into(),budget_krw:None,max_steps:None
        },5).unwrap();
        for _ in 0..10{
         run.messages.push(json!({"role":"user","content":format!(
          "New user instruction: {}", "constraint ".repeat(200))}));
        }
        assert!(verifier_messages(&run,"w","f","g","c",8000).is_err());
    }
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
