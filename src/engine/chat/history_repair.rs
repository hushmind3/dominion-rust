//! Provider-compatible recovery of interrupted assistant/tool transcripts.
//!
//! Inspired by the *problem* solved in DeerFlow's dangling_tool_call
//! middleware. This is DOMINION-owned Rust code, not an SDK adapter.
//!
//! Only the model-bound replay is repaired; original SQLite history and
//! execution operations stay immutable. Never fabricate a successful result
//! when a tool invocation may have taken effect before a crash.
use serde_json::{json,Value};
use std::collections::{HashMap,HashSet};

pub(super) struct ReplayedTools {
    pub results: Vec<Value>,
    pub repaired: bool,
    pub missing: usize,
    pub extraneous: usize,
}

/// Rebuild the tool response chain IN ASSISTANT CALL ORDER, which strict
/// OpenAI/Anthropic adapters require after an interrupted/compacted turn.
///
/// A malformed assistant call ID is deliberately not reconstructed; caller
/// uses its existing plain-text fallback instead.
pub(super) fn replay(calls:&[Value],rows:&[&Value])->Option<ReplayedTools>{
    if calls.is_empty() || calls.len()>64 {return None}
    let mut unique=HashSet::new();
    for call in calls{
        let id=call["id"].as_str()?;
        if id.is_empty() || !unique.insert(id) {return None}
    }
    let mut results=HashMap::<&str,&Value>::new();
    let mut duplicates=HashSet::<&str>::new();
    for row in rows{
        let Some(id)=row["tool_call_id"].as_str() else{continue};
        if results.insert(id,*row).is_some(){duplicates.insert(id);}
    }
    let mut repaired=false;
    let mut missing=0usize;
    let mut used=HashSet::new();
    let mut rendered=Vec::with_capacity(calls.len());
    for (position,call) in calls.iter().enumerate(){
        let id=call["id"].as_str()?;
        let found=results.get(id).copied().filter(|_|!duplicates.contains(id));
        used.insert(id);
        if let Some(row)=found {
            let raw=row["content"].as_str().unwrap_or("");
            // Empty results are real results; do not silently call the model
            // with a missing tool response when a record exists.
            rendered.push(json!({
                "role":"tool","tool_call_id":id,"content":raw,
                "_dominion_frame":row["id"],
            }));
            if rows.get(position).and_then(|entry|
                entry["tool_call_id"].as_str())!=Some(id){repaired=true}
        }else{
            missing+=1;
            repaired=true;
            let tool=call["function"]["name"].as_str()
                .or_else(||call["name"].as_str()).unwrap_or("unknown");
            rendered.push(json!({
                "role":"tool","tool_call_id":id,
                "content":json!({
                    "error":"이전 도구 실행의 결과가 저장되지 않았습니다. 실행 여부를 현재 상태에서 다시 확인해야 합니다.",
                    "tool":tool,"interruptedToolCall":true,
                    "effectUnknown":true,"requiresInspection":true,
                    "neverAssumeSuccess":true,
                }).to_string(),
                "_dominion_synthetic_interrupted":true,
            }));
        }
    }
    let extraneous=rows.iter().filter(|row|row["tool_call_id"]
        .as_str().is_none_or(|id| !used.contains(id))).count()
        +duplicates.len();
    if extraneous>0 {repaired=true}
    Some(ReplayedTools{results:rendered,repaired,missing,extraneous})
}

#[cfg(test)]
mod tests{
    use super::*;
    fn call(id:&str,tool:&str)->Value{
        json!({"id":id,"type":"function","function":{"name":tool,"arguments":"{}"}})
    }
    fn result(id:&str,content:&str)->Value{
        json!({"id":format!("row-{id}"),"role":"tool",
            "tool_call_id":id,"content":content})
    }
    #[test]
    fn preserves_observed_result_and_synthesizes_only_missing_after_crash(){
        let first=result("a","{\"file\":\"ok\"}");
        let out=replay(&[call("a","workspace_read"),call("b","shell_exec")],
            &[&first]).unwrap();
        assert!(out.repaired);
        assert_eq!(out.missing,1);
        assert_eq!(out.results[0]["content"],"{\"file\":\"ok\"}");
        assert_eq!(out.results[1]["tool_call_id"],"b");
        let missing:Value=serde_json::from_str(out.results[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(missing["effectUnknown"],true);
        assert_eq!(missing["neverAssumeSuccess"],true);
        assert_eq!(missing["tool"],"shell_exec");
    }
    #[test]
    fn out_of_order_results_are_repaired_into_call_order(){
        let first=result("a","alpha");
        let second=result("b","bravo");
        let out=replay(&[call("a","workspace_read"),call("b","web_search")],
            &[&second,&first]).unwrap();
        assert!(out.repaired);
        assert_eq!(out.missing,0);
        assert_eq!(out.results[0]["content"],"alpha");
        assert_eq!(out.results[1]["content"],"bravo");
    }
    #[test]
    fn duplicate_call_ids_are_not_guessed_or_reexecuted(){
        let row=result("a","record");
        assert!(replay(&[call("a","x"),call("a","y")],&[&row]).is_none());
        let out=replay(&[call("a","x")],&[&row,&row]).unwrap();
        assert!(out.repaired);
        assert_eq!(out.missing,1);
    }
    #[test]
    fn valid_complete_turn_is_byte_exact_and_no_fake_repair(){
        let first=result("a","{}");
        let second=result("b","ok");
        let out=replay(&[call("a","workspace_read"),call("b","web_search")],
            &[&first,&second]).unwrap();
        assert!(!out.repaired);
        assert_eq!(out.missing,0);
        assert_eq!(out.results[0]["content"],"{}");
        assert_eq!(out.results[1]["content"],"ok");
    }
    #[test]
    fn unexpected_orphan_is_never_forwarded_as_a_provider_tool_message(){
        let known=result("a","ok");
        let orphan=result("different-turn","secret");
        let out=replay(&[call("a","workspace_read")],&[&orphan,&known]).unwrap();
        assert_eq!(out.results.len(),1);
        assert_eq!(out.results[0]["content"],"ok");
        assert_eq!(out.extraneous,1);
    }
    #[test]
    fn unknown_execution_never_counts_as_a_successful_tool_receipt(){
        let out=replay(&[call("a","workspace_write")],&[]).unwrap();
        let value:Value=serde_json::from_str(out.results[0]["content"].as_str().unwrap()).unwrap();
        assert!(value.get("result").is_none());
        assert!(value.get("status").is_none());
        assert_eq!(value["requiresInspection"],true);
    }
}
