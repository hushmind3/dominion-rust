//! Product-owned implementation of the narrow native Run service port.
//! The algorithm/kernel see this only as `dyn RuntimeServices`.
use super::ProductHost;
use crate::{config::Settings,domain::{Run,ToolCall,ToolEffectOutcome},
    ports::RunStorePort,provider::{ModelClient,Reply},
    runtime_services::{RuntimeServices,ServiceFuture,ModelCapabilities,CapabilitySupport}};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

impl RuntimeServices for ProductHost {
    fn model_capabilities(&self,run:&Run)->ModelCapabilities{
        let entry=self.providers.get(&run.provider_id)
            .filter(|entry|entry.enabled&&entry.model==run.model_name);
        ModelCapabilities{
            tool_calling:CapabilitySupport::from(entry.as_ref().and_then(|p|p.tool_calling)),
            parallel_tool_calls:CapabilitySupport::from(entry.as_ref()
                .and_then(|p|p.parallel_tool_calls)),
            streaming:CapabilitySupport::from(entry.as_ref().and_then(|p|p.streaming)),
            reasoning:CapabilitySupport::from(entry.as_ref().and_then(|p|p.reasoning)),
            prompt_cache:CapabilitySupport::from(entry.as_ref().and_then(|p|p.prompt_cache)),
            vision:CapabilitySupport::from(entry.as_ref().and_then(|p|p.vision)),
            context_window:entry.and_then(|p|p.context_window)
        }
    }
    fn select_model(&self,settings:&Settings,base:&ModelClient,
        provider:&str,model:&str)->Result<ModelClient,String>{
        ProductHost::model_for_selection(self,settings,base,provider,model)
    }
    fn complete<'a>(&'a self,settings:&'a Settings,base:&'a ModelClient,
        run:&'a Run,messages:&'a [Value],tools:&'a [Value],economy:bool)
        ->ServiceFuture<'a,Result<Reply,String>>{
        Box::pin(async move{
            // A Run owns a stable routing identity even across restarts. No
            // cross-vendor silent fallback and no shared global cache key.
            let affinity=if run.provider_session_affinity.is_empty(){
                format!("dominion-run-{}",run.id)
            }else{run.provider_session_affinity.clone()};
            let selected=ProductHost::model_for_selection(self,settings,base,
                &run.provider_id,&run.model_name)?
                .with_session_affinity(&affinity);
            selected.complete(messages,tools,economy).await
        })
    }
    fn throttle<'a>(&'a self,provider:&'a str)->ServiceFuture<'a,()>{
        Box::pin(async move{self.providers.throttle(provider).await})
    }
    fn tools<'a>(&'a self,settings:&'a Settings)->ServiceFuture<'a,Vec<Value>>{
        Box::pin(async move{self.addons.tool_definitions(settings).await})
    }
    fn handles(&self,name:&str)->bool{self.addons.handles_tool(name)}
    fn requires_approval(&self,name:&str)->bool{
        self.addons.needs_external_approval(name)
    }
    fn connected_approval(&self,name:&str)->bool{
        self.addons.needs_connected_approval(name)
    }
    fn invoke<'a>(&'a self,settings:&'a Settings,store:&'a Arc<RunStorePort>,
        id:Uuid,call:&'a ToolCall,cancel:watch::Receiver<bool>)
        ->ServiceFuture<'a,Result<ToolEffectOutcome,String>>{
        Box::pin(async move{
            self.addons.invoke_external(settings,store,id,call,cancel).await
        })
    }
    fn memory_search<'a>(&'a self,scope:&'a str,query:&'a str,
        limit:usize)->ServiceFuture<'a,Value>{
        Box::pin(async move{
            serde_json::to_value(self.knowledge.search(scope,query,limit).await)
                .unwrap_or_else(|_|Value::Array(Vec::new()))
        })
    }
    fn memory_save<'a>(&'a self,scope:&'a str,title:&'a str,
        content:&'a str)->ServiceFuture<'a,Result<Value,String>>{
        Box::pin(async move{
            let entry=self.knowledge.remember(scope,title,content).await?;
            serde_json::to_value(entry).map_err(|e|e.to_string())
        })
    }
    fn focused_notes<'a>(&'a self,run:&'a Run)->ServiceFuture<'a,String>{
        Box::pin(async move{ProductHost::focused_notes(self,run).await})
    }
    fn release<'a>(&'a self,id:Uuid)->ServiceFuture<'a,()>{
        Box::pin(async move{self.release_run_context(id).await})
    }
}
