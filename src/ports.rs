//! Storage-neutral interface used by the native instruction executor.
//! No filesystem, database driver, serialization or persistence path lives
//! in this module. The production adapter is infrastructure::persistence.
//! A caller may inject a different durable repository without changing Engine.
use crate::domain::{Run,RunStatus,ToolCall};
use std::{future::Future,pin::Pin,sync::Arc};
use tokio::sync::broadcast;
use uuid::Uuid;

/// A single persisted Run update; no full snapshot or model transcript is copied.
#[derive(Clone,Debug)]
pub struct RunUpdate{
    pub id:Uuid,
    pub status:RunStatus,
    pub updated_at_ms:u64,
}
impl RunUpdate{
    pub(crate) fn from_run(run:&Run)->Self{
        Self{id:run.id,status:run.status.clone(),updated_at_ms:run.updated_at_ms}
    }
}
/// Lightweight status and listing projection: never clone model messages,
/// tool schemas, read receipts or whole event transcripts for UI polling.
#[derive(Clone,Debug)]
pub struct RunHeader {
    pub id:Uuid,
    pub instruction:String,
    pub status:RunStatus,
    pub provider_id:String,
    pub model_name:String,
    pub answer:Option<String>,
    pub cost_krw:f64,
    pub created_at_ms:u64,
    pub updated_at_ms:u64,
    pub steps:usize,
    pub input_tokens:u64,
    pub cached_input_tokens:u64,
    pub cache_write_tokens:u64,
    pub cache_unreported_requests:usize,
    pub output_tokens:u64,
    pub model_requests:usize,
    pub unmetered_model_requests:usize,
    pub tool_calls:usize,
    pub last_error:Option<String>,
}
impl RunHeader {
    pub(crate) fn from_run(run:&Run)->Self{
        Self{
            id:run.id,
            instruction:run.instruction.clone(),
            status:run.status.clone(),
            provider_id:run.provider_id.clone(),
            model_name:run.model_name.clone(),
            answer:if run.terminal(){run.answer.clone()}else{None},
            cost_krw:run.cost_krw,
            created_at_ms:run.created_at_ms,
            updated_at_ms:run.updated_at_ms,
            steps:run.steps,
            input_tokens:run.input_tokens,
            cached_input_tokens:run.cached_input_tokens,
            cache_write_tokens:run.cache_write_tokens,
            cache_unreported_requests:run.cache_unreported_requests,
            output_tokens:run.output_tokens,
            model_requests:run.events.iter().filter(|event|event.kind=="model").count(),
            unmetered_model_requests:run.events.iter()
                .filter(|event|event.kind=="model" && event.detail.as_ref()
                    .and_then(|data|data.get("usage_reported"))
                    .and_then(serde_json::Value::as_bool)!=Some(true)).count(),
            tool_calls:run.events.iter().filter(|event|event.kind=="tool_started").count(),
            last_error:if run.status==RunStatus::Failed{
                run.events.last().map(|e|e.message.clone())
            }else{None},
        }
    }
    pub fn terminal(&self)->bool{
        matches!(self.status,RunStatus::Completed|RunStatus::Failed|RunStatus::Cancelled)
    }
}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct RunStats{pub total:usize,pub active:usize,pub awaiting_approval:usize}

/// All persistence operations are asynchronous and emit a committed
/// RunUpdate only after an atomic durable update has succeeded.
pub type StorageFuture<'a,T>=Pin<Box<dyn Future<Output=T>+Send+'a>>;
pub type RunMutation<'a>=Box<dyn FnOnce(&mut Run)->Result<(),String>+Send+'a>;

/// Port rather than database implementation. A single read or mutation can
/// be safely shared across the core and product HTTP/SDK adapters.
pub trait RunRepository:Send+Sync{
    fn put(&self,run:Run)->StorageFuture<'_,Result<(),String>>;
    /// Atomic create-or-observe; remote re-deliveries must not overwrite an
    /// already-running or terminal Run, nor replay model/tool effects.
    fn put_if_absent(&self,run:Run)->StorageFuture<'_,Result<(Run,bool),String>>;
    fn get(&self,id:Uuid)->StorageFuture<'_,Option<Run>>;
    fn has_children(&self,id:Uuid)->StorageFuture<'_,bool>;
    fn children_of(&self,id:Uuid)->StorageFuture<'_,Vec<Run>>;
    fn family_of(&self,id:Uuid)->StorageFuture<'_,Vec<Run>>;
    fn stats(&self)->StorageFuture<'_,RunStats>;
    fn headers(&self)->StorageFuture<'_,Vec<RunHeader>>;
    fn strategy_evidence<'a>(&'a self,repository:&'a str)
        ->StorageFuture<'a,Vec<crate::strategy::StrategyEvidence>>;
    fn pending_approvals(&self)->StorageFuture<'_,Vec<(Uuid,ToolCall)>>;
    fn list(&self)->StorageFuture<'_,Vec<Run>>;
    fn recovery_candidates(&self)->StorageFuture<'_,(Vec<Uuid>,Vec<Uuid>)>;
    fn mutate<'a>(&'a self,id:Uuid,change:RunMutation<'a>)
        ->StorageFuture<'a,Result<Run,String>>;
    fn subscribe(&self)->broadcast::Receiver<RunUpdate>;
    fn changed<'a>(&'a self,id:Uuid,receiver:&'a mut broadcast::Receiver<RunUpdate>)
        ->StorageFuture<'a,bool>;
}

/// Object-safe repository handle. This facade keeps existing generic
/// closure-based Run updates without exposing filesystem-backed Store.
/// The repository is injected once during application composition.
pub struct RunStorePort{backend:Arc<dyn RunRepository>}
impl RunStorePort{
    pub fn new(backend:Arc<dyn RunRepository>)->Self{Self{backend}}
    pub async fn put(&self,run:Run)->Result<(),String>{self.backend.put(run).await}
    pub async fn put_if_absent(&self,run:Run)->Result<(Run,bool),String>{
        self.backend.put_if_absent(run).await
    }
    pub async fn get(&self,id:Uuid)->Option<Run>{self.backend.get(id).await}
    pub async fn has_children(&self,id:Uuid)->bool{self.backend.has_children(id).await}
    pub async fn children_of(&self,id:Uuid)->Vec<Run>{self.backend.children_of(id).await}
    pub async fn family_of(&self,id:Uuid)->Vec<Run>{self.backend.family_of(id).await}
    pub async fn stats(&self)->RunStats{self.backend.stats().await}
    pub async fn headers(&self)->Vec<RunHeader>{self.backend.headers().await}
    pub async fn strategy_evidence(&self,repository:&str)
        ->Vec<crate::strategy::StrategyEvidence>{
        self.backend.strategy_evidence(repository).await
    }
    pub async fn pending_approvals(&self)->Vec<(Uuid,ToolCall)>{
        self.backend.pending_approvals().await
    }
    pub async fn list(&self)->Vec<Run>{self.backend.list().await}
    pub async fn recovery_candidates(&self)->(Vec<Uuid>,Vec<Uuid>){
        self.backend.recovery_candidates().await
    }
    pub async fn mutate<'a,F>(&'a self,id:Uuid,change:F)->Result<Run,String>
    where F:FnOnce(&mut Run)->Result<(),String>+Send+'a{
        self.backend.mutate(id,Box::new(change)).await
    }
    pub fn subscribe(&self)->broadcast::Receiver<RunUpdate>{self.backend.subscribe()}
    pub async fn changed(&self,id:Uuid,receiver:&mut broadcast::Receiver<RunUpdate>)->bool{
        self.backend.changed(id,receiver).await
    }
}

#[cfg(test)]
mod memory_tests;
