//! Narrow runtime-to-host contract. The agent scheduler and loop depend on
//! capabilities, never on the GUI, memory database or provider registry types.
//! The application composes the concrete adapter in product/runtime_services.rs.
use crate::{config::Settings,domain::{Run,ToolCall,ToolEffectOutcome},
    ports::RunStorePort,provider::ModelClient};
use serde_json::Value;
use std::{future::Future,pin::Pin,sync::Arc};
use tokio::sync::watch;
use uuid::Uuid;

pub type ServiceFuture<'a,T> = Pin<Box<dyn Future<Output=T>+Send+'a>>;

/// The host may supply alternative memory, provider routing, or connected
/// tools, but it cannot override the native journal/approval/policy checks.
pub trait RuntimeServices:Send+Sync {
    fn select_model(&self,settings:&Settings,base:&ModelClient,
        provider:&str,model:&str)->Result<ModelClient,String>;
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
