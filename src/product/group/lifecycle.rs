//! Room mutation lifecycle. One frontend owner-turn lease blocks destructive
//! state changes while a real provider reply could still be billed or saved.
use super::*;

impl Groups {
    async fn mutation_lease(&self,id:Uuid)->Result<FrontendTurnLease,String>{
        let lease=self.try_frontend_turn(id)
            .ok_or("room turn active; wait for the original speaker receipts")?;
        if self.active.lock().await.contains(&id){
            return Err("room turn active; wait for the original speaker receipts".into());
        }
        Ok(lease)
    }
    pub async fn configure(&self,id:Uuid,config:FrontendRoom,participants:Vec<Participant>)
        ->Result<Room,String>{
        let _lease=self.mutation_lease(id).await?;
        if !(1..=32).contains(&participants.len()){return Err("room needs 1..32 participants".into());}
        let mut room=self.get(id).await.ok_or("room not found")?;
        if !room.messages.is_empty() &&
            room.frontend.as_ref().is_some_and(|v|v.agent_ids!=config.agent_ids){
            return Err("reset the room before changing participants".into());
        }
        let next_topic=if config.topic.trim().is_empty(){
            config.name.clone()
        }else{config.topic.clone()};
        // A changed speaker instruction/topic invalidates the previous
        // server-side model history; silently reusing it leaks old settings.
        if room.topic!=next_topic||room.participants.iter()
            .map(|p|(&p.name,&p.role,p.economy))
            .ne(participants.iter().map(|p|(&p.name,&p.role,p.economy))){
            room.remote_continuations.clear();
        }
        room.topic=next_topic;
        room.participants=participants;
        room.max_rounds=if config.lifetime_max_responses==0{10000}
            else{config.lifetime_max_responses.min(10000)};
        let mut config=config;
        if config.learned_context.is_empty(){
            config.learned_context=room.frontend.as_ref()
                .map(|previous|previous.learned_context.clone()).unwrap_or_default();
        }
        room.frontend=Some(config);
        room.updated_at_ms=now_ms();
        self.save(room).await
    }
    pub async fn set_knowledge_context(&self,id:Uuid,context:String)->Result<Room,String>{
        if context.len()>4000{return Err("group knowledge context too long".into());}
        if self.active.lock().await.contains(&id){return Err("room turn is active".into());}
        let mut room=self.get(id).await.ok_or("room not found")?;
        if let Some(ref mut config)=room.frontend{
            if config.learned_context==context{return Ok(room);}
            config.learned_context=context;
            room.updated_at_ms=now_ms();
            self.save(room).await
        }else{Err("room has no main-frontend configuration".into())}
    }
    pub async fn reset(&self,id:Uuid)->Result<Room,String>{
        let _lease=self.mutation_lease(id).await?;
        let mut room=self.get(id).await.ok_or("room not found")?;
        room.messages.clear();room.client_messages.clear();room.client_requested_agents.clear();room.recipients.clear();room.rounds=0;room.cost_krw=0.0;room.used_tokens=0;
        room.usage_receipts.clear();room.remote_continuations.clear();room.read_cursors.clear();
        room.finish_reason.clear();
        room.last_provider_failure=None;
        room.status=RoomStatus::Open;room.updated_at_ms=now_ms();
        self.save(room).await
    }
    pub async fn stop(&self,id:Uuid)->Result<Room,String>{
        let _lease=self.mutation_lease(id).await?;
        let mut room=self.get(id).await.ok_or("room not found")?;
        room.status=RoomStatus::Finished;room.finish_reason="stopped".into();room.updated_at_ms=now_ms();
        self.save(room).await
    }
    pub async fn remove(&self,id:Uuid)->Result<(),String>{
        let _lease=self.mutation_lease(id).await?;
        let gate=self.write_gate(id).await;
        let _serial=gate.lock().await;
        if !self.rooms.lock().await.contains_key(&id){return Err("room not found".into());}
        fs::remove_file(self.root.join(format!("{id}.json"))).await.map_err(|e|e.to_string())?;
        #[cfg(unix)]
        {
            let root=self.root.clone();
            tokio::task::spawn_blocking(move||std::fs::File::open(root)
                .and_then(|file|file.sync_all())).await.map_err(|e|e.to_string())?
                .map_err(|e|e.to_string())?;
        }
        self.rooms.lock().await.remove(&id);Ok(())
    }
}
