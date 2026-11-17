use crate::{domain::{Run,RunStatus},journal};
use std::{collections::{HashMap,HashSet}, path::{Path,PathBuf},sync::Arc};
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
    pub output_tokens:u64,
    pub tool_calls:usize,
    pub last_error:Option<String>,
}
impl RunHeader {
    fn from_run(run:&Run)->Self{
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
            output_tokens:run.output_tokens,
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
pub struct Store {
    root: PathBuf,
    state: Mutex<HashMap<Uuid, Run>>,
    // A run's write-ahead snapshot must commit in order; unrelated run
    // transactions must not hold the global metadata lock during fsync.
    gates: Mutex<HashMap<Uuid,Arc<Mutex<()>>>>,
    updates: broadcast::Sender<RunUpdate>,
}
impl Store {
    pub async fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).await.map_err(|e| e.to_string())?;
        let mut state = HashMap::new();
        // Only recovery or migration changes require a new fsync-backed
        // snapshot. Stable historical runs must not be rewritten on startup.
        let mut dirty = HashSet::<Uuid>::new();
        let mut entries = fs::read_dir(&root).await.map_err(|e| e.to_string())?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
            let data = fs::read(entry.path()).await.map_err(|e| e.to_string())?;
            let mut run: Run = serde_json::from_slice(&data).map_err(|e| format!("corrupt run {}: {e}", entry.path().display()))?;
            // Compare semantic Run snapshots, not raw file bytes: legacy
            // JSON whitespace/key ordering must not trigger disk writes.
            let before = serde_json::to_vec(&run).map_err(|e| e.to_string())?;
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
            if serde_json::to_vec(&run).map_err(|e| e.to_string())? != before {
                dirty.insert(run.id);
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
                    dirty.insert(parent);
                }
            }
        }
        // Commit only modified records before exposing the Store to callers.
        // This preserves uncertain-effect markers while avoiding one rename
        // and multiple fsyncs per unchanged historical Run on every boot.
        for id in dirty {
            let run=state.get(&id).ok_or("recovery record missing")?;
            persist(&root,run).await?;
        }
        let (updates,_) = broadcast::channel(2048);
        Ok(Self { root, state: Mutex::new(state), gates:Mutex::new(HashMap::new()), updates })
    }
    async fn gate(&self,id:Uuid)->Arc<Mutex<()>>{
        let mut locks=self.gates.lock().await;
        Arc::clone(locks.entry(id).or_insert_with(||Arc::new(Mutex::new(()))))
    }
    pub async fn put(&self, run: Run) -> Result<(), String> {
        // Serialize same-run writes, including put vs mutate, but release the
        // global Run index before any disk I/O. No dirty value is published
        // until rename + fsync completes.
        let gate=self.gate(run.id).await;
        let _run_write=gate.lock().await;
        persist(&self.root,&run).await?;
        let update=RunUpdate::from_run(&run);
        self.state.lock().await.insert(run.id,run);
        let _=self.updates.send(update);
        Ok(())
    }
    /// Event-driven updates: idle browser connections never rescan all Runs.
    /// Durable state is committed before a notification is published.
    pub fn subscribe(&self)->broadcast::Receiver<RunUpdate>{
        self.updates.subscribe()
    }
    pub async fn get(&self, id: Uuid) -> Option<Run> { self.state.lock().await.get(&id).cloned() }
    /// Inspect child existence under the store lock without cloning every
    /// transcript; the planner calls this once per model step.
    pub async fn has_children(&self, parent: Uuid) -> bool {
        self.state.lock().await.values().any(|run|run.parent_run_id==Some(parent))
    }
    /// Only clone direct children when checking delegation and settlement.
    /// Full history snapshots are reserved for explicit list operations.
    pub async fn children_of(&self, parent: Uuid) -> Vec<Run> {
        self.state.lock().await.values()
            .filter(|run|run.parent_run_id==Some(parent))
            .cloned().collect()
    }
    /// Capture a consistent 2-level family snapshot for budget settlement.
    /// Only direct children and grandchildren are cloned, never unrelated
    /// runs or their full transcripts. Grandchild settlement is required
    /// before a child's unused budget can be returned to its parent.
    pub async fn family_of(&self, parent: Uuid) -> Vec<Run> {
        let guard=self.state.lock().await;
        let child_ids=guard.values().filter(|run|run.parent_run_id==Some(parent))
            .map(|run|run.id).collect::<HashSet<_>>();
        guard.values().filter(|run|
            run.parent_run_id==Some(parent)
                || run.parent_run_id.is_some_and(|id|child_ids.contains(&id))
        ).cloned().collect()
    }
    /// Small hot-path overview for the /api/status poll. O(n) status
    /// inspection under one short lock; O(1) transcript allocation.
    pub async fn stats(&self)->RunStats{
        let guard=self.state.lock().await;
        let mut stats=RunStats{total:guard.len(),..RunStats::default()};
        for run in guard.values(){
            if matches!(run.status,RunStatus::Queued|RunStatus::Running){
                stats.active+=1;
            }
            if run.pending_approval.is_some()&&run.approval_decision.is_none(){
                stats.awaiting_approval+=1;
            }
        }
        stats
    }
    /// UI lists materialize only rows, never N complete transcripts.
    pub async fn headers(&self)->Vec<RunHeader>{
        let mut rows=self.state.lock().await.values()
            .map(RunHeader::from_run).collect::<Vec<_>>();
        rows.sort_by_key(|r|std::cmp::Reverse(r.created_at_ms));
        rows
    }
    pub async fn pending_approvals(&self)->Vec<(Uuid,crate::domain::ToolCall)>{
        self.state.lock().await.values().filter_map(|run|{
            run.pending_approval.as_ref()
                .filter(|_|run.approval_decision.is_none())
                .map(|call|(run.id,call.clone()))
        }).collect()
    }
    pub async fn list(&self) -> Vec<Run> {
        let mut runs: Vec<_> = self.state.lock().await.values().cloned().collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.created_at_ms));
        runs
    }
    /// Startup needs only IDs to recover delegated parents and resume safe
    /// interrupted runs. Do not clone every durable transcript, tool receipt,
    /// and event merely to inspect two status predicates.
    pub async fn recovery_candidates(&self) -> (Vec<Uuid>,Vec<Uuid>) {
        let guard=self.state.lock().await;
        let mut runs=guard.values().collect::<Vec<_>>();
        runs.sort_by_key(|run|std::cmp::Reverse(run.created_at_ms));
        let suspended=runs.iter().filter(|run|run.waiting_for_children)
            .map(|run|run.id).collect();
        let safe=runs.iter().filter(|run|
            run.status==RunStatus::Paused && run.in_flight.is_none()
            && !run.waiting_for_children
            && run.events.last().is_some_and(|event|event.kind=="recovered_ready")
        ).map(|run|run.id).collect();
        (suspended,safe)
    }
    pub async fn mutate<F>(&self, id: Uuid, change: F) -> Result<Run, String>
    where F: FnOnce(&mut Run) -> Result<(), String> {
        // A single run owns its serializable mutation order. Other workers
        // may read or update different Runs while this Run is in fsync.
        let gate=self.gate(id).await;
        let _run_write=gate.lock().await;
        let mut run=self.state.lock().await.get(&id).cloned()
            .ok_or("execution not found")?;
        change(&mut run)?;
        persist(&self.root,&run).await?;
        self.state.lock().await.insert(id,run.clone());
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

    #[tokio::test]
    async fn unrelated_run_updates_proceed_while_another_run_is_locked(){
        use tokio::time::{timeout,Duration};
        let root=std::env::temp_dir().join(format!("dominion-per-run-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let a=Run::new(CreateRun{instruction:"a".into(),
            budget_krw:None,max_steps:None},2).unwrap();
        let b=Run::new(CreateRun{instruction:"b".into(),
            budget_krw:None,max_steps:None},2).unwrap();
        let a_id=a.id;
        let b_id=b.id;
        store.put(a).await.unwrap();
        store.put(b).await.unwrap();
        let locked=store.gate(a_id).await;
        let hold=locked.lock().await;
        let pending=Arc::clone(&store);
        let mut a_task=tokio::spawn(async move{
            pending.mutate(a_id,|run|{run.event("a","one",None);Ok(())}).await.unwrap()
        });
        tokio::task::yield_now().await;
        let b_committed=timeout(Duration::from_secs(3),store.mutate(b_id,|run|{
            run.event("b","independent progress",None);Ok(())
        })).await.expect("unrelated Run blocked by another Run's write gate").unwrap();
        assert_eq!(b_committed.events.last().unwrap().kind,"b");
        assert!(!a_task.is_finished(),"same-run gate did not order the blocked mutation");
        drop(hold);
        let a_committed=timeout(Duration::from_secs(3),&mut a_task).await.unwrap().unwrap();
        assert_eq!(a_committed.events.last().unwrap().kind,"a");
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn same_run_mutations_preserve_fsync_order_and_no_phantom_updates(){
        let root=std::env::temp_dir().join(format!("dominion-serial-{}",Uuid::new_v4()));
        let store=Arc::new(Store::open(root.clone()).await.unwrap());
        let run=Run::new(CreateRun{instruction:"serial".into(),
            budget_krw:None,max_steps:None},3).unwrap();
        let id=run.id;
        store.put(run).await.unwrap();
        let gate=store.gate(id).await;
        let held=gate.lock().await;
        let one=Arc::clone(&store);
        let first=tokio::spawn(async move{
            one.mutate(id,|r|{r.event("first","persist first",None);Ok(())}).await.unwrap()
        });
        tokio::task::yield_now().await;
        drop(held);
        first.await.unwrap();
        let two=Arc::clone(&store);
        two.mutate(id,|r|{r.event("second","persist second",None);Ok(())}).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().events.iter().rev()
            .take(2).map(|v|v.kind.as_str()).collect::<Vec<_>>(),
            vec!["second","first"]);
        let reopened=Store::open(root.clone()).await.unwrap();
        // Recovery may append "recovered_ready" after the durable writes;
        // compare the persisted mutation receipts, not the last two events.
        assert_eq!(reopened.get(id).await.unwrap().events.iter().rev()
            .filter(|v|v.kind=="first"||v.kind=="second")
            .map(|v|v.kind.as_str()).collect::<Vec<_>>(),
            vec!["second","first"]);
        drop(reopened);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn overview_and_stats_do_not_include_model_transcript_payloads(){
        let root=std::env::temp_dir().join(format!("dominion-overview-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(crate::domain::CreateRun{
            instruction:"Inspect cheap overview".into(),budget_krw:None,max_steps:None
        },4).unwrap();
        run.messages.push(serde_json::json!({"role":"tool","content":"x".repeat(200_000)}));
        run.status=RunStatus::Running;
        let id=run.id;
        store.put(run).await.unwrap();
        let stats=store.stats().await;
        assert_eq!((stats.total,stats.active,stats.awaiting_approval),(1,1,0));
        let rows=store.headers().await;
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].id,id);
        assert_eq!(rows[0].instruction,"Inspect cheap overview");
        assert!(format!("{:?}",rows[0]).len()<1000,
            "overview unexpectedly copied historical tool transcripts");
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn startup_recovery_candidates_ignore_unrelated_full_transcripts(){
        let root=std::env::temp_dir().join(format!("dominion-recovery-ids-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut waiting=Run::new(CreateRun{
            instruction:"parent waiting for children".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        waiting.status=RunStatus::Paused;
        waiting.waiting_for_children=true;
        let waiting_id=waiting.id;
        let mut recovered=Run::new(CreateRun{
            instruction:"safe interrupted read".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        recovered.status=RunStatus::Paused;
        recovered.event("recovered_ready","Pure read may safely resume",None);
        let safe_id=recovered.id;
        let mut unresolved=Run::new(CreateRun{
            instruction:"unreconciled write".into(),
            budget_krw:None,max_steps:None
        },3).unwrap();
        unresolved.status=RunStatus::Paused;
        unresolved.event("tool_outcome_unknown","Do not replay external effect",None);
        store.put(waiting).await.unwrap();
        store.put(recovered).await.unwrap();
        store.put(unresolved).await.unwrap();
        let (suspended,safe)=store.recovery_candidates().await;
        assert_eq!(suspended,vec![waiting_id]);
        assert_eq!(safe,vec![safe_id]);
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn child_lookup_does_not_include_unrelated_runs(){
        let root=std::env::temp_dir().join(format!("dominion-child-lookup-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let parent=Run::new(CreateRun{
            instruction:"parent".into(),budget_krw:Some(100.0),max_steps:Some(2)
        },2).unwrap();
        let id=parent.id;
        let mut child=Run::new(CreateRun{
            instruction:"child".into(),budget_krw:Some(10.0),max_steps:Some(2)
        },2).unwrap();
        child.parent_run_id=Some(id);
        let child_id=child.id;
        let unrelated=Run::new(CreateRun{
            instruction:"unrelated".into(),budget_krw:None,max_steps:None
        },2).unwrap();
        store.put(parent).await.unwrap();
        store.put(child).await.unwrap();
        store.put(unrelated.clone()).await.unwrap();
        assert!(store.has_children(id).await);
        assert!(!store.has_children(child_id).await);
        assert!(!store.has_children(unrelated.id).await);
        let children=store.children_of(id).await;
        assert_eq!(children.len(),1);
        assert_eq!(children[0].id,child_id);
        assert!(store.children_of(unrelated.id).await.is_empty());
        let mut grandchild=Run::new(CreateRun{
            instruction:"grandchild".into(),budget_krw:Some(5.0),max_steps:Some(2)
        },2).unwrap();
        grandchild.parent_run_id=Some(child_id);
        let grandchild_id=grandchild.id;
        store.put(grandchild).await.unwrap();
        let family=store.family_of(id).await;
        assert_eq!(family.len(),2);
        assert!(family.iter().any(|run|run.id==child_id));
        assert!(family.iter().any(|run|run.id==grandchild_id));
        assert!(store.family_of(unrelated.id).await.is_empty());
        drop(store);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn startup_does_not_rewrite_unchanged_historical_runs(){
        let root=std::env::temp_dir().join(format!("dominion-stable-history-{}",Uuid::new_v4()));
        let store=Store::open(root.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{
            instruction:"preserve completed history".into(),
            budget_krw:Some(20.0),max_steps:Some(2)
        },2).unwrap();
        run.status=RunStatus::Completed;
        let id=run.id;
        store.put(run).await.unwrap();
        drop(store);

        // A valid historical JSON snapshot may contain insignificant
        // whitespace. Reopening it must not replace the disk file.
        let path=root.join(format!("{id}.json"));
        let mut bytes=fs::read(&path).await.unwrap();
        bytes.extend_from_slice(b"\n  \n");
        fs::write(&path,&bytes).await.unwrap();
        let reopened=Store::open(root.clone()).await.unwrap();
        assert_eq!(reopened.get(id).await.unwrap().status,RunStatus::Completed);
        assert_eq!(fs::read(&path).await.unwrap(),bytes,
            "startup rewrote an unchanged historical Run");
        drop(reopened);
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
