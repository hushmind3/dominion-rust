//! Read-only execution program: a model selects a bounded inspection workflow
//! once; the native kernel evaluates dependencies, branches and file iteration
//! without a paid model turn per observation. No nested writes, shell, MCP,
//! account access or model calls are possible in this interpreter.
use crate::{config::Settings,domain::{Run,ToolCall},policy,tool_schema,tools};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},time::Duration};
use uuid::Uuid;

const MAX_STEPS:usize=12;
const MAX_ITERATION:usize=8;
const MAX_NESTED_READS:usize=24;
const MAX_RECEIPT_CHARS:usize=1500;

#[derive(Debug)]
struct Step<'a>{
    id:&'a str,
    tool:&'a str,
    arguments:Value,
    condition:Option<(&'a str,&'a str,bool)>,
    from:Option<&'a str>,
    suffix:&'a str,
    needle:&'a str,
    max_files:usize,
    lines_containing:Option<&'a str>,
}
fn str_field<'a>(value:&'a Value,key:&str)->Result<&'a str,String>{
    value.get(key).and_then(Value::as_str).ok_or_else(||format!("read_program step needs {key}"))
}
fn parse<'a>(request:&'a Value,run:&Run,settings:&Settings)->Result<Vec<Step<'a>>,String>{
    if request.to_string().len()>24_000{return Err("read_program request exceeds 24 KB".into());}
    let steps=request.get("steps").and_then(Value::as_array)
        .ok_or("read_program requires steps array")?;
    if steps.is_empty()||steps.len()>MAX_STEPS {
        return Err(format!("read_program needs 1..{MAX_STEPS} steps"));
    }
    let mut ids=HashMap::<&str,&str>::new();
    let mut parsed=Vec::with_capacity(steps.len());
    let schemas=tools::definitions(false);
    for raw in steps {
        let id=str_field(raw,"id")?;
        if id.is_empty()||id.len()>40||!id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-'){
            return Err("step id must be 1..40 ASCII alphanumeric, _ or -".into());
        }
        if ids.contains_key(id){return Err(format!("duplicate step id: {id}"));}
        let tool=str_field(raw,"tool")?;
        if !matches!(tool,"list_files"|"read_file"|"search_files"|"git_status"|"git_diff"|"read_listed_files"){
            return Err(format!("read_program forbids nested tool {tool}"));
        }
        let condition=if let Some(when)=raw.get("when"){
            let previous=str_field(when,"step")?;
            let contains=str_field(when,"contains")?;
            if !ids.contains_key(previous)||contains.is_empty()||contains.len()>256{
                return Err("when must reference a preceding step with a 1..256 character marker".into());
            }
            Some((previous,contains,when.get("not").and_then(Value::as_bool).unwrap_or(false)))
        }else{None};
        let lines_containing=raw.get("lines_containing").map(|v|v.as_str()
            .ok_or("lines_containing must be text")).transpose()?;
        if lines_containing.is_some_and(|v|v.is_empty()||v.len()>256){
            return Err("lines_containing must contain 1..256 characters".into());
        }
        let arguments=raw.get("arguments").cloned().unwrap_or_else(||json!({}));
        let mut from=None;
        let mut suffix="";
        let mut needle="";
        let mut max_files=1;
        if tool=="read_listed_files" {
            if !policy::admitted_tool(run,"read_file",&settings.denied_tools){
                return Err("read_listed_files requires authorized read_file".into());
            }
            let reference=str_field(raw,"from")?;
            if ids.get(reference).copied()!=Some("list_files"){
                return Err("read_listed_files from must reference a previous list_files step".into());
            }
            from=Some(reference);
            suffix=raw.get("suffix").and_then(Value::as_str).unwrap_or("");
            needle=raw.get("contains").and_then(Value::as_str).unwrap_or("");
            if suffix.len()>80||needle.len()>256{return Err("iteration filters too long".into());}
            max_files=raw.get("max_files").and_then(Value::as_u64)
                .unwrap_or(4).try_into().map_err(|_|"max_files is invalid")?;
            if !(1..=MAX_ITERATION).contains(&max_files){return Err("max_files must be 1..8".into());}
            if raw.get("arguments").is_some(){return Err("read_listed_files does not accept arguments".into());}
        }else{
            if !policy::admitted_tool(run,tool,&settings.denied_tools){
                return Err(format!("nested tool disabled: {tool}"));
            }
            let call=ToolCall{id:format!("read-program-validation-{id}"),
                name:tool.to_string(),arguments:arguments.clone()};
            tool_schema::admit(&call,&schemas).map_err(|e|format!("step {id}: {}",e.reason))?;
            if matches!(tool,"git_status"|"git_diff") && run.repository.is_none(){
                return Err("Git inspection requires an attached repository".into());
            }
        }
        ids.insert(id,tool);
        parsed.push(Step{id,tool,arguments,condition,from,suffix,needle,max_files,lines_containing});
    }
    Ok(parsed)
}
fn bounded_text(text:&str,contains:Option<&str>)->String{
    let selected=match contains {
        Some(needle)=>text.lines().filter(|line|line.contains(needle))
            .take(24).collect::<Vec<_>>().join("\n"),
        None=>text.to_owned()
    };
    let total=selected.chars().count();
    if total<=MAX_RECEIPT_CHARS{return selected;}
    let clip=selected.chars().take(MAX_RECEIPT_CHARS).collect::<String>();
    format!("{clip}\n[{} further characters omitted; use direct read_file if needed]",total-MAX_RECEIPT_CHARS)
}
async fn read_many(settings:&Settings,id:Uuid,paths:&str,step:&Step<'_>)
    ->Result<(String,usize),String>{
    let listed:Vec<String>=serde_json::from_str(paths)
        .map_err(|_|"list_files receipt is not a JSON string array")?;
    let mut inspected=0;
    let mut results=Vec::new();
    for path in listed.into_iter().filter(|p|p.ends_with(step.suffix)).take(step.max_files){
        inspected+=1;
        let call=ToolCall{id:format!("read-program:{}:{}",step.id,inspected),
            name:"read_file".into(),arguments:json!({"path":path})};
        let outcome=tokio::time::timeout(Duration::from_secs(30),
            tools::execute(settings,id,&call)).await
            .map_err(|_|"read_listed_files read timed out")?;
        match outcome {
            Ok(content)=>{
                if step.needle.is_empty()||content.contains(step.needle){
                    results.push(json!({"path":path,
                        "text":bounded_text(&content,step.lines_containing),
                        "matched":true}));
                }
            }
            Err(error)=>results.push(json!({"path":path,"error":error}))
        }
    }
    Ok((json!({"inspected":inspected,"matches":results}).to_string(),inspected))
}

/// A failed nested read is a receipt (not a forged success); later steps can
/// branch on it. The top-level call remains one atomic durable tool intent.
pub async fn execute(settings:&Settings,run:&Run,request:&Value)->Result<String,String>{
    let steps=parse(request,run,settings)?;
    let mut raw=HashMap::<String,String>::new();
    let mut receipts=Vec::new();
    let mut calls=0usize;
    let mut failures=0usize;
    for mut step in steps {
        let id=step.id.to_string();
        if calls>=MAX_NESTED_READS {
            failures+=1;
            receipts.push(json!({"id":id,"status":"error",
                "error":"read_program nested read limit of 24 reached"}));
            raw.insert(id,"TOOL ERROR: read limit reached".into());
            continue;
        }
        if step.tool=="read_listed_files" {
            step.max_files=step.max_files.min(MAX_NESTED_READS-calls);
        }
        if let Some((previous,needle,negate))=step.condition{
            let previous_text=raw.get(previous).map(String::as_str).unwrap_or("");
            let present=previous_text.contains(needle);
            if present==negate{
                receipts.push(json!({"id":id,"status":"skipped","reason":"condition_not_met"}));
                raw.insert(id,String::new());
                continue;
            }
        }
        let result=if step.tool=="read_listed_files"{
            let source=raw.get(step.from.unwrap()).ok_or("missing list_files result")?;
            read_many(settings,run.id,source, &step).await
        }else{
            let call=ToolCall{id:format!("read-program:{}",step.id),
                name:step.tool.to_string(),arguments:step.arguments};
            calls+=1;
            tokio::time::timeout(Duration::from_secs(30),
                tools::execute(settings,run.id,&call)).await
                .map_err(|_|format!("nested {} timed out",step.tool))?
                .map(|text|(text,1))
        };
        match result {
            Ok((text,nested_calls))=>{
                if step.tool=="read_listed_files"{calls+=nested_calls;}
                receipts.push(json!({"id":id,"tool":step.tool,"status":"ok",
                    "result":bounded_text(&text,step.lines_containing),
                    "full_chars":text.chars().count()}));
                raw.insert(id,text);
            },
            Err(error)=>{
                failures+=1;
                let error=error.chars().take(450).collect::<String>();
                receipts.push(json!({"id":id,"tool":step.tool,"status":"error","error":error}));
                raw.insert(id,format!("TOOL ERROR: {error}"));
            }
        }
    }
    let result=json!({"kind":"read_program","model_calls_inside":0,
        "nested_read_calls":calls,"failed_steps":failures,"steps":receipts});
    // Model-visible evidence is bounded independently of raw tool outputs.
    // If a large program overflows, the model can issue narrower steps.
    if result.to_string().chars().count()>30_000{
        return Err("read_program output too large; narrow inspection filters".into());
    }
    Ok(result.to_string())
}
#[cfg(test)]
mod tests {
 use super::*;
 use crate::domain::CreateRun;
 #[tokio::test]
 async fn condition_branch_and_file_iteration_use_no_model_calls(){
    let mut settings=Settings::from_env().unwrap();
    let root=std::env::temp_dir().join(format!("dominion-read-program-{}",Uuid::new_v4()));
    settings.workspace_dir=root.clone();
    let run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:Some(10.0),max_steps:Some(3)},3).unwrap();
    let folder=root.join(run.id.to_string());
    tokio::fs::create_dir_all(&folder).await.unwrap();
    tokio::fs::write(folder.join("a.rs"),"fn alpha() { answer = 42; }\n").await.unwrap();
    tokio::fs::write(folder.join("b.rs"),"fn beta() { answer = 43; }\n").await.unwrap();
    tokio::fs::write(folder.join("third.txt"),"do not inspect").await.unwrap();
    let result=execute(&settings,&run,&json!({"steps":[
        {"id":"files","tool":"list_files","arguments":{"path":""}},
        {"id":"found","tool":"read_listed_files","from":"files","suffix":".rs",
         "contains":"answer","max_files":3},
        {"id":"skip","tool":"read_file","arguments":{"path":"a.rs"},
         "when":{"step":"files","contains":"does-not-exist"}},
        {"id":"read","tool":"read_file","arguments":{"path":"b.rs"},
         "when":{"step":"files","contains":"b.rs"},"lines_containing":"answer"}
    ]})).await.unwrap();
    let data:Value=serde_json::from_str(&result).unwrap();
    assert_eq!(data["model_calls_inside"],0);
    assert_eq!(data["nested_read_calls"],4);
    assert_eq!(data["steps"][2]["status"],"skipped");
    assert_eq!(data["steps"][3]["status"],"ok");
    assert!(result.contains("a.rs")&&result.contains("b.rs"));
    assert!(!result.contains("do not inspect"));
    tokio::fs::remove_dir_all(root).await.unwrap();
 }
 #[tokio::test]
 async fn nested_write_shell_and_operator_denials_fail_before_dispatch(){
    let settings=Settings::from_env().unwrap();
    let run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:None,max_steps:None},3).unwrap();
    for name in ["shell","write_file","memory_store","mcp__anything","compare_strategies"]{
        assert!(execute(&settings,&run,&json!({"steps":[
            {"id":"x","tool":name,"arguments":{"path":"a","content":"changed"}}
        ]})).await.is_err(),"{name} escaped read-only interpreter");
    }
    let mut denied=settings.clone();
    denied.denied_tools.push("read_file".into());
    assert!(execute(&denied,&run,&json!({"steps":[
        {"id":"x","tool":"read_file","arguments":{"path":"a"}}
    ]})).await.is_err());
 }
 #[test]
 fn rejects_forward_dependencies_and_unbounded_iteration(){
    let settings=Settings::from_env().unwrap();
    let run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:None,max_steps:None},3).unwrap();
    assert!(parse(&json!({"steps":[
        {"id":"later","tool":"read_listed_files","from":"upcoming","max_files":2},
        {"id":"upcoming","tool":"list_files","arguments":{"path":""}}
    ]}),&run,&settings).is_err());
    assert!(parse(&json!({"steps":[
        {"id":"listing","tool":"list_files","arguments":{"path":""}},
        {"id":"nested","tool":"read_listed_files","from":"listing","max_files":100000}
    ]}),&run,&settings).is_err());
 }
 #[test]
 fn interrupted_program_is_replayed_as_one_safe_read_only_call(){
    let mut run=Run::new(CreateRun{instruction:"inspect".into(),
        budget_krw:Some(20.0),max_steps:Some(4)},4).unwrap();
    run.status=crate::domain::RunStatus::Running;
    let call=ToolCall{id:"program-1".into(),name:"read_program".into(),
        arguments:json!({"steps":[{"id":"files","tool":"list_files","arguments":{"path":""}}]})};
    run.queued_calls.push(call.clone());
    crate::journal::begin(&mut run,&call).unwrap();
    assert!(run.in_flight.is_some());
    assert!(crate::journal::recover(&mut run));
    assert!(run.in_flight.is_none());
    assert_eq!(run.queued_calls.len(),1);
    assert!(run.events.iter().any(|e|e.kind=="tool_retry_safe"));
 }

}
