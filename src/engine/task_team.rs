//! Durable Task Team orchestrator. This is a coordinator over the SINGLE
//! native Engine/Harness, never another model/tool executor.
use super::*;
use crate::{domain::team::{TaskSpec,TeamGraph,TeamView,TeamRole,TeamTaskState},
    sdk::RunOptions};
use std::collections::HashMap;

impl Engine{
    /// Coordinator is a paused native Run with a versioned, owner-authored DAG.
    /// All child budget ceilings are reserved BEFORE workers can be created.
    /// This Run can never be launched as an AI/LLM agent.
    pub async fn create_task_team(self:&Arc<Self>,instruction:String,
        tasks:Vec<TaskSpec>,budget_krw:f64,options:RunOptions)->Result<Run,String>{
        if options.model_only{return Err("Task Team cannot be model_only".into());}
        let graph=TeamGraph::new(tasks,budget_krw)?;
        if !options.provider_id.is_empty()||!options.model.is_empty(){
            self.model_for_selection(&options.provider_id,&options.model)?;
        }
        let _lease=self.delegate_lock.lock().await;
        let mut root=Run::new(CreateRun{
            instruction,budget_krw:Some(budget_krw),max_steps:Some(1)
        },1)?;
        root.status=RunStatus::Paused;
        root.provider_id=options.provider_id;
        root.model_name=options.model;
        root.strategy_learning=options.strategy_learning;
        root.reserved_budget_krw=graph.tasks.iter().map(|t|t.spec.budget_krw).sum();
        root.event("team_created",
            "Owner-authored DAG persisted; team coordinator cannot execute tools",
            Some(json!({"schema_version":graph.schema_version,
                "task_count":graph.tasks.len(),"reservation_krw":root.reserved_budget_krw})));
        root.team=Some(graph);
        self.store.put(root.clone()).await?;
        Ok(root)
    }
    /// Snapshot is a projection of the root's immutable graph and actual
    /// child Run states; no model-authored progress can forge completion.
    pub async fn task_team(&self,id:Uuid)->Result<TeamView,String>{
        let root=self.store.get(id).await.ok_or("team coordinator not found")?;
        let graph=root.team.as_ref().ok_or("execution is not a Task Team")?;
        if graph.schema_version!=1{return Err("unsupported Task Team schema version".into());}
        let mut states=HashMap::new();
        for task in &graph.tasks{
            if let Some(child)=self.store.get(task.run_id).await{
                if child.parent_run_id!=Some(id)||child.team.is_some(){
                    return Err("team child Run lineage mismatch".into());
                }
                states.insert(child.id,child.status);
            }
        }
        Ok(graph.view(id,&states))
    }
    /// Idempotent tick: schedule only prepared dependency-ready workers.
    /// Repeated ticks/restarts use each task's preassigned Run UUID, never a
    /// new UUID for the same logical task. Uncertain effects stay paused.
    pub async fn advance_task_team(self:&Arc<Self>,id:Uuid)->Result<TeamView,String>{
        let _lock=self.delegate_lock.lock().await;
        let root=self.store.get(id).await.ok_or("team coordinator not found")?;
        let graph=root.team.as_ref().ok_or("execution is not a Task Team")?;
        if graph.schema_version!=1{return Err("unsupported Task Team schema version".into());}
        if root.status==RunStatus::Cancelled{return self.task_team(id).await;}
        if root.status==RunStatus::Completed||root.status==RunStatus::Failed{
            return self.task_team(id).await;
        }
        if root.status!=RunStatus::Paused{
            return Err("team coordinator must remain paused until its tasks settle".into());
        }
        // A committed start marker differentiates an owner-created draft from
        // an already-accepted long-running team after process restart.
        if !root.events.iter().any(|event|event.kind=="team_started"){
            self.store.mutate(id,|run|{
                if run.status!=RunStatus::Paused{
                    return Err("team stopped before its first schedule".into());
                }
                run.event("team_started","Owner started durable Task Team execution",None);
                Ok(())
            }).await?;
        }
        let mut existing=HashMap::new();
        for task in &graph.tasks{
            if let Some(child)=self.store.get(task.run_id).await{
                if child.parent_run_id!=Some(id)||child.team.is_some(){
                    return Err("team child Run lineage mismatch".into());
                }
                existing.insert(child.id,child.status);
            }
        }
        let preview=graph.view(id,&existing);
        let mut slots=4usize.saturating_sub(preview.tasks.iter().filter(|t|
            matches!(t.status,TeamTaskState::Active|TeamTaskState::ApprovalRequired)
        ).count());
        // Restore a queued worker that was prepared but not launched before
        // a crash. Never auto-resume Paused or AwaitingApproval workers.
        for task in &graph.tasks{
            if self.store.get(task.run_id).await.is_some_and(|r|r.status==RunStatus::Queued){
                self.launch(task.run_id).await?;
            }
        }
        // The graph already claims writer scopes in stable owner order.
        for ticket in &preview.tasks{
            if slots==0{break;}
            if ticket.status!=TeamTaskState::Ready{continue;}
            let task=graph.tasks.iter().find(|t|t.run_id==ticket.run_id)
                .ok_or("work ticket lost its canonical graph node")?;
            // Re-check: a previous process may have committed the child Run
            // after our initial snapshot. Never overwrite its durable journal.
            if self.store.get(task.run_id).await.is_some(){continue;}
            let mut child=Run::new(CreateRun{
                instruction:task.spec.instruction.clone(),
                budget_krw:Some(task.spec.budget_krw),
                max_steps:Some(task.spec.max_steps)
            },self.settings.max_steps)?;
            child.id=task.run_id;
            child.parent_run_id=Some(id);
            child.provider_id=root.provider_id.clone();
            child.model_name=root.model_name.clone();
            child.strategy_learning=root.strategy_learning;
            child.read_only=task.spec.role.read_only();
            child.delegation_profile=task.spec.role.profile().into();
            child.event("team_assigned",
                "Task Team worker assigned by owner-authored DAG",
                Some(json!({"team_id":id,"task_id":task.spec.id,
                    "workspace_scope":task.spec.workspace,
                    "role":task.spec.role.profile(),"read_only":child.read_only,
                    "depends_on":task.spec.depends_on})));
            self.store.put(child).await?;
            self.launch(task.run_id).await?;
            slots-=1;
        }
        let view=self.task_team(id).await?;
        // No premature success: child durable completion and exactly-once
        // parent budget settlement both must be committed.
        if view.all_complete && root.settled_child_ids.len()==graph.tasks.len(){
            self.store.mutate(id,|r|{
                if r.status==RunStatus::Paused{
                    r.status=RunStatus::Completed;
                    r.event("team_completed","All worker Runs and budget receipts settled",None);
                }
                Ok(())
            }).await?;
        }else if view.failed && view.tasks.iter().all(|t|matches!(t.status,
            TeamTaskState::Completed|TeamTaskState::Failed|
            TeamTaskState::Cancelled|TeamTaskState::Blocked)){
            // An unresolved dependency cannot be silently treated as success.
            self.store.mutate(id,|r|{
                if r.status==RunStatus::Paused{
                    r.status=RunStatus::Failed;
                    r.event("team_failed","A failed worker blocked the team DAG",None);
                }
                Ok(())
            }).await?;
        }
        Ok(view)
    }
    /// Cancel the root AND every live child. In-flight changes remain paused
    /// for reconciliation rather than silently losing tool receipts.
    pub async fn cancel_task_team(&self,id:Uuid)->Result<Run,String>{
        let _lock=self.delegate_lock.lock().await;
        let root=self.store.get(id).await.ok_or("team not found")?;
        let graph=root.team.as_ref().ok_or("execution is not a Task Team")?;
        let child_ids=graph.tasks.iter().map(|t|t.run_id).collect::<Vec<_>>();
        let parent=self.store.mutate(id,|r|{
            if r.terminal(){return Err("team already terminal".into());}
            r.status=RunStatus::Cancelled;
            r.event("team_cancelled","Owner cancelled the Task Team",None);
            Ok(())
        }).await?;
        for child_id in child_ids{
            let Some(child)=self.store.get(child_id).await else{continue};
            if child.parent_run_id!=Some(id){continue;}
            if child.terminal(){continue;}
            self.store.mutate(child_id,|r|{
                if r.terminal(){return Ok(());}
                if r.in_flight.is_some()||!r.read_batch_in_flight.is_empty(){
                    r.status=RunStatus::Paused;r.cancel_after_tool=true;
                    r.event("cancel_pending_reconciliation",
                        "Team cancellation after an in-flight tool must be reconciled",None);
                }else{
                    r.status=RunStatus::Cancelled;
                    r.event("cancelled","Team worker cancelled by owner",None);
                }
                Ok(())
            }).await?;
            if let Some(sender)=self.cancellation.lock().await.get(&child_id){
                sender.send_replace(true);
            }
        }
        Ok(parent)
    }
}
