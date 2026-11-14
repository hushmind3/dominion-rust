//! DOMINION-owned read-only batching and duplicate coalescing.
//!
//! Inspired by Haystack's state-aware scheduling and Strands' concurrent
//! executor. Native capabilities/approval remain in DOMINION. This helper
//! NEVER groups mutating tool calls or writes synthetic success receipts.
use super::{ToolCall,Value};
use std::collections::HashMap;

pub(super) struct BatchRead {
    pub call: ToolCall,
    pub args: Value,
    pub recipients: Vec<ToolCall>,
}
pub(super) struct ReadBatch {
    pub unique: Vec<BatchRead>,
    pub invalid: Vec<(ToolCall,String)>,
    pub duplicates: usize,
}

/// A read-only tool call may be coalesced only if the same tool and precisely
/// the same parsed JSON arguments were requested in ONE model response.
/// This does not cross sessions or rounds, where the underlying state may
/// legitimately have changed.
pub(super) fn plan(calls:&[ToolCall],definitions:&[Value])->ReadBatch{
    let mut indexes=HashMap::<String,usize>::new();
    let mut unique=Vec::<BatchRead>::new();
    let mut invalid=Vec::new();
    let mut duplicates=0;
    for call in calls {
        if !super::tool_catalog::tool_is_read_only(&call.name){
            invalid.push((call.clone(),"not a pure read tool".into()));
            continue;
        }
        let Some(definition)=definitions.iter().find(|definition|
            super::tool_catalog::tool_name(definition)==call.name)
        else {
            invalid.push((call.clone(),"tool not advertised".into()));
            continue;
        };
        let args=match serde_json::from_str::<Value>(&call.arguments){
            Ok(value) if value.is_object()=>value,
            _=>{
                invalid.push((call.clone(),"JSON object required".into()));
                continue
            }
        };
        if let Err(error)=super::tool_schema::validate(definition,&args){
            invalid.push((call.clone(),error));
            continue
        }
        // serde_json::Map is sorted by key by default. Do not use the
        // caller-supplied string bytes: whitespace/key order can vary.
        let key=format!("{}:{}",call.name,args);
        if let Some(index)=indexes.get(&key){
            unique[*index].recipients.push(call.clone());
            duplicates+=1;
        }else{
            indexes.insert(key,unique.len());
            unique.push(BatchRead{
                call:call.clone(),args,recipients:vec![call.clone()]
            });
        }
    }
    ReadBatch{unique,invalid,duplicates}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn call(id:&str,name:&str,args:&str)->ToolCall{
        ToolCall{id:id.into(),name:name.into(),arguments:args.into()}
    }
    #[test]
    fn identical_read_inputs_execute_once_but_respond_to_every_tool_call(){
        let tools=crate::engine::tools::definitions();
        let calls=[
            call("one","workspace_read",r#"{"path":"notes.md"}"#),
            call("two","workspace_read",r#"{"path":"notes.md"}"#),
            call("three","workspace_read",r#"{"path":"other.md"}"#),
        ];
        let batch=plan(&calls,&tools);
        assert_eq!(batch.unique.len(),2);
        assert_eq!(batch.duplicates,1);
        assert_eq!(batch.unique[0].recipients.len(),2);
        assert_eq!(batch.unique[0].recipients[0].id,"one");
        assert_eq!(batch.unique[0].recipients[1].id,"two");
        assert!(batch.invalid.is_empty());
    }
    #[test]
    fn semantic_json_equality_coalesces_reordered_properties(){
        let tools=crate::engine::tools::definitions();
        let calls=[
            call("one","workspace_read_range",r#"{"path":"a.rs","startLine":1}"#),
            call("two","workspace_read_range",r#"{"startLine":1,"path":"a.rs"}"#),
        ];
        let batch=plan(&calls,&tools);
        assert_eq!(batch.unique.len(),1);
    }
    #[test]
    fn invalid_or_mutating_calls_never_consume_execution_slots(){
        let tools=crate::engine::tools::definitions();
        let calls=[
            call("one","workspace_write",r#"{"path":"x","content":"hi"}"#),
            call("two","workspace_read",r#"{"path":2}"#),
            call("three","workspace_read",r#"{"path":"valid"}"#),
        ];
        let batch=plan(&calls,&tools);
        assert_eq!(batch.unique.len(),1);
        assert_eq!(batch.invalid.len(),2);
    }
    #[test]
    fn no_cross_tool_or_cross_round_reuse(){
        let tools=crate::engine::tools::definitions();
        let calls=[
            call("one","file_read",r#"{"path":"/tmp/x"}"#),
            call("two","workspace_read",r#"{"path":"/tmp/x"}"#),
        ];
        let batch=plan(&calls,&tools);
        assert_eq!(batch.unique.len(),2);
        assert_eq!(plan(&calls[..1],&tools).unique.len(),1);
    }
    #[test]
    fn empty_and_unknown_tools_return_no_phantom_execution(){
        let batch=plan(&[call("x","mcp__unknown", "{}")],&[]);
        assert!(batch.unique.is_empty());
        assert_eq!(batch.invalid.len(),1);
        assert!(serde_json::to_value(json!({"data":"untrusted"})).is_ok());
    }
}
