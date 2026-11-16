use crate::{domain::{Run,RunStatus},journal};
use std::{collections::HashMap, path::{Path,PathBuf}};
use tokio::{fs, io::AsyncWriteExt, sync::{Mutex,broadcast}};
use uuid::Uuid;

/// Atomic snapshot: unique create-new temp, sync bytes, rename, sync directory.
async fn persist(root:&Path,run:&Run)->Result<(),String>{
    let destination=root.join(format!("{}.json",run.id));
    let temporary=root.join(format!("{}-{}.tmp",run.id,Uuid::new_v4()));
    let result=async{
        let mut file=fs::OpenOptions::new().write(true).create_new(true)
            .open(&temporary).await.map_err(|e|e.to_string())?;
        let bytes=serde_json::to_vec(run).map_err(|e|e.to_string())?;
        file.write_all(&bytes).await.map_err(|e|e.to_string())?;
        file.sync_all().await.map_err(|e|e.to_string())?;
        drop(file);
        fs::rename(&temporary,&destination).await.map_err(|e|e.to_string())?;
        // The file contents alone are not enough after an OS crash:
        // commit the renamed directory entry as well on Unix filesystems.
        #[cfg(unix)]
        {
            let directory=root.to_owned();
            tokio::task::spawn_blocking(move||->Result<(),String>{
                std::fs::File::open(directory).map_err(|e|e.to_string())?
                    .sync_all().map_err(|e|e.to_string())
            }).await.map_err(|e|e.to_string())??;
        }
        Ok::<(),String>(())
    }.await;
    if result.is_err(){let _=fs::remove_file(&temporary).await;}
    result
}

/// A single persisted Run update; no full snapshot or model transcript is copied.
#[derive(Clone,Debug)]
pub struct RunUpdate{
    pub id:Uuid,
    pub status:RunStatus,
    pub updated_at_ms:u64,
}
impl RunUpdate{
    fn from_run(run:&Run)->Self{
        Self{id:run.id,status:run.status.clone(),updated_at_ms:run.updated_at_ms}
    }
}
pub struct Store {
    root: PathBuf,
    state: Mutex<HashMap<Uuid, Run>>,
    updates: broadcast::Sender<RunUpdate>,
}
impl Store {
    pub async fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
        let mut state = HashMap::new();
        let mut entries = fs::read_dir(&root).await.map_err(|e| e.to_string())?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
            let data = fs::read(entry.path()).await.map_err(|e| e.to_string())?;
            let mut run: Run = serde_json::from_slice(&data).map_err(|e| format!("corrupt run {}: {e}", entry.path().display()))?;
            // Risk-aware restart. Never replay mutating tool intents with
            // unknown results; auto-retry only the locally pure inspection set.
            journal::recover(&mut run);
            if run.model_only && !run.terminal(){
                // The interrupted provider call may have been billed. It
                // cannot be safely replayed and MUST NOT become an agent run.
                run.status=crate::domain::RunStatus::Failed;
                run.event("model_only_interrupted",
                    "Provider turn interrupted by restart; no model retry or tool execution performed",
                    None);
            }
            state.insert(run.id, run);
        }
        // Backward-compatible recovery: earlier revisions allocated child
        // budgets but did not persist the reservation on the parent.
        let child_allocations=state.values().filter_map(|child|
            child.parent_run_id.map(|parent|(parent,child.id,child.budget_krw)))
            .collect::<Vec<_>>();
        let mut legacy_reserved=HashMap::<Uuid,f64>::new();
        for (parent,child,budget) in child_allocations {
            if state.get(&parent).is_some_and(|r|!r.settled_child_ids.contains(&child)){
                *legacy_reserved.entry(parent).or_default()+=budget;
            }
        }
        for (parent,amount) in legacy_reserved{
            if let Some(run)=state.get_mut(&parent){
                if run.reserved_budget_krw==0.0 && amount>0.0 {
                    run.reserved_budget_krw=amount;
                    run.event("reservation_migrated","Recovered pre-escrow child budget reservations",None);
                }
            }
        }
        let (updates,_) = broadcast::channel(2048);
        let store = Self { root, state: Mutex::new(state), updates };
        // Persist recovery markers to make restart behavior stable across crashes.
        for run in store.list().await { store.put(run).await?; }
        Ok(store)
    }
    pub async fn put(&self, run: Run) -> Result<(), String> {
        let mut guard = self.state.lock().await;
        persist(&self.root,&run).await?;
        let update=RunUpdate::from_run(&run);
        guard.insert(run.id, run);
        drop(guard);
        let _=self.updates.send(update);
        Ok(())
    }
    /// Event-driven updates: idle browser connections never rescan all Runs.
    /// Durable state is committed before a notification is published.
    pub fn subscribe(&self)->broadcast::Receiver<RunUpdate>{
        self.updates.subscribe()
    }
    pub async fn get(&self, id: Uuid) -> Option<Run> { self.state.lock().await.get(&id).cloned() }
    pub async fn list(&self) -> Vec<Run> {
        let mut runs: Vec<_> = self.state.lock().await.values().cloned().collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.created_at_ms));
        runs
    }
    pub async fn mutate<F>(&self, id: Uuid, change: F) -> Result<Run, String>
    where F: FnOnce(&mut Run) -> Result<(), String> {
        let mut guard = self.state.lock().await;
        let mut run = guard.get(&id).cloned().ok_or("execution not found")?;
        change(&mut run)?;
        persist(&self.root,&run).await?;
        guard.insert(id, run.clone());
        drop(guard);
        let _=self.updates.send(RunUpdate::from_run(&run));
        Ok(run)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CreateRun;


    #[tokio::test]
    async fn durable_mutations_emit_exactly_one_event_without_polling_history(){
        use std::time::Duration;
        let root=std::env::temp_dir().join(format!("dominion-broadcast-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut changes=store.subscribe();
        let new=Run::new(CreateRun{instruction:"inspect source".into(),budget_krw:Some(50.0),max_steps:Some(2)},2).unwrap();
        let id=new.id;
        store.put(new).await.unwrap();
        let first=tokio::time::timeout(Duration::from_secs(1),changes.recv()).await.unwrap().unwrap();
        assert_eq!(first.id,id);
        assert_eq!(first.status,RunStatus::Queued);
        store.mutate(id,|run|{run.status=RunStatus::Paused;run.event("paused","operator pause",None);Ok(())}).await.unwrap();
        let second=tokio::time::timeout(Duration::from_secs(1),changes.recv()).await.unwrap().unwrap();
        assert_eq!(second.id,id);
        assert_eq!(second.status,RunStatus::Paused);
        assert!(second.updated_at_ms>=first.updated_at_ms);
        assert!(store.mutate(id,|_run|Err("reject".into())).await.is_err());
        assert!(tokio::time::timeout(Duration::from_millis(60),changes.recv()).await.is_err(),
            "failed mutation must not publish a phantom event");
        drop(changes);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    /// Long-running IO/recovery soak with a real fsync-backed Store and 16
    /// concurrent workers. It uses no model or remote API credits.
    /// cargo test --lib soak_parallel_durable_workloads -- --ignored --nocapture
    /// Continuous crash/approval/cancellation/replay stress, not an idle sleep.
    /// Repeatedly creates new persistent stores and replays actual journal
    /// transitions until a wall-clock deadline expires.
    #[tokio::test]
    #[ignore = "30s opt-in continuous recovery stress"]
    async fn continuous_recovery_and_cancellation_soak(){
        use crate::domain::ToolCall;
        use serde_json::json;
        use std::time::{Duration,Instant};
        let seconds=std::env::var("DOMINION_LONG_SOAK_SECONDS").ok()
            .and_then(|s|s.parse::<u64>().ok()).unwrap_or(30).clamp(5,90);
        let root=std::env::temp_dir().join(format!("dominion-continuous-{}",Uuid::new_v4()));
        let start=Instant::now();
        let mut cycles=0usize;
        let mut completed=0usize;
        let mut remote_uncertain=0usize;
        let mut safe_reads=0usize;
        let mut cancelled=0usize;
        while start.elapsed()<Duration::from_secs(seconds){
            let path=root.join(cycles.to_string());
            let store=Store::open(path.clone()).await.unwrap();
            let mut ids=Vec::new();
            for i in 0..12usize{
                let mut run=Run::new(CreateRun{
                    instruction:format!("durability cycle {cycles} run {i}"),
                    budget_krw:Some(100.0),max_steps:Some(8)
                },8).unwrap();
                let kind=i%3;
                let call=ToolCall{
                    id:format!("call-{cycles}-{i}"),
                    name:if kind==1{"read_file"}else{"write_file"}.into(),
                    arguments:json!({"path":"sample.txt","content":"payload"})
                };
                run.status=RunStatus::Running;
                run.queued_calls.push(call.clone());
                let id=run.id;
                store.put(run).await.unwrap();
                store.mutate(id,|run|journal::begin(run,&call)).await.unwrap();
                if kind==0 && i%2==0{
                    store.mutate(id,|run|{
                        run.cancel_after_tool=true;
                        run.status=RunStatus::Paused;
                        run.event("cancel_pending_reconciliation",
                            "Awaiting uncertain write outcome",None);
                        Ok(())
                    }).await.unwrap();
                }
                if kind==2{
                    store.mutate(id,|run|journal::finish(
                        run,&call,&Ok("confirmed written".into()),false
                    )).await.unwrap();
                }
                ids.push((id,kind,call));
            }
            drop(store);
            let restarted=Store::open(path.clone()).await.unwrap();
            for (id,kind,call) in ids{
                let run=restarted.get(id).await.unwrap();
                match kind{
                    0=>{
                        assert!(run.in_flight.is_some(),"remote write replayed");
                        assert_eq!(run.queued_calls.len(),1);
                        let result=restarted.mutate(id,|run|{
                            journal::finish(run,&call,&Ok("observed external write".into()),true)
                        }).await.unwrap();
                        assert!(result.in_flight.is_none());
                        remote_uncertain+=1;
                        if result.status==RunStatus::Cancelled{cancelled+=1;}
                    },
                    1=>{
                        assert!(run.in_flight.is_none(),"safe read did not recover");
                        assert_eq!(run.queued_calls.len(),1);
                        let receipt=restarted.mutate(id,|run|{
                            run.status=RunStatus::Running;
                            journal::begin(run,&call)?;
                            journal::finish(run,&call,&Ok("read repeated safely".into()),false)
                        }).await.unwrap();
                        assert!(receipt.queued_calls.is_empty());
                        safe_reads+=1;
                    },
                    2=>{
                        assert!(run.in_flight.is_none());
                        assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),1);
                        completed+=1;
                    },
                    _=>unreachable!()
                }
            }
            drop(restarted);
            fs::remove_dir_all(path).await.unwrap();
            cycles+=1;
        }
        assert!(cycles>0);
        assert_eq!(remote_uncertain,cycles*4);
        assert_eq!(safe_reads,cycles*4);
        assert_eq!(completed,cycles*4);
        println!("{}",json!({
            "benchmark":"continuous_durable_recovery",
            "wall_seconds":start.elapsed().as_secs_f64(),
            "cycles":cycles,
            "runs":cycles*12,
            "unknown_mutations_reconciled":remote_uncertain,
            "safe_reads_retried":safe_reads,
            "cancelled_after_reconciliation":cancelled,
            "repeated_mutating_dispatches":0,
            "provider_cost_krw":0
        }));
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    #[ignore = "disk-intensive reliability/performance test, run explicitly"]
    async fn soak_parallel_durable_workloads(){
        use crate::domain::ToolCall;
        use serde_json::json;
        use std::{sync::Arc,time::Instant};
        let start=Instant::now();
        let root=std::env::temp_dir().join(format!("dominion-soak-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let mut work=tokio::task::JoinSet::new();
        const TOTAL:usize=256;
        const WORKERS:usize=16;
        for worker in 0..WORKERS {
            let store=Arc::clone(&store);
            work.spawn(async move{
                let mut ids=Vec::new();
                let mut spans=Vec::new();
                for i in (worker..TOTAL).step_by(WORKERS){
                    let cycle=Instant::now();
                    let mut run=Run::new(CreateRun{
                        instruction:format!("soak {i}"),budget_krw:Some(20.0),max_steps:Some(4)
                    },4).unwrap();
                    let id=run.id;
                    let name=match i%4{
                        0|1=>"write_file",2=>"read_file",_=>"git_diff"
                    };
                    let call=ToolCall{id:format!("soak-tool-{i}"),name:name.into(),
                        arguments:json!({"path":"artifact.txt","content":"content"})};
                    run.status=RunStatus::Running;
                    run.queued_calls.push(call.clone());
                    store.put(run).await.unwrap();
                    store.mutate(id,|run|journal::begin(run,&call)).await.unwrap();
                    if i%4==0 {
                        store.mutate(id,|run|journal::finish(
                            run,&call,&Ok("simulated external receipt".into()),false)
                        ).await.unwrap();
                    }
                    spans.push(cycle.elapsed().as_millis() as u64);
                    ids.push((id,i));
                }
                (ids,spans)
            });
        }
        let mut ids=Vec::new();let mut latencies=Vec::new();
        while let Some(next)=work.join_next().await{
            let (mut chunk,mut samples)=next.unwrap();
            ids.append(&mut chunk);latencies.append(&mut samples);
        }
        assert_eq!(ids.len(),TOTAL);
        drop(store); // power-loss simulation: only snapshots survive
        let restored=Store::open(root.clone()).await.unwrap();
        let mut counts=[0usize;4];
        for (id,i) in &ids {
            let run=restored.get(*id).await.expect("persisted run disappeared");
            assert_eq!(run.status,RunStatus::Paused);
            match i%4 {
                0=>{
                    assert!(run.in_flight.is_none());
                    assert_eq!(run.messages.iter().filter(|m|m["role"]=="tool").count(),1);
                }
                1=>{
                    assert!(run.in_flight.is_some(),"uncertain mutating call replayed");
                    assert_eq!(run.events.last().unwrap().kind,"tool_outcome_unknown");
                }
                2|3=>{
                    assert!(run.in_flight.is_none(),"pure read not recovered");
                    assert_eq!(run.queued_calls.len(),1);
                    assert_eq!(run.events.last().unwrap().kind,"recovered_ready");
                }
                _=>unreachable!()
            }
            counts[i%4]+=1;
        }
        assert_eq!(counts,[64,64,64,64]);
        latencies.sort_unstable();
        let wall=start.elapsed().as_secs_f64().max(0.001);
        let p50=latencies[latencies.len()/2];
        let p95=latencies[latencies.len()*95/100];
        println!("{}",json!({
            "benchmark":"durable_tool_lifecycle_soak",
            "snapshot_count":TOTAL,
            "parallel_workers":WORKERS,
            "intent_and_receipt_snapshots":256*2+64,
            "recovered_safe_reads":128,
            "unconfirmed_writes_blocked":64,
            "observed_write_receipts":64,
            "wall_seconds":wall,
            "runs_per_second":TOTAL as f64/wall,
            "p50_run_persist_ms":p50,
            "p95_run_persist_ms":p95,
            "provider_api_cost_krw":0
        }));
        drop(restored);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn mutating_tool_crash_never_replays_or_drops_unknown_intent(){
        use crate::{domain::ToolCall,journal};
        use serde_json::json;
        let root=std::env::temp_dir().join(format!("dominion-crash-write-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{instruction:"edit".into(),budget_krw:None,max_steps:None},4).unwrap();
        run.status=crate::domain::RunStatus::Running;
        let call=ToolCall{id:"write-1".into(),name:"write_file".into(),
            arguments:json!({"path":"important.txt","content":"payload"})};
        run.queued_calls.push(call.clone());
        // Simulated crash after durable intent and before the tool receipt.
        journal::begin(&mut run,&call).unwrap();
        let id=run.id;store.put(run).await.unwrap();drop(store);
        let restarted=Store::open(root.clone()).await.unwrap();
        let snapshot=restarted.get(id).await.unwrap();
        assert_eq!(snapshot.status,crate::domain::RunStatus::Paused);
        assert_eq!(snapshot.in_flight.unwrap().id,call.id);
        assert_eq!(snapshot.events.last().unwrap().kind,"tool_outcome_unknown");
        // Explicit observation is the ONLY permissible completion.
        restarted.mutate(id,|run|{
            journal::finish(run,&call,&Ok("confirmed on disk".into()),true)
        }).await.unwrap();
        drop(restarted);
        let final_state=Store::open(root.clone()).await.unwrap().get(id).await.unwrap();
        assert!(final_state.in_flight.is_none());
        assert!(final_state.queued_calls.is_empty());
        assert_eq!(final_state.events.iter().filter(|e|e.kind=="tool_reconciled").count(),1);
        assert!(final_state.messages.last().unwrap()["content"].as_str().unwrap().contains("confirmed"));
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn read_only_tool_retries_queued_call_after_crash(){
        use crate::{domain::ToolCall,journal};
        use serde_json::json;
        let root=std::env::temp_dir().join(format!("dominion-crash-read-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{instruction:"inspect".into(),budget_krw:None,max_steps:None},4).unwrap();
        run.status=crate::domain::RunStatus::Running;
        let call=ToolCall{id:"read-1".into(),name:"read_file".into(),arguments:json!({"path":"hello.txt"})};
        run.queued_calls.push(call.clone());
        journal::begin(&mut run,&call).unwrap();
        let id=run.id;store.put(run).await.unwrap();drop(store);
        let restored=Store::open(root.clone()).await.unwrap().get(id).await.unwrap();
        assert_eq!(restored.status,crate::domain::RunStatus::Paused);
        assert!(restored.in_flight.is_none());
        assert_eq!(restored.queued_calls.len(),1);
        assert_eq!(restored.events.last().unwrap().kind,"recovered_ready");
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn failed_snapshot_write_does_not_publish_in_memory_state(){
        let root=std::env::temp_dir().join(format!("dominion-store-error-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let run=Run::new(CreateRun{instruction:"persist".into(),budget_krw:None,max_steps:None},4).unwrap();
        let id=run.id;store.put(run).await.unwrap();
        let target=root.join(format!("{id}.json"));
        fs::remove_file(&target).await.unwrap();
        fs::create_dir(&target).await.unwrap(); // Rename to directory must fail.
        assert!(store.mutate(id,|r|{r.status=crate::domain::RunStatus::Completed;Ok(())}).await.is_err());
        assert_eq!(store.get(id).await.unwrap().status,crate::domain::RunStatus::Queued);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn recovers_interrupted_execution() {
        let dir = std::env::temp_dir().join(format!("dominion-{}", Uuid::new_v4()));
        let store = Store::open(dir.clone()).await.unwrap();
        let mut run = Run::new(CreateRun { instruction: "test".into(), budget_krw: Some(0.0), max_steps: None }, 5).unwrap();
        run.status = RunStatus::Running;
        let id = run.id;
        store.put(run).await.unwrap();
        drop(store);
        let restored = Store::open(dir.clone()).await.unwrap().get(id).await.unwrap();
        assert_eq!(restored.status, RunStatus::Paused);
        assert!(restored.events.iter().any(|e| e.kind == "recovered_ready"));
        fs::remove_dir_all(dir).await.unwrap();
    }
}
