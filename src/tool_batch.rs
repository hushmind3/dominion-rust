//! Bounded, model-ordered read scheduling with exclusive mutation barriers.
//! Derived from the concurrency rules in OpenAI Codex parallel.rs and
//! DeepSeek Harness tools/batch.ts; independent Rust implementation.
use crate::{config::Settings,domain::{Run,ToolCall},journal,policy,tool_schema,tools};
use std::{future::Future,sync::Arc,time::Duration};
use tokio::{task::{JoinError,JoinSet},sync::watch};
use uuid::Uuid;

/// Only contiguous safe inspection calls can share a batch. Mutations,
/// unknown tools and denied calls stop the prefix before any invocation.
pub fn parallel_prefix(run:&Run,limit:usize)->Vec<ToolCall>{
    let mut calls=Vec::new();
    for call in &run.queued_calls {
        if calls.len()==limit.clamp(1,8) || !journal::parallel_safe(&call.name)
            || !policy::allowed_tool(run,&call.name) {
            break;
        }
        if !run.advertised_tools.is_empty()
            && tool_schema::admit(call,&run.advertised_tools).is_err() {
            break;
        }
        calls.push(call.clone());
    }
    if calls.len()>=2{calls}else{Vec::new()}
}

/// Shared bounded pool. Completion order does not affect the model transcript.
pub async fn ordered_pool<T,R,F,Fut>(inputs:Vec<T>,limit:usize,operation:F)->Vec<Result<R,String>>
where T:Send+'static,R:Send+'static,F:Fn(T)->Fut+Send+Sync+'static,
      Fut:Future<Output=Result<R,String>>+Send+'static
{
    ordered_pool_inner(inputs,limit,None,operation).await
}

/// Cancels all remaining read workers, then awaits their shutdown *before*
/// emitting any receipt to the model. There are no externally visible writes.
pub async fn ordered_pool_cancellable<T,R,F,Fut>(
    inputs:Vec<T>,limit:usize,cancel:watch::Receiver<bool>,operation:F
)->Vec<Result<R,String>>
where T:Send+'static,R:Send+'static,F:Fn(T)->Fut+Send+Sync+'static,
      Fut:Future<Output=Result<R,String>>+Send+'static
{
    ordered_pool_inner(inputs,limit,Some(cancel),operation).await
}

async fn ordered_pool_inner<T,R,F,Fut>(
    inputs:Vec<T>,limit:usize,mut cancellation:Option<watch::Receiver<bool>>,operation:F
)->Vec<Result<R,String>>
where T:Send+'static,R:Send+'static,F:Fn(T)->Fut+Send+Sync+'static,
      Fut:Future<Output=Result<R,String>>+Send+'static
{
    enum Awaited<T>{Finished(T),Cancelled,ChannelClosed}
    let count=inputs.len();
    if count==0{return Vec::new();}
    let operation=Arc::new(operation);
    let mut pending=inputs.into_iter().enumerate();
    let mut join=JoinSet::new();
    let mut results=(0..count).map(|_|None).collect::<Vec<Option<Result<R,String>>>>();
    let mut running=0usize;
    for _ in 0..limit.clamp(1,8).min(count){
        if let Some((index,input))=pending.next(){
            let op=Arc::clone(&operation);
            join.spawn(async move{(index,op(input).await)});
            running+=1;
        }
    }
    let mut cancelled=false;
    while running>0 {
        if cancellation.as_ref().is_some_and(|rx|*rx.borrow()){
            cancelled=true;break;
        }
        let next:Awaited<Option<Result<(usize,Result<R,String>),JoinError>>> =
            if let Some(rx)=cancellation.as_mut(){
                tokio::select!{
                    biased;
                    changed=rx.changed()=>{
                        if changed.is_ok() && *rx.borrow(){Awaited::Cancelled}
                        else{Awaited::ChannelClosed}
                    },
                    outcome=join.join_next()=>Awaited::Finished(outcome)
                }
            }else{
                Awaited::Finished(join.join_next().await)
            };
        let outcome=match next{
            Awaited::Cancelled=>{cancelled=true;break;},
            Awaited::ChannelClosed=>{cancellation=None;continue;},
            Awaited::Finished(outcome)=>outcome
        };
        let Some(outcome)=outcome else{break};
        running-=1;
        match outcome{
            Ok((index,result))=>results[index]=Some(result),
            Err(error)=>tracing::warn!("parallel tool worker terminated: {error}"),
        }
        if let Some((index,input))=pending.next(){
            let op=Arc::clone(&operation);
            join.spawn(async move{(index,op(input).await)});
            running+=1;
        }
    }
    if cancelled {
        join.abort_all();
        while join.join_next().await.is_some(){}
        // Aborted batch is one atomic set of ordered receipts. Even reads that
        // finished earlier are not misrepresented as a completed tool batch.
        return (0..count).map(|_|Err("ABORTED: read-only tool cancelled".into())).collect();
    }
    results.into_iter().map(|result|
        result.unwrap_or_else(||Err("read-only tool worker ended without a result".into()))
    ).collect()
}
pub async fn execute_reads(settings:&Settings,id:Uuid,calls:Vec<ToolCall>)->Vec<Result<String,String>>{
    execute_reads_inner(settings,id,calls,None).await
}
pub async fn execute_reads_cancellable(
    settings:&Settings,id:Uuid,calls:Vec<ToolCall>,cancel:watch::Receiver<bool>
)->Vec<Result<String,String>>{
    execute_reads_inner(settings,id,calls,Some(cancel)).await
}
async fn execute_reads_inner(
    settings:&Settings,id:Uuid,calls:Vec<ToolCall>,cancel:Option<watch::Receiver<bool>>
)->Vec<Result<String,String>>{
    let limit=settings.max_parallel_reads;
    let settings=Arc::new(settings.clone());
    let operation=move |call:ToolCall|{
        let settings=Arc::clone(&settings);
        async move{
            match tokio::time::timeout(Duration::from_secs(30),tools::execute(&settings,id,&call)).await{
                Ok(result)=>result,
                Err(_)=>Err(format!("read-only {} timed out after 30 seconds",call.name))
            }
        }
    };
    match cancel{
        Some(receiver)=>ordered_pool_cancellable(calls,limit,receiver,operation).await,
        None=>ordered_pool(calls,limit,operation).await,
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::{CreateRun,RunStatus};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize,Ordering};

    #[tokio::test]
    async fn cancelled_parallel_batch_waits_until_all_worker_futures_are_quiescent(){
        struct ActiveGuard(Arc<AtomicUsize>);
        impl Drop for ActiveGuard{
            fn drop(&mut self){self.0.fetch_sub(1,Ordering::SeqCst);}
        }
        let active=Arc::new(AtomicUsize::new(0));
        let (cancel_tx,cancel_rx)=watch::channel(false);
        let test=tokio::spawn(ordered_pool_cancellable(
            (0..4).collect::<Vec<_>>(),4,cancel_rx,{
                let active=Arc::clone(&active);
                move |_item|{
                    let active=Arc::clone(&active);
                    async move{
                        active.fetch_add(1,Ordering::SeqCst);
                        let _guard=ActiveGuard(active);
                        tokio::time::sleep(Duration::from_secs(8)).await;
                        Ok::<usize,String>(1)
                    }
                }
            }
        ));
        for _ in 0..100{
            if active.load(Ordering::SeqCst)==4{break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(active.load(Ordering::SeqCst),4);
        cancel_tx.send_replace(true);
        let results=tokio::time::timeout(Duration::from_secs(2),test).await
            .expect("pool did not settle after cancellation").unwrap();
        assert_eq!(results.len(),4);
        assert!(results.iter().all(|result|result.is_err()));
        assert_eq!(active.load(Ordering::SeqCst),0,
            "cancelled batch produced receipts before in-flight tools stopped");
    }

    #[tokio::test]
    async fn concurrency_is_bounded_and_results_remain_model_ordered(){
        let active=Arc::new(AtomicUsize::new(0));
        let maximum=Arc::new(AtomicUsize::new(0));
        let out=ordered_pool((0..8u64).collect(),4,{
            let active=Arc::clone(&active);let maximum=Arc::clone(&maximum);
            move |index|{
                let active=Arc::clone(&active);let maximum=Arc::clone(&maximum);
                async move{
                    let running=active.fetch_add(1,Ordering::SeqCst)+1;
                    maximum.fetch_max(running,Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis((8-index)*5)).await;
                    active.fetch_sub(1,Ordering::SeqCst);
                    Ok::<u64,String>(index)
                }
            }
        }).await;
        assert_eq!(out.into_iter().map(Result::unwrap).collect::<Vec<_>>(),(0..8).collect::<Vec<_>>());
        assert_eq!(active.load(Ordering::SeqCst),0);
        assert!(maximum.load(Ordering::SeqCst)>=2);
        assert!(maximum.load(Ordering::SeqCst)<=4);
    }
    #[test]
    fn writes_and_unavailable_tool_calls_are_exclusive_barriers(){
        let mut run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:None,max_steps:None},5).unwrap();
        run.status=RunStatus::Running;
        let read=|id:&str|ToolCall{id:id.into(),name:"read_file".into(),arguments:json!({"path":"a.txt"})};
        let write=ToolCall{id:"w".into(),name:"write_file".into(),arguments:json!({"path":"a.txt","content":"replace"})};
        run.queued_calls.extend([read("r1"),read("r2"),write,read("r3")]);
        let batch=parallel_prefix(&run,4);
        assert_eq!(batch.len(),2);
        assert_eq!(batch[0].id,"r1");
        assert_eq!(batch[1].id,"r2");
        run.read_only=true;
        assert_eq!(parallel_prefix(&run,4).len(),2);
    }
    #[tokio::test]
    async fn parallel_real_file_reads_preserve_order_and_errors(){
        let mut settings=Settings::from_env().unwrap();
        let root=std::env::temp_dir().join(format!("dominion-batch-{}",Uuid::new_v4()));
        settings.workspace_dir=root.clone();
        let id=Uuid::new_v4();
        let workspace=root.join(id.to_string());
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::write(workspace.join("a.txt"),"alpha").await.unwrap();
        tokio::fs::write(workspace.join("b.txt"),"beta").await.unwrap();
        let input=vec![
            ToolCall{id:"a".into(),name:"read_file".into(),arguments:json!({"path":"a.txt"})},
            ToolCall{id:"missing".into(),name:"read_file".into(),arguments:json!({"path":"not-exists"})},
            ToolCall{id:"b".into(),name:"read_file".into(),arguments:json!({"path":"b.txt"})},
        ];
        let result=execute_reads(&settings,id,input).await;
        assert_eq!(result.len(),3);
        assert_eq!(result[0].as_ref().unwrap(),"alpha");
        assert!(result[1].is_err());
        assert_eq!(result[2].as_ref().unwrap(),"beta");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
