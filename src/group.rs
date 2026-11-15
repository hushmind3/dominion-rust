use crate::{domain::now_ms, provider::ModelClient};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::{HashMap,HashSet}, path::PathBuf};
use tokio::{fs,sync::Mutex};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Participant { pub name: String, pub role: String, #[serde(default)] pub economy: bool }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Utterance { pub speaker: String, pub content: String, pub at_ms: u64, pub model: Option<String> }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum RoomStatus { Open, Finished }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    pub id: Uuid, pub topic: String, pub participants: Vec<Participant>,
    pub messages: Vec<Utterance>, pub status: RoomStatus, pub rounds: usize,
    pub max_rounds: usize, pub cost_krw: f64, pub budget_krw: f64,
    pub created_at_ms: u64, pub updated_at_ms: u64,
}
#[derive(Deserialize)]
pub struct NewRoom {
    pub topic: String, pub participants: Vec<Participant>,
    pub max_rounds: Option<usize>, pub budget_krw: Option<f64>,
}
#[derive(Deserialize)]
pub struct Say { pub content: String }
pub struct Groups {
    root: PathBuf,
    rooms: Mutex<HashMap<Uuid,Room>>,
    active: Mutex<HashSet<Uuid>>,
}
impl Groups {
    pub async fn open(root:PathBuf) -> Result<Self,String> {
        fs::create_dir_all(&root).await.map_err(|e|e.to_string())?;
        let mut rooms=HashMap::new();
        let mut dir=fs::read_dir(&root).await.map_err(|e|e.to_string())?;
        while let Some(entry)=dir.next_entry().await.map_err(|e|e.to_string())? {
            if entry.path().extension().and_then(|s|s.to_str())!=Some("json"){continue;}
            let room:Room=serde_json::from_slice(&fs::read(entry.path()).await.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            rooms.insert(room.id,room);
        }
        Ok(Self{root,rooms:Mutex::new(rooms),active:Mutex::new(HashSet::new())})
    }
    async fn save(&self,room:Room)->Result<Room,String> {
        let mut guard=self.rooms.lock().await;
        let temp=self.root.join(format!("{}.tmp",room.id));
        let file=self.root.join(format!("{}.json",room.id));
        fs::write(&temp,serde_json::to_vec(&room).map_err(|e|e.to_string())?).await.map_err(|e|e.to_string())?;
        fs::rename(&temp,&file).await.map_err(|e|e.to_string())?;
        guard.insert(room.id,room.clone());
        Ok(room)
    }
    pub async fn create(&self,req:NewRoom)->Result<Room,String>{
        if req.topic.trim().is_empty() || req.topic.len()>10_000 {return Err("invalid room topic".into());}
        if !(2..=8).contains(&req.participants.len()) {return Err("a room needs 2..8 participants".into());}
        for p in &req.participants {
            if p.name.trim().is_empty() || p.name.len()>40 || p.role.trim().is_empty() || p.role.len()>3000 {
                return Err("invalid participant".into());
            }
        }
        let mut names=HashSet::new();
        for p in &req.participants {if !names.insert(p.name.to_lowercase()){return Err("participant names must be unique".into());}}
        let budget=req.budget_krw.unwrap_or(5000.0);
        if !budget.is_finite() || !(0.0..=1_000_000.0).contains(&budget) {return Err("invalid budget_krw".into());}
        let now=now_ms();
        let room=Room{id:Uuid::new_v4(),topic:req.topic,participants:req.participants,messages:vec![],status:RoomStatus::Open,rounds:0,max_rounds:req.max_rounds.unwrap_or(12).clamp(1,50),cost_krw:0.0,budget_krw:budget,created_at_ms:now,updated_at_ms:now};
        self.save(room).await
    }
    pub async fn get(&self,id:Uuid)->Option<Room>{self.rooms.lock().await.get(&id).cloned()}
    pub async fn list(&self)->Vec<Room>{
        let mut list:Vec<_>=self.rooms.lock().await.values().cloned().collect();
        list.sort_by_key(|r|std::cmp::Reverse(r.created_at_ms));list
    }
    pub async fn say(&self,id:Uuid,content:String)->Result<Room,String>{
        if content.trim().is_empty()||content.len()>10_000{return Err("invalid message".into());}
        if self.active.lock().await.contains(&id){return Err("room turn is active".into());}
        let mut room=self.get(id).await.ok_or("room not found")?;
        if !matches!(room.status,RoomStatus::Open){return Err("room is closed".into());}
        room.messages.push(Utterance{speaker:"user".into(),content,at_ms:now_ms(),model:None});
        room.updated_at_ms=now_ms();
        self.save(room).await
    }
    fn select(room:&Room)->usize{
        if let Some(last)=room.messages.iter().rev().find(|m|m.speaker=="user"){
            for (i,p) in room.participants.iter().enumerate(){
                if last.content.to_lowercase().contains(&format!("@{}",p.name.to_lowercase())) {return i;}
            }
        }
        // Favor members whose specialties overlap the current topic, but avoid
        // letting the same model dominate every turn.
        let previous=room.messages.iter().rev().find(|m|m.speaker!="user").map(|m|m.speaker.as_str());
        let context=room.messages.last().map_or(room.topic.as_str(),|m|m.content.as_str()).to_lowercase();
        let mut scores=vec![];
        for (i,p) in room.participants.iter().enumerate(){
            let overlap=p.role.split(|c:char|!c.is_alphanumeric())
                .filter(|word|word.chars().count()>=3)
                .filter(|word|context.contains(&word.to_lowercase())).count();
            let fair=if previous==Some(p.name.as_str()){0}else{5};
            let rotation=if i==room.rounds%room.participants.len(){2}else{0};
            scores.push((overlap*3+fair+rotation,i));
        }
        scores.into_iter().max_by_key(|x|x.0).map(|x|x.1).unwrap_or(0)
    }
    pub async fn next(&self,model:&ModelClient,id:Uuid)->Result<Room,String>{
        {let mut busy=self.active.lock().await;if !busy.insert(id){return Err("room turn already running".into());}}
        let result=self.generate(model,id).await;
        self.active.lock().await.remove(&id);
        result
    }
    async fn generate(&self,model:&ModelClient,id:Uuid)->Result<Room,String>{
        let mut room=self.get(id).await.ok_or("room not found")?;
        if !matches!(room.status,RoomStatus::Open){return Err("room is closed".into());}
        if room.rounds>=room.max_rounds || room.cost_krw>=room.budget_krw{return Err("group turn or budget limit reached".into());}
        let index=Self::select(&room);
        let participant=&room.participants[index];
        let context=room.messages.iter().rev().take(14).cloned().collect::<Vec<_>>();
        let conversation=context.into_iter().rev().map(|m|format!("{}: {}",m.speaker,m.content)).collect::<Vec<_>>().join("\n\n");
        let messages=vec![
            json!({"role":"system","content":format!("You are {}, a participant in a multi-model working room. Your specialty is: {}. Topic: {}. Respond with distinct, useful reasoning and build on prior contributions without repeating them. Be direct. If the group has answered the question, start your response with [END] followed by the conclusion. You are not a group moderator or the other participants.",participant.name,participant.role,room.topic)}),
            json!({"role":"user","content":format!("Shared conversation:\n{}\n\nContribute your next turn.",conversation)})
        ];
        let reply=model.complete(&messages,&[],participant.economy).await?;
        if room.cost_krw+reply.cost_krw > room.budget_krw {return Err("next group turn would exceed budget".into());}
        room.cost_krw+=reply.cost_krw;
        room.rounds+=1;
        let done=reply.text.trim_start().starts_with("[END]") || room.rounds>=room.max_rounds;
        room.messages.push(Utterance{speaker:participant.name.clone(),content:reply.text,at_ms:now_ms(),model:Some(reply.model)});
        if done {room.status=RoomStatus::Finished;}
        room.updated_at_ms=now_ms();
        self.save(room).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mention_selects_correct_participant() {
        let root=std::env::temp_dir().join(format!("dominion-room-{}",Uuid::new_v4()));
        let store=Groups::open(root.clone()).await.unwrap();
        let mut room=store.create(NewRoom{topic:"review".into(),participants:vec![
            Participant{name:"architect".into(),role:"design".into(),economy:false},
            Participant{name:"critic".into(),role:"testing".into(),economy:true}
        ],max_rounds:Some(6),budget_krw:Some(0.0)}).await.unwrap();
        room.messages.push(Utterance{speaker:"user".into(),content:"@critic assess this".into(),at_ms:now_ms(),model:None});
        assert_eq!(Groups::select(&room),1);
        fs::remove_dir_all(root).await.unwrap();
    }
}
