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

/// 2026 evidence-indexed working context. Old tool bodies are not removed from
/// the durable Run: the model sees a stable, bounded preview and can load the
/// exact receipt by ID/offset using read_tool_receipt. The two most recent tool
/// receipts remain exact, along with original owner instructions/corrections.
/// This prevents an already-successful 16KB read from being repaid in every
/// subsequent model round just because the hard context window is still free.
pub fn compact_execution(messages:&[Value],max_chars:usize)->Result<Projection,String>{
    let original_chars=messages.iter().map(size).sum::<usize>();
    let receipts=messages.iter().filter(|m|role(m)=="tool")
        .filter_map(|m|m.get("tool_call_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if receipts.len()<3{return compact_strict(messages,max_chars);}
    let recent=&receipts[receipts.len()-2..];
    let mut projected=messages.to_vec();
    let mut reclaimed=0usize;
    for (index,message) in messages.iter().enumerate().skip(2){
        if role(message)=="tool"{
            let Some(id)=message.get("tool_call_id").and_then(Value::as_str)else{continue;};
            if recent.contains(&id){continue;}
            let Some(body)=message.get("content").and_then(Value::as_str)else{continue;};
            let length=body.chars().count();
            if length<=1200{continue;}
            let head=body.chars().take(620).collect::<String>();
            let tail=body.chars().skip(length.saturating_sub(180)).collect::<String>();
            projected[index]["content"]=json!(format!(
                "{head}\n[OLDER RECEIPT: {id}; {length} characters in durable Run; \
use read_tool_receipt with call_id={id}, offset and limit to inspect exact evidence]\n{tail}"));
            reclaimed+=1;
        }else if role(message)=="assistant"{
            let tool_calls=message.get("tool_calls").and_then(Value::as_array);
            if tool_calls.is_some_and(|calls|calls.iter().any(|call|
                call.get("id").and_then(Value::as_str)
                    .is_some_and(|id|!recent.contains(&id)))){
                projected[index]=clipped(message);
            }
        }
    }
    if reclaimed==0{return compact_strict(messages,max_chars);}
    // Soft target controls repeated prompt work, not execution eligibility.
    // If exact user constraints exceed it, the configured hard bound takes over.
    let target=(max_chars.saturating_mul(2)/5).clamp(12000,36000).min(max_chars);
    let mut view=compact_strict(&projected,target)
        .or_else(|_|compact_strict(&projected,max_chars))?;
    view.original_chars=original_chars;
    view.projected_chars=view.messages.iter().map(size).sum();
    Ok(view)
}

/// Bounded, exact, run-local lookup into archived tool evidence. Never executes
/// the tool again, touches another Run, or trusts a model-supplied file path.
pub fn receipt_slice(messages:&[Value],call_id:&str,offset:usize,limit:usize)
    ->Result<Value,String>{
    if call_id.is_empty()||call_id.len()>256{return Err("invalid receipt call ID".into());}
    let content=messages.iter().find(|m|role(m)=="tool"
        &&m.get("tool_call_id").and_then(Value::as_str)==Some(call_id))
        .and_then(|m|m.get("content")).and_then(Value::as_str)
        .ok_or("tool receipt not present in this Run")?;
    let total=content.chars().count();
    if offset>total{return Err("receipt offset beyond available characters".into());}
    let take=limit.clamp(1,6000);
    let slice=content.chars().skip(offset).take(take).collect::<String>();
    let next=offset.saturating_add(slice.chars().count());
    Ok(json!({"call_id":call_id,"offset":offset,"total_chars":total,
        "next_offset":next,"complete":next>=total,"content":slice}))
}

/// Search exact, durable tool receipts in THIS Run without replaying effects.
/// This is an ephemeral evidence projection, never a second index of record.
/// The result gives character offsets for read_tool_receipt and reports when
/// the bounded scan could not inspect the entire Run.
pub fn receipt_search(messages:&[Value],query:&str,limit:usize)->Result<Value,String>{
    const MAX_SCAN_BYTES:usize=16*1024*1024;
    let query=query.trim();
    if !(2..=128).contains(&query.chars().count()){
        return Err("receipt search query must contain 2..128 characters".into());
    }
    let terms=query.split_whitespace()
        .filter(|word|word.chars().count()>=2).take(8)
        .map(str::to_ascii_lowercase).collect::<Vec<_>>();
    if terms.is_empty(){return Err("receipt search needs a term of at least 2 characters".into());}
    let phrase=query.to_ascii_lowercase();
    let mut names=HashMap::<&str,&str>::new();
    for message in messages {
        if role(message)!="assistant"{continue;}
        if let Some(calls)=message.get("tool_calls").and_then(Value::as_array){
            for call in calls {
                if let (Some(id),Some(name))=(
                    call.get("id").and_then(Value::as_str),
                    call.pointer("/function/name").and_then(Value::as_str)){
                    names.entry(id).or_insert(name);
                }
            }
        }
    }
    let mut scanned_bytes=0usize;
    let mut scanned_receipts=0usize;
    let mut scan_truncated=false;
    let mut found=Vec::<(usize,usize,Value)>::new();
    for (recent_index,message) in messages.iter().rev().enumerate(){
        if role(message)!="tool"{continue;}
        let Some(id)=message.get("tool_call_id").and_then(Value::as_str)else{continue;};
        let name=names.get(id).copied().unwrap_or("unknown");
        // A lookup's own output must not become a recursively ranked source.
        if matches!(name,"read_tool_receipt"|"search_tool_receipts"){continue;}
        let Some(content)=message.get("content").and_then(Value::as_str)else{continue;};
        if scanned_bytes.saturating_add(content.len())>MAX_SCAN_BYTES{
            scan_truncated=true;
            break;
        }
        scanned_bytes+=content.len();
        scanned_receipts+=1;
        let haystack=content.to_ascii_lowercase();
        let exact=haystack.find(&phrase);
        let mut first=exact;
        let mut matched_terms=0usize;
        for term in &terms {
            if let Some(position)=haystack.find(term){
                matched_terms+=1;
                first=Some(first.map_or(position,|current|current.min(position)));
            }
        }
        let Some(byte_offset)=first else{continue;};
        // ASCII-only case folding keeps byte offsets valid for UTF-8 Korean
        // and Chinese receipt bodies without changing stored evidence.
        let match_offset=content[..byte_offset].chars().count();
        let offset=match_offset.saturating_sub(120);
        let snippet=content.chars().skip(offset).take(480).collect::<String>();
        let score=matched_terms*8+if exact.is_some(){40}else{0};
        found.push((score,recent_index,json!({
            "call_id":id,"tool":name,"offset":offset,
            "match_offset":match_offset,"total_chars":content.chars().count(),
            "matched_terms":matched_terms,"snippet":snippet
        })));
    }
    found.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)));
    let total_matches=found.len();
    let results=found.into_iter().take(limit.clamp(1,8))
        .map(|(_,_,value)|value).collect::<Vec<_>>();
    Ok(json!({
        "scope":"current_run","query":query,"total_matches":total_matches,
        "scanned_receipts":scanned_receipts,"scan_truncated":scan_truncated,
        "results":results
    }))
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
    fn execution_view_shrinks_old_receipts_before_the_hard_context_limit(){
        let mut msgs=vec![json!({"role":"system","content":"stable"}),
            json!({"role":"user","content":"Build artifact; preserve exact constraints"})];
        for i in 0..5 {
            msgs.push(json!({"role":"assistant","tool_calls":[{
                "id":format!("t{i}"),"type":"function",
                "function":{"name":"read_file","arguments":"{}"}}]}));
            msgs.push(json!({"role":"tool","tool_call_id":format!("t{i}"),
                "content":format!("SOURCE {i}: {} :END{i}", "x".repeat(7000))}));
        }
        let full=msgs.clone();
        let original=msgs.iter().map(size).sum::<usize>();
        assert!(original<80000);
        assert_eq!(compact_strict(&msgs,80000).unwrap().projected_chars,original);
        let view=compact_execution(&msgs,80000).unwrap();
        assert_eq!(msgs,full,"the durable execution history must remain exact");
        assert!(view.projected_chars*2<original,
            "old tool receipts must not be retransmitted in full while under the hard cap");
        let tool=|id:&str|view.messages.iter().find(|m|m["tool_call_id"]==id).unwrap();
        assert!(tool("t0")["content"].as_str().unwrap().contains("read_tool_receipt"));
        assert_eq!(tool("t3")["content"],full[9]["content"]);
        assert_eq!(tool("t4")["content"],full[11]["content"]);
        for (i,m) in view.messages.iter().enumerate() {
            if let Some(id)=m.get("tool_call_id").and_then(Value::as_str){
                assert!(i>0&&view.messages[i-1]["tool_calls"].to_string().contains(id),
                    "never detach a receipt from its tool call");
            }
        }
    }
    #[test]
    fn exact_receipt_retrieval_is_paginated_and_never_crosses_run_boundaries(){
        let msgs=vec![json!({"role":"tool","tool_call_id":"a","content":"가나다라마"})];
        let page=receipt_slice(&msgs,"a",1,2).unwrap();
        assert_eq!(page["content"],"나다");
        assert_eq!(page["next_offset"],3);
        assert_eq!(page["total_chars"],5);
        assert_eq!(page["complete"],false);
        assert!(receipt_slice(&msgs,"b",0,2).is_err());
        assert!(receipt_slice(&msgs,"a",8,2).is_err());
    }
    #[test]
    fn receipt_search_finds_exact_evidence_hidden_by_context_compaction(){
        let original_body=format!("{}중요한 배포 증거: 검증 완료{}",
            "unrelated source ".repeat(400)," end".repeat(200));
        let messages=vec![
            json!({"role":"system","content":"stable"}),
            json!({"role":"user","content":"Verify deployment"}),
            json!({"role":"assistant","tool_calls":[{"id":"source-1",
                "type":"function","function":{"name":"read_file","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"source-1","content":original_body}),
            json!({"role":"assistant","tool_calls":[{"id":"recall-1",
                "type":"function","function":{"name":"read_tool_receipt","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"recall-1",
                "content":"중요한 배포 증거: prior lookup should not rank"})
        ];
        let durable=messages.clone();
        let found=receipt_search(&messages,"중요한 배포 증거",4).unwrap();
        assert_eq!(found["scope"],"current_run");
        assert_eq!(found["total_matches"],1,"do not re-index previous lookups");
        assert_eq!(found["results"][0]["call_id"],"source-1");
        assert_eq!(found["scan_truncated"],false);
        let match_offset=found["results"][0]["match_offset"].as_u64().unwrap() as usize;
        let exact=receipt_slice(&messages,"source-1",match_offset,20).unwrap();
        assert!(exact["content"].as_str().unwrap().starts_with("중요한 배포 증거"));
        assert_eq!(messages,durable,"search must never rewrite the durable Run");
        assert_eq!(receipt_search(&messages,"unfindable",4).unwrap()["total_matches"],0);
        assert!(receipt_search(&messages," ",4).is_err());
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
