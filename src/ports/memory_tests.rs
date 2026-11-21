//! A test-only volatile repository proves the native Run kernel does not
//! require the production JSON database. It must never be enabled for real
//! side-effecting production Runs: crash safety requires durable intent.
use super::*;
use crate::{config::Settings,domain::CreateRun,engine::Engine,group::Groups,mcp::Mcp};
use std::collections::HashMap;
use tokio::sync::Mutex;

struct VolatileRepository{
    records:Mutex<HashMap<Uuid,Run>>,
    updates:broadcast::Sender<RunUpdate>,
}
impl VolatileRepository{
    fn new()->Self{
        let (updates,_)=broadcast::channel(16);
        Self{records:Mutex::new(HashMap::new()),updates}
    }
}
impl RunRepository for VolatileRepository{
    fn put(&self,run:Run)->StorageFuture<'_,Result<(),String>>{
        Box::pin(async move{
            self.records.lock().await.insert(run.id,run.clone());
            let _=self.updates.send(RunUpdate::from_run(&run));
            Ok(())
        })
    }
    fn put_if_absent(&self,run:Run)->StorageFuture<'_,Result<(Run,bool),String>>{
        Box::pin(async move{
            let mut guard=self.records.lock().await;
            if let Some(existing)=guard.get(&run.id){return Ok((existing.clone(),false));}
            guard.insert(run.id,run.clone());
            let _=self.updates.send(RunUpdate::from_run(&run));
            Ok((run,true))
        })
    }
    fn get(&self,id:Uuid)->StorageFuture<'_,Option<Run>>{
        Box::pin(async move{self.records.lock().await.get(&id).cloned()})
    }
    fn has_children(&self,id:Uuid)->StorageFuture<'_,bool>{
        Box::pin(async move{self.records.lock().await.values().any(|r|r.parent_run_id==Some(id))})
    }
    fn children_of(&self,id:Uuid)->StorageFuture<'_,Vec<Run>>{
        Box::pin(async move{self.records.lock().await.values()
            .filter(|r|r.parent_run_id==Some(id)).cloned().collect()})
    }
    fn family_of(&self,id:Uuid)->StorageFuture<'_,Vec<Run>>{
        Box::pin(async move{
            let guard=self.records.lock().await;
            let children=guard.values().filter(|r|r.parent_run_id==Some(id)).map(|r|r.id)
                .collect::<Vec<_>>();
            guard.values().filter(|r|children.contains(&r.id)
                ||r.parent_run_id.is_some_and(|parent|children.contains(&parent)))
                .cloned().collect()
        })
    }
    fn stats(&self)->StorageFuture<'_,RunStats>{
        Box::pin(async move{
            let guard=self.records.lock().await;
            RunStats{total:guard.len(),active:guard.values()
                .filter(|r|matches!(r.status,RunStatus::Running|RunStatus::Queued)).count(),
                awaiting_approval:guard.values().filter(|r|r.pending_approval.is_some()
                    &&r.approval_decision.is_none()).count()}
        })
    }
    fn headers(&self)->StorageFuture<'_,Vec<RunHeader>>{
        Box::pin(async move{self.records.lock().await.values()
            .map(RunHeader::from_run).collect()})
    }
    fn strategy_evidence<'a>(&'a self,_repository:&'a str)
        ->StorageFuture<'a,Vec<crate::strategy::StrategyEvidence>>{
        Box::pin(async{Vec::new()})
    }
    fn pending_approvals(&self)->StorageFuture<'_,Vec<(Uuid,ToolCall)>>{
        Box::pin(async move{self.records.lock().await.values().filter_map(|r|
            r.pending_approval.as_ref().filter(|_|r.approval_decision.is_none())
                .map(|call|(r.id,call.clone()))).collect()})
    }
    fn list(&self)->StorageFuture<'_,Vec<Run>>{
        Box::pin(async move{self.records.lock().await.values().cloned().collect()})
    }
    fn recovery_candidates(&self)->StorageFuture<'_,(Vec<Uuid>,Vec<Uuid>)>{
        Box::pin(async{(Vec::new(),Vec::new())})
    }
    fn mutate<'a>(&'a self,id:Uuid,change:RunMutation<'a>)
        ->StorageFuture<'a,Result<Run,String>>{
        Box::pin(async move{
            let mut guard=self.records.lock().await;
            let mut run=guard.get(&id).cloned().ok_or("execution not found")?;
            change(&mut run)?;
            guard.insert(id,run.clone());
            let _=self.updates.send(RunUpdate::from_run(&run));
            Ok(run)
        })
    }
    fn subscribe(&self)->broadcast::Receiver<RunUpdate>{self.updates.subscribe()}
    fn changed<'a>(&'a self,id:Uuid,rx:&'a mut broadcast::Receiver<RunUpdate>)
        ->StorageFuture<'a,bool>{
        Box::pin(async move{
            loop{
                match rx.recv().await{
                    Ok(msg) if msg.id==id=>return true,
                    Ok(_)=>{},
                    Err(broadcast::error::RecvError::Lagged(_))=>return true,
                    Err(broadcast::error::RecvError::Closed)=>return false,
                }
            }
        })
    }
}

#[tokio::test]
async fn instruction_engine_accepts_database_free_run_repository(){
    let root=std::env::temp_dir().join(format!("dominion-storage-port-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.addons_enabled=false;
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    tokio::fs::create_dir_all(&settings.workspace_dir).await.unwrap();
    let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let backend:Arc<dyn RunRepository>=Arc::new(VolatileRepository::new());
    let mcp=Arc::new(Mcp::new(Vec::new()).unwrap());
    let engine=Engine::new_with_repository(settings.clone(),backend,groups,mcp).unwrap();
    let run=Run::new(CreateRun{instruction:"explain and execute a small task".into(),
        budget_krw:Some(1.0),max_steps:Some(2)},2).unwrap();
    engine.store.put(run.clone()).await.unwrap();
    let mut notifications=engine.store.subscribe();
    let edited=engine.store.mutate(run.id,|state|{
        state.plan=Some("execute exactly one confirmed operation".into());
        Ok(())
    }).await.unwrap();
    assert_eq!(edited.plan.as_deref(),Some("execute exactly one confirmed operation"));
    assert!(engine.store.changed(run.id,&mut notifications).await);
    assert_eq!(engine.store.get(run.id).await.unwrap().id,run.id);
    assert_eq!(engine.store.stats().await.total,1);
    assert!(!settings.data_dir.join("runs").exists(),
        "core must never create the production JSON database directory");
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
