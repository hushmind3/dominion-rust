//! Endpoint-level 9-speaker discussion regression. A fake provider models
//! real request/usage shape; every utterance is generated through the actual
//! Rust Groups + registered frontend API path, not invented transcript rows.
use super::*;
use crate::{
    config::Settings,group::{Groups,NewRoom,Participant},
    store::Store,
};
use axum::{extract::State as MockState,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};

async fn model(MockState((requests,reject_at)):MockState<(Arc<AtomicUsize>,Option<usize>)>,
    Json(_input):Json<Value>)->(StatusCode,Json<Value>){
    let index=requests.fetch_add(1,Ordering::SeqCst);
    if reject_at==Some(index){
        return (StatusCode::BAD_REQUEST,Json(json!({
            "success":false,"errors":[{"code":5004,
                "message":"synthetic provider rejected the request"}]
        })));
    }
    // A single agent should not end a 9-speaker group early by emitting [END].
    let content=if index%9==0 {
        format!("[END] First participant said finished in turn {}",index/9+1)
    }else{
        format!("Agent reply {index}")
    };
    // Mock measured cache-hit receipts. The counters are provider-supplied,
    // not predicted from context character counts or local heuristics.
    let cached=if index>=9{15}else{0};
    (StatusCode::OK,Json(json!({"choices":[{"finish_reason":"stop",
        "message":{"role":"assistant","content":content}}],
        "usage":{"prompt_tokens":90,"completion_tokens":10,
            "prompt_tokens_details":{"cached_tokens":cached}}})))
}
async fn setup(max_turns:usize,routing_mode:&str)
    ->(Arc<Engine>,Uuid,Arc<AtomicUsize>,tokio::task::JoinHandle<()>,std::path::PathBuf){
    setup_with_failure(max_turns,routing_mode,None).await
}
async fn setup_with_failure(max_turns:usize,routing_mode:&str,reject_at:Option<usize>)
    ->(Arc<Engine>,Uuid,Arc<AtomicUsize>,tokio::task::JoinHandle<()>,std::path::PathBuf){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let requests=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let counter=requests.clone();
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(model)).with_state((counter,reject_at)))
                .await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!(
        "dominion-real-room-fanout-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.name="mock-room-model".into();
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings,store,Arc::clone(&groups)).unwrap();
    let participants=(0..9).map(|i|Participant{
        name:format!("agent-{i}"),role:format!("topic expert {i}"),economy:false
    }).collect::<Vec<_>>();
    let room=groups.create(NewRoom{
        topic:"Nine experts review a problem".into(),
        participants:participants.clone(),max_rounds:Some(50),
        budget_krw:Some(5000.0)
    }).await.unwrap();
    let frontend=FrontendRoom{
        name:"Nine-speaker room".into(),topic:room.topic.clone(),
        owner_name:"owner".into(),
        agent_ids:participants.iter().map(|p|p.name.clone()).collect(),
        routing_mode:routing_mode.into(),rounds:3,max_turns,
        lifetime_max_responses:0,
        token_budget:0,context_budget_tokens:2000,
        tools_enabled:false,auto_learning:false,auto_models:false,
        models:vec![json!({"providerId":"dominion-native","model":"mock-room-model"})],
        reasoning_effort:String::new(),learned_context:String::new()
    };
    groups.configure(room.id,frontend,participants).await.unwrap();
    (engine,room.id,requests,server,root)
}

#[tokio::test]
async fn nine_agents_three_owner_turns_produce_exactly_thirty_messages_and_durable_usage(){
    // The old implementation stopped after 10 lifetime responses, midway
    // through the SECOND owner message. maxTurns=10 now covers each owner.
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    for index in 0..3{
        let response=send(State(Arc::clone(&engine)),Path(id),Json(SayInput{
            input:format!("Review this issue in round {}",index+1),
            agent_ids:vec![],client_message_id:format!("owner-turn-{}",index+1),
            reasoning_language:None
        })).await.unwrap().0;
        assert_eq!(response["job"]["status"],"completed");
        assert_eq!(response["job"]["result"]["expectedResponses"],9);
        assert_eq!(response["job"]["result"]["actualResponses"],9);
        assert_eq!(response["job"]["result"]["missingResponses"],0);
        let room=engine.groups.get(id).await.unwrap();
        assert_eq!(room.messages.len(),(index+1)*10);
        let block=&room.messages[index*10..(index+1)*10];
        assert_eq!(block[0].speaker,"user");
        let unique=block[1..].iter().map(|m|m.speaker.clone())
            .collect::<HashSet<_>>();
        assert_eq!(unique.len(),9,"all nine speakers must have unique responses");
        assert_eq!(unique,room.participants.iter()
            .map(|p|p.name.clone()).collect::<HashSet<_>>());
    }
    let room=engine.groups.get(id).await.unwrap();
    assert_eq!(room.status,RoomStatus::Open);
    assert_eq!(room.rounds,27);
    assert_eq!(room.max_rounds,10_000,
        "maxTurns must not overwrite the independent room lifetime ceiling");
    assert_eq!(room.usage_receipts.len(),27);
    assert_eq!(room.used_tokens,2700);
    assert_eq!(room.usage_receipts.iter().map(|r|r.input_tokens).sum::<u64>(),2430);
    assert_eq!(room.usage_receipts.iter().map(|r|r.output_tokens).sum::<u64>(),270);
    assert_eq!(requests.load(Ordering::SeqCst),27);
    let report=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(report["usage"]["calls"],27);
    assert_eq!(report["usage"]["providerUsageReported"],true);
    assert_eq!(report["usage"]["unmeteredCalls"],0);
    assert_eq!(report["usage"]["usageStatus"],"reported");
    assert_eq!(report["usage"]["promptTokens"],2430);
    assert_eq!(report["usage"]["cachedPromptTokens"],270,
        "only the upstream-reported cache hits are counted");
    assert_eq!(report["usage"]["uncachedPromptTokens"],2160,
        "new input tokens exclude the measured cached prefix");
    assert_eq!(report["usage"]["completionTokens"],270);
    assert_eq!(report["usage"]["byAgent"].as_array().unwrap().len(),9);
    assert!(report["usage"]["byAgent"].as_array().unwrap().iter()
        .all(|row|row["calls"]==3 && row["promptTokens"]==270));
    let rows=message_rows(&room);
    assert_eq!(rows.len(),30);
    assert_eq!(rows[1].metadata.as_ref().unwrap()["usage"]["totalTokens"],100);
    assert_eq!(rows[1].metadata.as_ref().unwrap()["usage"]["providerUsageReported"],true);
    // Old journal receipts and providers omitting usage are explicitly
    // "unreported", never presented as zero-cost requests.
    let mut legacy=room.clone();
    legacy.usage_receipts[0].provider_usage_reported=None;
    let usage=room_usage(&legacy);
    assert_eq!(usage["unmeteredCalls"],1);
    assert_eq!(usage["usageStatus"],"partial_or_unreported");
    assert_eq!(usage["providerUsageReported"],false);

    // Re-delivery of the same owner message ID cannot start another group.
    let duplicate=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Review this issue in round 3".into(),agent_ids:vec![],
        client_message_id:"owner-turn-3".into(),reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(duplicate["job"]["status"],"completed");
    assert_eq!(requests.load(Ordering::SeqCst),27);
    assert_eq!(engine.groups.get(id).await.unwrap().messages.len(),30);
    let serialized=serde_json::to_value(&room).unwrap();
    let recovered:Room=serde_json::from_value(serialized).unwrap();
    assert_eq!(recovered.usage_receipts.len(),27);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn running_out_of_max_turns_never_claims_full_nine_agent_completion(){
    let (engine,id,requests,server,root)=setup(4,"discussion").await;
    let response=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Everyone should answer".into(),
        agent_ids:vec![],client_message_id:"capped-turn".into(),
        reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(response["job"]["status"],"interrupted");
    assert_eq!(response["job"]["result"]["expectedResponses"],9);
    assert_eq!(response["job"]["result"]["actualResponses"],4);
    assert_eq!(response["job"]["result"]["missingResponses"],5);
    assert_eq!(response["room"]["status"],"idle");
    assert_eq!(response["room"]["finishReason"],"",
        "per-owner cap must not close the entire room");
    assert_eq!(requests.load(Ordering::SeqCst),4);
    assert_eq!(engine.groups.get(id).await.unwrap().messages.len(),5);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn per_owner_limit_of_four_allows_the_next_owner_to_use_a_fresh_four(){
    let (engine,id,requests,server,root)=setup(4,"discussion").await;
    for index in 0..3{
        let result=send(State(engine.clone()),Path(id),Json(SayInput{
            input:format!("Owner request {index}"),agent_ids:vec![],
            client_message_id:format!("limited-{index}"),reasoning_language:None,
        })).await.unwrap().0;
        assert_eq!(result["job"]["status"],"interrupted");
        assert_eq!(result["job"]["result"]["expectedResponses"],9);
        assert_eq!(result["job"]["result"]["actualResponses"],4);
        assert_eq!(result["room"]["status"],"idle");
    }
    assert_eq!(requests.load(Ordering::SeqCst),12);
    let room=engine.groups.get(id).await.unwrap();
    assert_eq!(room.rounds,12);
    assert_eq!(room.messages.len(),15);
    let duplicate=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Owner request 2".into(),agent_ids:vec![],
        client_message_id:"limited-2".into(),reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(duplicate["job"]["result"]["actualResponses"],4);
    assert_eq!(requests.load(Ordering::SeqCst),12,
        "retrying at the same cap must not issue another billable model call");
    server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn after_call_token_budget_rejection_is_metered_but_never_fabricates_an_answer(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let existing=engine.groups.get(id).await.unwrap();
    let mut config=existing.frontend.unwrap();
    config.token_budget=95; // observed fake provider emits 90+10=100
    engine.groups.configure(id,config,existing.participants).await.unwrap();
    let failed=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Make the expensive call but do not pretend it was delivered".into(),
        agent_ids:vec![],client_message_id:"budget-overflow".into(),
        reasoning_language:None
    })).await.err().expect("post-inference budget breach must interrupt");
    assert_eq!(failed.0,StatusCode::BAD_REQUEST);
    assert_eq!(requests.load(Ordering::SeqCst),1);
    let room=engine.groups.get(id).await.unwrap();
    assert_eq!(room.messages.len(),1,"no accepted agent answer");
    assert_eq!(room.rounds,0,"the model did not deliver an agent response");
    assert_eq!(room.status,RoomStatus::Finished);
    assert_eq!(room.finish_reason,"token_budget_exceeded");
    assert_eq!(room.used_tokens,100,"upstream model already consumed tokens");
    assert_eq!(room.usage_receipts.len(),1,
        "every billed provider completion must have a durable receipt");
    assert!(!room.usage_receipts[0].accepted);
    assert_eq!(room.usage_receipts[0].input_tokens,90);
    assert_eq!(room.usage_receipts[0].output_tokens,10);
    let visible=message_rows(&room);
    assert_eq!(visible.len(),1);
    assert_eq!(visible[0].role,"user");
    assert!(visible[0].metadata.as_ref().is_none_or(|m|m["usage"].is_null()),
        "denied agent tokens must never be attributed to the owner");
    let report=room_usage(&room);
    assert_eq!(report["totalTokens"],100);
    assert_eq!(report["promptTokens"],90);
    assert_eq!(report["completionTokens"],10);
    assert_eq!(report["calls"],1);
    assert_eq!(report["deliveredCalls"],0);
    assert_eq!(report["billedUndeliveredCalls"],1);
    assert_eq!(report["byAgent"][0]["billedUndeliveredCalls"],1);
    assert_eq!(serde_json::from_value::<Room>(serde_json::to_value(&room).unwrap())
        .unwrap().usage_receipts[0].accepted,false);
    server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn room_lifetime_cap_is_independent_from_per_owner_speaker_limit(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let current=engine.groups.get(id).await.unwrap();
    let mut config=current.frontend.unwrap();
    let participants=current.participants;
    config.lifetime_max_responses=12;
    engine.groups.configure(id,config,participants).await.unwrap();
    let first=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"First owner turn".into(),agent_ids:vec![],
        client_message_id:"lifetime-1".into(),reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(first["job"]["status"],"completed");
    assert_eq!(first["job"]["result"]["actualResponses"],9);
    let second=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Second owner turn".into(),agent_ids:vec![],
        client_message_id:"lifetime-2".into(),reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(second["job"]["status"],"interrupted");
    assert_eq!(second["job"]["result"]["actualResponses"],3);
    assert_eq!(second["room"]["finishReason"],"room_lifetime_limit_reached");
    assert_eq!(requests.load(Ordering::SeqCst),12);
    assert_eq!(engine.groups.get(id).await.unwrap().rounds,12);
    server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn named_member_selection_remains_scoped_without_broadcasting_to_all_nine(){
    let (engine,id,requests,server,root)=setup(100,"discussion").await;
    let response=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Only agent-2 should respond".into(),
        agent_ids:vec!["agent-2".into()],client_message_id:"targeted".into(),
        reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(response["job"]["status"],"completed");
    assert_eq!(response["job"]["result"]["expectedResponses"],1);
    let room=engine.groups.get(id).await.unwrap();
    assert_eq!(room.messages.len(),2);
    assert_eq!(room.messages[1].speaker,"agent-2");
    assert_eq!(requests.load(Ordering::SeqCst),1);
    let retry=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Only agent-2 should respond".into(),
        agent_ids:vec!["agent-2".into()],client_message_id:"targeted".into(),
        reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(retry["job"]["status"],"completed");
    assert_eq!(requests.load(Ordering::SeqCst),1);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}


#[tokio::test]
async fn plain_participant_name_inside_owner_prompt_does_not_silence_other_speakers(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let response=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Compare agent-2's earlier finding with the overall plan".into(),
        agent_ids:vec![],client_message_id:"plain-name-all-nine".into(),
        reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(response["job"]["result"]["actualResponses"],9);
    assert_eq!(requests.load(Ordering::SeqCst),9);
    let room=engine.groups.get(id).await.unwrap();
    let distinct=room.messages.iter().filter(|m|m.speaker!="user")
        .map(|m|m.speaker.as_str()).collect::<HashSet<_>>();
    assert_eq!(distinct.len(),9,
        "mentioning a speaker in ordinary task text is not targeting");
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn background_nine_agent_room_returns_before_fanout_and_preserves_same_id_retry(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let first=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"All nine agents: give an independent result".into(),
        agent_ids:vec![],client_message_id:"background-round-1".into(),
        reasoning_language:None
    })).await.unwrap();
    assert_eq!(first.0,StatusCode::ACCEPTED);
    assert_eq!(first.1.0["job"]["status"],"running");
    assert_eq!(first.1.0["job"]["stepTotal"],9);
    // HTTP acceptance must not depend on waiting for nine paid model calls.
    let mut finished=None;
    for _ in 0..150{
        let room=engine.groups.get(id).await.unwrap();
        if room.messages.len()==10 && !engine.groups.frontend_turn_active(id){
            finished=Some(room);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let room=finished.expect("background fanout failed to finish all nine speakers");
    assert_eq!(room.usage_receipts.len(),9);
    assert_eq!(room.messages.iter().filter(|m|m.speaker=="user").count(),1);
    assert_eq!(requests.load(Ordering::SeqCst),9);
    let state_snapshot=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(state_snapshot["status"],"idle");
    assert!(state_snapshot["jobs"].as_array().unwrap().is_empty());
    let repeated=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"All nine agents: give an independent result".into(),
        agent_ids:vec![],client_message_id:"background-round-1".into(),
        reasoning_language:None
    })).await.unwrap();
    assert_eq!(repeated.0,StatusCode::OK);
    assert_eq!(requests.load(Ordering::SeqCst),9,
        "repeating the original clientMessageId must not double-bill inference");
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn active_room_turn_rejects_destructive_frontend_controls_with_conflict(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    engine.groups.say_once(id,"Retain owner request".into(),
        "owner-inflight",vec![]).await.unwrap();
    let lease=engine.groups.try_frontend_turn(id).unwrap();
    let erased=reset(State(engine.clone()),Path(id)).await.unwrap_err();
    assert_eq!(erased.0,StatusCode::CONFLICT);
    let deleted=remove(State(engine.clone()),Path(id)).await.unwrap_err();
    assert_eq!(deleted.0,StatusCode::CONFLICT);
    let stopped=stop(State(engine.clone()),Path(id)).await.unwrap_err();
    assert_eq!(stopped.0,StatusCode::CONFLICT);
    let room=engine.groups.get(id).await.unwrap();
    assert_eq!(room.messages.len(),1);
    assert_eq!(room.status,RoomStatus::Open);
    assert_eq!(requests.load(Ordering::SeqCst),0);
    drop(lease);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn background_room_lease_rejects_overlapping_sends_and_exposes_running_state(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let seeded=engine.groups.say_once(id,"First request".into(),
        "seeded-request",vec![]).await.unwrap();
    assert!(seeded.1);
    let lease=engine.groups.try_frontend_turn(id).unwrap();
    let same=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"First request".into(),agent_ids:vec![],
        client_message_id:"seeded-request".into(),reasoning_language:None
    })).await.unwrap();
    assert_eq!(same.0,StatusCode::ACCEPTED);
    assert_eq!(same.1.0["job"]["status"],"running");
    assert_eq!(engine.groups.get(id).await.unwrap().messages.len(),1,
        "a retry must not create a second durable owner message");
    let wrong_audience=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"First request".into(),agent_ids:vec!["agent-0".into()],
        client_message_id:"seeded-request".into(),reasoning_language:None
    })).await.unwrap_err();
    assert_eq!(wrong_audience.0,StatusCode::CONFLICT,
        "the in-flight idempotency shortcut must not bypass exact owner target binding");
    assert_eq!(requests.load(Ordering::SeqCst),0);
    let overlapping=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"Overlapping owner request".into(),agent_ids:vec![],
        client_message_id:"overlap-1".into(),reasoning_language:None
    })).await.unwrap_err();
    assert_eq!(overlapping.0,StatusCode::CONFLICT);
    let snapshot=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(snapshot["status"],"running");
    assert_eq!(requests.load(Ordering::SeqCst),0);
    drop(lease);
    assert_eq!(state(State(engine.clone()),Path(id)).await.unwrap().0["status"],"idle");
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn third_owner_turn_upstream_400_retains_exact_failed_speaker_and_resumes_to_30_messages(){
    let (engine,id,requests,server,root)=setup_with_failure(100,"discussion",Some(18)).await;
    for round in 0..2{
        let receipt=send(State(engine.clone()),Path(id),Json(SayInput{
            input:format!("Please review stage {}",round+1),
            agent_ids:vec![],client_message_id:format!("round-{}",round+1),
            reasoning_language:None
        })).await.unwrap().0;
        assert_eq!(receipt["job"]["status"],"completed");
        assert_eq!(receipt["job"]["result"]["actualResponses"],9);
    }
    let rejected=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Third owner request: every agent must contribute".into(),
        agent_ids:vec![],client_message_id:"round-3".into(),
        reasoning_language:None
    })).await.err().expect("synthetic Cloudflare 400 must not look completed");
    assert_eq!(rejected.0,StatusCode::BAD_GATEWAY,
        "upstream 400 cannot be reported as an invalid owner input");
    assert_eq!(rejected.1.0["providerStatus"],400);
    assert_eq!(rejected.1.0["providerCode"],5004);
    assert_eq!(rejected.1.0["errorCode"],"upstream_bad_request");
    assert_eq!(rejected.1.0["failedSpeaker"],"agent-0");
    assert_eq!(rejected.1.0["actualResponses"],0);
    assert_eq!(rejected.1.0["expectedResponses"],9);
    let interrupted=engine.groups.get(id).await.unwrap();
    assert_eq!(interrupted.messages.len(),21);
    assert_eq!(interrupted.usage_receipts.len(),19,
        "a pre-inference rejected Run still has a durable non-delivered receipt");
    assert!(!interrupted.usage_receipts[18].accepted);
    assert_eq!(interrupted.used_tokens,1800);
    assert_eq!(interrupted.last_provider_failure.as_ref().unwrap().participant,
        "agent-0");
    let meta=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(meta["status"],"interrupted");
    assert_eq!(meta["usage"]["calls"],19);
    assert_eq!(meta["lastProviderFailure"]["providerStatus"],Value::Null,
        "status field is named http_status for a durable Rust receipt");
    assert_eq!(meta["lastProviderFailure"]["http_status"],400);
    let resumed=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Third owner request: every agent must contribute".into(),
        agent_ids:vec![],client_message_id:"round-3".into(),
        reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(resumed["job"]["status"],"completed");
    assert_eq!(resumed["job"]["result"]["expectedResponses"],9);
    assert_eq!(resumed["job"]["result"]["actualResponses"],9);
    let finished=engine.groups.get(id).await.unwrap();
    assert_eq!(finished.messages.len(),30);
    assert_eq!(finished.usage_receipts.len(),28);
    assert_eq!(finished.usage_receipts.iter().filter(|r|r.accepted).count(),27);
    assert_eq!(finished.used_tokens,2700);
    assert!(finished.last_provider_failure.is_none());
    assert_eq!(finished.messages.iter().filter(|m|m.speaker=="user").count(),3);
    assert_eq!(requests.load(Ordering::SeqCst),28,
        "27 observed completions plus exactly one rejected provider attempt");
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn sync_and_background_share_message_validation_and_targeted_admission(){
    let (engine,id,requests,server,root)=setup(10,"discussion").await;
    let sync_invalid=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"Target one agent".into(),agent_ids:vec!["agent-1".into()],
        client_message_id:"invalid-sync".into(),reasoning_language:Some("unsupported".into())
    })).await.unwrap_err();
    let background_invalid=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"Target one agent".into(),agent_ids:vec!["agent-1".into()],
        client_message_id:"invalid-background".into(),reasoning_language:Some("unsupported".into())
    })).await.unwrap_err();
    assert_eq!(sync_invalid.0,StatusCode::BAD_REQUEST);
    assert_eq!(background_invalid.0,StatusCode::BAD_REQUEST);
    for (is_background,key) in [(false,"bad-sync-target"),(true,"bad-background-target")]{
        let input=SayInput{input:"Target absent agent".into(),
            agent_ids:vec!["not-a-member".into()],client_message_id:key.into(),
            reasoning_language:None};
        let rejected=if is_background{
            send_background(State(engine.clone()),Path(id),Json(input)).await.unwrap_err()
        }else{
            send(State(engine.clone()),Path(id),Json(input)).await.unwrap_err()
        };
        assert_eq!(rejected.0,StatusCode::BAD_REQUEST);
    }
    assert!(engine.groups.get(id).await.unwrap().messages.is_empty());
    assert_eq!(requests.load(Ordering::SeqCst),0);

    let synchronous=send(State(engine.clone()),Path(id),Json(SayInput{
        input:"First targeted response".into(),agent_ids:vec!["agent-1".into()],
        client_message_id:"valid-sync".into(),reasoning_language:None
    })).await.unwrap().0;
    assert_eq!(synchronous["job"]["result"]["actualResponses"],1);
    let background=send_background(State(engine.clone()),Path(id),Json(SayInput{
        input:"Second targeted response".into(),agent_ids:vec!["agent-2".into()],
        client_message_id:"valid-background".into(),reasoning_language:None
    })).await.unwrap();
    assert_eq!(background.0,StatusCode::ACCEPTED);
    let mut completed=false;
    for _ in 0..150{
        let room=engine.groups.get(id).await.unwrap();
        if room.messages.len()==4&&!engine.groups.frontend_turn_active(id){
            assert_eq!(room.messages[1].speaker,"agent-1");
            assert_eq!(room.messages[3].speaker,"agent-2");
            completed=true;break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(completed,"background targeted fanout should finish");
    assert_eq!(requests.load(Ordering::SeqCst),2);
    server.abort();drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}


#[tokio::test]
async fn tool_room_uses_one_native_harness_run_with_source_linked_receipt_and_idempotent_retry(){
    let (engine,id,requests,server,root)=setup(2,"round_robin").await;
    let mut room=engine.groups.get(id).await.unwrap();
    let mut config=room.frontend.take().unwrap();
    config.tools_enabled=true;
    engine.groups.configure(id,config,room.participants.clone()).await.unwrap();
    let send_message=||SayInput{
        input:"Inspect code without changing files".into(),
        agent_ids:vec!["agent-0".into()],client_message_id:"native-owner-1".into(),
        reasoning_language:None
    };
    let first=send(State(engine.clone()),Path(id),Json(send_message())).await.unwrap().0;
    assert_eq!(first["job"]["status"],"completed");
    assert_eq!(requests.load(Ordering::SeqCst),1);
    let persisted=engine.groups.get(id).await.unwrap();
    assert_eq!(persisted.messages.len(),2);
    let receipt=&persisted.usage_receipts[0];
    assert!(receipt.accepted);
    let id_of_run=receipt.origin_run_id.expect("native Room response must have a Run");
    let native=engine.store.get(id_of_run).await.unwrap();
    assert_eq!(native.status,crate::domain::RunStatus::Completed);
    assert_eq!(native.room_binding.as_ref().unwrap().room_id,id);
    assert!(native.read_only);
    assert!(native.events.iter().any(|e|e.kind=="room_context_attached"));
    assert_eq!(native.answer.as_deref(),Some(persisted.messages[1].content.as_str()));
    let event=crate::sdk::Dominion::from_engine(engine.clone())
        .unified_room_events(id,1).await.unwrap();
    assert_eq!(event.len(),1);
    assert_eq!(event[0].detail.as_ref().unwrap()["originRunId"],id_of_run.to_string());
    let room_state=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(room_state["executions"][0]["id"],id_of_run.to_string());
    assert_eq!(room_state["executions"][0]["delivered"],true);
    assert_eq!(persisted.used_tokens,native.input_tokens+native.output_tokens);
    assert_eq!(first["job"]["result"]["actualResponses"],1);
    // The very same owner key must never launch another paid native Run.
    let repeated=send(State(engine.clone()),Path(id),Json(send_message())).await.unwrap().0;
    assert_eq!(repeated["job"]["result"]["actualResponses"],1);
    assert_eq!(requests.load(Ordering::SeqCst),1);
    let settled=engine.groups.settle_native_run(&native).await.unwrap();
    assert_eq!(settled.messages.len(),2);
    assert_eq!(settled.usage_receipts.len(),1);
    server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}


#[tokio::test]
async fn native_room_pending_approval_blocks_new_owner_and_surfaces_same_execution_id(){
    use crate::domain::{CreateRun,Run,RunStatus,ToolCall};
    let (engine,id,_requests,server,root)=setup(2,"round_robin").await;
    let before=engine.groups.get(id).await.unwrap();
    let mut config=before.frontend.clone().unwrap();
    config.tools_enabled=true;
    engine.groups.configure(id,config,before.participants).await.unwrap();
    engine.groups.say_once(id,"First owner instruction".into(),
        "owner-key",vec!["agent-0".into()]).await.unwrap();
    let room=engine.groups.get(id).await.unwrap();
    let (run_id,binding)=room_native::identity(&room,"agent-0").unwrap();
    let mut run=Run::new(CreateRun{instruction:"Inspect using tool".into(),
        budget_krw:Some(100.0),max_steps:Some(2)},2).unwrap();
    run.id=run_id;
    run.room_binding=Some(binding);
    run.provider_id="dominion-native".into();
    run.model_name="mock-room-model".into();
    run.status=RunStatus::AwaitingApproval;
    run.pending_approval=Some(ToolCall{
        id:"read-1".into(),name:"read_file".into(),
        arguments:json!({"path":"a.txt"})
    });
    engine.store.put(run).await.unwrap();
    let err=admit_owner_message(&engine,id,&SayInput{
        input:"Second owner instruction".into(),agent_ids:vec![],
        client_message_id:"different-key".into(),reasoning_language:None
    }).await.unwrap_err();
    assert_eq!(err.0,StatusCode::CONFLICT);
    assert_eq!(engine.groups.get(id).await.unwrap().messages.len(),1);
    let state=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(state["status"],"awaiting_approval");
    assert_eq!(state["approvals"][0]["id"],run_id.to_string());
    assert_eq!(state["approvals"][0]["sessionId"],format!("room-{id}"));
    let reset=reset(State(engine.clone()),Path(id)).await.unwrap_err();
    assert_eq!(reset.0,StatusCode::CONFLICT);
    server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}
