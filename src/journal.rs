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
        |"github_list_repositories"|"github_read_file"|"check_subagent"|"read_subagent_file"|"memory_search"
        |"github_list_branches"|"github_list_issues"|"github_get_issue"|"github_list_pull_requests"
        |"github_get_pull_request"|"google_drive_search"|"google_drive_read_text"
        |"google_docs_get"|"google_sheets_get"
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
    for call in &run.read_batch_in_flight{previous.insert(call.id.clone());}
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


/// Pure reads have no external effects, so the engine can dispatch them in a
/// bounded pool. Writers, shell, MCP, memory changes and delegates are always
/// an exclusive barrier, regardless of the model's claims.
pub fn parallel_safe(name:&str)->bool{
    matches!(name,"list_files"|"read_file"|"search_files"|"git_status"|"git_diff")
}

/// The complete read batch is recorded before dispatch. It is safe to replay
/// after a crash because these tools never alter user-owned files.
pub fn begin_read_batch(run:&mut Run,calls:&[ToolCall])->Result<(),String>{
    if run.status!=RunStatus::Running || run.in_flight.is_some()
        || !run.read_batch_in_flight.is_empty() || run.pending_approval.is_some(){
        return Err("read batch cannot start in current run state".into());
    }
    if calls.len()<2 || calls.len()>8 || calls.len()>run.queued_calls.len(){
        return Err("parallel read batch requires 2..8 queued tool calls".into());
    }
    for (a,b) in calls.iter().zip(run.queued_calls.iter()){
        if !same_call(a,b)||!parallel_safe(&a.name)||!policy::allowed_tool(run,&a.name){
            return Err("read batch contains nonread, unauthorized or reordered tool call".into());
        }
    }
    run.read_batch_in_flight=calls.to_vec();
    run.event("read_batch_started",format!("Scheduled {} concurrent read-only tools",calls.len()),
        Some(json!({"ids":calls.iter().map(|call|&call.id).collect::<Vec<_>>()})));
    Ok(())
}
pub fn finish_read_batch(run:&mut Run,calls:&[ToolCall],results:&[Result<String,String>])->Result<(),String>{
    if calls.len()!=results.len() || run.read_batch_in_flight.len()!=calls.len()
        || run.in_flight.is_some(){
        return Err("read batch receipt does not match durable execution batch".into());
    }
    for (index,call) in calls.iter().enumerate(){
        if !same_call(call,&run.read_batch_in_flight[index]) || !same_call(call,&run.queued_calls[index]){
            return Err("read batch receipts are not in the original model order".into());
        }
    }
    for (call,result) in calls.iter().zip(results.iter()){
        let body=match result{
            Ok(v)=>v.clone(),Err(e)=>format!("TOOL ERROR: {e}")
        };
        run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":body}));
        run.event(if result.is_ok(){"tool_result"}else{"tool_error"},
            format!("Parallel read {} ({})",call.name,if result.is_ok(){"ok"}else{"failed"}),
            Some(json!({"call_id":call.id,"name":call.name,"result":body.chars().take(1200).collect::<String>()})));
    }
    run.queued_calls.drain(0..calls.len());
    run.read_batch_in_flight.clear();
    if run.cancel_after_tool{
        // Keep transcript pairing valid even for the tail that was never
        // dispatched after the user requested cancellation.
        for call in run.queued_calls.drain(..){
            run.messages.push(json!({"role":"tool","tool_call_id":call.id,
                "content":"TOOL ERROR [ABORTED_BEFORE_DISPATCH]: cancelled"}));
        }
        run.cancel_after_tool=false;
        run.status=RunStatus::Cancelled;
        run.event("cancelled","Cancellation completed after all started read tools settled",None);
    }
    Ok(())
}

/// Persist this transition before calling the tool executor.
pub fn begin(run:&mut Run,call:&ToolCall)->Result<(),String>{
    if run.status!=RunStatus::Running{return Err("execution is not running".into());}
    if !policy::allowed_tool(run,&call.name){return Err("tool denied by execution policy".into());}
    if run.in_flight.is_some() || !run.read_batch_in_flight.is_empty(){return Err("another tool outcome is unresolved".into());}
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
        Some(json!({"name":call.name,"id":call.id,"arguments":call.arguments,
            "remote_request_id":if call.name.starts_with("mcp__"){
                Some(crate::mcp::request_identity(run.id,call))
            }else{None::<String>}})));
    Ok(())
}

/// User denial has no side effect and must not be recorded as "in flight".
pub fn deny(run:&mut Run,call:&ToolCall)->Result<(),String>{
    if run.status!=RunStatus::Running || !pending_exact(run,call)
        || run.approval_decision!=Some(false){
        return Err("tool denial does not match a rejected approval".into());
    }
    if run.in_flight.is_some() || !run.read_batch_in_flight.is_empty(){return Err("cannot deny tool after it started".into());}
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
    if run.status!=RunStatus::Running || run.in_flight.is_some() || !run.read_batch_in_flight.is_empty()
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


/// Complete a malformed, stale, unavailable, or unsupported model tool call
/// without ever entering tool dispatch. It stays correlated to the exact
/// assistant call ID, so the next model step can correct its input.
pub fn reject_admission(run:&mut Run,call:&ToolCall,code:&str,reason:&str)->Result<(),String>{
    if run.status!=RunStatus::Running || run.in_flight.is_some() ||
        run.pending_approval.is_some() || !queued_exact(run,call){
        return Err("admission rejection does not match queued tool call".into());
    }
    let index=run.queued_calls.iter().position(|c|same_call(c,call)).ok_or("tool not queued")?;
    run.queued_calls.remove(index);
    let reason=reason.chars().take(650).collect::<String>();
    let body=format!("TOOL ERROR [{code}]: {reason}");
    run.messages.push(json!({"role":"tool","tool_call_id":call.id,"content":body}));
    run.event("tool_admission_rejected",format!("Rejected {} ({code})",call.name),
        Some(json!({"call_id":call.id,"name":call.name,"code":code,"reason":reason})));
    Ok(())
}


/// Deny a call that was already explicitly approved but is newly forbidden by
/// a runtime hook. This never dispatches and never consumes a second approval.
pub fn reject_approved_hook(run:&mut Run,call:&ToolCall,code:&str,reason:&str)->Result<(),String>{
    if run.status!=RunStatus::Running || run.in_flight.is_some() ||
        !run.read_batch_in_flight.is_empty() || !pending_exact(run,call) ||
        run.approval_decision!=Some(true){
        return Err("hook denial does not match an approved, unstarted call".into());
    }
    run.pending_approval=None;
    run.approval_decision=None;
    let reason=reason.chars().take(650).collect::<String>();
    run.messages.push(json!({"role":"tool","tool_call_id":call.id,
        "content":format!("TOOL ERROR [{code}]: {reason}")}));
    run.event("tool_hook_denied",format!("Tool hook blocked approved {}",call.name),
        Some(json!({"call_id":call.id,"code":code,"reason":reason})));
    Ok(())
}


/// Remote execution was dispatched but the transport could not establish its
/// outcome. Keep the immutable intent and tool arguments for reconciliation.
/// Never write a fake failure receipt, return the budget, or retry.
pub fn mark_remote_unknown(run:&mut Run,call:&ToolCall,reason:&str)->Result<(),String>{
    if !run.in_flight.as_ref().is_some_and(|pending|same_call(pending,call)){
        return Err("uncertain remote result does not match the durable tool intent".into());
    }
    run.status=RunStatus::Paused;
    run.event("mcp_remote_outcome_unknown",
        format!("MCP tool {} may still have remote effects; do not retry",call.name),
        Some(json!({"call_id":call.id,
            "remote_request_id":crate::mcp::request_identity(run.id,call),
            "reason":reason.chars().take(1000).collect::<String>(),
            "reconciliation_required":true})));
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
/// Single authority for deciding whether startup recovery can change a
/// snapshot. The Store uses this to avoid re-serializing settled history.
pub fn needs_recovery(run:&Run)->bool{
    matches!(run.status,RunStatus::Running|RunStatus::Queued)
        || (run.status==RunStatus::AwaitingApproval && run.approval_decision.is_some())
        || (run.terminal() && run.in_flight.is_some())
        || (run.status==RunStatus::Paused && !run.read_batch_in_flight.is_empty())
}
pub fn recover(run:&mut Run)->bool{
    if !needs_recovery(run){return false;}
    let interrupted=matches!(run.status,RunStatus::Running|RunStatus::Queued);
    let approved_but_not_started=run.status==RunStatus::AwaitingApproval
        && run.approval_decision.is_some();
    run.status=RunStatus::Paused;
    if !run.read_batch_in_flight.is_empty(){
        // Nothing in this batch can cause user-visible mutations.
        // The entire batch was durably marked before dispatch and no partial
        // receipts were ever committed, so queued reads can safely restart.
        let count=run.read_batch_in_flight.len();
        run.read_batch_in_flight.clear();
        if run.cancel_after_tool {
            for call in run.queued_calls.drain(..){
                run.messages.push(json!({"role":"tool","tool_call_id":call.id,
                    "content":"TOOL ERROR [ABORTED_BEFORE_DISPATCH]: cancelled on restart"}));
            }
            run.cancel_after_tool=false;run.status=RunStatus::Cancelled;
            run.event("cancelled","Read batch cancellation recovered without side effects",None);
            return false;
        }
        run.event("read_batch_retry_safe",format!("Replaying {count} idempotent reads in original order"),None);
        run.event("recovered_ready","Parallel read batch can resume",None);
        return true;
    }
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
mod tests;
