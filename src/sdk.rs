//! Primary native Rust SDK. HTTP/TypeScript/Studio are optional adapters.
use crate::{domain::team::{TaskSpec,TeamView},hooks::ToolHook,config::Settings,domain::{CreateRun,ReconcileTool,Run,RunStatus,TimelineEvent,ToolCall},engine::Engine,tools};
use serde_json::{json,Value};
use std::{sync::Arc,time::{Duration,Instant}};
use uuid::Uuid;

// Optional GUI, room, catalog, memory and skill methods are not part of
// the instruction/Run SDK implementation.
mod product;

/// Portable launch options owned by the Rust SDK instead of individual HTTP adapters.
#[derive(Clone,Debug,Default)]
pub struct RunOptions {
    pub provider_id:String,
    pub model:String,
    pub strategy_learning:bool,
    pub model_only:bool,
}
#[derive(Clone)]
pub struct Dominion { engine: Arc<Engine> }

impl Dominion {
    pub(crate) fn from_engine(engine:Arc<Engine>)->Self{Self{engine}}
    pub fn engine(&self)->Arc<Engine>{Arc::clone(&self.engine)}
    /// Native extension point: ordered pre-dispatch veto and post-receipt policy.
    /// HTTP/TypeScript clients use the same engine with operator-owned hooks.
    pub async fn register_tool_hook(&self,hook:Arc<dyn ToolHook>){
        self.engine.register_tool_hook(hook).await;
    }
    /// Persist a configured execution before binding a chat session or invoking a
    /// model. No external effect occurs until the caller explicitly starts it.
    /// The Run identity, budget, provider and strategy choice survive process restart.
    fn configured_run(&self,input:CreateRun,options:RunOptions)->Result<Run,String>{
        let mut run=Run::new(input,self.engine.settings.max_steps)?;
        if !options.provider_id.is_empty()||!options.model.is_empty(){
            self.engine.model_for_selection(&options.provider_id,&options.model)?;
            run.provider_id=options.provider_id;
            run.model_name=options.model;
            let unknown=self.engine.pricing_unknown(
                &self.engine.settings,&run.provider_id,&run.model_name);
            if unknown{
                run.event("pricing_unknown",
                    "Supplier token tariffs are not configured; zero recorded KRW is not free inference",None);
            }
        }
        run.strategy_learning=options.strategy_learning;
        run.model_only=options.model_only;
        Ok(run)
    }
    pub async fn prepare_run(&self,input:CreateRun,options:RunOptions)->Result<Run,String>{
        let run=self.configured_run(input,options)?;
        self.engine.store.put(run.clone()).await?;
        Ok(run)
    }
    /// The same SDK launch path is used for direct SDK, browser jobs and
    /// frontend "SDK Run" jobs. Model-only turns keep their own provider path.
    pub async fn submit_selected(&self,input:CreateRun,options:RunOptions)->Result<Run,String>{
        if options.model_only{return Err("model-only turns require chat session binding".into());}
        let run=self.prepare_run(input,options).await?;
        self.engine.launch(run.id).await?;
        self.get(run.id).await
    }
    pub async fn submit(&self,input:CreateRun)->Result<Run,String>{
        self.submit_selected(input,RunOptions::default()).await
    }
    /// Run a coding task against an isolated Git worktree of an existing project.
    /// The original checkout remains unchanged until the user explicitly adopts edits.
    pub async fn submit_from_repository(&self,input:CreateRun,repository:&str)->Result<Run,String>{
        self.submit_from_repository_with_checks(input,repository,vec![]).await
    }
    /// Trusted caller-specified changed paths are immutable owner acceptance
    /// criteria, stored before the model is launched. No tool/model may alter
    /// the required paths mid-run.
    pub async fn submit_from_repository_with_checks(&self,input:CreateRun,repository:&str,
        required_changed_paths:Vec<String>)->Result<Run,String>{
        crate::acceptance::validate_paths(&required_changed_paths)?;
        let mut run=Run::new(input,self.engine.settings.max_steps)?;
        run.required_changed_paths=required_changed_paths;
        let repo=repository.trim();
        let commit=crate::workspace::attach(&self.engine.settings,run.id,repo).await?;
        run.repository=Some(repo.to_string());
        run.base_commit=Some(commit.clone());
        run.event("repository_attached",format!("Isolated checkout at commit {commit}"),None);
        self.engine.store.put(run.clone()).await?;
        self.engine.launch(run.id).await?;
        self.get(run.id).await
    }
    /// Starts independent workers without waiting for prior run completion.
    pub async fn submit_parallel(&self,inputs:Vec<CreateRun>)->Result<Vec<Run>,String>{
        if inputs.is_empty()||inputs.len()>8{return Err("requires 1..8 independent requests".into());}
        let mut out=Vec::with_capacity(inputs.len());
        for input in inputs{out.push(self.submit(input).await?);}
        Ok(out)
    }
    /// Create a durable owner-controlled task DAG without launching a model.
    /// Each Task has a stable native Run ID and a reserved local budget.
    pub async fn create_task_team(&self,instruction:String,tasks:Vec<TaskSpec>,
        budget_krw:f64,options:RunOptions)->Result<Run,String>{
        self.engine.create_task_team(instruction,tasks,budget_krw,options).await
    }
    /// Advance only dependency-ready tasks through the existing native Harness.
    /// Never retries uncertain or approval-paused worker effects implicitly.
    pub async fn advance_task_team(&self,root:Uuid)->Result<TeamView,String>{
        self.engine.advance_task_team(root).await
    }
    pub async fn task_team(&self,root:Uuid)->Result<TeamView,String>{
        self.engine.task_team(root).await
    }
    pub async fn cancel_task_team(&self,root:Uuid)->Result<Run,String>{
        self.engine.cancel_task_team(root).await
    }
    /// Reuse the engine's budget- and lineage-enforced delegated execution.
    pub async fn delegate(&self,parent:Uuid,input:CreateRun)->Result<Run,String>{
        self.engine.spawn_subagent(parent,input).await
    }
    /// Optional task-specific child tool scope; defaults remain compatible
    /// with preexisting general-purpose delegation.
    pub async fn delegate_profile(&self,parent:Uuid,input:CreateRun,profile:&str)->Result<Run,String>{
        self.engine.spawn_subagent_with_profile(parent,input,profile).await
    }
    pub async fn delegate_read_only(&self,parent:Uuid,input:CreateRun)->Result<Run,String>{
        self.engine.spawn_subagent_with_policy(parent,input,true).await
    }
    pub async fn get(&self,id:Uuid)->Result<Run,String>{self.engine.store.get(id).await.ok_or("execution not found".into())}
    pub async fn list(&self)->Vec<Run>{self.engine.store.list().await}
    /// Chronological strategy→tool receipt associations for controlled
    /// offline evaluation. Does not infer causal benefit or launch a model.
    pub async fn repair_observations(&self,id:Uuid)->Result<Value,String>{
        let run=self.get(id).await?;
        Ok(crate::repair_guidance::observations(&run))
    }
    pub async fn events(&self,id:Uuid,since:u64)->Result<Vec<TimelineEvent>,String>{
        Ok(self.get(id).await?.events.into_iter().filter(|e|e.seq>since).collect())
    }
    pub async fn approve(&self,id:Uuid,allow:bool)->Result<(),String>{self.engine.approve(id,allow).await}
    pub async fn resume(&self,id:Uuid)->Result<(),String>{
        if self.get(id).await?.model_only{
            return Err("model-only chat cannot be resumed without its bound chat session".into());
        }
        self.engine.launch(id).await
    }
    pub async fn cancel(&self,id:Uuid)->Result<Run,String>{self.engine.cancel(id).await}
    pub async fn instruct(&self,id:Uuid,text:String)->Result<Run,String>{self.engine.directive(id,text).await}
    pub async fn reconcile(&self,id:Uuid,input:ReconcileTool)->Result<Run,String>{self.engine.reconcile(id,input).await}
    /// Wait for a persisted state transition without polling full JSON Runs.
    /// Subscribe before reading to avoid missing the transition race.
    pub async fn wait(&self,id:Uuid,timeout:Duration)->Result<Run,String>{
        let started=Instant::now();
        let mut updates=self.engine.store.subscribe();
        loop{
            let run=self.get(id).await?;
            if run.terminal()||run.status==RunStatus::AwaitingApproval ||
                (run.status==RunStatus::Paused && !run.waiting_for_children){return Ok(run);}
            let remaining=timeout.checked_sub(started.elapsed())
                .ok_or("execution wait timeout")?;
            match tokio::time::timeout(remaining,updates.recv()).await{
                Ok(Ok(event)) if event.id==id=>{},
                Ok(Ok(_))=>{
                    // Other runs may produce thousands of events. Do not
                    // clone this run's transcript for unrelated updates.
                    loop {
                        let left=timeout.checked_sub(started.elapsed())
                            .ok_or("execution wait timeout")?;
                        match tokio::time::timeout(left,updates.recv()).await{
                            Ok(Ok(event)) if event.id==id=>break,
                            Ok(Ok(_))=>{},
                            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_)))=>break,
                            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed))=>
                                return Err("execution update channel closed".into()),
                            Err(_)=>return Err("execution wait timeout".into()),
                        }
                    }
                },
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_)))=>{},
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed))=>
                    return Err("execution update channel closed".into()),
                Err(_)=>return Err("execution wait timeout".into()),
            }
        }
    }
    pub async fn artifacts(&self,id:Uuid)->Result<Vec<String>,String>{
        self.get(id).await?;
        let out=tools::execute(&self.engine.settings,id,&ToolCall{id:"sdk-list".into(),name:"list_files".into(),arguments:json!({"path":""})}).await?;
        serde_json::from_str(&out).map_err(|e|e.to_string())
    }
    pub async fn read_artifact(&self,id:Uuid,path:&str)->Result<String,String>{
        self.get(id).await?;
        tools::execute(&self.engine.settings,id,&ToolCall{id:"sdk-read".into(),name:"read_file".into(),arguments:json!({"path":path})}).await
    }

}
#[cfg(test)]
mod tests {
    use super::*;
    async fn cleanup_test_dir(path:std::path::PathBuf){
        for attempt in 0..20 {
            match tokio::fs::remove_dir_all(&path).await{
                Ok(())=>return,
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>return,
                // Linux/macOS can return ENOTEMPTY when an async worker is
                // finishing its final fsync/rename during directory teardown.
                // This is a bounded test cleanup retry, not a runtime ignore.
                Err(error) if attempt<19 && (
                    error.kind()==std::io::ErrorKind::DirectoryNotEmpty
                    ||error.raw_os_error()==Some(39) // Linux ENOTEMPTY
                    ||error.raw_os_error()==Some(66) // macOS ENOTEMPTY
                    ||error.raw_os_error()==Some(32) // Windows sharing violation
                )=>{
                    tokio::time::sleep(Duration::from_millis(50)).await;
                },
                Err(error)=>panic!("test directory cleanup failed: {error}")
            }
        }
    }





    #[tokio::test]
    async fn historical_experiment_receipts_never_restart_or_resume(){
        let root=std::env::temp_dir().join(format!("dominion-old-experiment-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");settings.workspace_dir=root.join("workspace");
        let sdk=Dominion::open(settings.clone()).await.unwrap();
        let mut ids=Vec::new();
        for kind in ["strategy_candidate","strategy_trials_launched","strategy_trials_partial"]{
            let mut run=Run::new(CreateRun{instruction:"Archived experiment".into(),
                budget_krw:Some(30.0),max_steps:Some(4)},4).unwrap();
            run.status=RunStatus::Paused;
            run.event(kind,"Archived run marker",None);
            // Simulate a previous process recovering an apparently safe turn.
            run.event("recovered_ready","Recovered state",None);
            ids.push(run.id);
            sdk.engine.store.put(run).await.unwrap();
        }
        drop(sdk);
        let reopened=Dominion::open(settings).await.unwrap();
        for id in ids{
            let run=reopened.get(id).await.unwrap();
            assert_eq!(run.status,RunStatus::Paused);
            assert!(run.events.iter().any(|event|event.kind=="archived_run_quarantined"));
            let error=reopened.resume(id).await.unwrap_err();
            assert!(error.contains("retired experiment run cannot be resumed"));
            assert_eq!(reopened.get(id).await.unwrap().status,RunStatus::Paused);
        }
        drop(reopened);cleanup_test_dir(root).await;
    }
    #[tokio::test]
    async fn retired_reserved_runs_cannot_be_reactivated_after_restart(){
        let root=std::env::temp_dir().join(format!("dominion-retired-reservation-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        let sdk=Dominion::open(settings.clone()).await.unwrap();
        let mut reserved_ids=Vec::new();
        for has_old_receipt in [false,true] {
            let mut run=Run::new(CreateRun{
                instruction:"Historical inactive reservation".into(),
                budget_krw:Some(25.0),max_steps:Some(5),
            },sdk.engine.settings.max_steps).unwrap();
            run.status=RunStatus::Paused;
            run.event("reserved_unlaunched","Historical inactive reservation",None);
            if has_old_receipt {
                run.event("domain_receipt_committed",
                    "Historical approval no longer has an owning module",
                    Some(json!({"domain":"retired","receipt":"old-record"})));
            }
            reserved_ids.push(run.id);
            sdk.engine.store.put(run).await.unwrap();
        }
        drop(sdk);
        let reopened=Dominion::open(settings).await.unwrap();
        for id in reserved_ids {
            let restored=reopened.get(id).await.unwrap();
            assert_eq!(restored.status,RunStatus::Paused);
            assert!(!restored.events.iter().any(|event|event.kind=="recovered_ready"));
            let error=reopened.resume(id).await.unwrap_err();
            assert!(error.contains("retired reserved run cannot be resumed"));
            assert_eq!(reopened.get(id).await.unwrap().status,RunStatus::Paused);
        }
        drop(reopened);
        cleanup_test_dir(root).await;
    }
    #[tokio::test]
    async fn configured_sdk_preparation_freezes_model_budget_and_tool_policy(){
        let root=std::env::temp_dir().join(format!("dominion-sdk-configured-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        let model=settings.main.name.clone();
        let sdk=Dominion::open(settings.clone()).await.unwrap();
        let run=sdk.prepare_run(CreateRun{
            instruction:"Return a concise source summary".into(),
            budget_krw:Some(37.5),max_steps:Some(3)
        },RunOptions{
            provider_id:"dominion-native".into(),model:model.clone(),
            strategy_learning:true,model_only:true
        }).await.unwrap();
        assert_eq!(run.status,RunStatus::Queued);
        assert_eq!(run.provider_id,"dominion-native");
        assert_eq!(run.model_name,model);
        assert_eq!(run.budget_krw,37.5);
        assert_eq!(run.max_steps,3);
        assert!(run.strategy_learning);
        assert!(run.model_only);
        // A prepared model-only chat may never be executed as an agent.
        assert!(sdk.resume(run.id).await.is_err());
        let reopened=Dominion::open(settings).await.unwrap();
        let restored=reopened.get(run.id).await.unwrap();
        assert!(restored.model_only);
        assert!(restored.strategy_learning);
        assert_eq!(restored.model_name,run.model_name);
        // Fail-closed restart: even an unlaunched model-only turn cannot be
        // silently replayed, as the durable record cannot prove no provider
        // request escaped before process termination.
        assert_eq!(restored.status,RunStatus::Failed);
        assert!(restored.events.iter().any(|e|e.kind=="model_only_interrupted"));
        drop(reopened);drop(sdk);
        cleanup_test_dir(root).await;
    }

    #[tokio::test]
    async fn sdk_wait_returns_on_durable_approval_event_without_polling(){
        let root=std::env::temp_dir().join(format!("dominion-sdk-wait-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        let sdk=Dominion::open(settings).await.unwrap();
        let run=sdk.prepare_run(CreateRun{
            instruction:"Wait for human approval".into(),budget_krw:None,max_steps:Some(2)
        },RunOptions::default()).await.unwrap();
        let id=run.id;
        let waiter=sdk.clone();
        let task=tokio::spawn(async move{
            waiter.wait(id,Duration::from_secs(3)).await.unwrap()
        });
        tokio::task::yield_now().await;
        sdk.engine.store.mutate(id,|state|{
            state.status=RunStatus::AwaitingApproval;
            state.pending_approval=Some(ToolCall{
                id:"approval-1".into(),name:"write_file".into(),
                arguments:json!({"path":"demo","content":"example"})
            });
            state.event("approval_required","Human consent required",None);
            Ok(())
        }).await.unwrap();
        let result=tokio::time::timeout(Duration::from_secs(3),task)
            .await.unwrap().unwrap();
        assert_eq!(result.status,RunStatus::AwaitingApproval);
        assert_eq!(result.pending_approval.unwrap().id,"approval-1");
        drop(sdk);
        cleanup_test_dir(root).await;
    }

    #[tokio::test]
    async fn sdk_catalog_does_not_advertise_operator_denied_tools(){
        let root=std::env::temp_dir().join(format!("dominion-sdk-mask-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        settings.denied_tools=vec!["write_file".into(),"read_file".into()];
        let sdk=Dominion::open(settings).await.unwrap();
        let catalog=sdk.tool_catalog().await;
        let names=catalog.iter().filter_map(|v|v.pointer("/function/name")
            .and_then(Value::as_str)).collect::<Vec<_>>();
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"read_file"));
        assert!(names.contains(&"list_files"));
        drop(sdk);
        cleanup_test_dir(root).await;
    }

    #[tokio::test]
    async fn approved_unstarted_write_replays_once_after_sdk_restart(){
        let dir=std::env::temp_dir().join(format!("dominion-approved-restart-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        let sdk=Dominion::open(settings.clone()).await.unwrap();
        let mut run=Run::new(CreateRun{
            instruction:"write approved artifact".into(),budget_krw:Some(10.0),max_steps:Some(1)
        },1).unwrap();
        let id=run.id;
        let call=ToolCall{id:"approved-1".into(),name:"write_file".into(),
            arguments:json!({"path":"recovered.txt","content":"written after restart"})};
        run.status=RunStatus::AwaitingApproval;
        run.plan=Some("Approved file creation".into());
        run.steps=1; // Never invoke an external model after this tool.
        run.pending_approval=Some(call.clone());
        run.approval_decision=Some(true);
        run.messages.push(json!({"role":"assistant","content":null,"tool_calls":[{
            "id":call.id,"type":"function","function":{
                "name":call.name,"arguments":call.arguments.to_string()
            }
        }]}));
        sdk.engine.store.put(run).await.unwrap();
        drop(sdk);
        // An approved pending call has no execution intent; it can be resumed
        // automatically without treating it as an already-executed side effect.
        let reopened=Dominion::open(settings.clone()).await.unwrap();
        let mut observed=None;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let run=reopened.get(id).await.unwrap();
            if run.terminal(){observed=Some(run);break;}
        }
        let state=observed.expect("approved tool did not complete after restart");
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),1);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_result").count(),1);
        assert!(state.in_flight.is_none());
        assert_eq!(tokio::fs::read_to_string(settings.workspace_dir.join(id.to_string())
            .join("recovered.txt")).await.unwrap(),"written after restart");
        drop(reopened);
        cleanup_test_dir(dir).await;
    }

    #[tokio::test]
    async fn sdk_and_http_share_persisted_core(){
        let dir=std::env::temp_dir().join(format!("dominion-sdk-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        let sdk=Dominion::open(settings.clone()).await.unwrap();
        let run=Run::new(CreateRun{instruction:"test".into(),budget_krw:Some(0.0),max_steps:Some(2)},2).unwrap();
        sdk.engine.store.put(run.clone()).await.unwrap();
        let _router=sdk.http_router();
        assert_eq!(sdk.list().await.len(),1);
        assert_eq!(sdk.get(run.id).await.unwrap().id,run.id);
        assert_eq!(sdk.events(run.id,0).await.unwrap().len(),1);
        assert!(sdk.artifacts(run.id).await.unwrap().is_empty());
        let reopened=Dominion::open(settings).await.unwrap();
        assert_eq!(reopened.get(run.id).await.unwrap().id,run.id);
        drop(_router);drop(reopened);drop(sdk);
        cleanup_test_dir(dir).await;
    }
}
