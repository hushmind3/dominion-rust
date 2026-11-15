//! Deterministic tool admission and hierarchical budget policy, owned by Dominion.
//! This policy applies independently from model prompts and tool descriptions.
use crate::domain::Run;
use serde_json::Value;

pub fn effective_budget_krw(run:&Run)->f64{
    (run.budget_krw-run.reserved_budget_krw-run.delegated_spent_krw).max(0.0)
}

pub fn allowed_tool(run:&Run,name:&str)->bool{
    if !run.read_only{return true}
    // Unknown and MCP tools are denied: a read-only investigator cannot
    // escalate through dynamically registered extensions.
    matches!(name,"list_files"|"read_file"|"search_files"|"git_status"|"git_diff"|"check_subagent"|"read_subagent_file"|"spawn_subagent"|"memory_search"|"list_skills"|"load_skill")
}

/// Select only tools that the agent can actually use in this state.
/// The durable transcript is not changed; this reduces repeated tool-schema tokens.
pub fn select_catalog(run:&Run,tools:Vec<Value>,has_children:bool)->(Vec<Value>,usize){
    let original=tools.iter().map(|t|t.to_string().len()).sum::<usize>();
    let selected=tools.into_iter().filter(|tool|{
        let Some(name)=tool.pointer("/function/name").and_then(Value::as_str)else{return false;};
        if !allowed_tool(run,name){return false;}
        if name.starts_with("git_") && run.repository.is_none(){return false;}
        if matches!(name,"check_subagent"|"read_subagent_file"|"merge_subagent") && !has_children{return false;}
        if name=="merge_subagent" && run.repository.is_none(){return false;}
        true
    }).collect::<Vec<_>>();
    let remaining=selected.iter().map(|t|t.to_string().len()).sum::<usize>();
    (selected,original.saturating_sub(remaining))
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::{domain::{CreateRun,ToolCall},tools};
    use serde_json::json;
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
    fn readonly_investigator_cannot_write_or_use_mcp_or_merge(){
        let mut run=Run::new(CreateRun{instruction:"audit".into(),budget_krw:None,max_steps:None},5).unwrap();
        run.read_only=true;
        run.repository=Some("demo".into());
        let (selected,saved)=select_catalog(&run,tools::definitions(true),true);
        let names=selected.iter().filter_map(|v|v.pointer("/function/name").and_then(Value::as_str)).collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"git_diff"));
        assert!(!names.contains(&"shell"));
        assert!(!names.contains(&"edit_file"));
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"merge_subagent"));
        assert!(!allowed_tool(&run,"mcp__host__danger"));
        assert!(saved>0);
        let _=ToolCall{id:"test".into(),name:"read_file".into(),arguments:json!({})};
    }
}
