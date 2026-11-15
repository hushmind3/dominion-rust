//! Primary native Rust SDK. HTTP/TypeScript/Studio are optional adapters.
use crate::{hooks::ToolHook,api,config::Settings,domain::{CreateRun,ReconcileTool,Run,RunStatus,TimelineEvent,ToolCall},engine::Engine,group::{Groups,NewRoom,Room,Say},improvement::{Improvement,Improvements,NewImprovement},store::Store,tools,video::{Video,Videos,NewVideo}};
use axum::Router;
use serde_json::{json,Value};
use std::{sync::Arc,time::{Duration,Instant}};
use uuid::Uuid;

#[derive(Clone)]
pub struct Dominion { engine: Arc<Engine> }

impl Dominion {
    pub async fn open(settings: Settings) -> Result<Self,String> {
        tokio::fs::create_dir_all(&settings.workspace_dir).await.map_err(|e|e.to_string())?;
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await?);
        let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await?);
        let videos=Arc::new(Videos::open(settings.data_dir.join("videos")).await?);
        let improvements=Arc::new(Improvements::open(settings.data_dir.join("improvements"),settings.workspace_dir.clone()).await?);
        let engine=Engine::new(settings,store,groups,videos,improvements)?;
        // Recover orphaned RSI controller states from the independent durable
        // evaluator before any new campaign may start. Never silently adopt.
        crate::frontend_rsi::reconcile_recovered_campaigns(&engine).await?;
        // Reconcile durable parent/child settlement after a previous process
        // died between a child completion and the parent wake notification.
        let snapshots=engine.store.list().await;
        let suspended=snapshots.iter().filter(|r|r.waiting_for_children).map(|r|r.id).collect::<Vec<_>>();
        for id in suspended {engine.settle_and_wake_ancestors(id).await?;}
        // Restart only work for which recovery proved there was no unfinished
        // mutating side effect. A previously approved call that never reached
        // the execution-intent checkpoint is also safe to start.
        for snapshot in snapshots {
            if snapshot.status==RunStatus::Paused && snapshot.in_flight.is_none()
                && !snapshot.waiting_for_children
                && snapshot.events.last().is_some_and(|e|e.kind=="recovered_ready"){
                engine.launch(snapshot.id).await?;
            }
        }
        crate::frontend_automations::spawn_scheduler(&engine);
        Ok(Self{engine})
    }
    pub(crate) fn from_engine(engine:Arc<Engine>)->Self{Self{engine}}
    pub fn engine(&self)->Arc<Engine>{Arc::clone(&self.engine)}
    pub fn http_router(&self)->Router{api::router(Arc::clone(&self.engine))}
    /// Native extension point: ordered pre-dispatch veto and post-receipt policy.
    /// HTTP/TypeScript clients use the same engine with operator-owned hooks.
    pub async fn register_tool_hook(&self,hook:Arc<dyn ToolHook>){
        self.engine.register_tool_hook(hook).await;
    }
    pub async fn submit(&self,input:CreateRun)->Result<Run,String>{
        let run=Run::new(input,self.engine.settings.max_steps)?;
        self.engine.store.put(run.clone()).await?;
        self.engine.launch(run.id).await?;
        self.get(run.id).await
    }
    /// Run a coding task against an isolated Git worktree of an existing project.
    /// The original checkout remains unchanged until the user explicitly adopts edits.
    pub async fn submit_from_repository(&self,input:CreateRun,repository:&str)->Result<Run,String>{
        let mut run=Run::new(input,self.engine.settings.max_steps)?;
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
    /// Reuse the engine's budget- and lineage-enforced delegated execution.
    pub async fn delegate(&self,parent:Uuid,input:CreateRun)->Result<Run,String>{
        self.engine.spawn_subagent(parent,input).await
    }
    pub async fn delegate_read_only(&self,parent:Uuid,input:CreateRun)->Result<Run,String>{
        self.engine.spawn_subagent_with_policy(parent,input,true).await
    }
    pub async fn get(&self,id:Uuid)->Result<Run,String>{self.engine.store.get(id).await.ok_or("execution not found".into())}
    pub async fn list(&self)->Vec<Run>{self.engine.store.list().await}
    pub async fn events(&self,id:Uuid,since:u64)->Result<Vec<TimelineEvent>,String>{
        Ok(self.get(id).await?.events.into_iter().filter(|e|e.seq>since).collect())
    }
    pub async fn approve(&self,id:Uuid,allow:bool)->Result<(),String>{self.engine.approve(id,allow).await}
    pub async fn resume(&self,id:Uuid)->Result<(),String>{self.engine.launch(id).await}
    pub async fn cancel(&self,id:Uuid)->Result<Run,String>{self.engine.cancel(id).await}
    pub async fn instruct(&self,id:Uuid,text:String)->Result<Run,String>{self.engine.directive(id,text).await}
    pub async fn reconcile(&self,id:Uuid,input:ReconcileTool)->Result<Run,String>{self.engine.reconcile(id,input).await}
    /// Does not hang on awaiting user approval or paused failed calls.
    pub async fn wait(&self,id:Uuid,timeout:Duration)->Result<Run,String>{
        let started=Instant::now();
        loop{
            let run=self.get(id).await?;
            if run.terminal()||run.status==RunStatus::AwaitingApproval ||
                (run.status==RunStatus::Paused && !run.waiting_for_children){return Ok(run);}
            if started.elapsed()>=timeout{return Err("execution wait timeout".into());}
            tokio::time::sleep(Duration::from_millis(100)).await;
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
    /// Main frontend and native SDK share the same durable profile/session
    /// records. These entries are presentation data; runs still execute only
    /// through the independent Engine and Store.
    pub async fn chat_sessions(&self)->Vec<crate::frontend_state::UiSession>{
        self.engine.frontend.sessions().await
    }
    pub async fn chat_session(&self,id:&str)->Option<crate::frontend_state::UiSession>{
        self.engine.frontend.session(id).await
    }
    pub async fn agent_profiles(&self)->Vec<crate::frontend_state::UiAgent>{
        self.engine.frontend.agents().await
    }
    pub async fn owner_profile(&self)->crate::profile::OwnerProfile{
        self.engine.owner_profile.get().await
    }
    pub async fn save_owner_profile(&self,req:crate::profile::NewOwnerProfile)
        ->Result<crate::profile::OwnerProfile,String>{
        self.engine.owner_profile.save(req).await
    }
    pub async fn create_group(&self,req:NewRoom)->Result<Room,String>{self.engine.groups.create(req).await}
    pub async fn say_group(&self,id:Uuid,input:Say)->Result<Room,String>{self.engine.groups.say(id,input.content).await}
    pub async fn read_subagent_file(&self,parent:Uuid,child:Uuid,path:&str)->Result<String,String>{
        self.engine.read_subagent_file(parent,child,path).await
    }
    pub async fn group_turn(&self,id:Uuid)->Result<Room,String>{self.engine.groups.next(&self.engine.model,id).await}
    pub async fn create_video(&self,req:NewVideo)->Result<Video,String>{self.engine.videos.create(&self.engine.model,req).await}
    pub async fn render_video(&self,id:Uuid)->Result<Video,String>{self.engine.videos.start(id).await}
    /// Evidence-ranked lessons from adopted RSI work and verified agent runs.
    /// Never combines private repositories or treats unadopted experiments as successes.
    pub async fn search_strategies(&self,repository:&str,goal:&str,max:usize)
        ->Vec<crate::strategy::StrategyEvidence>{
        self.engine.improvements.strategy_evidence(repository,goal,max).await
    }
    pub async fn create_improvement(&self,req:NewImprovement)->Result<Improvement,String>{self.engine.improvements.create(req).await}
    pub async fn evaluate_improvement(&self,id:Uuid)->Result<Improvement,String>{
        self.engine.improvements.start(self.engine.model.clone(),id,self.engine.settings.shell_enabled).await
    }
    pub async fn adopt_improvement(&self,id:Uuid)->Result<Improvement,String>{self.engine.improvements.adopt(id).await}
    pub async fn descendant_improvement(&self,id:Uuid)->Result<Improvement,String>{self.engine.improvements.descendant(id).await}
    pub async fn list_skills(&self,run:Uuid)->Result<Vec<crate::skills::SkillInfo>,String>{
        self.get(run).await?;
        crate::skills::list(&self.engine.settings,run).await
    }
    pub async fn load_skill(&self,run:Uuid,name:&str)->Result<String,String>{
        self.get(run).await?;
        crate::skills::load(&self.engine.settings,run,name).await
    }
    pub async fn remember(&self,scope:&str,title:&str,content:&str)->Result<crate::knowledge::MemoryEntry,String>{
        self.engine.knowledge.remember(scope,title,content).await
    }
    pub async fn search_memory(&self,scope:&str,query:&str)->Vec<crate::knowledge::MemoryEntry>{
        self.engine.knowledge.search(scope,query,20).await
    }
    pub async fn list_memory(&self,scope:&str)->Vec<crate::knowledge::MemoryEntry>{
        self.engine.knowledge.list(scope).await
    }
    pub async fn memory_scopes(&self)->Vec<String>{
        self.engine.knowledge.scopes().await
    }
    pub async fn revise_memory(&self,scope:&str,id:Uuid,title:&str,content:&str)
        ->Result<crate::knowledge::MemoryEntry,String>{
        self.engine.knowledge.revise(scope,id,title,content).await
    }
    pub async fn forget_memory(&self,scope:&str,id:Uuid)->Result<(),String>{
        self.engine.knowledge.forget(scope,id).await
    }
    pub async fn tool_catalog(&self)->Vec<Value>{
        let mut tools=tools::definitions(self.engine.settings.shell_enabled);
        tools.extend(self.engine.mcp.definitions().await);
        // The SDK and model must see the same operator-owned tool mask.
        tools.retain(|tool|{
            let name=tool.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
            !self.engine.settings.denied_tools.iter().any(|blocked|blocked==name)
        });
        tools
    }
}
#[cfg(test)]
mod tests {
    use super::*;


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
        tokio::fs::remove_dir_all(root).await.unwrap();
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
        tokio::fs::remove_dir_all(dir).await.unwrap();
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
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
