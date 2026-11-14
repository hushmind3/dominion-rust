//! DOMINION-native action/observation invariant and evidence-based stuck guard.
//! Informed by OpenHands event-view atomicity and stuck-loop analysis; this
//! module does not depend on or execute any external agent SDK.
use super::*;

/// An LLM tool batch is atomic: every proposed call must receive exactly one
/// matching tool observation before the next provider invocation. This check
/// uses the *actual* sent batch, not merely logs that happened to be stored.
pub(super) fn check_tool_result_pairs(calls:&[ToolCall],new_messages:&[Value])->Result<(),String>{
    use std::collections::HashMap;
    let mut expected=HashMap::<&str,usize>::new();
    for call in calls {
        if call.id.is_empty()||expected.insert(call.id.as_str(),0).is_some(){
            return Err("도구 배치에 비어 있거나 중복된 호출 ID가 있습니다.".into());
        }
    }
    for item in new_messages.iter().filter(|item|item["role"]=="tool") {
        let id=item["tool_call_id"].as_str()
            .ok_or("호출 ID 없는 도구 결과가 있습니다.")?;
        let Some(count)=expected.get_mut(id) else{
            return Err("요청하지 않은 도구 결과가 대화에 섞였습니다.".into());
        };
        *count+=1;
        if *count>1{return Err("동일 도구 호출의 결과가 중복되었습니다.".into());}
    }
    if expected.values().any(|count|*count!=1){
        return Err("도구 실행 결과가 누락됐습니다. 모델에 불완전한 호출 기록을 전달하지 않습니다.".into());
    }
    Ok(())
}

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub(super) enum LoopDecision { Normal, Nudge, Stuck }

/// Detect only cycles where the model repeats exactly the same read-only
/// actions and receives exactly the same observed bytes. Different results,
/// changed arguments, or a write reset the streak; no inferred success.
#[derive(Default)]
pub(super) struct ReadLoopGuard {
    previous:Option<String>,
    repeats:u8,
}
impl ReadLoopGuard{
    pub(super) fn observe(&mut self,calls:&[ToolCall],new_messages:&[Value])->LoopDecision{
        if calls.is_empty()||calls.len()>24
            ||calls.iter().any(|call|!tool_is_read_only(&call.name)){
            self.previous=None;self.repeats=0;
            return LoopDecision::Normal;
        }
        let mut observations=Vec::with_capacity(calls.len());
        for call in calls {
            let outputs=new_messages.iter().filter(|item|
                item["role"]=="tool"&&item["tool_call_id"].as_str()==Some(call.id.as_str()))
                .collect::<Vec<_>>();
            if outputs.len()!=1 {
                self.previous=None;self.repeats=0;
                return LoopDecision::Normal;
            }
            let args:Value=match serde_json::from_str(&call.arguments) {
                Ok(args)=>args,
                Err(_)=>{
                    self.previous=None;self.repeats=0;
                    return LoopDecision::Normal;
                }
            };
            let raw=outputs[0]["content"].as_str().unwrap_or("");
            observations.push(format!("{}|{}|{:x}",
                call.name,args,Sha256::digest(raw.as_bytes())));
        }
        // Parallel independent reads may arrive in a different order.
        observations.sort_unstable();
        let signature=format!("{:x}",Sha256::digest(observations.join("\n").as_bytes()));
        if self.previous.as_deref()==Some(signature.as_str()){
            self.repeats=self.repeats.saturating_add(1);
        }else{
            self.repeats=0;
            self.previous=Some(signature);
        }
        if self.repeats>=4{LoopDecision::Stuck}
        else if self.repeats==2{LoopDecision::Nudge}
        else{LoopDecision::Normal}
    }
}


/// Detect repeated *failed* tool batches, including non-read operations.
/// A successful call in the batch resets this guard: never claim that a
/// repeated write failed merely because its textual output looks similar.
/// Error records must be observed, not guessed from model commentary.
#[derive(Default)]
pub(super) struct ErrorLoopGuard {
    previous:Option<String>,
    repeats:u8,
}
impl ErrorLoopGuard {
    pub(super) fn observe(&mut self,calls:&[ToolCall],messages:&[Value])->LoopDecision {
        let reset=|guard:&mut Self|{
            guard.previous=None;
            guard.repeats=0;
            LoopDecision::Normal
        };
        if calls.is_empty()||calls.len()>24{return reset(self);}
        let mut failed=Vec::with_capacity(calls.len());
        for call in calls {
            let observations=messages.iter().filter(|item|
                item["role"]=="tool"&&item["tool_call_id"].as_str()==Some(call.id.as_str())
            ).collect::<Vec<_>>();
            if observations.len()!=1{return reset(self);}
            let content=observations[0]["content"].as_str().unwrap_or("");
            let value:Value=match serde_json::from_str(content){
                Ok(value)=>value,
                Err(_)=>return reset(self),
            };
            let is_error=value["error"].as_str().is_some_and(|s|!s.is_empty())
                ||value["success"]==false||value["ok"]==false
                ||matches!(value["status"].as_str(),Some("error"|"failed"|"denied"));
            if !is_error{return reset(self);}
            let args:Value=match serde_json::from_str(&call.arguments){
                Ok(value)=>value,
                Err(_)=>return reset(self),
            };
            failed.push(format!("{}|{}|{:x}",call.name,args,
                Sha256::digest(content.as_bytes())));
        }
        failed.sort_unstable();
        let signature=format!("{:x}",Sha256::digest(failed.join("\n").as_bytes()));
        if self.previous.as_deref()==Some(signature.as_str()){
            self.repeats=self.repeats.saturating_add(1);
        }else {
            self.previous=Some(signature);
            self.repeats=0;
        }
        if self.repeats>=4 {LoopDecision::Stuck}
        else if self.repeats==2 {LoopDecision::Nudge}
        else {LoopDecision::Normal}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(id:&str,path:&str)->ToolCall{
        ToolCall{id:id.into(),name:"workspace_read".into(),
            arguments:format!(r#"{{"path":"{path}"}}"#)}
    }
    fn result(id:&str,body:&str)->Value{
        json!({"role":"tool","tool_call_id":id,"content":body})
    }
    #[test]
    fn atomic_tool_batch_rejects_missing_duplicate_or_orphaned_results(){
        let calls=vec![read("a","a.txt"),read("b","b.txt")];
        assert!(check_tool_result_pairs(&calls,&[result("b","ok"),result("a","ok")]).is_ok());
        assert!(check_tool_result_pairs(&calls,&[result("a","ok")]).is_err());
        assert!(check_tool_result_pairs(&calls,&[
            result("a","ok"),result("a","again"),result("b","ok")
        ]).is_err());
        assert!(check_tool_result_pairs(&calls,&[
            result("a","ok"),result("b","ok"),result("c","spoofed")
        ]).is_err());
    }
    #[test]
    fn read_only_infinite_loops_get_nudged_then_blocked_without_false_success(){
        let mut guard=ReadLoopGuard::default();
        for turn in 0..5 {
            let id=format!("request-{turn}");
            let decision=guard.observe(&[read(&id,"file.rs")],&[result(&id,"identical bytes")]);
            assert_eq!(decision,if turn==4{LoopDecision::Stuck}
                else if turn==2{LoopDecision::Nudge}else{LoopDecision::Normal});
        }
    }
    #[test]
    fn changed_read_result_or_write_resets_stuck_counter(){
        let mut guard=ReadLoopGuard::default();
        let read=read("a","file.rs");
        assert_eq!(guard.observe(&[read.clone()],&[result("a","v1")]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[read.clone()],&[result("a","v1")]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[read.clone()],&[result("a","v2")]),LoopDecision::Normal);
        let write=ToolCall{id:"w".into(),name:"workspace_write".into(),arguments:"{}".into()};
        assert_eq!(guard.observe(&[write],&[result("w","done")]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[read],&[result("a","v1")]),LoopDecision::Normal);
    }
    #[test]
    fn identical_failed_effectful_tool_calls_cannot_spin_forever(){
        let mut guard=ErrorLoopGuard::default();
        for round in 0..5 {
            let id=format!("retry-{round}");
            let call=ToolCall{id:id.clone(),name:"workspace_write".into(),
                arguments:r#"{"path":"out.txt","content":"x"}"#.into()};
            let decision=guard.observe(&[call],&[result(&id,r#"{"error":"stale sha"}"#)]);
            assert_eq!(decision,if round==4{LoopDecision::Stuck}
                else if round==2{LoopDecision::Nudge}else{LoopDecision::Normal});
        }
    }
    #[test]
    fn changed_errors_and_successful_effects_reset_failure_streak(){
        let mut guard=ErrorLoopGuard::default();
        let call=ToolCall{id:"id".into(),name:"shell_exec".into(),
            arguments:r#"{"command":"cargo test"}"#.into()};
        assert_eq!(guard.observe(&[call.clone()],&[result("id",r#"{"error":"compile fail"}"#)]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[call.clone()],&[result("id",r#"{"error":"compile fail"}"#)]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[call.clone()],&[result("id",r#"{"error":"test fail"}"#)]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[call.clone()],&[result("id",r#"{"ok":true}"#)]),LoopDecision::Normal);
        assert_eq!(guard.observe(&[call.clone()],&[result("id",r#"{"error":"compile fail"}"#)]),LoopDecision::Normal);
        let other=ToolCall{arguments:r#"{"command":"cargo check"}"#.into(),..call};
        assert_eq!(guard.observe(&[other],&[result("id",r#"{"error":"compile fail"}"#)]),LoopDecision::Normal);
    }
    #[test]
    fn parallel_read_order_does_not_hide_identical_observations(){
        let mut guard=ReadLoopGuard::default();
        let calls=[read("1","first"),read("2","second")];
        assert_eq!(guard.observe(&calls,&[result("1","a"),result("2","b")]),LoopDecision::Normal);
        assert_eq!(guard.observe(&calls,&[result("2","b"),result("1","a")]),LoopDecision::Normal);
        assert_eq!(guard.observe(&calls,&[result("1","a"),result("2","b")]),LoopDecision::Nudge);
    }
}
