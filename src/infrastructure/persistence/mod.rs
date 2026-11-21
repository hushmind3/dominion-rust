use crate::{domain::{Run,RunStatus},journal};
use std::{collections::{HashMap,HashSet}, path::{Path,PathBuf},sync::{Arc,Weak}};
use tokio::{fs, io::AsyncWriteExt, sync::{Mutex,broadcast}};
use uuid::Uuid;
mod sqlite;

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

// Projection DTOs belong to the pure storage port, never the file backend.
pub use crate::ports::{RunUpdate,RunHeader,RunStats};

/// The Run map and parent links live behind ONE metadata lock.  A separate
/// child-index lock would allow budget settlement to see mixed generations.
struct RunIndex {
    runs: HashMap<Uuid,Run>,
    child_ids: HashMap<Uuid,HashSet<Uuid>>,
}
impl RunIndex {
    fn from_runs(runs:HashMap<Uuid,Run>)->Self {
        let mut child_ids=HashMap::<Uuid,HashSet<Uuid>>::new();
        for run in runs.values() {
            if let Some(parent)=run.parent_run_id {
                child_ids.entry(parent).or_default().insert(run.id);
            }
        }
        Self{runs,child_ids}
    }
    fn insert(&mut self,id:Uuid,run:Run)->Option<Run> {
        // Replacement is used by both put and mutate.  Remove the old link
        // before publishing the new one; no reader sees a half-updated index.
        if let Some(parent)=self.runs.get(&id).and_then(|old|old.parent_run_id) {
            if let Some(children)=self.child_ids.get_mut(&parent) {
                children.remove(&id);
                if children.is_empty(){self.child_ids.remove(&parent);}
            }
        }
        if let Some(parent)=run.parent_run_id {
            self.child_ids.entry(parent).or_default().insert(id);
        }
        self.runs.insert(id,run)
    }
}
impl std::ops::Deref for RunIndex {
    type Target=HashMap<Uuid,Run>;
    fn deref(&self)->&Self::Target {&self.runs}
}

pub struct Store {
    root: PathBuf,
    sqlite_path:Option<PathBuf>,
    state: Mutex<RunIndex>,
    // A run's write-ahead snapshot must commit in order; unrelated run
    // transactions must not hold the global metadata lock during fsync.
    // Weak entries cannot retain completed Run transaction locks indefinitely.
    // An active put/mutate always owns a strong Arc until its fsync completes.
    gates: Mutex<HashMap<Uuid,Weak<Mutex<()>>>>,
    updates: broadcast::Sender<RunUpdate>,
}
impl Store {
    pub async fn open(root:PathBuf)->Result<Self,String>{
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let mut rows=Vec::new();
        let mut entries=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=entries.next_entry().await.map_err(|e|e.to_string())?{
            if entry.path().extension().and_then(|x|x.to_str())!=Some("json"){continue;}
            let raw=fs::read(entry.path()).await.map_err(|e|e.to_string())?;
            let run:Run=serde_json::from_slice(&raw)
                .map_err(|e|format!("corrupt Run {}: {e}",entry.path().display()))?;
            rows.push(run);
        }
        Self::from_snapshots(root,None,rows).await
    }
    /// Production bootstrap: migrate old JSON once, then use SQLite only.
    pub fn checkpoint_sqlite(data_dir:&Path)->Result<(),String>{
        sqlite::checkpoint(&data_dir.join("dominion.sqlite3"))
    }
    pub async fn open_sqlite(root:PathBuf)->Result<Self,String>{
        let parent=root.parent().ok_or("Run directory has no parent")?;
        let database=parent.join("dominion.sqlite3");
        let rows=sqlite::import_or_read(database.clone(),root.clone()).await?;
        Self::from_snapshots(root,Some(database),rows).await
    }
    async fn from_snapshots(root:PathBuf,sqlite_path:Option<PathBuf>,
        rows:Vec<Run>)->Result<Self,String>{
        let mut state=HashMap::new();
        let mut dirty=HashSet::<Uuid>::new();
        for mut run in rows{
            // Historical terminal and stable paused records have no recovery
            // transition. Do not serialize them twice merely to prove that a
            // no-op remains a no-op. The journal owns the recovery predicate.
            // Changed candidates still receive semantic JSON comparison to
            // preserve legacy formatting and migration compatibility.
            let before = if journal::needs_recovery(&run)
                || (run.model_only && !run.terminal()) {
                Some(serde_json::to_vec(&run).map_err(|e| e.to_string())?)
            } else {None};
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
            if let Some(previous)=before {
                if serde_json::to_vec(&run).map_err(|e|e.to_string())? != previous {
                    dirty.insert(run.id);
                }
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
            if let Some(db)=sqlite_path.as_ref(){
                sqlite::upsert(db.clone(),run.clone()).await?;
            }else{persist(&root,run).await?;}
        }
        let (updates,_) = broadcast::channel(2048);
        Ok(Self { root,sqlite_path, state: Mutex::new(RunIndex::from_runs(state)), gates:Mutex::new(HashMap::new()), updates })
    }
    async fn persist_current(&self,run:&Run)->Result<(),String>{
        if let Some(path)=self.sqlite_path.as_ref(){
            sqlite::upsert(path.clone(),run.clone()).await
        }else{persist(&self.root,run).await}
    }
    async fn gate(&self,id:Uuid)->Arc<Mutex<()>>{
        const SWEEP_AT:usize=128;
        let mut locks=self.gates.lock().await;
        if let Some(active)=locks.get(&id).and_then(Weak::upgrade){
            return active;
        }
        // Sweep only when the index reaches a useful batch size. Expired
        // weak entries have no active writer or waiter; live locks cannot be
        // removed while their transactions still hold a strong Arc.
        if locks.len()>=SWEEP_AT {
            locks.retain(|_,weak|weak.strong_count()>0);
        }
        let active=Arc::new(Mutex::new(()));
        locks.insert(id,Arc::downgrade(&active));
        active
    }
    /// The same per-Run fsync gate fences a duplicate remote create and a
    /// concurrent local mutation; a repeated request sees the original Run.
    pub async fn put_if_absent(&self,run:Run)->Result<(Run,bool),String>{
        let gate=self.gate(run.id).await;
        let _run_write=gate.lock().await;
        if let Some(existing)=self.state.lock().await.get(&run.id).cloned(){
            return Ok((existing,false));
        }
        self.persist_current(&run).await?;
        let update=RunUpdate::from_run(&run);
        self.state.lock().await.insert(run.id,run.clone());
        let _=self.updates.send(update);
        Ok((run,true))
    }
    pub async fn put(&self, run: Run) -> Result<(), String> {
        // Serialize same-run writes, including put vs mutate, but release the
        // global Run index before any disk I/O. No dirty value is published
        // until rename + fsync completes.
        let gate=self.gate(run.id).await;
        let _run_write=gate.lock().await;
        self.persist_current(&run).await?;
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
    /// Wait for a durable update to ONE Run without timer polling or copying
    /// unrelated model transcripts. A lagged receiver returns immediately:
    /// the caller must re-read the authoritative persisted snapshot.
    pub async fn changed(&self,id:Uuid,rx:&mut broadcast::Receiver<RunUpdate>)->bool{
        loop{
            match rx.recv().await{
                Ok(update) if update.id==id=>return true,
                Ok(_)=>continue,
                Err(broadcast::error::RecvError::Lagged(_))=>return true,
                Err(broadcast::error::RecvError::Closed)=>return false,
            }
        }
    }
    pub async fn get(&self, id: Uuid) -> Option<Run> { self.state.lock().await.get(&id).cloned() }
    /// O(1) indexed child existence for every model step.  Index and Run
    /// snapshots are committed together, after the durable write succeeds.
    pub async fn has_children(&self, parent: Uuid) -> bool {
        self.state.lock().await.child_ids.get(&parent)
            .is_some_and(|children|!children.is_empty())
    }
    /// Clone only the parent's actual children, not all historical Runs.
    pub async fn children_of(&self, parent: Uuid) -> Vec<Run> {
        let guard=self.state.lock().await;
        guard.child_ids.get(&parent).into_iter()
            .flat_map(|ids|ids.iter().filter_map(|id|guard.runs.get(id).cloned()))
            .collect()
    }
    /// A single metadata-lock snapshot of the child and grandchild closure
    /// preserves the original two-level settlement rule without an O(N)
    /// history scan on each delegated completion.
    pub async fn family_of(&self, parent: Uuid) -> Vec<Run> {
        let guard=self.state.lock().await;
        let mut ids=HashSet::<Uuid>::new();
        if let Some(children)=guard.child_ids.get(&parent) {
            for child in children {
                ids.insert(*child);
                if let Some(grandchildren)=guard.child_ids.get(child) {
                    ids.extend(grandchildren.iter().copied());
                }
            }
        }
        ids.into_iter().filter_map(|id|guard.runs.get(&id).cloned()).collect()
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
    /// Project the verified procedure records under the existing metadata
    /// lock. A strategy lookup must never clone every historical Run,
    /// model transcript, tool schema and durable receipt into the SDK/API.
    pub async fn strategy_evidence(&self,repository:&str)
        ->Vec<crate::strategy::StrategyEvidence>{
        if repository.trim().is_empty() || repository=="global"{return Vec::new();}
        self.state.lock().await.values()
            .filter(|run|run.repository.as_deref()==Some(repository)
                && run.strategy_learning && run.status==RunStatus::Completed)
            .filter_map(crate::strategy::from_run).collect()
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
        self.persist_current(&run).await?;
        self.state.lock().await.insert(id,run.clone());
        let _=self.updates.send(RunUpdate::from_run(&run));
        Ok(run)
    }
}

// Concrete JSON+fsync adapter belongs to infrastructure, not the core.
// Existing external users can still create Store::open(data_dir/runs).
impl crate::ports::RunRepository for Store {
    fn put(&self,run:Run)->crate::ports::StorageFuture<'_,Result<(),String>>{
        Box::pin(Store::put(self,run))
    }
    fn put_if_absent(&self,run:Run)->crate::ports::StorageFuture<'_,Result<(Run,bool),String>>{
        Box::pin(Store::put_if_absent(self,run))
    }
    fn get(&self,id:Uuid)->crate::ports::StorageFuture<'_,Option<Run>>{
        Box::pin(Store::get(self,id))
    }
    fn has_children(&self,id:Uuid)->crate::ports::StorageFuture<'_,bool>{
        Box::pin(Store::has_children(self,id))
    }
    fn children_of(&self,id:Uuid)->crate::ports::StorageFuture<'_,Vec<Run>>{
        Box::pin(Store::children_of(self,id))
    }
    fn family_of(&self,id:Uuid)->crate::ports::StorageFuture<'_,Vec<Run>>{
        Box::pin(Store::family_of(self,id))
    }
    fn stats(&self)->crate::ports::StorageFuture<'_,RunStats>{
        Box::pin(Store::stats(self))
    }
    fn headers(&self)->crate::ports::StorageFuture<'_,Vec<RunHeader>>{
        Box::pin(Store::headers(self))
    }
    fn strategy_evidence<'a>(&'a self,repository:&'a str)
        ->crate::ports::StorageFuture<'a,Vec<crate::strategy::StrategyEvidence>>{
        Box::pin(Store::strategy_evidence(self,repository))
    }
    fn pending_approvals(&self)
        ->crate::ports::StorageFuture<'_,Vec<(Uuid,crate::domain::ToolCall)>>{
        Box::pin(Store::pending_approvals(self))
    }
    fn list(&self)->crate::ports::StorageFuture<'_,Vec<Run>>{
        Box::pin(Store::list(self))
    }
    fn recovery_candidates(&self)->crate::ports::StorageFuture<'_,(Vec<Uuid>,Vec<Uuid>)>{
        Box::pin(Store::recovery_candidates(self))
    }
    fn mutate<'a>(&'a self,id:Uuid,change:crate::ports::RunMutation<'a>)
        ->crate::ports::StorageFuture<'a,Result<Run,String>>{
        Box::pin(Store::mutate(self,id,change))
    }
    fn subscribe(&self)->broadcast::Receiver<RunUpdate>{Store::subscribe(self)}
    fn changed<'a>(&'a self,id:Uuid,receiver:&'a mut broadcast::Receiver<RunUpdate>)
        ->crate::ports::StorageFuture<'a,bool>{
        Box::pin(Store::changed(self,id,receiver))
    }
}

#[cfg(test)]
mod tests;
