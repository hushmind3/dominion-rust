//! Model-authored bounded program steps, executed by the existing Run journal.
//! This is a durable cursor, NOT an alternative tool executor or agent loop.
//! Read and write calls flow through Engine::drive / journal::begin / finish.
//! The program may consume typed prior tool receipts without a model round.
use crate::{config::Settings,domain::{Run,ToolCall},policy,program_ir,tool_schema,tools};
use serde::{Deserialize,Serialize};
use serde_json::{json,Map,Value};
use sha2::{Digest,Sha256};
use std::collections::HashSet;

const MAX_STEPS:usize=12;
const MAX_REQUEST_BYTES:usize=24_000;
const MAX_OBSERVED_CHARS:usize=8_000;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ProgramStep {
    pub id:String,
    pub tool:String,
    #[serde(default)]
    pub arguments:Value,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub when:Option<Value>,
    #[serde(default)]
    pub on_error:Option<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ProgramSession{
    pub id:String,
    pub steps:Vec<ProgramStep>,
    pub cursor:usize,
    #[serde(default)]
    pub pending_step:Option<String>,
    #[serde(default)]
    pub observations:Map<String,Value>,
}
fn proof(raw:&str)->String{format!("{:x}",Sha256::digest(raw.as_bytes()))}
fn native_allowed(name:&str,shell_enabled:bool)->bool{
    matches!(name,"list_files"|"read_file"|"search_files"|"git_status"
        |"git_diff"|"write_file"|"edit_file")
        || (shell_enabled&&name=="shell")
}
fn dependencies(predicate:&Value,prior:&HashSet<String>,depth:usize)->Result<(),String>{
    if depth>8{return Err("program predicate nesting exceeded eight levels".into());}
    let ty=predicate["type"].as_str().ok_or("when.type must be a string")?;
    match ty {
        "exists"|"equals"|"compare"=>{
            let source=predicate["from"].as_str().ok_or("when.from required")?;
            if !prior.contains(source){return Err(format!("when predicate references future/unknown step: {source}"));}
            if predicate.get("path").is_some() && !predicate["path"].is_string(){
                return Err("when.path must be JSON pointer string".into());
            }
            Ok(())
        },
        "not"=>dependencies(&predicate["predicate"],prior,depth+1),
        "all"|"any"=>{
            let children=predicate["predicates"].as_array()
                .ok_or("when.predicates must be an array")?;
            if children.is_empty()||children.len()>12{
                return Err("when.predicates must include 1..12 predicates".into());
            }
            for child in children{dependencies(child,prior,depth+1)?;}
            Ok(())
        },
        _=>Err(format!("unsupported program predicate {ty}"))
    }
}
pub fn validate(run:&Run,request:&Value,settings:&Settings)->Result<ProgramSession,String>{
    if request.to_string().len()>MAX_REQUEST_BYTES{return Err("program request exceeds 24 KB".into());}
    let raw=request["steps"].as_array().ok_or("run_program needs steps array")?;
    if raw.is_empty()||raw.len()>MAX_STEPS{return Err("run_program needs 1..12 steps".into());}
    let mut seen=HashSet::new();
    let mut steps=Vec::new();
    let catalog=tools::definitions(settings.shell_enabled);
    for entry in raw {
        let step:ProgramStep=serde_json::from_value(entry.clone())
            .map_err(|error|format!("invalid program step: {error}"))?;
        if step.id.is_empty()||step.id.len()>40||
            !step.id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'){
            return Err("program step id must be short ASCII alphanumeric".into());
        }
        if !seen.insert(step.id.clone()){return Err("duplicate program step id".into());}
        if !native_allowed(&step.tool,settings.shell_enabled){
            return Err(format!("program cannot dispatch {}, only native workspace tools",step.tool));
        }
        if !policy::admitted_tool(run,&step.tool,&settings.denied_tools){
            return Err(format!("program step {} blocked by current tool policy",step.id));
        }
        if matches!(step.tool.as_str(),"git_status"|"git_diff") && run.repository.is_none(){
            return Err("Git steps require an attached repository".into());
        }
        if !matches!(step.on_error.as_deref(),None|Some("stop")|Some("continue")){
            return Err("on_error must be stop or continue".into());
        }
        if let Some(when)=&step.when{
            dependencies(when,&seen.iter().filter(|id|*id!=&step.id).cloned().collect(),0)?;
        }
        let fields=step.arguments.as_object().ok_or("step.arguments must be object")?;
        let mut dynamic=false;
        for (name,value) in fields {
            if let Some(source)=value.get("from").and_then(Value::as_str){
                if !seen.contains(source) || source==step.id{
                    return Err(format!("input {name} references unknown or future observation {source}"));
                }
                if value.as_object().is_some_and(|m|m.keys().any(|key|key!="from"&&key!="path"))
                    ||value.get("path").is_some_and(|x|!x.is_string()){
                    return Err("dynamic input binding accepts only from and path".into());
                }
                dynamic=true;
            }
        }
        if !dynamic {
            let call=ToolCall{id:format!("validation:{}",step.id),
                name:step.tool.clone(),arguments:step.arguments.clone()};
            tool_schema::admit(&call,&catalog).map_err(|e|format!("step {}: {}",step.id,e.reason))?;
        }
        steps.push(step);
    }
    Ok(ProgramSession{
        id:String::new(),steps,cursor:0,pending_step:None,observations:Map::new()
    })
}
/// Called in one Store mutation while the outer run_program tool intent is
/// durably in-flight. Replaying the pure install after a crash is idempotent.
pub fn install(run:&mut Run,call:&ToolCall,settings:&Settings)->Result<String,String>{
    if let Some(active)=&run.program_session {
        if active.id==call.id {
            return Ok(json!({"state":"already_installed","program_id":active.id,
                "steps":active.steps.len()}).to_string());
        }
        return Err("another program is active; complete it before submitting a new program".into());
    }
    // Avoid interleaving a model's unrelated sibling calls with program steps.
    if run.queued_calls.len()!=1 || run.queued_calls.first().is_none_or(|c|c.id!=call.id){
        return Err("run_program must be the only tool call in its model turn".into());
    }
    let mut program=validate(run,&call.arguments,settings)?;
    program.id=call.id.clone();
    let steps=program.steps.len();
    run.program_session=Some(program);
    run.event("program_installed",
        format!("Scheduled {steps} native tool operations without intermediate model calls"),
        Some(json!({"program_id":call.id,"steps":steps})));
    Ok(json!({"state":"scheduled","program_id":call.id,"steps":steps,
        "note":"Every step uses the ordinary Run journal, approval, tool policy and crash recovery"}).to_string())
}
/// One cursor transition per Store mutation. The caller only invokes this
/// when no pending approval, queued calls or side effects are in-flight.
/// Install and all progress updates use the same existing persistent Run.
pub fn advance(run:&mut Run,settings:&Settings)->Result<(),String>{
    if run.status!=crate::domain::RunStatus::Running || run.in_flight.is_some()
        ||run.pending_approval.is_some()||!run.queued_calls.is_empty(){
        return Err("program advance requires an idle running Run".into());
    }
    let Some(mut state)=run.program_session.take()else{return Ok(())};
    if let Some(id)=state.pending_step.take(){
        let receipt=run.messages.iter().rev().find(|m|
            m["role"]=="tool"&&m["tool_call_id"]==id);
        let Some(receipt)=receipt else{
            state.pending_step=Some(id);
            run.program_session=Some(state);
            return Ok(());
        };
        let body=receipt["content"].as_str().ok_or("program receipt must contain text")?;
        let evidence=run.events.iter().rev().find(|ev|
            matches!(ev.kind.as_str(),"tool_result"|"tool_error"|"tool_reconciled"
                |"tool_admission_rejected"|"tool_policy_denied"|"tool_hook_denied"|"tool_denied")
                &&ev.detail.as_ref().is_some_and(|v|v["call_id"]==id));
        let succeeded=evidence.is_some_and(|ev|
            matches!(ev.kind.as_str(),"tool_result"|"tool_reconciled")
                &&ev.detail.as_ref().is_some_and(|d|d["succeeded"]==true));
        let step=state.steps.get(state.cursor).ok_or("program cursor out of bounds")?;
        let body_value:Value=serde_json::from_str(body)
            .unwrap_or_else(|_|Value::String(body.chars().take(MAX_OBSERVED_CHARS).collect()));
        let clipped=body.chars().take(MAX_OBSERVED_CHARS).collect::<String>();
        let complete=state.id.clone();
        state.observations.insert(step.id.clone(),json!({
            "status":if succeeded{"ok"}else{"error"},
            "value":body_value,"text":clipped,"sha256":proof(body),
            "full_chars":body.chars().count()
        }));
        run.event("program_observed",
            format!("Native program step {}: {}",step.id,if succeeded{"ok"}else{"error"}),
            Some(json!({"program_id":complete,"step":step.id,
                "call_id":id,"succeeded":succeeded,"sha256":proof(body)})));
        let stopping=!succeeded&&step.on_error.as_deref()!=Some("continue");
        state.cursor+=1;
        if stopping {
            let message=format!("Program {} stopped after step {} failed. Review the real journal/tool error and replan only the remaining work.",state.id,step.id);
            run.messages.push(json!({"role":"user","content":message}));
            run.event("program_stopped",message,None);
            return Ok(());
        }
    }
    while state.cursor<state.steps.len(){
        let step=state.steps[state.cursor].clone();
        if let Some(when)=&step.when {
            let outcome=program_ir::predicate_verdict(when,
                &Value::Object(state.observations.clone()));
            match outcome{
                Ok(program_ir::Verdict::True)=>{},
                Ok(program_ir::Verdict::False)=>{
                    state.observations.insert(step.id.clone(),
                        json!({"status":"skipped","value":Value::Null}));
                    run.event("program_skipped",
                        format!("Skipped {} after a false observation predicate",step.id),
                        Some(json!({"program_id":state.id,"step":step.id})));
                    state.cursor+=1;
                    continue;
                },
                Ok(program_ir::Verdict::Unknown)=>{
                    let message=format!("Program {} could not determine predicate for step {}; missing evidence. Replan the unresolved step instead of assuming false.",state.id,step.id);
                    run.messages.push(json!({"role":"user","content":message}));
                    run.event("program_unknown",message,None);
                    return Ok(());
                },
                Err(error)=>{
                    let message=format!("Program {} predicate rejected for {}: {error}",state.id,step.id);
                    run.messages.push(json!({"role":"user","content":message.clone()}));
                    run.event("program_predicate_error",message,None);
                    return Ok(());
                }
            }
        }
        let arguments=match program_ir::resolve_inputs(
            &step.arguments,&Value::Object(state.observations.clone())){
            Ok(inputs)=>inputs,
            Err(error)=>{
                let reason=format!("Program {} input binding for {} rejected: {error}",state.id,step.id);
                run.messages.push(json!({"role":"user","content":reason.clone()}));
                run.event("program_binding_error",reason,None);
                return Ok(());
            }
        };
        let call=ToolCall{
            id:format!("program:{}:{}",state.id,step.id),name:step.tool.clone(),arguments
        };
        // Resolve after every tool outcome, then repeat schema + operator policy
        // admission before dispatch. Previously-approved schema is not authority.
        let catalog=tools::definitions(settings.shell_enabled);
        if let Err(error)=tool_schema::admit(&call,&catalog){
            let message=format!("Program step {} invalid after binding: {}",step.id,error.reason);
            run.messages.push(json!({"role":"user","content":message.clone()}));
            run.event("program_step_invalid",message,None);
            return Ok(());
        }
        if !policy::admitted_tool(run,&call.name,&settings.denied_tools){
            let message=format!("Program step {} denied by current operator policy",step.id);
            run.messages.push(json!({"role":"user","content":message.clone()}));
            run.event("program_step_denied",message,None);
            return Ok(());
        }
        // Extend the frozen accepted schema only for this internally generated
        // exact step, not with a new privilege or the whole tool catalog.
        if !run.advertised_tools.iter().any(|t|
            t.pointer("/function/name").and_then(Value::as_str)==Some(&call.name)){
            if let Some(schema)=catalog.iter().find(|t|
                t.pointer("/function/name").and_then(Value::as_str)==Some(&call.name)){
                run.advertised_tools.push(schema.clone());
            }
        }
        run.messages.push(json!({"role":"assistant","content":Value::Null,
            "tool_calls":[{"id":call.id,"type":"function",
                "function":{"name":call.name,"arguments":call.arguments.to_string()}}]}));
        run.queued_calls.push(call.clone());
        state.pending_step=Some(call.id.clone());
        run.event("program_step_ready",
            format!("Native program scheduling {} through existing Run tool queue",step.id),
            Some(json!({"program_id":state.id,"step":step.id,
                "tool":call.name,"call_id":call.id,"index":state.cursor})));
        run.program_session=Some(state);
        return Ok(());
    }
    run.event("program_completed",
        format!("Completed {} native program steps",state.steps.len()),
        Some(json!({"program_id":state.id,"observations":state.observations.len(),
            "model_calls_inside":0})));
    run.messages.push(json!({"role":"user","content":format!(
        "PROGRAM RECEIPTS: Program {} has finished {} native tool operations. \
         Examine real tool outputs and the artifact state before writing a final answer.",
        state.id,state.steps.len())}));
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::{CreateRun,RunStatus};
    fn run()->Run{
        let mut r=Run::new(CreateRun{instruction:"write then inspect".into(),
            budget_krw:Some(100.0),max_steps:Some(8)},8).unwrap();
        r.status=RunStatus::Running;
        r
    }
    #[test]
    fn rejects_unbounded_or_external_programs(){
        let s=Settings::from_env().unwrap();
        let r=run();
        assert!(validate(&r,&json!({"steps":[{"id":"x","tool":"mcp__remote",
            "arguments":{}}]}),&s).is_err());
        assert!(validate(&r,&json!({"steps":[{"id":"x","tool":"run_program",
            "arguments":{}}]}),&s).is_err());
        assert!(validate(&r,&json!({"steps":[{"id":"x","tool":"read_file",
            "arguments":{"path":{"from":"future","path":"/value"}}}]}),&s).is_err());
    }
    #[test]
    fn unknown_evidence_never_executes_a_destructive_branch(){
        let mut r=run();let s=Settings::from_env().unwrap();
        r.program_session=Some(ProgramSession{
            id:"p".into(),steps:vec![ProgramStep{id:"danger".into(),
                tool:"write_file".into(),arguments:json!({"path":"x","content":"x"}),
                when:Some(json!({"type":"equals","from":"missing","value":"ok"})),
                on_error:None}],cursor:0,pending_step:None,observations:Map::new()});
        advance(&mut r,&s).unwrap();
        assert!(r.program_session.is_none());
        assert!(r.queued_calls.is_empty());
        assert!(r.events.iter().any(|e|e.kind=="program_unknown"));
    }
    #[test]
    fn recovered_write_receipt_advances_without_replaying_mutation(){
        let mut r=run();let s=Settings::from_env().unwrap();
        let outer=ToolCall{id:"p".into(),name:"run_program".into(),
            arguments:json!({"steps":[{"id":"a","tool":"write_file",
                "arguments":{"path":"a.txt","content":"hello"}},
                {"id":"b","tool":"read_file","arguments":{"path":"a.txt"}}]})};
        r.queued_calls.push(outer.clone());
        crate::journal::begin(&mut r,&outer).unwrap();
        install(&mut r,&outer,&s).unwrap();
        crate::journal::finish(&mut r,&outer,&Ok("scheduled".into()),false).unwrap();
        advance(&mut r,&s).unwrap();
        assert_eq!(r.queued_calls.len(),1);
        let first=r.queued_calls[0].clone();
        crate::journal::begin(&mut r,&first).unwrap();
        crate::journal::finish(&mut r,&first,&Ok("wrote 5 bytes".into()),false).unwrap();
        let persisted=serde_json::to_string(&r).unwrap();
        let mut recovered:Run=serde_json::from_str(&persisted).unwrap();
        assert_eq!(recovered.program_session.as_ref().unwrap().cursor,0);
        advance(&mut recovered,&s).unwrap();
        assert_eq!(recovered.queued_calls.len(),1);
        assert_eq!(recovered.queued_calls[0].name,"read_file");
        assert!(recovered.events.iter().any(|e|e.kind=="program_observed"));
    }
}
