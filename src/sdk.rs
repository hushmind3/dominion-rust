//! Primary native Rust SDK. HTTP/TypeScript/Studio are optional adapters.
use crate::{api,config::Settings,domain::{CreateRun,ReconcileTool,Run,RunStatus,TimelineEvent,ToolCall},engine::Engine,group::{Groups,NewRoom,Room,Say},improvement::{Improvement,Improvements,NewImprovement},store::Store,tools,video::{Video,Videos,NewVideo}};
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
        Ok(Self{engine})
    }
    pub fn engine(&self)->Arc<Engine>{Arc::clone(&self.engine)}
    pub fn http_router(&self)->Router{api::router(Arc::clone(&self.engine))}
    pub async fn submit(&self,input:CreateRun)->Result<Run,String>{
        let run=Run::new(input,self.engine.settings.max_steps)?;
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
    /// A child run has explicit lineage, an independent workspace and cost limit.
    pub async fn delegate(&self,parent:Uuid,mut input:CreateRun)->Result<Run,String>{
        let p=self.get(parent).await?;
        if p.terminal(){return Err("parent already finished".into());}
        let allowance=(p.budget_krw-p.cost_krw).max(0.0);
        if input.budget_krw.unwrap_or(allowance)>allowance{return Err("child exceeds available parent budget".into());}
        input.budget_krw=Some(input.budget_krw.unwrap_or(allowance));
        let mut run=Run::new(input,self.engine.settings.max_steps)?;
        run.parent_run_id=Some(parent);
        run.event("delegated",format!("Related to parent {parent}"),None);
        self.engine.store.put(run.clone()).await?;
        self.engine.launch(run.id).await?;
        self.get(run.id).await
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
            if run.terminal()||matches!(run.status,RunStatus::AwaitingApproval|RunStatus::Paused){return Ok(run);}
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
    pub async fn create_group(&self,req:NewRoom)->Result<Room,String>{self.engine.groups.create(req).await}
    pub async fn say_group(&self,id:Uuid,input:Say)->Result<Room,String>{self.engine.groups.say(id,input.content).await}
    pub async fn group_turn(&self,id:Uuid)->Result<Room,String>{self.engine.groups.next(&self.engine.model,id).await}
    pub async fn create_video(&self,req:NewVideo)->Result<Video,String>{self.engine.videos.create(&self.engine.model,req).await}
    pub async fn render_video(&self,id:Uuid)->Result<Video,String>{self.engine.videos.start(id).await}
    pub async fn create_improvement(&self,req:NewImprovement)->Result<Improvement,String>{self.engine.improvements.create(req).await}
    pub async fn evaluate_improvement(&self,id:Uuid)->Result<Improvement,String>{
        self.engine.improvements.start(self.engine.model.clone(),id,self.engine.settings.shell_enabled).await
    }
    pub async fn adopt_improvement(&self,id:Uuid)->Result<Improvement,String>{self.engine.improvements.adopt(id).await}
    pub async fn descendant_improvement(&self,id:Uuid)->Result<Improvement,String>{self.engine.improvements.descendant(id).await}
    pub async fn tool_catalog(&self)->Vec<Value>{
        let mut tools=tools::definitions(self.engine.settings.shell_enabled);
        tools.extend(self.engine.mcp.definitions().await);
        tools
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
