//! Deterministic stuck-tool protection for the native Rust execution loop.
//! Consult only durable assistant tool calls and tool receipts. No model
//! inference, heuristic on tool names, or second execution scheduler.
use crate::domain::{Run,ToolCall};
use serde_json::Value;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action { Reject, Pause }

/// Two identical failed attempts trigger a corrective receipt. If the model
/// repeats that same failing call four or more times, halt instead of burning
/// another paid model turn. Any successful tool result, different arguments
/// or new user instruction resets this consecutive-failure detector.
pub fn action(run:&Run,call:&ToolCall)->Option<Action>{
 let count=consecutive_failures(run,call);
 if count>=4 {Some(Action::Pause)}
 else if count>=2 {Some(Action::Reject)}
 else {None}
}

pub fn consecutive_failures(run:&Run,candidate:&ToolCall)->usize{
 let mut seen=0;
 for message in run.messages.iter().rev(){
  match message.get("role").and_then(Value::as_str){
   Some("user")=>break, // a new directive can legitimately change the task
   Some("tool")=>{
    let content=message["content"].as_str().unwrap_or("");
    if !content.starts_with("TOOL ERROR"){break;}
    let Some(id)=message["tool_call_id"].as_str()else{break};
    let Some(call)=find_call(&run.messages,id)else{break};
    if call.0!=candidate.name||call.1!=candidate.arguments{break;}
    seen+=1;
   },
   _=>{}
  }
 }
 seen
}
fn find_call<'a>(messages:&'a [Value],id:&str)->Option<(&'a str,Value)>{
 for row in messages.iter().rev(){
  let Some(calls)=row.get("tool_calls").and_then(Value::as_array)else{continue};
  for candidate in calls {
   if candidate.get("id").and_then(Value::as_str)!=Some(id){continue;}
   let function=candidate.get("function")?;
   let name=function.get("name").and_then(Value::as_str)?;
   let args=function.get("arguments").and_then(Value::as_str)?;
   return serde_json::from_str::<Value>(args).ok().map(|value|(name,value));
  }
 }
 None
}

#[cfg(test)]
mod tests{
 use super::*;
 use crate::domain::{CreateRun,RunStatus};
 use crate::journal;
 use serde_json::json;
 fn fixture()->Run{
  let mut run=Run::new(CreateRun{
   instruction:"inspect workspace".into(),budget_krw:None,max_steps:None
  },20).unwrap();
  run.status=RunStatus::Running;
  run
 }
 fn attempt(run:&mut Run,name:&str,path:&str,success:bool){
  let id=format!("call-{}",run.messages.len());
  let call=ToolCall{id:id.clone(),name:name.into(),
   arguments:json!({"path":path})};
  run.messages.push(json!({"role":"assistant","content":null,"tool_calls":[{
   "id":id,"type":"function","function":{
    "name":name,"arguments":call.arguments.to_string()}}]}));
  run.queued_calls.push(call.clone());
  journal::begin(run,&call).unwrap();
  let receipt=if success{Ok("success".into())}else{Err("file missing".into())};
  journal::finish(run,&call,&receipt,false).unwrap();
 }
 #[test]
 fn third_failure_is_rejected_and_fifth_pauses(){
  let mut run=fixture();
  let call=ToolCall{id:"pending".into(),name:"read_file".into(),
   arguments:json!({"path":"missing.txt"})};
  attempt(&mut run,"read_file","missing.txt",false);
  assert_eq!(action(&run,&call),None);
  attempt(&mut run,"read_file","missing.txt",false);
  assert_eq!(action(&run,&call),Some(Action::Reject));
  for _ in 0..2{attempt(&mut run,"read_file","missing.txt",false);}
  assert_eq!(action(&run,&call),Some(Action::Pause));
  assert_eq!(consecutive_failures(&run,&call),4);
 }
 #[test]
 fn changing_arguments_or_success_or_user_directive_resets_guard(){
  let mut run=fixture();
  for _ in 0..3{attempt(&mut run,"read_file","wrong.txt",false);}
  let changed=ToolCall{id:"next".into(),name:"read_file".into(),
   arguments:json!({"path":"correct.txt"})};
  assert_eq!(action(&run,&changed),None);
  attempt(&mut run,"read_file","correct.txt",true);
  let old=ToolCall{id:"next".into(),name:"read_file".into(),
   arguments:json!({"path":"wrong.txt"})};
  assert_eq!(action(&run,&old),None);
  attempt(&mut run,"read_file","wrong.txt",false);
  run.messages.push(json!({"role":"user",
   "content":"New user instruction: the file was just created"}));
  assert_eq!(action(&run,&old),None);
 }
 #[test]
 fn original_queued_tool_and_receipts_are_not_mutated_by_check(){
  let mut run=fixture();
  for _ in 0..2{attempt(&mut run,"read_file","f",false);}
  let call=ToolCall{id:"pending".into(),name:"read_file".into(),
   arguments:json!({"path":"f"})};
  run.queued_calls.push(call.clone());
  let before=serde_json::to_value(&run).unwrap();
  assert_eq!(action(&run,&call),Some(Action::Reject));
  assert_eq!(serde_json::to_value(&run).unwrap(),before);
  // The existing journal owns rejection, preserving the provider call ID.
  run.messages.push(json!({"role":"assistant","tool_calls":[{
   "id":call.id,"type":"function","function":{
    "name":call.name,"arguments":call.arguments.to_string()}}]}));
  journal::reject_admission(&mut run,&call,"REPEATED_TOOL_FAILURE",
   "Use another approach after two failed attempts").unwrap();
  assert_eq!(run.messages.last().unwrap()["tool_call_id"],"pending");
  assert_eq!(run.events.iter().filter(|e|e.kind=="tool_started").count(),2);
 }
}
