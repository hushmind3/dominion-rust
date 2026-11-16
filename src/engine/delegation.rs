//! Delegation, child result inspection and durable budget settlement.
//! One engine scheduler owns all child wakeups; no independent agent loop.
use super::*;

impl Engine {
    /// Settle finished child allocations exactly once and wake parents that
    /// paused *specifically* for child completion. Never auto-resume a human
    /// approval, an uncertain side effect, or a generic paused failure.
    pub(crate) async fn settle_and_wake_ancestors(self:&Arc<Self>,start:Uuid)->Result<(),String>{
        let mut ancestor=Some(start);
        while let Some(id)=ancestor {
            {
                let _guard=self.delegate_lock.lock().await;
                let family=self.store.family_of(id).await;
                let running=self.active.lock().await.clone();
                let direct=family.iter().filter(|r|r.parent_run_id==Some(id)).collect::<Vec<_>>();
                let finished=direct.iter().filter(|child|{
                    if !child.terminal()||child.in_flight.is_some()||running.contains(&child.id){return false;}
                    // A cancelled child may still own active grandchildren.
                    // Its unused allocation is returned only after those bills
                    // have also been durably settled into that child's record.
                    let grandchildren=family.iter().filter(|r|r.parent_run_id==Some(child.id)).collect::<Vec<_>>();
                    grandchildren.iter().all(|r|r.terminal()&&r.in_flight.is_none()
                        &&!running.contains(&r.id)&&child.settled_child_ids.contains(&r.id))
                }).map(|r|(*r).clone()).collect::<Vec<_>>();
                if !finished.is_empty(){
                    self.store.mutate(id,|run|{
                        for child in &finished {
                            if run.settled_child_ids.contains(&child.id){continue;}
                            run.reserved_budget_krw=(run.reserved_budget_krw-child.budget_krw).max(0.0);
                            let actual=child.cost_krw+child.delegated_spent_krw;
                            run.delegated_spent_krw+=actual;
                            run.settled_child_ids.push(child.id);
                            run.event("budget_settled",format!("Child {} used {:.4} of {:.4} KRW",child.id,actual,child.budget_krw),
                                Some(json!({"child_id":child.id,"cost_krw":actual,"released_krw":(child.budget_krw-actual).max(0.0)})));
                        }
                        if run.cost_krw>policy::effective_budget_krw(run) && !run.terminal(){
                            run.status=RunStatus::Failed;
                            run.event("budget_exceeded","Aggregate parent and delegated model charges exceeded budget",None);
                        }
                        Ok(())
                    }).await?;
                }
            }
            let children=self.store.children_of(id).await;
            let active_ids=self.active.lock().await.clone();
            let pending=children.iter().any(|r|
                !r.terminal()||r.in_flight.is_some()||active_ids.contains(&r.id));
            let parent=self.store.get(id).await.ok_or("parent run missing")?;
            if parent.waiting_for_children && !pending && !parent.terminal(){
                self.store.mutate(id,|run|{
                    if run.waiting_for_children && run.status==RunStatus::Paused && run.in_flight.is_none() && run.pending_approval.is_none(){
                        run.waiting_for_children=false;
                        run.event("subagents_ready","All delegated workers finished; resuming parent verification",None);
                    }
                    Ok(())
                }).await?;
                // The scheduler waits for the current parent's worker to
                // detach before relaunch. No fixed-delay dropped wakeups.
                self.wake_tx.send(id).map_err(|_|"engine wake scheduler unavailable".to_string())?;
            }
            ancestor=parent.parent_run_id;
        }
        Ok(())
    }

    /// Native delegated work: separate durable transcript and workspace.
    /// Budget reservations across siblings prevent oversubscription.
    pub async fn spawn_subagent(self:&Arc<Self>,parent:Uuid,input:CreateRun)->Result<Run,String>{
        self.spawn_subagent_with_policy(parent,input,false).await
    }
    pub async fn spawn_subagent_with_policy(self:&Arc<Self>,parent:Uuid,input:CreateRun,read_only:bool)->Result<Run,String>{
        let _guard=self.delegate_lock.lock().await;
        let root=self.store.get(parent).await.ok_or("parent execution not found")?;
        if root.terminal(){return Err("parent already terminated".into());}
        let children=self.store.children_of(parent).await;
        if children.len()>=4{return Err("maximum of four direct subagents per run".into());}
        let remaining=(policy::effective_budget_krw(&root)-root.cost_krw).max(0.0);
        let budget=input.budget_krw.unwrap_or(remaining.min(2000.0));
        if !budget.is_finite()||budget<0.0||budget>remaining{
            return Err(format!("subagent budget {budget} KRW exceeds unreserved {remaining} KRW"));
        }
        let mut child=Run::new(CreateRun{
            instruction:input.instruction,
            budget_krw:Some(budget),
            max_steps:input.max_steps
        },self.settings.max_steps)?;
        child.parent_run_id=Some(parent);
        child.read_only=read_only||root.read_only;
        if let Some(repository)=root.repository.as_deref(){
            let commit=crate::workspace::attach_at(
                &self.settings,child.id,repository,root.base_commit.as_deref()
            ).await?;
            child.repository=Some(repository.to_string());
            child.base_commit=Some(commit);
        }
        // Reserve first under the delegation lock. The parent model loop checks
        // this same durable field before and after every charged model request.
        self.store.mutate(parent,|run|{
            if run.terminal(){return Err("parent already terminated".into());}
            if budget>policy::effective_budget_krw(run)-run.cost_krw {
                return Err("parent budget changed before child reservation".into());
            }
            run.reserved_budget_krw+=budget;
            run.event("budget_reserved",format!("Reserved {budget:.4} KRW for delegated child {}",child.id),Some(json!({"child_id":child.id,"read_only":child.read_only})));
            Ok(())
        }).await?;
        child.event("delegated",format!("Delegated by parent {parent} with {budget} KRW reserved"),None);
        if let Err(error)=self.store.put(child.clone()).await {
            self.store.mutate(parent,|run|{run.reserved_budget_krw=(run.reserved_budget_krw-budget).max(0.0);Ok(())}).await?;
            return Err(error);
        }
        // Only the independent scheduler is allowed to call launch().
        // Calling launch().await from this model-driven tool would recursively
        // embed drive() inside its own Send future (and deadlock on retries).
        // The child is durably queued before the scheduler receives its ID.
        if let Err(error)=self.wake_tx.send(child.id){
            let reason=format!("subagent scheduler unavailable: {error}");
            self.store.mutate(parent,|run|{
                run.reserved_budget_krw=(run.reserved_budget_krw-budget).max(0.0);
                Ok(())
            }).await?;
            let _=self.store.mutate(child.id,|run|{
                run.status=RunStatus::Failed;
                run.event("start_failed",reason.clone(),None);
                Ok(())
            }).await;
            return Err(reason);
        }
        self.store.get(child.id).await.ok_or("subagent missing after spawn".into())
    }

    /// A parent can observe only its own children; never return another run.
    pub async fn check_subagent(&self,parent:Uuid,child:Uuid)->Result<String,String>{
        let run=self.store.get(child).await.ok_or("subagent not found")?;
        if run.parent_run_id!=Some(parent){return Err("subagent does not belong to this parent".into());}
        Ok(json!({
            "id":run.id,
            "status":run.status,
            "answer":run.answer,
            "steps":run.steps,
            "cost_krw":run.cost_krw,
            "base_commit":run.base_commit,
            "repository":run.repository,
            "error":run.events.iter().rev().find(|e|e.kind=="error"||e.kind=="paused_error").map(|e|&e.message)
        }).to_string())
    }

    /// Integrates only a completed direct child at exactly the parent base revision.
    /// This method is invoked only by the human-approved, journaled tool path.
    pub(super) async fn merge_subagent(&self,parent:Uuid,child:Uuid)->Result<String,String>{
        let root=self.store.get(parent).await.ok_or("parent not found")?;
        let worker=self.store.get(child).await.ok_or("child not found")?;
        if worker.parent_run_id!=Some(parent){return Err("child does not belong to this parent".into());}
        if worker.status!=RunStatus::Completed{return Err("child must complete before patch integration".into());}
        if root.repository.is_none()||root.repository!=worker.repository||root.base_commit.is_none()||root.base_commit!=worker.base_commit{
            return Err("parent and child must share a recorded Git repository and base SHA".into());
        }
        if root.read_only{return Err("read-only parent cannot integrate patches".into());}
        crate::workspace::merge_child(&self.settings,parent,child,root.base_commit.as_deref().unwrap()).await
    }

    /// Read-only child artifact access scoped to the owning parent.
    pub async fn read_subagent_file(&self,parent:Uuid,child:Uuid,path:&str)->Result<String,String>{
        let run=self.store.get(child).await.ok_or("subagent not found")?;
        if run.parent_run_id!=Some(parent) {return Err("subagent does not belong to this parent".into());}
        tools::execute(&self.settings,child,&ToolCall{
            id:"parent-inspection".into(),name:"read_file".into(),
            arguments:json!({"path":path})
        }).await
    }
}
