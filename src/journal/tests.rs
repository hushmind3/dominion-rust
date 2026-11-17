//! Extracted regression tests; production modules stay compact.
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
    fn parallel_reads_persist_and_settle_in_model_order(){
        let mut run=running();
        let a=tool("read_file","r1");let b=tool("list_files","r2");
        run.queued_calls.extend([a.clone(),b.clone()]);
        begin_read_batch(&mut run,&[a.clone(),b.clone()]).unwrap();
        assert_eq!(run.read_batch_in_flight.len(),2);
        assert!(begin(&mut run,&a).is_err());
        finish_read_batch(&mut run,&[a.clone(),b.clone()],
            &[Ok("first".into()),Err("second failed".into())]).unwrap();
        assert!(run.read_batch_in_flight.is_empty());
        assert!(run.queued_calls.is_empty());
        let outputs=run.messages.iter().filter(|m|m["role"]=="tool").collect::<Vec<_>>();
        assert_eq!(outputs.len(),2);
        assert_eq!(outputs[0]["tool_call_id"],"r1");
        assert_eq!(outputs[1]["tool_call_id"],"r2");
    }
    #[test]
    fn exclusive_writes_are_barriers_to_parallel_reads(){
        let mut run=running();
        let a=tool("read_file","r1");let b=tool("write_file","w1");
        run.queued_calls.extend([a.clone(),b.clone()]);
        assert!(begin_read_batch(&mut run,&[a,b]).is_err());
        assert!(run.read_batch_in_flight.is_empty());
        assert!(!parallel_safe("memory_store"));
        assert!(!parallel_safe("mcp__other__search"));
    }
    #[test]
    fn interrupted_parallel_reads_recover_without_manual_approval(){
        let mut run=running();
        let a=tool("read_file","r1");let b=tool("git_diff","r2");
        run.queued_calls.extend([a.clone(),b.clone()]);
        begin_read_batch(&mut run,&[a,b]).unwrap();
        assert!(recover(&mut run));
        assert!(run.read_batch_in_flight.is_empty());
        assert_eq!(run.queued_calls.len(),2);
        assert_eq!(run.events.last().unwrap().kind,"recovered_ready");
    }
    #[test]
    fn restart_cancellation_finishes_paused_parallel_batch_without_replay(){
        let mut run=running();
        let a=tool("read_file","r1");let b=tool("read_file","r2");
        run.queued_calls.extend([a.clone(),b.clone()]);
        begin_read_batch(&mut run,&[a,b]).unwrap();
        run.cancel_after_tool=true;
        run.status=RunStatus::Paused;
        assert!(!recover(&mut run));
        assert!(run.read_batch_in_flight.is_empty());
        assert_eq!(run.status,RunStatus::Cancelled);
        assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),2);
    }
    #[test]
    fn cancel_parallel_batch_waits_for_ordered_receipts(){
        let mut run=running();
        let a=tool("read_file","r1");let b=tool("read_file","r2");
        let tail=tool("write_file","w3");
        run.queued_calls.extend([a.clone(),b.clone(),tail]);
        begin_read_batch(&mut run,&[a.clone(),b.clone()]).unwrap();
        run.cancel_after_tool=true;
        run.status=RunStatus::Paused;
        finish_read_batch(&mut run,&[a,b],&[Ok("a".into()),Ok("b".into())]).unwrap();
        assert_eq!(run.status,RunStatus::Cancelled);
        assert!(run.queued_calls.is_empty());
        assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),3);
    }

    #[test]
    fn cancelled_remote_mcp_request_preserves_unconfirmed_intent(){
        let mut run=running();
        let call=tool("mcp__remote__charge","payment-1");
        run.queued_calls.push(call.clone());
        begin(&mut run,&call).unwrap();
        run.cancel_after_tool=true;
        run.status=RunStatus::Paused;
        mark_remote_unknown(&mut run,&call,"MCP HTTP stream dropped").unwrap();
        assert_eq!(run.status,RunStatus::Paused);
        assert_eq!(run.in_flight.as_ref().unwrap().id,"payment-1");
        assert!(run.cancel_after_tool);
        assert_eq!(run.events.last().unwrap().kind,"mcp_remote_outcome_unknown");
        assert_eq!(run.events.last().unwrap().detail.as_ref().unwrap()["remote_request_id"],
            crate::mcp::request_identity(run.id,&call));
        assert!(!recover(&mut run),"uncertain remote work must never auto-resume");
        assert_eq!(run.in_flight.as_ref().unwrap().id,"payment-1");
        finish(&mut run,&call,&Ok("confirmed by remote audit".into()),true).unwrap();
        assert_eq!(run.status,RunStatus::Cancelled);
        assert!(run.in_flight.is_none());
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_reconciled").count(),1);
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
    fn approved_call_can_be_vetoed_by_new_hook_without_dispatch(){
        let mut run=running();
        let call=tool("shell","approved-then-blocked");
        run.pending_approval=Some(call.clone());
        run.approval_decision=Some(true);
        reject_approved_hook(&mut run,&call,"HOOK_POLICY","blocked after approval").unwrap();
        assert!(run.pending_approval.is_none());
        assert!(run.in_flight.is_none());
        assert!(run.approval_decision.is_none());
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_hook_denied").count(),1);
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert_eq!(run.messages.last().unwrap()["tool_call_id"],"approved-then-blocked");
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
    fn invalid_model_tool_is_rejected_without_dispatch(){
        let mut run=running();let call=tool("unavailable","invalid");
        run.queued_calls.push(call.clone());
        reject_admission(&mut run,&call,"UNKNOWN_TOOL","Not in advertised catalog").unwrap();
        assert_eq!(run.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert!(run.in_flight.is_none());
        assert!(run.messages.last().unwrap()["content"].as_str().unwrap().contains("UNKNOWN_TOOL"));
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
