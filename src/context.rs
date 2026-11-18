//! Context projection for the native Rust agent loop.
//! Full history stays durable; only outbound model requests are compacted.
use serde_json::{json,Value};
use std::collections::HashMap;

#[derive(Debug,Clone)]
pub struct Projection {
    pub messages:Vec<Value>,
    pub original_chars:usize,
    pub projected_chars:usize,
    pub omitted_messages:usize,
}
fn size(m:&Value)->usize { m.to_string().chars().count() }
fn role(m:&Value)->&str {m.get("role").and_then(Value::as_str).unwrap_or("")}
fn is_verifier_feedback(message:&Value)->bool{
    role(message)=="user" && message.get("content").and_then(Value::as_str)
        .is_some_and(|text|text.starts_with("Verifier requested corrections."))
}
fn is_user_change(message:&Value)->bool{
    role(message)=="user" && message.get("content").and_then(Value::as_str)
        .is_some_and(|text|text.starts_with("New user instruction:"))
}
fn clipped(message:&Value)->Value {
    let mut result=message.clone();
    // Preserve call IDs, function names and their paired tool receipts.
    // Historical input arguments do not need to be paid for repeatedly.
    // Use valid JSON so OpenAI-compatible tool-call parsers still accept it.
    if role(message)=="assistant" {
        if let Some(calls)=result.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for call in calls {
                let raw=call.get("function").and_then(|f|f.get("arguments"))
                    .and_then(Value::as_str);
                if raw.is_some_and(|v|v.chars().count()>1200) {
                    call["function"]["arguments"]=json!(
                        json!({"historical_arguments_compacted":true}).to_string());
                }
            }
        }
    }
    if matches!(role(message),"assistant"|"tool") {
        if let Some(content)=message.get("content").and_then(Value::as_str) {
            let len=content.chars().count();
            if len>2200 {
                let prefix=content.chars().take(1400).collect::<String>();
                let suffix=content.chars().skip(len-600).collect::<String>();
                result["content"]=json!(format!("{prefix}\n[{} characters omitted from model view; persisted receipt is unchanged]\n{suffix}",len-2000));
            }
        }
    }
    result
}
/// Preserve complete tool-call exchanges as one projection unit, even if a
/// newer owner directive arrived while the original operation was in flight.
/// The durable journal is immutable; this is provider-facing ordering only.
fn exchanges(messages:&[Value])->Vec<Vec<Value>> {
    let mut output:Vec<Vec<Value>>=Vec::new();
    let mut awaiting=HashMap::<String,usize>::new();
    for item in messages {
        if role(item)=="assistant" {
            if let Some(calls)=item.get("tool_calls").and_then(Value::as_array){
                let idx=output.len();
                for call in calls {
                    if let Some(id)=call.get("id").and_then(Value::as_str){
                        awaiting.insert(id.to_owned(),idx);
                    }
                }
            }
            output.push(vec![item.clone()]);
        } else if role(item)=="tool" {
            let index=item.get("tool_call_id").and_then(Value::as_str)
                .and_then(|id|awaiting.remove(id));
            if let Some(idx)=index {
                output[idx].push(item.clone());
            } else {
                output.push(vec![item.clone()]);
            }
        } else {
            output.push(vec![item.clone()]);
        }
    }
    output
}
/// Direct user changes are part of the durable task contract, never ordinary
/// dialogue to summarize. Keep every direct user change verbatim in
/// the model projection, or pause before calling a paid provider if the
/// configured window is too small. Tool receipts are taken as atomic groups
/// so the provider never sees an orphaned tool result.
pub fn compact_strict(messages:&[Value],max_chars:usize)->Result<Projection,String>{
 let original_chars=messages.iter().map(size).sum::<usize>();
 // Do not silently discard older evidence or pay for a synthetic summary
 // while the exact transcript already fits. The old unconditional 16-group
 // cap lost meaningful context even for short tasks.
 if original_chars<=max_chars{
  // Even with no compaction, an owner correction can land between a started
  // tool call and its eventual receipt. Providers must see paired tool calls.
  let ordered=exchanges(messages).into_iter().flatten().collect::<Vec<_>>();
  return Ok(Projection{messages:ordered,original_chars,
   projected_chars:original_chars,omitted_messages:0});
 }
 if messages.len()<=2{
  if original_chars>max_chars{
   return Err(format!("task instructions exceed context limit ({original_chars}/{max_chars} chars); increase DOMINION_CONTEXT_MAX_CHARS"));
  }
  return Ok(Projection{messages:messages.to_vec(),original_chars,
   projected_chars:original_chars,omitted_messages:0});
 }
 let prefix=messages[..2].to_vec();
 let mut result=prefix;
 let changes=messages[2..].iter()
  .filter(|message|is_user_change(message))
  .filter_map(|message|message.get("content").and_then(Value::as_str))
  .collect::<Vec<_>>();
 if !changes.is_empty(){
  let content=format!("PERSISTENT TASK CONTRACT — direct user changes only. \
These requirements survive context compaction and must not be silently discarded:\n{}",
   changes.join("\n\n--- Next requirement ---\n\n"));
  result.push(json!({"role":"user","content":content}));
 }
 // Verifier feedback is a current corrective task, not an authoritative
 // user change. Preserve the latest request to repair missing artifacts even
 // when an oversized recent tool exchange would otherwise evict it.
 let latest_feedback=messages.iter().enumerate().rev()
  .find(|(_,message)|is_verifier_feedback(message));
 let feedback_index=latest_feedback.map(|(index,_)|index);
 if let Some((_,message))=latest_feedback{
  let raw=message["content"].as_str().unwrap_or("");
  let limit=(max_chars/8).clamp(240,1600);
  let length=raw.chars().count();
  let excerpt=if length>limit{
   let front=limit*3/4;
   let tail=limit-front;
   format!("{}\n[{} verifier characters omitted from model view; full feedback remains in durable Run history]\n{}",
    raw.chars().take(front).collect::<String>(),
    length-limit,
    raw.chars().skip(length-tail).collect::<String>())
  }else{raw.to_owned()};
  result.push(json!({"role":"user","content":format!(
   "CURRENT AUDITOR CORRECTION (advisory, not a new user instruction):\n{excerpt}")}));
 }
 let mandatory_chars=result.iter().map(size).sum::<usize>();
 // Reserve enough room for a lightweight omission marker. If the exact
 // user's instructions cannot fit, fail closed rather than shaving off the
 // end of a directive and silently altering the task.
 let reserve=192usize.min(max_chars/16);
 if mandatory_chars.saturating_add(reserve)>max_chars{
  return Err(format!(
   "persistent task constraints exceed context budget ({mandatory_chars}/{max_chars} chars); increase DOMINION_CONTEXT_MAX_CHARS"));
 }
 // Group function calls with corresponding results before removing any
 // history. The durable transcript remains completely unmodified.
 let history=messages[2..].iter().enumerate()
  .filter(|(index,message)|!is_user_change(message)
    && feedback_index!=Some(index+2))
  .map(|(_,message)|message.clone()).collect::<Vec<_>>();
 let groups=exchanges(&history);
 let mut selected=Vec::<(usize,Vec<Value>)>::new();
 let mut budget=max_chars-mandatory_chars-reserve;
 for (index,group) in groups.iter().enumerate().rev(){
  let bounded=group.iter().map(clipped).collect::<Vec<_>>();
  let cost=bounded.iter().map(size).sum::<usize>();
  if cost>budget {continue}
  budget-=cost;
  selected.push((index,bounded));
 }
 selected.sort_by_key(|(index,_)|*index);
 let kept=selected.iter().map(|(_,items)|items.len()).sum::<usize>();
 let omitted=history.len().saturating_sub(kept);
 if omitted>0{
  let notice=json!({"role":"user","content":format!(
   "{omitted} earlier non-contract messages remain in durable history. \
Re-read exact files and tool receipts before relying on old observations.")});
  if size(&notice)<=reserve {result.push(notice);}
 }
 for (_,group) in selected {result.extend(group);}
 let projected_chars=result.iter().map(size).sum::<usize>();
 debug_assert!(projected_chars<=max_chars);
 Ok(Projection{
  omitted_messages:messages.len().saturating_sub(result.len()),
  messages:result,original_chars,projected_chars
 })
}

/// Optional material is turn-scoped, not a new system instruction. Append it
/// AFTER the already-sent durable transcript so model prompt caches retain
/// their longest unchanged prefix. Inserting at index two invalidated every
/// subsequent historical message whenever a memory or repair hint changed.
/// Tool exchanges stay adjacent; provider-side caching remains best-effort.
pub fn insert_optional(messages:&mut Vec<Value>,value:Value,max_chars:usize)->bool{
 let used=messages.iter().map(size).sum::<usize>();
 if used.saturating_add(size(&value))>max_chars{return false}
 messages.push(value);
 true
}

/// Planning must not stringify an entire execution transcript into a paid
/// model request. Only explicit, authoritative user changes are injected;
/// tool/model text is excluded. The root instruction is passed separately.
pub fn planning_directives(messages:&[Value],max_chars:usize)->Result<String,String>{
 let directives=messages.iter().filter(|message|is_user_change(message))
  .filter_map(|message|message.get("content").and_then(Value::as_str))
  .collect::<Vec<_>>();
 let joined=directives.join("\n\n--- Next requirement ---\n\n");
 if joined.chars().count()>max_chars{
  return Err(format!("planning directives exceed {}-character context allowance",max_chars));
 }
 Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_tool_receipt_is_paired_before_new_user_message_without_touching_journal(){
        let messages=vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"publish after reading"}),
            json!({"role":"assistant","content":null,"tool_calls":[
                {"id":"a1","type":"function","function":{"name":"read_file","arguments":"{}"}},
                {"id":"a2","type":"function","function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"a1","content":"first"}),
            json!({"role":"user","content":"New user instruction: cancel future publishing"}),
            json!({"role":"tool","tool_call_id":"a2","content":"second"})
        ];
        let original=messages.clone();
        let output=compact_strict(&messages,10000).unwrap();
        assert_eq!(messages,original);
        assert_eq!(output.messages[2]["role"],"assistant");
        assert_eq!(output.messages[3]["tool_call_id"],"a1");
        assert_eq!(output.messages[4]["tool_call_id"],"a2");
        assert!(output.messages[5]["content"].as_str().unwrap().contains("cancel future"));
        let mut expanded=messages.clone();
        for i in 0..40{
            expanded.push(json!({"role":"assistant","content":format!("filler {i} {}", "x".repeat(1800))}));
        }
        let compressed=compact_strict(&expanded,6000).unwrap();
        let prompt=compressed.messages.iter().map(Value::to_string).collect::<String>();
        assert!(prompt.contains("cancel future publishing"),
            "owner directive must survive clipping and reordering");
        for (idx,msg) in compressed.messages.iter().enumerate() {
            if let Some(id)=msg.get("tool_call_id").and_then(Value::as_str) {
                assert!(idx>0);
                assert!(compressed.messages[idx-1]["tool_calls"].to_string().contains(id)
                    ||compressed.messages[idx-2]["tool_calls"].to_string().contains(id));
            }
        }
    }
    #[test]
    fn compaction_uses_available_context_budget_beyond_sixteen_small_exchanges(){
        let mut messages=vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"inspect the repository"})];
        messages.push(json!({"role":"assistant","content":"z".repeat(40_000)}));
        for i in 0..40{
            messages.push(json!({"role":"assistant","content":format!("evidence {i}")}));
        }
        let output=compact_strict(&messages,9000).unwrap();
        assert!(output.projected_chars<=9000);
        assert!(output.messages.iter().filter(|m|m["role"]=="assistant").count()>16,
            "history should be limited by actual budget rather than a 16-group artifact");
    }
    #[test]
    fn strict_projection_is_lossless_when_ordinary_history_fits(){
        let mut messages=vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"Build the actual artifact"})];
        // Before this fix a >16-group conversation was compacted even when
        // well inside the model's context budget.
        for i in 0..32{
            messages.push(json!({"role":"assistant","content":format!("Inspected file {i}: evidence")}));
        }
        let total=messages.iter().map(size).sum::<usize>();
        let view=compact_strict(&messages,total+1000).unwrap();
        assert_eq!(view.messages,messages);
        assert_eq!(view.original_chars,view.projected_chars);
        assert_eq!(view.omitted_messages,0);
    }
    #[test]
    fn strict_projection_keeps_latest_verifier_correction_when_tool_history_overflows(){
        let mut messages=vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"Fix application"})];
        messages.push(json!({"role":"user","content":
            "Verifier requested corrections. Produce artifact checksum and verify the file before PASS."}));
        for index in 0..24{
            let id=format!("tool-{index}");
            messages.push(json!({"role":"assistant","tool_calls":[{
                "id":id,"type":"function","function":{"name":"read_file","arguments":"{}"}}]}));
            messages.push(json!({"role":"tool","tool_call_id":id,
                "content":"unrelated source scan ".repeat(420)}));
        }
        let original=messages.clone();
        let view=compact_strict(&messages,7000).unwrap();
        assert_eq!(messages,original,"the durable transcript was modified");
        assert!(view.projected_chars<=7000);
        let prompt=view.messages.iter().map(Value::to_string).collect::<String>();
        assert!(prompt.contains("CURRENT AUDITOR CORRECTION"));
        assert!(prompt.contains("Produce artifact checksum"));
        for (index,message) in view.messages.iter().enumerate(){
            if let Some(id)=message.get("tool_call_id").and_then(Value::as_str){
                assert!(index>0);
                assert!(view.messages[index-1]["tool_calls"].to_string().contains(id),
                    "never emit an orphan read receipt");
            }
        }
    }
    #[test]
    fn strict_projection_pins_all_directives_and_keeps_receipt_pairs(){
        let mut messages=vec![json!({"role":"system","content":"stable"}),
            json!({"role":"user","content":"Original user goal"})];
        for index in 0..25 {
            messages.push(json!({"role":"user","content":format!(
             "New user instruction: preserve hard constraint number {index} for the final artifact")}));
            messages.push(json!({"role":"assistant","tool_calls":[{
             "id":format!("call-{index}"),"type":"function",
             "function":{"name":"read_file","arguments":"{}"}}]}));
            messages.push(json!({"role":"tool","tool_call_id":format!("call-{index}"),
             "content":"large fixture text ".repeat(500)}));
        }
        let original=messages.clone();
        let result=compact_strict(&messages,10000).unwrap();
        assert_eq!(messages,original);
        assert!(result.projected_chars<=10000);
        let view=result.messages.iter().map(Value::to_string).collect::<String>();
        for index in 0..25{
            assert!(view.contains(&format!("hard constraint number {index}")),
                "an earlier user requirement was silently dropped");
        }
        for (index,entry) in result.messages.iter().enumerate(){
            if let Some(id)=entry.get("tool_call_id").and_then(Value::as_str){
                assert!(index>0);
                assert!(result.messages[index-1]["tool_calls"].to_string().contains(id),
                    "tool result was orphaned by projection");
            }
        }
    }
    #[test]
    fn strict_projection_refuses_to_truncate_live_user_contract(){
        let mut messages=vec![json!({"role":"system","content":"stable"}),
            json!({"role":"user","content":"original"})];
        for _ in 0..10{
            messages.push(json!({"role":"user","content":format!(
             "New user instruction: {}", "protect this condition ".repeat(80))}));
        }
        let error=compact_strict(&messages,8000).unwrap_err();
        assert!(error.contains("persistent task constraints"));
    }
    #[test]
    fn planning_directives_exclude_model_and_tool_payloads(){
        let messages=vec![
            json!({"role":"assistant","content":"injected model phrase"}),
            json!({"role":"tool","content":"secret tool output"}),
            json!({"role":"user","content":"New user instruction: check recovery"}),
            json!({"role":"user","content":"Verifier requested corrections. inspect tests"})
        ];
        let plan=planning_directives(&messages,1000).unwrap();
        assert!(plan.contains("check recovery"));
        assert!(!plan.contains("inspect tests"),"verifier feedback is advisory and not a user directive");
        assert!(!plan.contains("injected model phrase"));
        assert!(!plan.contains("secret tool output"));
        assert!(planning_directives(&messages,20).is_err());
    }
    #[test]
    fn optional_memory_is_a_suffix_and_does_not_rebase_unchanged_history(){
        let mut messages=vec![json!({"role":"system","content":"stable shared instructions"}),
            json!({"role":"user","content":"original owner goal"}),
            json!({"role":"assistant","tool_calls":[{"id":"tool-1","type":"function",
                "function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"tool-1","content":"verified result"})];
        let initial=messages.clone();
        assert!(insert_optional(&mut messages,json!({"role":"user",
            "content":"Current scoped memory from a recorded owner note"}),10000));
        assert_eq!(&messages[..initial.len()],initial.as_slice(),
            "new optional context must not rewrite the cacheable transcript prefix");
        assert_eq!(messages[2]["tool_calls"][0]["id"],"tool-1");
        assert_eq!(messages[3]["tool_call_id"],"tool-1");
        assert_eq!(messages.last().unwrap()["role"],"user");
        let snapshot=messages.clone();
        assert!(!insert_optional(&mut messages,json!({"role":"user",
            "content":"x".repeat(10_000)}),10000));
        assert_eq!(messages,snapshot,"rejected hints must not mutate the model view");
    }
    #[test]
    fn unchanged_when_under_limit(){
        let msgs=vec![json!({"role":"system","content":"s"}),json!({"role":"user","content":"u"})];
        let p=compact_strict(&msgs,8000).unwrap();
        assert_eq!(p.messages,msgs);
    }

    #[test]
    fn historical_large_tool_arguments_are_elided_without_breaking_call_ids(){
        let mut msgs=vec![json!({"role":"system","content":"A system"}),
            json!({"role":"user","content":"Repair the code"})];
        for index in 0..20 {
            let id=format!("call-{index}");
            msgs.push(json!({"role":"assistant","tool_calls":[{
                "id":id,"type":"function","function":{"name":"read_file",
                "arguments":format!("{{\"path\":\"{}\"}}","a".repeat(6000))}}]}));
            msgs.push(json!({"role":"tool","tool_call_id":id,"content":"ok ".repeat(500)}));
        }
        let original=msgs.clone();
        let projection=compact_strict(&msgs,14000).unwrap();
        assert_eq!(msgs,original,"durable history was modified");
        assert!(projection.projected_chars<14000);
        assert!(projection.projected_chars*2<projection.original_chars);
        for message in &projection.messages {
            if let Some(calls)=message["tool_calls"].as_array(){
                for call in calls {
                    let args=call["function"]["arguments"].as_str().unwrap();
                    let _:Value=serde_json::from_str(args).unwrap();
                }
            }
        }
    }
    #[test]
    fn compresses_history_but_keeps_directives_and_tool_receipts_adjacent(){
        let mut msgs=vec![json!({"role":"system","content":"s"}),json!({"role":"user","content":"goal"})];
        for i in 0..20 {
            msgs.push(json!({"role":"assistant","tool_calls":[{"id":format!("c{i}"),"type":"function","function":{"name":"read_file","arguments":"{}"}}]}));
            msgs.push(json!({"role":"tool","tool_call_id":format!("c{i}"),"content":"result ".repeat(1200)}));
            if i==4 {msgs.push(json!({"role":"user","content":"New user instruction: preserve layout"}));}
        }
        let original=msgs.clone();
        let p=compact_strict(&msgs,13000).unwrap();
        assert_eq!(msgs,original);
        assert!(p.projected_chars<p.original_chars);
        assert!(p.messages.iter().any(|m|m.to_string().contains("preserve layout")));
        for (i,m) in p.messages.iter().enumerate() {
            if role(m)=="tool" {
                let id=m["tool_call_id"].as_str().unwrap();
                assert!(i>0);
                assert!(p.messages[i-1]["tool_calls"].to_string().contains(id));
            }
        }
    }
}
