//! Durable room-turn work board: pure projection of the existing Room journal.
//! No second database or second runner. A task's source is the indexed owner
//! message; completed replies are referenced by their original message index.
//! Rebuilding after restart yields the same schedule, without copying other
//! agents' full transcripts or treating their text as commands.
use crate::group::{Groups,Room};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum WorkStatus { Pending, Completed }

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct WorkTicket {
    pub id:String,
    pub room_id:Uuid,
    pub owner_message_index:Option<usize>,
    pub agent_id:String,
    pub status:WorkStatus,
    pub result_message_index:Option<usize>,
    /// Source index interval not yet acknowledged by this participant's
    /// last accepted reply, independent of other participants' cursors.
    pub unread_start_index:usize,
    pub unread_end_index:usize,
}

#[derive(Clone,Debug)]
pub struct RoomWorkBoard {
    pub tickets:Vec<WorkTicket>,
}
impl RoomWorkBoard {
    pub fn project(room:&Room)->Self{
        let owner=room.messages.iter().rposition(|m|m.speaker=="user");
        let mode=room.frontend.as_ref().map(|r|r.routing_mode.as_str())
            .unwrap_or("discussion");
        let candidates:Vec<String>=match mode{
            "discussion"=>if room.recipients.is_empty(){
                room.participants.iter().map(|p|p.name.clone()).collect()
            }else{room.recipients.clone()},
            "mentioned"=>if !room.recipients.is_empty(){
                room.recipients.clone()
            }else{
                let directive=owner.and_then(|i|room.messages.get(i))
                    .map_or("",|m|m.content.as_str()).to_lowercase();
                room.participants.iter().filter(|p|
                    directive.contains(&format!("@{}",p.name.to_lowercase())))
                    .map(|p|p.name.clone()).collect()
            },
            _=>if let Some(first)=room.recipients.first(){
                vec![first.clone()]
            }else{
                vec![room.participants[Groups::select(room)].name.clone()]
            },
        };
        let mut seen=HashSet::new();
        let tickets=candidates.into_iter().filter(|id|{
            // Operator-supplied targets never authorize a deleted speaker.
            seen.insert(id.clone()) && room.participants.iter().any(|p|p.name==*id)
        }).map(|agent_id|{
            let result_message_index=owner.and_then(|idx|
                room.messages.iter().enumerate().skip(idx+1)
                    .take_while(|(_,m)|m.speaker!="user")
                    .find(|(_,m)|m.speaker==agent_id).map(|(i,_)|i));
            let unread=room.unread_range(&agent_id);
            let status=if result_message_index.is_some(){
                WorkStatus::Completed
            }else{WorkStatus::Pending};
            WorkTicket{
                id:format!("room:{}:owner:{}:agent:{}",
                    room.id,owner.unwrap_or(0),agent_id),
                room_id:room.id,owner_message_index:owner,
                agent_id,status,result_message_index,
                unread_start_index:unread.start,unread_end_index:unread.end,
            }
        }).collect();
        Self{tickets}
    }
    pub fn pending_agents(&self)->Vec<String>{
        self.tickets.iter().filter(|ticket|ticket.status==WorkStatus::Pending)
            .map(|ticket|ticket.agent_id.clone()).collect()
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use crate::group::{FrontendRoom,NewRoom,Participant,Utterance};
    use crate::domain::now_ms;

    async fn room(mode:&str)->Room{
        let root=std::env::temp_dir().join(format!("dominion-board-{}",Uuid::new_v4()));
        let store=Groups::open(root.clone()).await.unwrap();
        let mut room=store.create(NewRoom{topic:"Review".into(),participants:vec![
            Participant{name:"coder".into(),role:"implement".into(),economy:false},
            Participant{name:"reviewer".into(),role:"review".into(),economy:false},
        ],max_rounds:Some(20),budget_krw:Some(0.0)}).await.unwrap();
        room.frontend=Some(FrontendRoom{name:"review".into(),topic:"Review".into(),
            owner_name:"owner".into(),agent_ids:vec!["coder".into(),"reviewer".into()],
            routing_mode:mode.into(),rounds:1,max_turns:2,
            lifetime_max_responses:20,token_budget:0,context_budget_tokens:2000,
            tools_enabled:false,auto_learning:false,auto_models:true,models:vec![],
            reasoning_effort:String::new(),learned_context:String::new()});
        let _=tokio::fs::remove_dir_all(root).await;
        room
    }
    fn message(speaker:&str,content:&str)->Utterance{
        Utterance{speaker:speaker.into(),content:content.into(),
            at_ms:now_ms(),model:None}
    }
    #[tokio::test]
    async fn owner_scoped_work_board_reconstructs_pending_and_completed_receipts(){
        let mut room=room("discussion").await;
        room.messages.push(message("user","Implement a patch and review it"));
        room.messages.push(message("coder","Patch complete"));
        let projected=RoomWorkBoard::project(&room);
        assert_eq!(projected.tickets[0].owner_message_index,Some(0));
        assert_eq!(projected.tickets.len(),2);
        assert_eq!(projected.tickets.iter().filter(|t|t.status==WorkStatus::Completed).count(),1);
        assert_eq!(projected.pending_agents(),vec!["reviewer"]);
        assert_eq!(projected.tickets[0].result_message_index,Some(1));
        assert_eq!(projected.tickets[0].unread_start_index,2);
        assert_eq!(projected.tickets[0].unread_end_index,2);
        assert_eq!(projected.tickets[1].unread_start_index,0);
        room.messages.push(message("user","Start a fresh review"));
        let next=RoomWorkBoard::project(&room);
        assert_eq!(next.tickets[0].owner_message_index,Some(2));
        assert_eq!(next.pending_agents(),vec!["coder","reviewer"]);
        assert_ne!(projected.tickets[0].id,next.tickets[0].id);
    }
    #[tokio::test]
    async fn mentioned_route_never_schedules_unmentioned_agent(){
        let mut room=room("mentioned").await;
        room.messages.push(message("user","@reviewer check this"));
        let board=RoomWorkBoard::project(&room);
        assert_eq!(board.pending_agents(),vec!["reviewer"]);
        assert_eq!(board.tickets.len(),1);
    }
    #[tokio::test]
    async fn pinned_round_robin_does_not_generate_second_speaker_on_retry(){
        let mut room=room("round_robin").await;
        room.recipients=vec!["coder".into()];
        room.messages.push(message("user","fix one file"));
        room.messages.push(message("coder","done"));
        room.rounds+=1;
        let board=RoomWorkBoard::project(&room);
        assert_eq!(board.tickets.len(),1);
        assert_eq!(board.tickets.iter().filter(|t|t.status==WorkStatus::Completed).count(),1);
        assert!(board.pending_agents().is_empty());
    }
}
