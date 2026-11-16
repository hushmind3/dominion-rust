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

#[cfg(test)]
mod tests {
    use super::*;
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
