//! Dominion's durable tool-call state machine.
//!
//! Intent is committed to Store before dispatch. Result and removal of intent
//! are committed together. An unreceipted mutating call is NEVER replayed on
//! recovery: its outcome is unknown, even when the process died before spawn.
//! Read-only, local inspection tools can be retried from their queued call.
//!
//! This offers at-most-once automatic dispatch after crashes, NOT exactly-once
//! effects in external systems. Human reconciliation supplies missing evidence.
use crate::{
    domain::{Run,RunStatus,ToolCall},
    policy,
};
use serde_json::{json,Value};
use std::collections::HashSet;

pub fn replay_safe(name:&str)->bool {
    matches!(name,
        "list_files"|"read_file"|"search_files"|"git_status"|"git_diff"
        |"check_subagent"|"read_subagent_file"|"memory_search"
        |"list_skills"|"load_skill"
    )
}

fn same_call(a:&ToolCall,b:&ToolCall)->bool {
    a.id==b.id && a.name==b.name && a.arguments==b.arguments
}
fn queued_exact(run:&Run,call:&ToolCall)->bool {
    run.queued_calls.iter().filter(|c|c.id==call.id).count()==1
        && run.queued_calls.iter().any(|c|same_call(c,call))
}
fn pending_exact(run:&Run,call:&ToolCall)->bool {
    run.pending_approval.as_ref().is_some_and(|p|same_call(p,call))
}

/// Validate before adding the assistant response to durable model history.
/// Tool-call IDs are correlation keys, not mere display values.
pub fn validate_new_calls(run:&Run,calls:&[ToolCall])->Result<(),String>{
    if calls.len()>16{return Err("model returned more than 16 tool calls in a turn".into());}
    let mut previous=HashSet::<String>::new();
    for message in &run.messages {
        if let Some(ids)=message.get("tool_calls").and_then(Value::as_array) {
            for item in ids {
                if let Some(id)=item.get("id").and_then(Value::as_str){
                    previous.insert(id.to_owned());
                }
            }
        }
        if message.get("role").and_then(Value::as_str)==Some("tool"){
            if let Some(id)=message.get("tool_call_id").and_then(Value::as_str){
                previous.insert(id.to_owned());
            }
        }
    }
    for call in &run.queued_calls{previous.insert(call.id.clone());}
    if let Some(call)=&run.pending_approval{previous.insert(call.id.clone());}
    if let Some(call)=&run.in_flight{previous.insert(call.id.clone());}
    for call in calls {
        if call.id.is_empty()||call.id.len()>128
            || call.id.bytes().any(|b|b<32||b==127){
            return Err("invalid model tool-call ID".into());
        }
        if !call.arguments.is_object(){return Err("tool-call arguments must be a JSON object".into());}
        if !previous.insert(call.id.clone()){
            return Err(format!("duplicate/replayed tool-call ID {}",call.id));
        }
    }
    Ok(())
}

/// Persist this transition before calling the tool executor.
pub fn begin(run:&mut Run,call:&ToolCall)->Result<(),String>{
    if run.status!=RunStatus::Running{return Err("execution is not running".into());}
    if !policy::allowed_tool(run,&call.name){return Err("tool denied by execution policy".into());}
    if run.in_flight.is_some(){return Err("another tool outcome is unresolved".into());}
    if pending_exact(run,call) {
        if run.approval_decision!=Some(true){
            return Err("explicit tool approval missing".into());
        }
    } else if run.pending_approval.is_some() {
        return Err("pending approval belongs to another call".into());
    } else if !queued_exact(run,call) {
        return Err("tool call does not match exactly one queued request".into());
    }
    run.in_flight=Some(call.clone());
    run.event("tool_started",format!("Recorded execution intent for {} ({})",call.name,call.id),
        Some(json!({"name":call.name,"id":call.id,"arguments":call.arguments})));
    Ok(())
}

/// User denial has no side effect and must not be recorded as "in flight".
pub fn deny(run:&mut Run,call:&ToolCall)->Result<(),String>{
    if run.status!=RunStatus::Running || !pending_exact(run,call)
        || run.approval_decision!=Some(false){
        return Err("tool denial does not match a rejected approval".into());
    }
    if run.in_flight.is_some(){return Err("cannot deny tool after it started".into());}
    run.pending_approval=None;
    run.approval_decision=None;
    let message="TOOL ERROR: User denied the requested operation";
    run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":message}));
    run.event("tool_denied",format!("User denied {} ({})",call.name,call.id),
        Some(json!({"call_id":call.id})));
    Ok(())
}


/// A disabled/unavailable tool is a model protocol error, not a reason to
/// strand an entire autonomous run in "paused". Reject it with one real tool
/// receipt and no execution intent so the model can correct itself.
pub fn reject_policy(run:&mut Run,call:&ToolCall)->Result<(),String>{
    if run.status!=RunStatus::Running || run.in_flight.is_some()
        || run.pending_approval.is_some() || !queued_exact(run,call){
        return Err("tool policy rejection does not match the durable queue".into());
    }
    if policy::allowed_tool(run,&call.name){
        return Err("tool is allowed; policy rejection is not applicable".into());
    }
    let index=run.queued_calls.iter().position(|c|same_call(c,call)).ok_or("queued tool missing")?;
    run.queued_calls.remove(index);
    run.messages.push(json!({"role":"tool","tool_call_id":call.id,
        "content":"TOOL ERROR: Tool denied by the current run's read-only execution policy"}));
    run.event("tool_policy_denied",format!("Blocked {} ({}) by execution policy",call.name,call.id),
        Some(json!({"call_id":call.id,"name":call.name})));
    Ok(())
}

/// Applies the durable receipt in the same state-store transaction that
/// releases the intent. It is legal to finish while cancellation is pending.
pub fn finish(run:&mut Run,call:&ToolCall,outcome:&Result<String,String>,observed:bool)->Result<(),String>{
    if !run.in_flight.as_ref().is_some_and(|c|same_call(c,call)){
        return Err("tool receipt does not match the durable execution intent".into());
    }
    if pending_exact(run,call){
        run.pending_approval=None;
        run.approval_decision=None;
    }else{
        let index=run.queued_calls.iter().position(|c|same_call(c,call))
            .ok_or("tool call was removed from durable queue")?;
        run.queued_calls.remove(index);
    }
    let text=match outcome {
        Ok(result)=>result.clone(),
        Err(error)=>format!("TOOL ERROR: {error}"),
    };
    // Even on a cancellation transition the tool result must complete the
    // assistant tool-call pair, so history remains valid on a later inspection.
    run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":text}));
    run.in_flight=None;
    let kind=if observed{"tool_reconciled"}else if outcome.is_ok(){"tool_result"}else{"tool_error"};
    run.event(kind,
        format!("{} {} ({})",if observed{"Observed"}else{"Executed"},call.name,
            if outcome.is_ok(){"ok"}else{"failed"}),
        Some(json!({"call_id":call.id,"name":call.name,"result":text.chars().take(1200).collect::<String>(),
            "observed":observed,"succeeded":outcome.is_ok()})));
    if run.cancel_after_tool{
        run.cancel_after_tool=false;
        run.status=RunStatus::Cancelled;
        run.queued_calls.clear();
        run.event("cancelled","Cancelled after durable tool result/reconciliation",None);
    }
    Ok(())
}

/// Returns true only for a previously running operation whose pending work can
/// be retried without externally-visible duplicate side effects.
pub fn recover(run:&mut Run)->bool{
    let interrupted=matches!(run.status,RunStatus::Running|RunStatus::Queued);
    let approved_but_not_started=run.status==RunStatus::AwaitingApproval
        && run.approval_decision.is_some();
    let malformed_terminal=run.terminal() && run.in_flight.is_some();
    if !interrupted && !approved_but_not_started && !malformed_terminal{return false;}
    run.status=RunStatus::Paused;
    if let Some(call)=run.in_flight.clone(){
        if replay_safe(&call.name) && !run.cancel_after_tool {
            // Pure inspections may be retried; the queued call remains intact.
            // If history was written by older versions without a queue, treat
            // that state as unknown rather than discarding it.
            if queued_exact(run,&call) && run.pending_approval.is_none(){
                run.in_flight=None;
                run.event("tool_retry_safe",
                    format!("Interrupted read-only {} ({}) scheduled for retry",call.name,call.id),
                    Some(json!({"call_id":call.id})));
                run.event("recovered_ready","Read-only interrupted tool can resume automatically",None);
                return true;
            }
        }
        run.event("tool_outcome_unknown",
            format!("Outcome of {} ({}) cannot be established; manual reconciliation required",
                call.name,call.id),
            Some(json!({"call_id":call.id,"name":call.name})));
        return false;
    }
    if run.cancel_after_tool {
        run.event("recovered_uncertain","Cancellation pending a durable tool outcome",None);
        return false;
    }
    if run.pending_approval.is_some() && run.approval_decision.is_none(){
        // Pending human review cannot be silently approved on restart.
        run.status=RunStatus::AwaitingApproval;
        run.event("recovered_approval","Waiting for original human approval",None);
        return false;
    }
    run.event("recovered_ready",
        if approved_but_not_started{"Approved but unstarted tool may resume"}
        else{"Interrupted before a tool started; resuming persisted queue"},None);
    true
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::CreateRun;

    fn running()->Run{
        let mut run=Run::new(CreateRun{instruction:"change a project".into(),
            budget_krw:Some(200.0),max_steps:Some(5)},5).unwrap();
        run.status=RunStatus::Running;
        run
    }
    fn tool(name:&str,id:&str)->ToolCall{
        ToolCall{id:id.into(),name:name.into(),arguments:json!({"path":"src/main.rs"})}
    }
    #[test]
    fn execution_records_intent_then_exactly_one_receipt(){
        let mut run=running();let call=tool("edit_file","c1");
        run.queued_calls.push(call.clone());
        assert!(begin(&mut run,&call).is_ok());
        assert_eq!(run.in_flight.as_ref().unwrap().id,"c1");
        assert!(begin(&mut run,&call).is_err());
        finish(&mut run,&call,&Ok("edited".into()),false).unwrap();
        assert!(run.in_flight.is_none());
        assert!(run.queued_calls.is_empty());
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_started").count(),1);
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_result").count(),1);
        assert_eq!(run.messages.last().unwrap()["tool_call_id"],"c1");
        assert!(finish(&mut run,&call,&Ok("again".into()),false).is_err());
    }
    #[test]
    fn never_replays_unknown_mutations_but_retries_unfinished_reads(){
        let mut writer=running();let call=tool("edit_file","write1");
        writer.queued_calls.push(call.clone());
        begin(&mut writer,&call).unwrap();
        assert!(!recover(&mut writer));
        assert!(writer.in_flight.is_some());
        assert_eq!(writer.events.last().unwrap().kind,"tool_outcome_unknown");
        let mut reader=running();let read=tool("read_file","read1");
        reader.queued_calls.push(read.clone());
        begin(&mut reader,&read).unwrap();
        assert!(recover(&mut reader));
        assert!(reader.in_flight.is_none());
        assert_eq!(reader.queued_calls.len(),1);
        assert_eq!(reader.events.last().unwrap().kind,"recovered_ready");
        assert!(!replay_safe("mcp__external__lookup"));
        assert!(!replay_safe("shell"));
    }
    #[test]
    fn rejection_never_creates_an_uncertain_side_effect(){
        let mut run=running();let call=tool("shell","danger");
        run.pending_approval=Some(call.clone());run.approval_decision=Some(false);
        deny(&mut run,&call).unwrap();
        assert!(run.in_flight.is_none());
        assert!(run.pending_approval.is_none());
        assert!(run.events.iter().any(|e|e.kind=="tool_denied"));
        assert!(!run.events.iter().any(|e|e.kind=="tool_started"));
    }
    #[test]
    fn policy_denial_is_a_normal_tool_result_without_any_dispatch(){
        let mut run=running();run.read_only=true;
        let call=tool("write_file","blocked-1");
        run.queued_calls.push(call.clone());
        reject_policy(&mut run,&call).unwrap();
        assert!(run.queued_calls.is_empty());
        assert!(run.in_flight.is_none());
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_policy_denied").count(),1);
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert_eq!(run.messages.last().unwrap()["tool_call_id"],"blocked-1");
    }
    #[test]
    fn malformed_or_reused_model_call_ids_cannot_dispatch(){
        let mut run=running();let call=tool("read_file","dup");
        assert!(validate_new_calls(&run,&[call.clone(),call.clone()]).is_err());
        run.messages.push(json!({"role":"assistant","tool_calls":[{"id":"dup"}]}));
        assert!(validate_new_calls(&run,&[call]).is_err());
    }
    #[test]
    fn approved_unstarted_tool_and_cancellation_recover_correctly(){
        let mut approval=running();let command=tool("shell","cmd1");
        approval.status=RunStatus::AwaitingApproval;
        approval.pending_approval=Some(command.clone());
        approval.approval_decision=Some(true);
        assert!(recover(&mut approval));
        assert_eq!(approval.status,RunStatus::Paused);
        assert_eq!(approval.events.last().unwrap().kind,"recovered_ready");
        let mut cancelling=running();let call=tool("write_file","write2");
        cancelling.queued_calls.push(call.clone());
        begin(&mut cancelling,&call).unwrap();
        cancelling.cancel_after_tool=true;
        cancelling.status=RunStatus::Paused;
        assert!(!recover(&mut cancelling));
        finish(&mut cancelling,&call,&Ok("observed".into()),true).unwrap();
        assert_eq!(cancelling.status,RunStatus::Cancelled);
        assert_eq!(cancelling.messages.last().unwrap()["tool_call_id"],"write2");
    }
}
