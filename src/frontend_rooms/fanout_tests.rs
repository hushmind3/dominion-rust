//! Endpoint-level 9-speaker discussion regression. A fake provider models
//! real request/usage shape; every utterance is generated through the actual
//! Rust Groups + registered frontend API path, not invented transcript rows.
use super::*;
use crate::{
    config::Settings,domain::CreateRun,group::{Groups,NewRoom,Participant},
    store::Store,
};
use axum::{extract::State as MockState,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};

async fn model(MockState(requests):MockState<Arc<AtomicUsize>>,
    Json(_input):Json<Value>)->Json<Value>{
    let index=requests.fetch_add(1,Ordering::SeqCst);
    // A single agent should not end a 9-speaker group early by emitting [END].
    let content=if index%9==0 {
        format!("[END] First participant said finished in turn {}",index/9+1)
    }else{
        format!("Agent reply {index}")
    };
    Json(json!({"choices":[{"finish_reason":"stop",
        "message":{"role":"assistant","content":content}}],
        "usage":{"prompt_tokens":90,"completion_tokens":10}}))
}
async fn setup(max_turns:usize,routing_mode:&str)
    ->(Arc<Engine>,Uuid,Arc<AtomicUsize>,tokio::task::JoinHandle<()>,std::path::PathBuf){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let requests=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let counter=requests.clone();
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(model)).with_state(counter))
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
    let (engine,id,requests,server,root)=setup(100,"discussion").await;
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
    assert_eq!(room.usage_receipts.len(),27);
    assert_eq!(room.used_tokens,2700);
    assert_eq!(room.usage_receipts.iter().map(|r|r.input_tokens).sum::<u64>(),2430);
    assert_eq!(room.usage_receipts.iter().map(|r|r.output_tokens).sum::<u64>(),270);
    assert_eq!(requests.load(Ordering::SeqCst),27);
    let report=state(State(engine.clone()),Path(id)).await.unwrap().0;
    assert_eq!(report["usage"]["calls"],27);
    assert_eq!(report["usage"]["promptTokens"],2430);
    assert_eq!(report["usage"]["completionTokens"],270);
    assert_eq!(report["usage"]["byAgent"].as_array().unwrap().len(),9);
    assert!(report["usage"]["byAgent"].as_array().unwrap().iter()
        .all(|row|row["calls"]==3 && row["promptTokens"]==270));
    let rows=message_rows(&room);
    assert_eq!(rows.len(),30);
    assert_eq!(rows[1].metadata.as_ref().unwrap()["usage"]["totalTokens"],100);

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
    assert_eq!(response["room"]["status"],"interrupted");
    assert_eq!(response["room"]["finishReason"],"max_turns_reached");
    assert_eq!(requests.load(Ordering::SeqCst),4);
    assert_eq!(engine.groups.get(id).await.unwrap().messages.len(),5);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
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
