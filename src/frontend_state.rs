//! Durable presentation-side session and agent records for the canonical main UI.
//! Execution and tool authority never moves out of Engine/Store. This is not
//! another harness or a second model runtime.
use crate::domain::now_ms;
use serde::{Serialize,Deserialize};
use serde_json::Value;
use std::{collections::HashMap,path::PathBuf};
use tokio::{io::AsyncWriteExt,sync::Mutex};
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct UiAgent {
    pub id:String,
    pub name:String,
    pub description:String,
    pub system_prompt:String,
    pub provider_id:String,
    pub model:String,
    #[serde(default)]
    pub color:String,
    #[serde(default)]
    pub role:String,
    #[serde(default)]
    pub reasoning_effort:Option<String>,
    #[serde(default)]
    pub thinking:Option<bool>,
    #[serde(default)]
    pub aliases:Vec<String>,
    #[serde(default)]
    pub avatar_url:String,
    #[serde(default)]
    pub avatar:String,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct UiSession {
    pub id:String,
    pub title:String,
    pub agent_id:String,
    pub provider_id:String,
    pub model:String,
    pub created_at:String,
    pub updated_at:String,
    #[serde(default)]
    pub pending_run:Option<Uuid>,
    #[serde(default)]
    pub messages:Vec<UiMessage>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct UiMessage {
    pub id:String,
    pub session_id:String,
    pub role:String,
    pub content:String,
    pub created_at:String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub agent_id:Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub metadata:Option<Value>,
}
#[derive(Clone,Debug,Serialize,Deserialize,Default)]
pub struct Snapshot{
    pub sessions:HashMap<String,UiSession>,
    pub agents:HashMap<String,UiAgent>,
}
pub struct FrontendStore{
    file:PathBuf,
    state:Mutex<Snapshot>,
}
pub fn timestamp(ms:u64)->String{
    let seconds=ms/1000;
    let days=(seconds/86400) as i64;
    let sec=seconds%86400;
    let z=days+719468;
    let era=if z>=0{z/146097}else{(z-146096)/146097};
    let doe=z-era*146097;
    let yoe=(doe-doe/1460+doe/36524-doe/146096)/365;
    let y=yoe+era*400;
    let doy=doe-(365*yoe+yoe/4-yoe/100);
    let mp=(5*doy+2)/153;
    let d=doy-(153*mp+2)/5+1;
    let m=mp+if mp<10{3}else{-9};
    let year=y+if m<=2{1}else{0};
    format!("{year:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",sec/3600,(sec%3600)/60,sec%60,ms%1000)
}
impl FrontendStore {
    pub fn open(root:PathBuf,model:&str)->Result<Self,String>{
        std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
        let file=root.join("frontend-state.json");
        let mut state=if file.exists(){
            serde_json::from_slice::<Snapshot>(&std::fs::read(&file).map_err(|e|e.to_string())?)
                .map_err(|e|format!("frontend state file corrupt: {e}"))?
        }else{Snapshot::default()};
        state.agents.entry("general".into()).or_insert_with(||UiAgent{
            id:"general".into(),name:"일반 에이전트".into(),
            description:"Dominion Rust 기본 자율 에이전트".into(),
            system_prompt:"Follow the user's objective faithfully. Ground responses in actual tool evidence.".into(),
            provider_id:"dominion-native".into(),model:model.into(),color:"#C96442".into(),
            role:"assistant".into(),reasoning_effort:None,thinking:None,
            aliases:Vec::new(),avatar_url:String::new(),avatar:String::new(),
        });
        // Model configuration is operator-owned; an old saved default must
        // never silently route a chat to a model that is no longer configured.
        if let Some(default)=state.agents.get_mut("general"){
            default.provider_id="dominion-native".into();
            default.model=model.into();
        }
        Ok(Self{file,state:Mutex::new(state)})
    }
    async fn persist(&self,new:&Snapshot)->Result<(),String>{
        let bytes=serde_json::to_vec(new).map_err(|e|e.to_string())?;
        if bytes.len()>8*1024*1024{return Err("frontend state exceeds 8 MiB limit".into());}
        let parent=self.file.parent().ok_or("invalid frontend data directory")?;
        let stage=parent.join(format!(".frontend-{}.tmp",Uuid::new_v4()));
        let result=async{
            let mut opened=tokio::fs::OpenOptions::new().write(true).create_new(true)
                .open(&stage).await.map_err(|e|e.to_string())?;
            opened.write_all(&bytes).await.map_err(|e|e.to_string())?;
            opened.sync_all().await.map_err(|e|e.to_string())?;
            tokio::fs::rename(&stage,&self.file).await.map_err(|e|e.to_string())?;
            #[cfg(unix)]
            {
                let path=parent.to_path_buf();
                tokio::task::spawn_blocking(move||std::fs::File::open(path)
                    .and_then(|file|file.sync_all())).await
                    .map_err(|e|e.to_string())?.map_err(|e|e.to_string())?;
            }
            Ok::<(),String>(())
        }.await;
        if result.is_err(){let _=tokio::fs::remove_file(&stage).await;}
        result
    }
    pub async fn snapshot(&self)->Snapshot{self.state.lock().await.clone()}
    pub async fn mutate<T,F>(&self,change:F)->Result<T,String>
    where F:FnOnce(&mut Snapshot)->Result<T,String>{
        let mut guard=self.state.lock().await;
        let mut candidate=guard.clone();
        let result=change(&mut candidate)?;
        self.persist(&candidate).await?;
        *guard=candidate;
        Ok(result)
    }
    pub async fn agents(&self)->Vec<UiAgent>{
        let mut result=self.state.lock().await.agents.values().cloned().collect::<Vec<_>>();
        result.sort_by(|a,b|a.id.cmp(&b.id));
        result
    }
    pub async fn sessions(&self)->Vec<UiSession>{
        let mut result=self.state.lock().await.sessions.values().cloned().collect::<Vec<_>>();
        result.sort_by(|a,b|b.updated_at.cmp(&a.updated_at));
        result
    }
    pub async fn session(&self,id:&str)->Option<UiSession>{
        self.state.lock().await.sessions.get(id).cloned()
    }
    pub async fn begin_turn(&self,id:&str,input:&str,run:Uuid)->Result<UiSession,String>{
        let message=UiMessage{id:Uuid::new_v4().to_string(),session_id:id.into(),
            role:"user".into(),content:input.into(),created_at:timestamp(now_ms()),
            agent_id:None,metadata:None};
        self.mutate(|state|{
            let session=state.sessions.get_mut(id).ok_or("session not found")?;
            if session.pending_run.is_some(){return Err("session already has an active run".into());}
            session.messages.push(message);
            session.pending_run=Some(run);
            session.updated_at=timestamp(now_ms());
            Ok(session.clone())
        }).await
    }
    pub async fn finish_turn(&self,id:&str,run:Uuid,reply:Option<UiMessage>)->Result<Option<UiMessage>,String>{
        self.mutate(|state|{
            let session=state.sessions.get_mut(id).ok_or("session not found")?;
            if session.pending_run!=Some(run){return Err("session turn changed while executing".into());}
            if let Some(ref record)=reply{session.messages.push(record.clone());}
            session.pending_run=None;
            session.updated_at=timestamp(now_ms());
            Ok(reply)
        }).await
    }
}
