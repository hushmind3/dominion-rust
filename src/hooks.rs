//! Deterministic pre/post tool hooks owned by Dominion's runtime.
//! Hooks can DENY a call or transform its user-visible receipt. They cannot
//! approve a tool, alter an admitted argument, or bypass the execution journal.
use crate::{domain::{Run,ToolCall},config::Settings};
use std::{collections::HashSet,sync::Arc};
use uuid::Uuid;

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct HookDenial{pub code:String,pub reason:String}
impl HookDenial{
    pub fn new(code:&str,reason:&str)->Self{
        Self{
            code:code.chars().filter(|c|c.is_ascii_alphanumeric()||*c=='_').take(60).collect(),
            reason:reason.chars().take(650).collect(),
        }
    }
}
pub struct ToolContext<'a>{
    pub run_id:Uuid,
    pub read_only:bool,
    pub call:&'a ToolCall,
}
pub trait ToolHook:Send+Sync{
    /// Return an explicit block, never auto-approve. No async I/O here:
    /// the before gate is ordered, deterministic and bounded.
    fn before(&self,_context:&ToolContext<'_>)->Result<(),HookDenial>{Ok(())}
    /// Runs after body quiescence and before the durable result receipt.
    /// This may redact/truncate, not change tool arguments or authority.
    fn after(&self,_context:&ToolContext<'_>,_result:&mut Result<String,String>){}
}
#[derive(Clone,Default)]
pub struct HookChain{hooks:Vec<Arc<dyn ToolHook>>}
impl HookChain{
    pub fn from_settings(settings:&Settings)->Self{
        let mut chain=Self::default();
        chain.add(Arc::new(PreToolDenyList{
            denied:settings.denied_tools.iter().cloned().collect(),
        }));
        chain.add(Arc::new(ReceiptLimit{limit:settings.tool_result_max_chars}));
        chain
    }
    pub fn add(&mut self,hook:Arc<dyn ToolHook>){self.hooks.push(hook);}
    pub fn before(&self,run:&Run,call:&ToolCall)->Result<(),HookDenial>{
        let context=ToolContext{run_id:run.id,read_only:run.read_only,call};
        for hook in &self.hooks{hook.before(&context)?;}
        Ok(())
    }
    pub fn after(&self,run:&Run,call:&ToolCall,result:&mut Result<String,String>){
        let context=ToolContext{run_id:run.id,read_only:run.read_only,call};
        for hook in &self.hooks{hook.after(&context,result);}
    }
}
struct PreToolDenyList{denied:HashSet<String>}
impl ToolHook for PreToolDenyList{
    fn before(&self,context:&ToolContext<'_>)->Result<(),HookDenial>{
        if self.denied.contains(&context.call.name){
            return Err(HookDenial::new("TOOL_BLOCKED",
                "Tool disabled by operator policy; ask the user to change DOMINION_DENIED_TOOLS"));
        }
        Ok(())
    }
}
struct ReceiptLimit{limit:usize}
impl ToolHook for ReceiptLimit{
    fn after(&self,_context:&ToolContext<'_>,result:&mut Result<String,String>){
        let text=match result{Ok(text)|Err(text)=>text};
        let count=text.chars().count();
        if count>self.limit{
            let clipped=text.chars().take(self.limit).collect::<String>();
            *text=format!("{clipped}\n[tool output truncated: {count} characters, cap {}]",self.limit);
        }
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::CreateRun;
    use serde_json::json;
    fn test_run()->Run{Run::new(CreateRun{
        instruction:"test".into(),budget_krw:None,max_steps:None
    },5).unwrap()}
    fn call()->ToolCall{ToolCall{id:"1".into(),name:"read_file".into(),arguments:json!({"path":"data.txt"})}}
    struct CustomVeto;
    impl ToolHook for CustomVeto{
        fn before(&self,_:&ToolContext<'_>)->Result<(),HookDenial>{
            Err(HookDenial::new("CUSTOM_POLICY","blocked by user hook"))
        }
    }
    #[test]
    fn chain_can_veto_but_cannot_approve_or_dispatch(){
        let mut chain=HookChain::default();
        chain.add(Arc::new(CustomVeto));
        let error=chain.before(&test_run(),&call()).unwrap_err();
        assert_eq!(error.code,"CUSTOM_POLICY");
        assert_eq!(error.reason,"blocked by user hook");
    }
    #[test]
    fn bounded_receipt_is_stable_for_success_and_error(){
        let mut chain=HookChain::default();chain.add(Arc::new(ReceiptLimit{limit:4}));
        let mut success=Ok("abcdef".into());
        let mut error=Err("abcdefgh".into());
        chain.after(&test_run(),&call(),&mut success);
        chain.after(&test_run(),&call(),&mut error);
        assert!(success.unwrap().starts_with("abcd\n[tool output truncated"));
        assert!(error.unwrap_err().starts_with("abcd\n[tool output truncated"));
    }
}
