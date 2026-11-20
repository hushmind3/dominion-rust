//! Product adapter for tool-enabled Rooms. Never owns an agent loop or a
//! privileged tool executor: each speaker is one source-linked native Run.
use super::*;
use crate::{domain::{CreateRun,Run,RunStatus,RoomRunBinding},
    group::Groups,sdk::Dominion};
use sha2::{Digest,Sha256};
use std::time::Duration;

/// Stable after a retry or restart; distinct from previous owner turns and
/// after a reset. No stored sequence counter or parallel session allocator.
fn identity(room:&Room,participant:&str)->Option<(Uuid,RoomRunBinding)>{
    let owner_message_index=room.messages.iter().rposition(|m|m.speaker=="user")?;
    let owner=&room.messages[owner_message_index];
    let binding=RoomRunBinding{room_id:room.id,owner_message_index,
        owner_at_ms:owner.at_ms,participant:participant.to_owned()};
    let mut digest=Sha256::new();
    digest.update(b"dominion-room-speaker-run-v1\0");
    digest.update(room.id.as_bytes());
    digest.update((owner_message_index as u64).to_be_bytes());
    digest.update(owner.at_ms.to_be_bytes());
    digest.update(participant.as_bytes());
    let hash=digest.finalize();
    let mut uuid=[0u8;16];
    uuid.copy_from_slice(&hash[..16]);
    Some((Uuid::from_bytes(uuid),binding))
}
async fn source_run(engine:&Arc<Engine>,room:&Room,participant:&str)->Option<Run>{
    let (id,binding)=identity(room,participant)?;
    let run=engine.store.get(id).await?;
    (run.room_binding.as_ref()==Some(&binding)).then_some(run)
}
pub(super) async fn running(engine:&Arc<Engine>,room:&Room)->bool{
    for participant in &room.participants{
        if let Some(run)=source_run(engine,room,&participant.name).await{
            if run.status==RunStatus::Running||run.status==RunStatus::Queued{
                return true;
            }
        }
    }
    false
}
pub(super) async fn approvals(engine:&Arc<Engine>,room:&Room)->Vec<Value>{
    let mut requests=Vec::new();
    for participant in &room.participants{
        if let Some(run)=source_run(engine,room,&participant.name).await{
            if let Some(call)=run.pending_approval.as_ref().filter(|_|
                run.status==RunStatus::AwaitingApproval && run.approval_decision.is_none()){
                requests.push(json!({"id":run.id,"tool":call.name,
                    "args":call.arguments,"status":"pending",
                    "sessionId":room_session(room.id),"agentId":participant.name}));
            }
        }
    }
    requests
}
/// Called by Room state and transcript reads after an approval may have
/// finished while the original HTTP sender was disconnected.
pub(super) async fn reconcile_ready(engine:&Arc<Engine>,id:Uuid)->Result<(),String>{
    let room=engine.groups.get(id).await.ok_or("room not found")?;
    if !config_of(&room).tools_enabled{return Ok(());}
    let Some(owner_index)=room.messages.iter().rposition(|m|m.speaker=="user")else{
        return Ok(());
    };
    for participant in &room.participants{
        let Some(run)=source_run(engine,&room,&participant.name).await else{continue};
        if run.terminal(){
            if !room.usage_receipts.iter().any(|r|r.origin_run_id==Some(run.id)){
                // The authoritative Room snapshot may have advanced after
                // another accepted speaker. Re-read on each settlement.
                engine.groups.settle_native_run(&run).await?;
            }
        }
    }
    // No new model calls are made merely by reading a Room.
    let _=owner_index;
    Ok(())
}
/// Execute once; on retry only observe the original Run. At most one
/// billable model execution is launched for this owner/speaker identity.
pub(super) async fn execute(engine:&Arc<Engine>,id:Uuid,speaker:&str,
    provider:&str,model:&str)->Result<Room,String>{
    let room=engine.groups.get(id).await.ok_or("room not found")?;
    if room.status!=RoomStatus::Open{return Err("room is closed".into());}
    let (run_id,binding)=identity(&room,speaker).ok_or("Room has no owner message")?;
    let existing=engine.store.get(run_id).await;
    if let Some(run)=existing.as_ref(){
        if run.room_binding.as_ref()!=Some(&binding){
            return Err("native Room Run identity collision or changed origin".into());
        }
        if run.provider_id!=provider||run.model_name!=model{
            return Err("Room model changed during an in-progress native Run".into());
        }
    }else{
        let available=(room.budget_krw-room.cost_krw).max(0.0);
        if available<=0.0{return Err("Room KRW budget exhausted before model dispatch".into());}
        let (context,metrics)=Groups::native_speaker_context(&room,speaker)?;
        let mut run=Run::new(CreateRun{
            instruction:room.messages[binding.owner_message_index].content.clone(),
            budget_krw:Some(available),max_steps:None
        },engine.settings.max_steps)?;
        run.id=run_id;
        run.room_binding=Some(binding);
        run.chat_context=context;
        run.read_only=true; // room peer text cannot authorize mutations
        run.delegation_profile="inspection".into();
        run.provider_id=provider.to_owned();run.model_name=model.to_owned();
        let key=format!("{id}:{speaker}");
        let digest=Sha256::digest(key.as_bytes());
        run.provider_session_affinity=format!("dominion-room-{}",
            digest[..12].iter().map(|b|format!("{b:02x}")).collect::<String>());
        run.event("room_context_attached","Room original evidence bound to native Run",
            Some(metrics));
        engine.store.put(run).await?;
        engine.launch(run_id).await?;
    }
    let sdk=Dominion::from_engine(Arc::clone(engine));
    let observed=sdk.wait(run_id,Duration::from_secs(25)).await;
    let run=match observed{
        Ok(run)=>run,
        Err(error) if error=="execution wait timeout"=>
            return engine.groups.get(id).await.ok_or("room no longer available".into()),
        Err(error)=>return Err(error),
    };
    if !run.terminal(){
        // Pending approvals remain in the same native journal and are
        // resolved by the ordinary /approvals endpoint, never auto-granted.
        return engine.groups.get(id).await.ok_or("room no longer available".into());
    }
    let result=engine.groups.settle_native_run(&run).await?;
    if run.status!=RunStatus::Completed{
        return Err(format!("native Room speaker {} ended with status {:?}; no reply fabricated",
            speaker,run.status));
    }
    if !result.usage_receipts.iter().any(|r|
        r.origin_run_id==Some(run.id)&&r.accepted){
        return Err("native Room reply was billed but rejected by room budget".into());
    }
    Ok(result)
}
