//! Narrow runtime-to-host contract. The agent scheduler and loop depend on
//! capabilities, never on the GUI, memory database or provider registry types.
//! The application composes the concrete adapter in product/runtime_services.rs.
use crate::{config::Settings,domain::{Run,ToolCall,ToolEffectOutcome},
    ports::RunStorePort,provider::{ModelClient,Reply}};
use serde_json::Value;
use std::{future::Future,pin::Pin,sync::Arc};
use tokio::sync::watch;
use uuid::Uuid;

pub type ServiceFuture<'a,T> = Pin<Box<dyn Future<Output=T>+Send+'a>>;

/// Exact endpoint/model capability. Never inferred from a model's name.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum CapabilitySupport{#[default] Unknown,Supported,Unsupported}
impl From<Option<bool>> for CapabilitySupport{
    fn from(value:Option<bool>)->Self{
        match value {Some(true)=>Self::Supported,
                     Some(false)=>Self::Unsupported,None=>Self::Unknown}
    }
}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct ModelCapabilities{
    pub tool_calling:CapabilitySupport,
    pub parallel_tool_calls:CapabilitySupport,
    pub streaming:CapabilitySupport,
    pub prompt_cache:CapabilitySupport,
    pub vision:CapabilitySupport,
    pub context_window:Option<u32>,
}

/// The host may supply alternative memory, provider routing, or connected
/// tools, but it cannot override the native journal/approval/policy checks.
pub trait RuntimeServices:Send+Sync {
    /// Unknown is backwards-compatible; Unsupported is a hard runtime gate.
    fn model_capabilities(&self,_run:&Run)->ModelCapabilities{
        ModelCapabilities::default()
    }
    fn select_model(&self,settings:&Settings,base:&ModelClient,
        provider:&str,model:&str)->Result<ModelClient,String>;
    /// The native loop requests an inference receipt through this host port.
    /// Provider protocols, credentials, local llama.cpp endpoints and cache
    /// affinity remain outside the agent decision/execution loop.
    fn complete<'a>(&'a self,settings:&'a Settings,base:&'a ModelClient,
        run:&'a Run,messages:&'a [Value],tools:&'a [Value],economy:bool)
        ->ServiceFuture<'a,Result<Reply,String>>;
    fn throttle<'a>(&'a self,provider:&'a str)->ServiceFuture<'a,()>;
    fn tools<'a>(&'a self,settings:&'a Settings)->ServiceFuture<'a,Vec<Value>>;
    fn handles(&self,name:&str)->bool;
    fn requires_approval(&self,name:&str)->bool;
    fn connected_approval(&self,name:&str)->bool;
    fn invoke<'a>(&'a self,settings:&'a Settings,
        store:&'a Arc<RunStorePort>,id:Uuid,call:&'a ToolCall,
        cancel:watch::Receiver<bool>)
        ->ServiceFuture<'a,Result<ToolEffectOutcome,String>>;
    fn memory_search<'a>(&'a self,scope:&'a str,query:&'a str,
        limit:usize)->ServiceFuture<'a,Value>;
    fn memory_save<'a>(&'a self,scope:&'a str,title:&'a str,
        content:&'a str)->ServiceFuture<'a,Result<Value,String>>;
    fn focused_notes<'a>(&'a self,run:&'a Run)->ServiceFuture<'a,String>;
    fn release<'a>(&'a self,id:Uuid)->ServiceFuture<'a,()>;
}
