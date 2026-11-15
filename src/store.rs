use crate::domain::{Run, RunStatus};
use std::{collections::HashMap, path::PathBuf};
use tokio::{fs, sync::Mutex};
use uuid::Uuid;

pub struct Store {
    root: PathBuf,
    state: Mutex<HashMap<Uuid, Run>>,
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
            if run.status == RunStatus::Running || run.status == RunStatus::Queued {
                run.status = RunStatus::Paused;
                run.event("recovered", "Interrupted execution paused for recovery", None);
                if let Some(call) = run.in_flight.clone() {
                    run.event("tool_uncertain", format!("Interrupted {} ({}) needs reconciliation",call.name,call.id), None);
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
                }
            }
        }
        let store = Self { root, state: Mutex::new(state) };
        // Persist recovery markers to make restart behavior stable across crashes.
        for run in store.list().await { store.put(run).await?; }
        Ok(store)
    }
    pub async fn put(&self, run: Run) -> Result<(), String> {
        let mut guard = self.state.lock().await;
        let file = self.root.join(format!("{}.json", run.id));
        let tmp = self.root.join(format!("{}.tmp", run.id));
        let bytes = serde_json::to_vec(&run).map_err(|e| e.to_string())?;
        fs::write(&tmp, bytes).await.map_err(|e| e.to_string())?;
        fs::rename(tmp, file).await.map_err(|e| e.to_string())?;
        guard.insert(run.id, run);
        Ok(())
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
        let file = self.root.join(format!("{id}.json"));
        let tmp = self.root.join(format!("{id}.tmp"));
        let bytes = serde_json::to_vec(&run).map_err(|e| e.to_string())?;
        fs::write(&tmp, bytes).await.map_err(|e| e.to_string())?;
        fs::rename(tmp, file).await.map_err(|e| e.to_string())?;
        guard.insert(id, run.clone());
        Ok(run)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CreateRun;
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
        assert!(restored.events.iter().any(|e| e.kind == "recovered"));
        fs::remove_dir_all(dir).await.unwrap();
    }
}
