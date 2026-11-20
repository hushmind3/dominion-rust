//! Deterministic tool admission and hierarchical budget policy, owned by Dominion.
//! This policy applies independently from model prompts and tool descriptions.
use crate::domain::Run;
use serde_json::Value;

pub fn effective_budget_krw(run:&Run)->f64{
    (run.budget_krw-run.reserved_budget_krw-run.delegated_spent_krw).max(0.0)
}

pub fn allowed_tool(run:&Run,name:&str)->bool{
    // Room participants are lower-trust peers, not independent delegated
    // Worker owners. Even a corrupted read_only flag cannot grant a Room
    // shell, write, external MCP effect or spawn permission.
    if run.room_binding.is_some(){
        return run.read_only && matches!(name,
            "list_files"|"read_file"|"read_tool_receipt"|
            "search_tool_receipts"|"search_files"|"git_status"|"git_diff"|
            "read_program"|"memory_search"|"list_skills"|"load_skill"|
            "github_list_repositories"|"github_read_file"|
            "github_list_branches"|"github_list_issues"|"github_get_issue"|
            "github_list_pull_requests"|"github_get_pull_request"|
            "google_drive_search"|"google_drive_read_text"|
            "google_docs_get"|"google_sheets_get"|"tool_search");
    }
    // Task-oriented children share the parent model/runtime but expose only
    // task-relevant tool classes. Unknown tools never bypass this scope.
    if run.parent_run_id.is_some(){
        match run.delegation_profile.as_str(){
            "inspection"=>return matches!(name,
                "list_files"|"read_file"|"read_tool_receipt"|"search_tool_receipts"|"search_files"|"git_status"|"git_diff"|"read_program"
                |"github_list_repositories"|"github_read_file"|"github_list_branches"
                |"github_list_issues"|"github_get_issue"|"github_list_pull_requests"
                |"github_get_pull_request"|"google_drive_search"|"google_drive_read_text"
                |"google_docs_get"|"google_sheets_get"|"memory_search"
                |"list_skills"|"load_skill"),
            "implementation"=>return !run.read_only&&matches!(name,
                "list_files"|"read_file"|"read_tool_receipt"|"search_tool_receipts"|"search_files"|"git_status"|"git_diff"
                |"read_program"|"run_program"|"write_file"|"edit_file"|"shell"
                |"external_agent_edit"
                |"memory_search"|"list_skills"|"load_skill"),
            "verification"=>return !run.read_only&&matches!(name,
                "list_files"|"read_file"|"read_tool_receipt"|"search_tool_receipts"|"search_files"|"git_status"|"git_diff"
                |"read_program"|"shell"|"memory_search"|"list_skills"|"load_skill"),
            _=>{}
        }
    }
    if !run.read_only{return true}
    // Unknown and MCP tools are denied: a read-only investigator cannot
    // escalate through dynamically registered extensions.
    matches!(name,"list_files"|"read_file"|"read_tool_receipt"|"search_tool_receipts"|"search_files"|"git_status"|"git_diff"|"read_program"|"github_list_repositories"|"github_read_file"|"github_list_branches"|"github_list_issues"
        |"github_get_issue"|"github_list_pull_requests"|"github_get_pull_request"
        |"google_drive_search"|"google_drive_read_text"|"google_docs_get"|"google_sheets_get"
        |"check_subagent"|"read_subagent_file"|"spawn_subagent"|"memory_search"|"tool_search"|"list_skills"|"load_skill")
}

/// This same admission check must run before each actual tool dispatch.
/// A tool hidden from a new model turn must not remain executable through
/// a persisted queued call, a resumed approval, or a read batch.
pub fn admitted_tool(run:&Run,name:&str,denied:&[String])->bool{
    allowed_tool(run,name)&&!denied.iter().any(|blocked|blocked==name)
}

/// Select only tools that the agent can actually use in this state.
/// The durable transcript is not changed; this reduces repeated tool-schema tokens.
pub fn select_catalog(run:&Run,tools:Vec<Value>,has_children:bool,denied:&[String])->(Vec<Value>,usize){
    let original=tools.iter().map(|t|t.to_string().len()).sum::<usize>();
    let selected=tools.into_iter().filter(|tool|{
        let Some(name)=tool.pointer("/function/name").and_then(Value::as_str)else{return false;};
        if !admitted_tool(run,name,denied){return false;}
        if name.starts_with("git_") && run.repository.is_none(){return false;}
        if matches!(name,"check_subagent"|"read_subagent_file"|"merge_subagent") && !has_children{return false;}
        if name=="merge_subagent" && run.repository.is_none(){return false;}
        if name=="external_agent_edit" && (run.repository.is_none()||run.parent_run_id.is_none()||run.read_only){return false;}
        true
    }).collect::<Vec<_>>();
    let remaining=selected.iter().map(|t|t.to_string().len()).sum::<usize>();
    (selected,original.saturating_sub(remaining))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn room_peer_cannot_delegate_or_escalate_through_read_only_flag(){
        let mut run=crate::domain::Run::new(crate::domain::CreateRun{
            instruction:"Inspect".into(),budget_krw:Some(100.0),
            max_steps:Some(3)},3).unwrap();
        run.room_binding=Some(crate::domain::RoomRunBinding{
            room_id:uuid::Uuid::new_v4(),
            owner_message_index:0,owner_at_ms:10,participant:"peer".into()
        });
        run.read_only=true;
        assert!(admitted_tool(&run,"read_file",&[]));
        assert!(admitted_tool(&run,"memory_search",&[]));
        assert!(!admitted_tool(&run,"spawn_subagent",&[]));
        assert!(!admitted_tool(&run,"merge_subagent",&[]));
        assert!(!admitted_tool(&run,"run_program",&[]));
        assert!(!admitted_tool(&run,"shell",&[]));
        assert!(!admitted_tool(&run,"mcp__remote__get",&[]));
        run.read_only=false;
        assert!(!admitted_tool(&run,"write_file",&[]));
        assert!(!admitted_tool(&run,"read_file",&[]));
    }
    use crate::{domain::{CreateRun,ToolCall},tools};
    use serde_json::json;
    #[test]
    fn role_scoped_child_uses_original_policy_for_prompt_and_dispatch(){
        let mut child=Run::new(CreateRun{instruction:"Verify patch".into(),
            budget_krw:Some(10.0),max_steps:Some(5)},5).unwrap();
        child.parent_run_id=Some(uuid::Uuid::new_v4());
        child.delegation_profile="verification".into();
        assert!(allowed_tool(&child,"read_file"));
        assert!(allowed_tool(&child,"shell"),
            "verification may run owner-approved shell checks");
        for name in ["write_file","edit_file","run_program","spawn_subagent",
            "mcp__remote__write","google_docs_create"]{
            assert!(!allowed_tool(&child,name),
                "{name} must not escape verifier scope");
        }
        let (catalog,_)=select_catalog(&child,tools::definitions(true),false,&[]);
        assert!(catalog.iter().all(|t|t.pointer("/function/name")
            .and_then(Value::as_str).is_some_and(|n|allowed_tool(&child,n))));
        child.delegation_profile="implementation".into();
        assert!(allowed_tool(&child,"edit_file"));
        assert!(!allowed_tool(&child,"mcp__remote__write"));
        assert!(!allowed_tool(&child,"spawn_subagent"));
        child.delegation_profile="inspection".into();
        child.read_only=true;
        assert!(allowed_tool(&child,"google_drive_search"));
        assert!(!allowed_tool(&child,"shell"));
        assert!(!allowed_tool(&child,"write_file"));
        assert!(!allowed_tool(&child,"spawn_subagent"));
        let serialized=serde_json::to_value(&child).unwrap();
        let recovered:Run=serde_json::from_value(serialized).unwrap();
        assert_eq!(recovered.delegation_profile,"inspection");
        let mut legacy=Run::new(CreateRun{instruction:"old".into(),budget_krw:None,
            max_steps:None},5).unwrap();
        legacy.parent_run_id=Some(uuid::Uuid::new_v4());
        assert!(allowed_tool(&legacy,"spawn_subagent"),
            "ordinary legacy delegation must retain its previous capabilities");
        legacy.read_only=true;
        assert!(!allowed_tool(&legacy,"write_file"));
    }
    #[test]
    fn reserved_child_budgets_reduce_parent_effective_limit(){
        let mut run=Run::new(CreateRun{instruction:"x".into(),budget_krw:Some(100.0),max_steps:None},5).unwrap();
        run.reserved_budget_krw=60.0;
        assert_eq!(effective_budget_krw(&run),40.0);
        run.reserved_budget_krw=40.0;
        run.delegated_spent_krw=20.0;
        assert_eq!(effective_budget_krw(&run),40.0);
        run.cost_krw=39.0;
        assert!(run.cost_krw<=effective_budget_krw(&run));
        run.cost_krw=41.0;
        assert!(run.cost_krw>effective_budget_krw(&run));
    }
    #[test]
    fn persisted_tool_calls_must_obey_current_operator_policy(){
        let mut run=Run::new(CreateRun{instruction:"work".into(),
            budget_krw:None,max_steps:None},5).unwrap();
        assert!(admitted_tool(&run,"read_file",&[]));
        let revoked=vec!["read_file".to_string(),"shell".to_string()];
        assert!(!admitted_tool(&run,"read_file",&revoked));
        assert!(!admitted_tool(&run,"shell",&revoked));
        run.read_only=true;
        assert!(!admitted_tool(&run,"write_file",&[]));
        assert!(!admitted_tool(&run,"mcp__unknown__write",&[]));
        assert!(admitted_tool(&run,"read_file",&[]));
    }
    #[test]
    fn recovered_queue_and_approved_tool_receive_durable_deny_receipts(){
        use crate::domain::RunStatus;
        let forbidden=vec!["write_file".to_owned(),"shell".to_owned()];
        let call=ToolCall{id:"old-call".into(),name:"write_file".into(),
            arguments:json!({"path":"important.txt","content":"overwrite"})};
        let mut original=Run::new(CreateRun{instruction:"work".into(),
            budget_krw:Some(100.0),max_steps:Some(8)},8).unwrap();
        original.status=RunStatus::Running;
        original.messages.push(json!({"role":"assistant","tool_calls":[{
            "id":call.id,"type":"function","function":{"name":call.name,
                "arguments":call.arguments.to_string()}}]}));
        original.queued_calls.push(call.clone());
        let mut restored:Run=serde_json::from_value(
            serde_json::to_value(original).unwrap()).unwrap();
        assert!(!admitted_tool(&restored,&call.name,&forbidden));
        crate::journal::reject_admission(&mut restored,&call,
            "OPERATOR_TOOL_DENIED","Tool disabled after restart").unwrap();
        assert!(restored.queued_calls.is_empty());
        assert!(restored.in_flight.is_none());
        assert!(restored.messages.last().unwrap()["content"].as_str().unwrap()
            .contains("OPERATOR_TOOL_DENIED"));

        let pending=ToolCall{id:"approved-old".into(),name:"shell".into(),
            arguments:json!({"command":"echo must-not-run"})};
        let mut resumed=Run::new(CreateRun{instruction:"work".into(),
            budget_krw:Some(100.0),max_steps:Some(8)},8).unwrap();
        resumed.status=RunStatus::Running;
        resumed.pending_approval=Some(pending.clone());
        resumed.approval_decision=Some(true);
        assert!(!admitted_tool(&resumed,&pending.name,&forbidden));
        crate::journal::reject_approved_hook(&mut resumed,&pending,
            "OPERATOR_TOOL_DENIED","Tool disabled after approval").unwrap();
        assert!(resumed.pending_approval.is_none());
        assert!(resumed.approval_decision.is_none());
        assert!(resumed.in_flight.is_none());
        assert!(resumed.messages.last().unwrap()["content"].as_str().unwrap()
            .contains("OPERATOR_TOOL_DENIED"));
    }
    #[test]
    fn operator_denied_tools_are_absent_from_model_prompt_and_schema_budget(){
        let run=Run::new(CreateRun{instruction:"test".into(),budget_krw:None,max_steps:None},5).unwrap();
        let denied=vec!["write_file".to_string(),"shell".to_string()];
        let (visible,saved)=select_catalog(&run,tools::definitions(true),false,&denied);
        let names=visible.iter().filter_map(|v|v.pointer("/function/name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"shell"));
        assert!(names.contains(&"read_file"));
        assert!(saved>0);
    }
    #[test]
    fn readonly_investigator_cannot_write_or_use_mcp_or_merge(){
        let mut run=Run::new(CreateRun{instruction:"audit".into(),budget_krw:None,max_steps:None},5).unwrap();
        run.read_only=true;
        run.repository=Some("demo".into());
        let (selected,saved)=select_catalog(&run,tools::definitions(true),true,&[]);
        let names=selected.iter().filter_map(|v|v.pointer("/function/name").and_then(Value::as_str)).collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"git_diff"));
        assert!(!names.contains(&"shell"));
        assert!(!names.contains(&"edit_file"));
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"merge_subagent"));
        assert!(!allowed_tool(&run,"mcp__host__danger"));
        assert!(allowed_tool(&run,"github_read_file"));
        assert!(!allowed_tool(&run,"github_commit_file"));
        assert!(saved>0);
        let _=ToolCall{id:"test".into(),name:"read_file".into(),arguments:json!({})};
    }
}
