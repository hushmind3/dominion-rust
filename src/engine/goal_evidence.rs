//! Bounded, evidence-preserving summary for planner/reviewer model calls.
//! The original evidence stays in the immutable job / verifier; never score
//! completion from this lossy synopsis. This is not semantic caching.
use crate::engine::verification;
use serde_json::{json,Value};

fn clip(text:&str,limit:usize)->String {
    let length=text.chars().count();
    let mut result=text.chars().take(limit).collect::<String>();
    if length>limit {result.push_str(" …[truncated]");}
    result
}
fn selected_args(source:&Value)->Value {
    let mut obj=serde_json::Map::new();
    for key in ["path","cwd","command","args","startLine","endLine",
                "sourceSha256","expectedSha256","oldText"] {
        if let Some(v)=source.get(key) {
            let text=match v {
                Value::String(s)=>clip(s,240),
                _=>clip(&v.to_string(),240),
            };
            obj.insert(key.into(),json!(text));
        }
    }
    Value::Object(obj)
}
fn selected_result(source:&Value)->Value {
    let mut obj=serde_json::Map::new();
    for key in ["exitCode","written","verified","postSha256","bytes","status",
                "isError","success","error","message","path","changed","updated"] {
        if let Some(v)=source.get(key) {
            let clipped=if let Some(s)=v.as_str(){json!(clip(s,320))}
                else{v.clone()};
            obj.insert(key.into(),clipped);
        }
    }
    // Content is evidence only as an excerpt. The deterministic acceptance
    // checker continues to examine complete, original tool outputs.
    for key in ["stdout","stderr","content","output"] {
        if let Some(s)=source[key].as_str(){
            obj.insert(format!("{key}Excerpt"),json!(clip(s,360)));
        }
    }
    Value::Object(obj)
}
pub fn synopsis(evidence:&[Value],char_limit:usize)->String {
    let cap=char_limit.clamp(1000,16_000);
    let mut items=Vec::new();
    let mut taken=0usize;
    // Prefer latest operations; a file-artifact manifest separately lists
    // all output paths, even when early verbose tool observations are omitted.
    for item in evidence.iter().rev().take(64){
        let tool=verification::tool_name(item);
        if tool.is_empty(){continue;}
        let args=item.get("arguments").or_else(||item.get("args"))
            .unwrap_or(&Value::Null);
        let line=json!({"tool":tool,"arguments":selected_args(args),
            "result":selected_result(&item["result"])}).to_string();
        if taken+line.chars().count()+2>cap.saturating_sub(180){break;}
        taken+=line.chars().count()+2;
        items.push(line);
    }
    items.reverse();
    let omitted=evidence.len().saturating_sub(items.len());
    let mut result=format!("도구 증거 요약 (원본 총 {}건, 생략 {}건; 검증기는 원본 사용):\n",
        evidence.len(),omitted);
    result.push_str(&items.join("\n"));
    clip(&result,cap)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_long_tool_output_without_claiming_completeness(){
        let events=vec![json!({"tool":"terminal_exec","arguments":{"command":"cargo",
            "args":["test"]},"result":{"exitCode":0,"stdout":"x".repeat(50_000)}})];
        let s=synopsis(&events,1300);
        assert!(s.chars().count()<=1300);
        assert!(s.contains("terminal_exec"));
        assert!(s.contains("exitCode"));
        assert!(!s.contains(&"x".repeat(500)));
    }
}
