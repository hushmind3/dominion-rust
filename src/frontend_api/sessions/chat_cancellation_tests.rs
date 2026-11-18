//! Real frontend chat -> native Run -> in-flight HTTP inference cancellation.
//! The first provider request deliberately never responds until released;
 //! Stop must commit a terminal Run without accepting its late answer.
use super::*;
use crate::{config::Settings,group::Groups,store::Store,domain::{Run,CreateRun,ToolCall}};
use axum::{extract::State as MockState,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};
use tokio::{sync::Notify,time::{Duration,timeout}};

struct Inference {
    calls:AtomicUsize,
    first_released:Notify,
}
async fn provider(MockState(state):MockState<Arc<Inference>>,
    Json(_body):Json<Value>)->Json<Value>{
    let call=state.calls.fetch_add(1,Ordering::SeqCst);
    if call==0{
        state.first_released.notified().await;
        // If cancellation dropped the HTTP future, this answer cannot win
        // over the persisted cancelled Run, even after arriving late.
    }
    let text=if call==0{"STALE ANSWER MUST NOT SAVE"}else{"SECOND REQUEST OK"};
    Json(json!({"choices":[{"finish_reason":"stop",
        "message":{"role":"assistant","content":text}}],
        "usage":{"prompt_tokens":11,"completion_tokens":5}}))
}
fn input(session_id:&str,value:&str)->super::super::chat::ChatInput{
    super::super::chat::ChatInput{
        session_id:session_id.into(),agent_id:"general".into(),input:value.into(),
        provider_id:None,model:None,tools_enabled:false,
        tools_execution_enabled:false,auto_learning:false,
        reasoning_effort:None,reasoning_language:None,thinking:None
    }
}
async fn fixture()->(Arc<Engine>,Arc<Inference>,String,
    tokio::task::JoinHandle<()>,std::path::PathBuf){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr=listener.local_addr().unwrap();
    let remote=Arc::new(Inference{calls:AtomicUsize::new(0),
        first_released:Notify::new()});
    let server=tokio::spawn({
        let state=remote.clone();
        async move{
            axum::serve(listener,Router::new().route("/chat/completions",post(provider))
                .with_state(state)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-chat-stop-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{addr}");
    settings.main.name="mock-slow-provider".into();
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings,store,groups).unwrap();
    let id=Uuid::new_v4().to_string();
    engine.frontend.mutate(|snapshot|{
        snapshot.sessions.insert(id.clone(),UiSession{
            id:id.clone(),title:"interruptible chat".into(),
            agent_id:"general".into(),provider_id:"dominion-native".into(),
            model:"mock-slow-provider".into(),created_at:timestamp(now_ms()),
            updated_at:timestamp(now_ms()),room_id:None,pending_run:None,
            messages:Vec::new()
        });
        Ok(())
    }).await.unwrap();
    (engine,remote,id,server,root)
}

#[tokio::test]
async fn stop_preempts_silent_model_http_and_allows_next_same_session_message(){
    let (engine,remote,id,server,root)=fixture().await;
    let _first_sse=super::super::chat::chat(State(engine.clone()),
        Json(input(&id,"First instruction with slow model"))).await.unwrap();
    timeout(Duration::from_secs(5),async{
        while remote.calls.load(Ordering::SeqCst)<1{
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("actual upstream model HTTP request did not start");
    let first_id=engine.frontend.session(&id).await.unwrap()
        .pending_run.expect("first chat not bound to native Run");
    let receipt=cancel_session(State(engine.clone()),Path(id.clone()))
        .await.unwrap().0;
    assert_eq!(receipt["cancelled"],true);
    assert_eq!(receipt["status"],"cancelled");
    assert_eq!(receipt["pending"],false);
    assert_eq!(receipt["runId"],first_id.to_string());
    assert_eq!(engine.store.get(first_id).await.unwrap().status,RunStatus::Cancelled);
    let settled=engine.frontend.session(&id).await.unwrap();
    assert_eq!(settled.pending_run,None,
        "chat should be reusable immediately after durable model cancellation");
    assert_eq!(settled.messages.len(),1);
    assert_eq!(settled.messages[0].role,"user");
    remote.first_released.notify_waiters();
    // A late model completion cannot invent an assistant or change the
    // cancelled journal while the next Run is being created.
    let _second_sse=super::super::chat::chat(State(engine.clone()),
        Json(input(&id,"Second message should complete"))).await.unwrap();
    let second_id=engine.frontend.session(&id).await.unwrap()
        .pending_run.expect("second chat did not start");
    assert_ne!(first_id,second_id);
    timeout(Duration::from_secs(5),async{
        loop{
            let _=sync_session(&engine,&id).await;
            if engine.frontend.session(&id).await.unwrap().pending_run.is_none(){
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("second chat failed to settle");
    let final_run=engine.store.get(second_id).await.unwrap();
    assert_eq!(final_run.status,RunStatus::Completed);
    assert_eq!(final_run.answer.as_deref(),Some("SECOND REQUEST OK"));
    assert_eq!(final_run.input_tokens,11);
    assert_eq!(final_run.output_tokens,5);
    let messages=engine.frontend.session(&id).await.unwrap().messages;
    assert_eq!(messages.len(),3);
    assert_eq!(messages[0].content,"First instruction with slow model");
    assert_eq!(messages[1].content,"Second message should complete");
    assert_eq!(messages[2].content,"SECOND REQUEST OK");
    assert!(!messages.iter().any(|m|m.content.contains("STALE ANSWER")));
    assert_eq!(engine.store.get(first_id).await.unwrap().status,RunStatus::Cancelled);
    assert!(engine.store.get(first_id).await.unwrap().answer.is_none());
    assert_eq!(remote.calls.load(Ordering::SeqCst),2);
    for _ in 0..100{
        if !engine.worker_active_for_test(first_id).await && !engine.worker_active_for_test(second_id).await{break;}
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!engine.worker_active_for_test(first_id).await && !engine.worker_active_for_test(second_id).await,
        "in-flight provider future must exit the native worker after cancellation");
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn mutating_tool_in_flight_keeps_pending_reconciliation_and_blocks_next_chat(){
    let (engine,_remote,id,server,root)=fixture().await;
    let mut run=Run::new(CreateRun{
        instruction:"Edit a file".into(),budget_krw:Some(15.0),
        max_steps:Some(5)},5).unwrap();
    run.status=RunStatus::Running;
    run.in_flight=Some(ToolCall{
        id:"write-1".into(),name:"write_file".into(),
        arguments:json!({"path":"result.txt","content":"new"})
    });
    let run_id=run.id;
    engine.store.put(run).await.unwrap();
    engine.frontend.mutate(|snapshot|{
        snapshot.sessions.get_mut(&id).unwrap().pending_run=Some(run_id);
        Ok(())
    }).await.unwrap();
    let receipt=cancel_session(State(engine.clone()),Path(id.clone()))
        .await.unwrap().0;
    assert_eq!(receipt["cancelled"],false);
    assert_eq!(receipt["pending"],true);
    assert_eq!(receipt["status"],"paused");
    let run=engine.store.get(run_id).await.unwrap();
    assert_eq!(run.status,RunStatus::Paused);
    assert!(run.cancel_after_tool);
    assert!(run.in_flight.is_some());
    assert_eq!(engine.frontend.session(&id).await.unwrap().pending_run,Some(run_id));
    let err=super::super::chat::chat(State(engine.clone()),
        Json(input(&id,"This message must not overtake an unsafe write")))
        .await.err().expect("a pending write must block the next chat");
    assert_eq!(err.0,axum::http::StatusCode::CONFLICT);
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
