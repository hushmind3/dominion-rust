//! Context projection for the native Rust agent loop.
//! Full history stays durable; only outbound model requests are compacted.
use serde_json::{json,Value};

#[derive(Debug,Clone)]
pub struct Projection {
    pub messages:Vec<Value>,
    pub original_chars:usize,
    pub projected_chars:usize,
    pub omitted_messages:usize,
}
fn size(m:&Value)->usize { m.to_string().chars().count() }
fn role(m:&Value)->&str {m.get("role").and_then(Value::as_str).unwrap_or("")}
fn important(m:&Value)->bool {
    role(m)=="user" && m.get("content").and_then(Value::as_str).is_some_and(|s| {
        s.starts_with("New user instruction:") || s.starts_with("Verifier requested corrections.")
    })
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
fn exchanges(messages:&[Value])->Vec<Vec<Value>> {
    let mut output:Vec<Vec<Value>>=Vec::new();
    for item in messages {
        let tail_is_call=output.last().and_then(|group|group.first())
            .is_some_and(|first| role(first)=="assistant" && first.get("tool_calls").is_some());
        if role(item)=="tool" && tail_is_call {
            output.last_mut().unwrap().push(item.clone());
        } else {output.push(vec![item.clone()]);}
    }
    output
}
pub fn compact(messages:&[Value],max_chars:usize)->Projection {
    let original_chars=messages.iter().map(size).sum::<usize>();
    if original_chars<=max_chars || messages.len()<=2 {
        return Projection{messages:messages.to_vec(),original_chars,projected_chars:original_chars,omitted_messages:0};
    }
    let prefix=messages[..2].to_vec();
    let groups=exchanges(&messages[2..]);
    let mut selected:Vec<Vec<Value>>=vec![];
    let mut current=prefix.iter().map(size).sum::<usize>();
    let mut first_selected=groups.len();
    for (idx,group) in groups.iter().enumerate().rev() {
        let trimmed=group.iter().map(clipped).collect::<Vec<_>>();
        let delta=trimmed.iter().map(size).sum::<usize>();
        // Keep a proportional budget for brief recovered directives instead
        // of a fixed 2,500-char reservation on very small context models.
        let reserve=(max_chars/8).clamp(200,2500);
        if !selected.is_empty() && current.saturating_add(delta)>max_chars.saturating_sub(reserve) {break;}
        current+=delta;
        first_selected=idx;
        selected.push(trimmed);
        if selected.len()>=16 {break;}
    }
    selected.reverse();
    let hidden=&groups[..first_selected];
    let mut directives=hidden.iter().flat_map(|g|g.iter())
        .filter(|m|important(m))
        .filter_map(|m|m.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if directives.len()>5 {directives=directives.split_off(directives.len()-5);}
    let mut summary=format!("{} earlier messages retained in durable history. Re-read files and receipts before asserting facts.",hidden.iter().map(Vec::len).sum::<usize>());
    for directive in directives {
        summary.push_str("\nPRESERVED INSTRUCTION:\n");
        summary.push_str(&directive.chars().take(1100).collect::<String>());
    }
    let recent_errors=hidden.iter().flat_map(|g|g.iter())
        .filter(|m|role(m)=="tool")
        .filter_map(|m|m.get("content").and_then(Value::as_str))
        .filter(|text|text.contains("TOOL ERROR"))
        .rev().take(3).collect::<Vec<_>>();
    for err in recent_errors.into_iter().rev() {
        summary.push_str("\nEarlier error excerpt:\n");
        summary.push_str(&err.chars().take(500).collect::<String>());
    }
    let mut result=prefix;
    if !hidden.is_empty(){result.push(json!({"role":"user","content":summary}));}
    for group in selected {result.extend(group);}
    let projected_chars=result.iter().map(size).sum();
    Projection{
        omitted_messages:messages.len().saturating_sub(result.len()),
        messages:result,original_chars,projected_chars
    }
}



/// User changes and verifier corrections are part of the durable task
/// contract, not ordinary dialogue to summarize. Keep every one verbatim in
/// the model projection, or pause before calling a paid provider if the
/// configured window is too small. Tool receipts are taken as atomic groups
/// so the provider never sees an orphaned tool result.
pub fn compact_strict(messages:&[Value],max_chars:usize)->Result<Projection,String>{
 let original_chars=messages.iter().map(size).sum::<usize>();
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
  .filter(|message|important(message))
  .filter_map(|message|message.get("content").and_then(Value::as_str))
  .collect::<Vec<_>>();
 if !changes.is_empty(){
  let content=format!("PERSISTENT TASK CONTRACT — current user changes and verifier corrections. \
These requirements survive context compaction and must not be silently discarded:\n{}",
   changes.join("\n\n--- Next requirement ---\n\n"));
  result.push(json!({"role":"user","content":content}));
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
 let history=messages[2..].iter().filter(|message|!important(message))
  .cloned().collect::<Vec<_>>();
 let groups=exchanges(&history);
 let mut selected=Vec::<(usize,Vec<Value>)>::new();
 let mut budget=max_chars-mandatory_chars-reserve;
 for (index,group) in groups.iter().enumerate().rev(){
  let bounded=group.iter().map(clipped).collect::<Vec<_>>();
  let cost=bounded.iter().map(size).sum::<usize>();
  if cost>budget {continue}
  budget-=cost;
  selected.push((index,bounded));
  if selected.len()>=16{break}
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

/// Planning must not stringify an entire execution transcript into a paid
/// model request. Only explicit, authoritative user changes are injected;
/// tool/model text is excluded. The root instruction is passed separately.
pub fn planning_directives(messages:&[Value],max_chars:usize)->Result<String,String>{
 let directives=messages.iter().filter(|message|important(message))
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
        assert!(plan.contains("inspect tests"));
        assert!(!plan.contains("injected model phrase"));
        assert!(!plan.contains("secret tool output"));
        assert!(planning_directives(&messages,20).is_err());
    }
    #[test]
    fn unchanged_when_under_limit(){
        let msgs=vec![json!({"role":"system","content":"s"}),json!({"role":"user","content":"u"})];
        let p=compact(&msgs,8000);
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
        let projection=compact(&msgs,14000);
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
        let p=compact(&msgs,13000);
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
