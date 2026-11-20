//! Shared, versioned event view for native Run/Team, durable Rooms and
//! presentation Chat. An event view never elevates chat content to an
//! execution receipt. Sources remain independent authorities.
use serde::{Deserialize,Serialize};
use serde_json::Value;
use uuid::Uuid;
use super::{Run,TimelineEvent};

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum EventOrigin{Run,Team,Room,Chat}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct UnifiedEvent{
    pub schema_version:u16,
    pub origin:EventOrigin,
    pub source_id:String,
    /// Only set for append-only journals (Run/Team and Room).
    /// Editable Chat records use stable_id, not a fabricated cursor.
    pub seq:Option<u64>,
    pub stable_id:String,
    pub actor:Option<String>,
    pub kind:String,
    pub content:String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub detail:Option<Value>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub at_ms:Option<u64>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub created_at:Option<String>,
}
impl UnifiedEvent{
    pub fn run(run:&Run,event:&TimelineEvent)->Self{
        let origin=if run.team.is_some(){EventOrigin::Team}else{EventOrigin::Run};
        Self{schema_version:1,origin,source_id:run.id.to_string(),
            seq:Some(event.seq),
            stable_id:format!("{}:{}",run.id,event.seq),
            actor:None,kind:event.kind.clone(),
            content:event.message.clone(),detail:event.detail.clone(),
            at_ms:Some(event.at_ms),created_at:None}
    }
    pub fn room(room_id:Uuid,index:usize,speaker:&str,content:&str,
        at_ms:u64,model:Option<&str>)->Self{
        Self::room_with_run(room_id,index,speaker,content,at_ms,model,None)
    }
    pub fn room_with_run(room_id:Uuid,index:usize,speaker:&str,content:&str,
        at_ms:u64,model:Option<&str>,run_id:Option<Uuid>)->Self{
        let seq=(index as u64).saturating_add(1);
        Self{schema_version:1,origin:EventOrigin::Room,
            source_id:room_id.to_string(),seq:Some(seq),
            stable_id:format!("{room_id}:{seq}"),
            actor:Some(speaker.to_string()),
            kind:if speaker=="user"{"owner_message"}else{"participant_message"}.into(),
            content:content.to_string(),
            detail:if model.is_some()||run_id.is_some(){
                Some(serde_json::json!({"model":model,"originRunId":run_id}))
            }else{None},
            at_ms:Some(at_ms),created_at:None}
    }
    pub fn chat(session_id:&str,message_id:&str,role:&str,content:&str,
        created_at:&str,agent:Option<&str>)->Self{
        Self{schema_version:1,origin:EventOrigin::Chat,
            source_id:session_id.to_string(),seq:None,
            stable_id:format!("{session_id}:{message_id}"),
            actor:agent.or(Some(role)).map(str::to_owned),
            kind:role.to_owned(),content:content.to_owned(),
            detail:None,at_ms:None,created_at:Some(created_at.to_owned())}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::domain::{CreateRun,Run};
    #[test]
    fn projections_preserve_source_identity_without_fake_cross_store_sequence(){
        let mut run=Run::new(CreateRun{instruction:"test".into(),
            budget_krw:Some(10.0),max_steps:Some(2)},2).unwrap();
        let a=UnifiedEvent::run(&run,&run.events[0]);
        run.event("worker_attached","worker started",None);
        let b=UnifiedEvent::run(&run,&run.events[1]);
        assert_eq!(a.seq,Some(1));assert_eq!(b.seq,Some(2));
        assert_ne!(a.stable_id,b.stable_id);
        let room=UnifiedEvent::room(run.id,0,"user","test",123,None);
        assert_eq!(room.seq,Some(1));
        assert_eq!(room.kind,"owner_message");
        let linked=UnifiedEvent::room_with_run(run.id,1,"expert","evidence",
            124,Some("local-model"),Some(run.id));
        assert_eq!(linked.detail.as_ref().unwrap()["originRunId"],run.id.to_string());
        assert_eq!(linked.seq,Some(2));
        let chat=UnifiedEvent::chat("chat-2","message-9","assistant",
            "response","2026-11-20T00:00:00Z",None);
        assert_eq!(chat.seq,None,
            "editable chat UI record has no append-only event cursor");
        assert_eq!(chat.stable_id,"chat-2:message-9");
    }
}
