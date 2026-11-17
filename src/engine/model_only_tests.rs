//! Native no-tools chat must use the same Engine worker and cancellation
//! contract as autonomous work. Mock provider = no paid model calls.
use super::*;
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
