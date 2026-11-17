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
mod tests;
