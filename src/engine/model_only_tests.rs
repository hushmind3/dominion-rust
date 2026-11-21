//! Native no-tools chat must use the same Engine worker and cancellation
//! contract as autonomous work. Mock provider = no paid model calls.
use super::*;
use crate::store::Store;
use axum::{extract::State,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};
use tokio::time::{timeout,Duration};

struct Fixture{requests:AtomicUsize,delay_ms:u64}
async fn completion(State(fixture):State<Arc<Fixture>>,Json(payload):Json<Value>)->Json<Value>{
 assert!(payload.get("tools").is_none(),"model-only leaked tool definitions");
 fixture.requests.fetch_add(1,Ordering::SeqCst);
 tokio::time::sleep(Duration::from_millis(fixture.delay_ms)).await;
 Json(json!({"model":"mock-model","choices":[{"message":{"role":"assistant",
    "content":"Verified model-only answer"},"finish_reason":"stop"}],
    "usage":{"prompt_tokens":12,"completion_tokens":4}}))
}
async fn fixture(delay_ms:u64)->(Arc<Engine>,Arc<Fixture>,std::path::PathBuf,tokio::task::JoinHandle<()>) {
 let root=std::env::temp_dir().join(format!("dominion-model-only-{}",Uuid::new_v4()));
 let context=Arc::new(Fixture{requests:AtomicUsize::new(0),delay_ms});
 let app=Router::new().route("/v1/chat/completions",post(completion))
  .with_state(context.clone());
 let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
 let addr=listener.local_addr().unwrap();
 let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
 let mut settings=Settings::from_env().unwrap();
 settings.data_dir=root.join("data");settings.workspace_dir=root.join("workspace");
 settings.main.name="mock-model".into();settings.main.base_url=format!("http://{addr}/v1");
 settings.main.api_key=String::new();
 settings.main.input_krw_per_million=1_000_000.0;
 settings.main.cached_input_krw_per_million=1_000_000.0;
 settings.main.output_krw_per_million=2_000_000.0;
 settings.economy=settings.main.clone();
 let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
 let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
 (Engine::new(settings,store,groups).unwrap(),context,root,server)
}
async fn wait_terminal(engine:&Engine,id:Uuid)->Run{
 let mut changes=engine.store.subscribe();
 timeout(Duration::from_secs(4),async{
  loop{
   let run=engine.store.get(id).await.unwrap();
   if run.terminal(){break run;}
   assert!(engine.store.changed(id,&mut changes).await);
  }
 }).await.unwrap()
}
#[tokio::test]
async fn model_only_chat_keeps_role_preserving_context_but_never_tools(){
 let (engine,requests,root,server)=fixture(0).await;
 let mut run=Run::new(CreateRun{instruction:"latest owner question".into(),
   budget_krw:Some(200.0),max_steps:Some(3)},3).unwrap();
 run.model_only=true;
 run.chat_context=vec![
  json!({"role":"user","content":"stable operator profile","dominion_cache_anchor":true}),
  json!({"role":"user","content":"first question","dominion_cache_anchor":true}),
  json!({"role":"assistant","content":"earlier answer"})
 ];
 let id=run.id;engine.store.put(run).await.unwrap();
 engine.launch(id).await.unwrap();
 let done=wait_terminal(&engine,id).await;
 assert_eq!(done.status,RunStatus::Completed);
 assert_eq!(requests.requests.load(Ordering::SeqCst),1);
 assert_eq!(done.chat_context[2]["role"],"assistant");
 assert_eq!(done.instruction,"latest owner question");
 server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn model_only_chat_uses_native_worker_once_and_records_usage(){
 let (engine,requests,root,server)=fixture(25).await;
 let mut run=Run::new(CreateRun{instruction:"Answer using no tools".into(),
   budget_krw:Some(200.0),max_steps:Some(3)},3).unwrap();
 run.model_only=true;
 let id=run.id;engine.store.put(run).await.unwrap();
 engine.launch(id).await.unwrap();
 let result=wait_terminal(&engine,id).await;
 assert_eq!(result.status,RunStatus::Completed);
 assert_eq!(result.answer.as_deref(),Some("Verified model-only answer"));
 assert_eq!(result.input_tokens,12);
 assert_eq!(result.output_tokens,4);
 assert!((result.cost_krw-20.0).abs()<0.00001);
 assert_eq!(requests.requests.load(Ordering::SeqCst),1);
 assert_eq!(result.events.iter().filter(|e|
   e.kind=="model_only_request_intent").count(),1);
 assert!(!result.events.iter().any(|e|e.kind=="tool_started"));
 server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}
#[tokio::test]
async fn model_only_cancellation_interrupts_provider_without_duplicate_call(){
 let (engine,requests,root,server)=fixture(3500).await;
 let mut run=Run::new(CreateRun{instruction:"Wait for the model".into(),
   budget_krw:Some(100.0),max_steps:Some(2)},2).unwrap();
 run.model_only=true;
 let id=run.id;engine.store.put(run).await.unwrap();
 engine.launch(id).await.unwrap();
 timeout(Duration::from_secs(2),async{
  while requests.requests.load(Ordering::SeqCst)==0{
   tokio::time::sleep(Duration::from_millis(10)).await;
  }
 }).await.unwrap();
 engine.cancel(id).await.unwrap();
 let stopped=wait_terminal(&engine,id).await;
 assert_eq!(stopped.status,RunStatus::Cancelled);
 assert!(stopped.answer.is_none());
 assert_eq!(requests.requests.load(Ordering::SeqCst),1);
 timeout(Duration::from_secs(2),async{
  while engine.active.lock().await.contains(&id){
   engine.active_notify.notified().await;
  }
 }).await.unwrap();
 server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn model_only_reserved_budget_failure_cannot_become_completed(){
 let (engine,requests,root,server)=fixture(0).await;
 let mut run=Run::new(CreateRun{instruction:"Budget-constrained answer".into(),
   budget_krw:Some(20.0),max_steps:Some(2)},2).unwrap();
 run.model_only=true;
 // Model receipt is 20 KRW, but delegated reservations leave only 8 KRW.
 // account() must win over the later success branch.
 run.reserved_budget_krw=12.0;
 let id=run.id;engine.store.put(run).await.unwrap();
 engine.launch(id).await.unwrap();
 let result=wait_terminal(&engine,id).await;
 assert_eq!(result.status,RunStatus::Failed);
 assert!(result.answer.is_none());
 assert!((result.cost_krw-20.0).abs()<0.00001);
 assert_eq!(requests.requests.load(Ordering::SeqCst),1);
 assert!(result.events.iter().any(|e|e.kind=="budget_exceeded"));
 assert!(!result.events.iter().any(|e|e.kind=="completed"));
 server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn model_only_uncertain_request_is_never_replayed_after_restart(){
 let (engine,requests,root,server)=fixture(0).await;
 let mut run=Run::new(CreateRun{instruction:"Do not charge twice".into(),
   budget_krw:Some(100.0),max_steps:Some(2)},2).unwrap();
 run.model_only=true;
 // The process died after its durable send-intent and before receiving any
 // model receipt. A resumed scheduler must not send a second paid request.
 run.status=RunStatus::Paused;
 run.event("model_only_request_intent","uncertain provider request",None);
 let id=run.id;engine.store.put(run).await.unwrap();
 let error=engine.launch(id).await.unwrap_err();
 assert!(error.contains("reconcile"),"{error}");
 assert_eq!(requests.requests.load(Ordering::SeqCst),0);
 let persisted=engine.store.get(id).await.unwrap();
 assert_eq!(persisted.status,RunStatus::Paused);
 assert!(persisted.events.iter().any(|e|e.kind=="model_only_request_intent"));
 server.abort();drop(engine);tokio::fs::remove_dir_all(root).await.unwrap();
}

async fn refusing_agent_provider(
    State(fixture):State<Arc<Fixture>>,
    Json(_request):Json<Value>
)->axum::http::StatusCode{
    fixture.requests.fetch_add(1,Ordering::SeqCst);
    axum::http::StatusCode::SERVICE_UNAVAILABLE
}

#[tokio::test]
async fn agent_http_503_durable_intent_blocks_duplicate_billable_retry(){
    let root=std::env::temp_dir()
        .join(format!("dominion-agent-uncertain-{}",Uuid::new_v4()));
    let observed=Arc::new(Fixture{
        requests:AtomicUsize::new(0),delay_ms:0
    });
    let app=Router::new()
        .route("/v1/chat/completions",post(refusing_agent_provider))
        .with_state(Arc::clone(&observed));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0")
        .await.unwrap();
    let addr=listener.local_addr().unwrap();
    let server=tokio::spawn(async move {
        axum::serve(listener,app).await.unwrap();
    });
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.name="mock-agent-model".into();
    settings.main.base_url=format!("http://{addr}/v1");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs"))
        .await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(
        settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
    let run=Run::new(CreateRun{
        instruction:"one paid agent inference; do not retry unknown outcome".into(),
        budget_krw:Some(500.0),max_steps:Some(3)
    },3).unwrap();
    let id=run.id;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut changes=store.subscribe();
    let paused=timeout(Duration::from_secs(8),async {
        loop{
            let run=store.get(id).await.unwrap();
            if run.status==RunStatus::Paused{break run;}
            assert!(store.changed(id,&mut changes).await);
        }
    }).await.unwrap();
    assert_eq!(observed.requests.load(Ordering::SeqCst),1);
    assert!(crate::journal::unsettled_model_request(&paused).is_some());
    assert_eq!(paused.events.iter()
        .filter(|e|e.kind=="model_request_intent").count(),1);
    assert!(!paused.events.iter()
        .any(|e|e.kind=="model_request_settled"));
    assert!(engine.launch(id).await.is_err(),
        "failed upstream response does not prove the provider was unbilled");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(observed.requests.load(Ordering::SeqCst),1,
        "the same billable agent request must never be submitted twice");
    let reloaded=Store::open(root.join("data/runs")).await.unwrap();
    let recovered=reloaded.get(id).await.unwrap();
    assert_eq!(recovered.status,RunStatus::Paused);
    assert!(crate::journal::unsettled_model_request(&recovered).is_some());
    drop(reloaded);
    server.abort();drop(engine);drop(store);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
