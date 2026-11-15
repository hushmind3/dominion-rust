//! Durable original-main workflow definitions and ordered execution checkpoints.
//! The real agent engine executes each step; this module never fabricates results.
use crate::domain::now_ms;
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{collections::{HashMap,HashSet},path::PathBuf};
use tokio::{fs,io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct WorkflowStep {
    pub id:String,
    pub name:String,
    pub prompt:String,
    pub agent_id:String,
    #[serde(default)] pub provider_id:String,
    #[serde(default)] pub model:String,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct Workflow {
    pub id:String,
    pub name:String,
    pub description:String,
    pub steps:Vec<WorkflowStep>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct StepResult {
    pub step_id:String,
    pub step_name:String,
    pub run_id:Uuid,
    pub answer:String,
    pub cost_krw:f64,
    pub tokens:u64,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct WorkflowJob {
    pub id:Uuid,
    pub kind:String,
    pub status:String,
    pub input:Value,
    pub step_index:usize,
    pub step_total:usize,
    pub active_run_id:Option<Uuid>,
    pub result:Option<Value>,
    pub error:Option<String>,
    pub created_at:String,
    pub updated_at:String,
    pub workflow:Workflow,
    pub user_input:String,
    #[serde(default="default_budget")] pub budget_krw:f64,
    pub outputs:Vec<StepResult>,
    #[serde(default)] pub cancelled:bool,
}
#[derive(Clone,Default,Serialize,Deserialize)]
struct Snapshot {
    workflows:HashMap<String,Workflow>,
    jobs:HashMap<Uuid,WorkflowJob>,
}
pub struct Workflows {
    file:PathBuf,
    state:Mutex<Snapshot>,
    active:Mutex<HashSet<Uuid>>,
}
fn stamp()->String{crate::frontend_state::timestamp(now_ms())}
fn default_budget()->f64{10_000.0}
fn write_sync(file:&std::path::Path,contents:&[u8])->Result<(),String>{
    let dir=file.parent().ok_or("workflow directory missing")?;
    let tmp=dir.join(format!(".workflows-{}.tmp",Uuid::new_v4()));
    let result=(||->Result<(),String>{
        use std::io::Write;
        let mut stage=std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&tmp).map_err(|e|e.to_string())?;
        stage.write_all(contents).map_err(|e|e.to_string())?;
        stage.sync_all().map_err(|e|e.to_string())?;
        std::fs::rename(&tmp,file).map_err(|e|e.to_string())?;
        #[cfg(unix)] std::fs::File::open(dir).and_then(|dir|dir.sync_all())
            .map_err(|e|e.to_string())?;
        Ok(())
    })();
    if result.is_err(){let _=std::fs::remove_file(tmp);}
    result
}
impl Workflows {
    pub fn open(root:PathBuf)->Result<Self,String>{
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("workflows.json");
        let mut state=if file.exists(){
            serde_json::from_slice::<Snapshot>(&std::fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("corrupt workflow state: {e}"))?
        }else{Snapshot::default()};
        // No job can keep an in-memory executor alive across process restart.
        // A durable in-flight run ID remains attached; explicit resume can only
        // continue that same native run, never duplicate its side effects.
        let mut recovered=false;
        for job in state.jobs.values_mut(){
            if job.status=="running" || job.status=="queued" {
                job.status="needs_review".into();
                job.error=Some("Service restarted; inspect the recorded native step before resuming".into());
                job.updated_at=stamp();
                recovered=true;
            }
        }
        if recovered{
            let bytes=serde_json::to_vec(&state).map_err(|e|e.to_string())?;
            write_sync(&file,&bytes)?;
        }
        Ok(Self{file,state:Mutex::new(state),active:Mutex::new(HashSet::new())})
    }
    async fn persist(&self,state:&Snapshot)->Result<(),String>{
        let bytes=serde_json::to_vec(state).map_err(|e|e.to_string())?;
        if bytes.len()>32*1024*1024{return Err("workflow data exceeds 32 MiB".into());}
        let root=self.file.parent().ok_or("invalid workflow directory")?;
        let tmp=root.join(format!(".workflows-{}.tmp",Uuid::new_v4()));
        let outcome=async{
            let mut stage=fs::OpenOptions::new().write(true).create_new(true)
                .open(&tmp).await.map_err(|e|e.to_string())?;
            stage.write_all(&bytes).await.map_err(|e|e.to_string())?;
            stage.sync_all().await.map_err(|e|e.to_string())?;
            fs::rename(&tmp,&self.file).await.map_err(|e|e.to_string())?;
            #[cfg(unix)]{
                let dir=root.to_path_buf();
                tokio::task::spawn_blocking(move||std::fs::File::open(dir)
                    .and_then(|file|file.sync_all())).await
                    .map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
            }
            Ok::<(),String>(())
        }.await;
        if outcome.is_err(){let _=fs::remove_file(&tmp).await;}
        outcome
    }
    async fn change<T,F>(&self,edit:F)->Result<T,String> where F:FnOnce(&mut Snapshot)->Result<T,String>{
        let mut guard=self.state.lock().await;
        let mut next=guard.clone();
        let value=edit(&mut next)?;
        self.persist(&next).await?;
        *guard=next;
        Ok(value)
    }
    pub async fn list(&self)->Vec<Workflow>{
        let mut flows=self.state.lock().await.workflows.values().cloned().collect::<Vec<_>>();
        flows.sort_by(|a,b|a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        flows
    }
    pub async fn get(&self,id:&str)->Option<Workflow>{
        self.state.lock().await.workflows.get(id).cloned()
    }
    pub async fn save(&self,record:Workflow,update:bool)->Result<Workflow,String>{
        self.change(|state|{
            if update&&!state.workflows.contains_key(&record.id){
                return Err("workflow to edit does not exist".into());
            }
            if state.workflows.len()>=256&&!state.workflows.contains_key(&record.id){
                return Err("maximum 256 workflows".into());
            }
            state.workflows.insert(record.id.clone(),record.clone());
            Ok(record)
        }).await
    }
    pub async fn delete(&self,id:&str)->Result<(),String>{
        self.change(|state|{
            if state.jobs.values().any(|job|job.workflow.id==id
                && matches!(job.status.as_str(),"running"|"queued"|"needs_review")){
                return Err("workflow has an active or interrupted run; cancel it first".into());
            }
            state.workflows.remove(id).ok_or("workflow not found")?;
            Ok(())
        }).await
    }
    pub async fn start(&self,id:&str,input:String)->Result<WorkflowJob,String>{
        self.change(|state|{
            let flow=state.workflows.get(id).ok_or("workflow not found")?.clone();
            if state.jobs.len()>=512{return Err("maximum 512 stored workflow runs".into());}
            let now=stamp();
            let job=WorkflowJob{
                id:Uuid::new_v4(),kind:"workflow".into(),status:"queued".into(),
                input:serde_json::json!({"workflowId":id,"input":input}),
                step_index:0,step_total:flow.steps.len(),active_run_id:None,
                result:None,error:None,created_at:now.clone(),updated_at:now,
                workflow:flow,user_input:input,budget_krw:default_budget(),
                outputs:Vec::new(),cancelled:false,
            };
            state.jobs.insert(job.id,job.clone());
            Ok(job)
        }).await
    }
    pub async fn jobs(&self)->Vec<WorkflowJob>{
        let mut jobs=self.state.lock().await.jobs.values().cloned().collect::<Vec<_>>();
        jobs.sort_by(|a,b|b.created_at.cmp(&a.created_at));jobs
    }
    pub async fn job(&self,id:Uuid)->Option<WorkflowJob>{
        self.state.lock().await.jobs.get(&id).cloned()
    }
    pub async fn mutate_job<F>(&self,id:Uuid,edit:F)->Result<WorkflowJob,String>
    where F:FnOnce(&mut WorkflowJob)->Result<(),String>{
        self.change(|state|{
            let job=state.jobs.get_mut(&id).ok_or("workflow job not found")?;
            edit(job)?;job.updated_at=stamp();Ok(job.clone())
        }).await
    }
    pub async fn reserve(&self,id:Uuid)->Result<(),String>{
        let mut active=self.active.lock().await;
        if !active.insert(id){return Err("workflow worker is already running".into());}
        Ok(())
    }
    pub async fn release(&self,id:Uuid){self.active.lock().await.remove(&id);}
}
