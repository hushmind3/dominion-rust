//! Optional tool-transport adapter. The only source of Run authorization
//! remains the native journal in Engine::perform_tool; this module begins no
//! Run, grants no approval and never retries uncertain remote mutations.
use super::{Host,agent_bridge,remote_worker,connected_tools,mcp::{McpCallOutcome,McpProgress}};
use crate::{config::Settings,domain::{ToolCall,ToolEffectOutcome},ports::RunStorePort};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{mpsc,watch};
use uuid::Uuid;

pub async fn invoke(host:&Host,settings:&Settings,store:&Arc<RunStorePort>,
    id:Uuid,call:&ToolCall,cancellation:watch::Receiver<bool>)
    ->Result<ToolEffectOutcome,String>{
    if !host.enabled{
        return Ok(ToolEffectOutcome::Settled(
            Err("external add-ons are disabled by the operator".into())));
    }
    if matches!(call.name.as_str(),agent_bridge::TOOL|agent_bridge::EDIT_TOOL){
        return host.bridge.execute(settings,store,id,call,cancellation).await;
    }
    if matches!(call.name.as_str(),remote_worker::TOOL|remote_worker::EDIT_TOOL){
        return match host.remote_worker.as_ref(){
            Some(remote)=>remote.execute(settings,store,id,call,cancellation).await,
            None=>Ok(ToolEffectOutcome::Settled(Err(
                "remote worker is not configured by the operator".into())))
        };
    }
    if call.name.starts_with("mcp__"){
        return invoke_mcp_with_progress(host,store,id,call,cancellation).await;
    }
    if connected_tools::supports(&call.name){
        return Ok(connected_tools::execute(settings,call).await);
    }
    Err("unsupported external tool adapter".into())
}

async fn record_mcp_progress(store:&Arc<RunStorePort>,id:Uuid,call:&ToolCall,event:&McpProgress){
    let result=store.mutate(id,|run|{
        if run.in_flight.as_ref().is_some_and(|pending|pending.id==call.id){
            run.event("mcp_progress",
                format!("{}: {}",call.name,event.message.as_deref().unwrap_or("In progress")),
                Some(json!({
                    "call_id":call.id,"request_id":event.request_id,
                    "progress":event.progress,"total":event.total,
                    "message":event.message
                })));
        }
        Ok(())
    }).await;
    if let Err(error)=result {
        tracing::warn!(%id,"MCP progress event could not be stored: {error}");
    }
}
async fn invoke_mcp_with_progress(host:&Host,store:&Arc<RunStorePort>,id:Uuid,call:&ToolCall,
    cancellation:watch::Receiver<bool>)->Result<McpCallOutcome,String>{
    let (tx,mut rx)=mpsc::channel::<McpProgress>(16);
    let remote=host.mcp.call_with_progress(id,call,cancellation,tx);
    tokio::pin!(remote);
    let mut forwarded=0usize;
    let mut last_progress=None::<f64>;
    loop{
        tokio::select!{
            biased;
            result=&mut remote=>{
                while let Ok(progress)=rx.try_recv(){
                    if forwarded<32 && last_progress.is_none_or(|last|progress.progress>last){
                        record_mcp_progress(store,id,call,&progress).await;
                        forwarded+=1;
                        last_progress=Some(progress.progress);
                    }
                }
                return result;
            }
            Some(progress)=rx.recv()=>{
                if forwarded<32 && last_progress.is_none_or(|last|progress.progress>last){
                    record_mcp_progress(store,id,call,&progress).await;
                    forwarded+=1;
                    last_progress=Some(progress.progress);
                }
            }
        }
    }
}


