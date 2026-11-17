//! Deterministic coding workload: one missing-file failure, corrected reads,
//! two edited source files, and a final verified artifact. The model is fake;
//! this tests kernel behavior rather than claiming frontier model capability.
use super::*;
use axum::{extract::State,routing::post,Json,Router};
use std::sync::atomic::{AtomicUsize,Ordering};

async fn repair_model(State(turn):State<Arc<AtomicUsize>>,Json(request):Json<Value>)->Json<Value>{
    let step=turn.fetch_add(1,Ordering::SeqCst);
    let message=match step{
        0=>json!({"role":"assistant","content":
            "1. Inspect both source files. 2. Fix alpha and beta. 3. Verify artifacts."}),
        1=>json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"wrong-path","type":"function","function":{"name":"read_file",
                "arguments":"{\"path\":\"src/absent.rs\"}"}
        }]}),
        2=>{
            let saw_failure=request["messages"].as_array().is_some_and(|messages|
                messages.iter().any(|m|m["role"]=="tool"
                    &&m["tool_call_id"]=="wrong-path"
                    &&m["content"].as_str().is_some_and(|text|text.starts_with("TOOL ERROR:"))));
            if !saw_failure{
                json!({"role":"assistant","content":"FAIL: tool failure not in model context"})
            }else{
                json!({"role":"assistant","content":null,"tool_calls":[
                    {"id":"inspect-alpha","type":"function","function":{"name":"read_file",
                        "arguments":"{\"path\":\"src/alpha.rs\"}"}},
                    {"id":"inspect-beta","type":"function","function":{"name":"read_file",
                        "arguments":"{\"path\":\"src/beta.rs\"}"}}
                ]})
            }
        },
        3=>json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"repair-alpha","type":"function","function":{"name":"write_file",
                "arguments":"{\"path\":\"src/alpha.rs\",\"content\":\"pub fn alpha()->i32 { 1 }\\n\"}"}
        }]}),
        4=>json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"repair-beta","type":"function","function":{"name":"write_file",
                "arguments":"{\"path\":\"src/beta.rs\",\"content\":\"pub fn beta()->i32 { 2 }\\n\"}"}
        }]}),
        5=>json!({"role":"assistant","content":"Repaired alpha and beta source files."}),
        _=>json!({"role":"assistant","content":"PASS"})
    };
    Json(json!({"choices":[{"message":message}],"usage":{
        "prompt_tokens":40,"completion_tokens":12}}))
}
#[tokio::test]
async fn coding_fixture_repairs_two_files_after_missing_path_feedback(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let model_turns=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let count=Arc::clone(&model_turns);
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(repair_model))
                .with_state(count)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-coding-matrix-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.main.parallel_tool_calls=true;
    settings.economy=settings.main.clone();
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let run=Run::new(CreateRun{
        instruction:"Fix alpha() to return 1 and beta() to return 2; use observed tool errors".into(),
        budget_krw:Some(1000.0),max_steps:Some(16)
    },16).unwrap();
    let id=run.id;
    let folder=settings.workspace_dir.join(id.to_string()).join("src");
    tokio::fs::create_dir_all(&folder).await.unwrap();
    tokio::fs::write(folder.join("alpha.rs"),"pub fn alpha()->i32 { 0 }\n").await.unwrap();
    tokio::fs::write(folder.join("beta.rs"),"pub fn beta()->i32 { 0 }\n").await.unwrap();
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut done=None;
    for _ in 0..160{
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let run=store.get(id).await.unwrap();
        if run.terminal(){done=Some(run);break;}
    }
    let run=done.expect("mock coding fixture timed out");
    assert_eq!(run.status,RunStatus::Completed,"events: {:?}",run.events);
    assert_eq!(tokio::fs::read_to_string(folder.join("alpha.rs")).await.unwrap(),
        "pub fn alpha()->i32 { 1 }\n");
    assert_eq!(tokio::fs::read_to_string(folder.join("beta.rs")).await.unwrap(),
        "pub fn beta()->i32 { 2 }\n");
    let tool_ids=run.messages.iter().filter(|v|v["role"]=="tool")
        .filter_map(|v|v["tool_call_id"].as_str()).collect::<Vec<_>>();
    assert_eq!(tool_ids,vec![
        "wrong-path","inspect-alpha","inspect-beta","repair-alpha","repair-beta"
    ]);
    assert!(run.events.iter().any(|e|e.kind=="verification"));
    assert!(model_turns.load(Ordering::SeqCst)>=6);
    for _ in 0..100{
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server.abort();
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn connected_github_commit_requires_approval_before_external_dispatch(){
    let root=std::env::temp_dir().join(format!("dominion-github-approval-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
    let mut run=Run::new(CreateRun{
        instruction:"Update an existing GitHub file after explicit approval".into(),
        budget_krw:Some(250.0),max_steps:Some(4)
    },4).unwrap();
    run.plan=Some("Only the queued GitHub commit remains".into());
    let id=run.id;
    run.queued_calls.push(ToolCall{
        id:"github-commit-1".into(),name:"github_commit_file".into(),
        arguments:json!({
            "owner":"test","repo":"test","branch":"feature/fix",
            "path":"src/main.rs","content":"new text",
            "expected_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "message":"update source"
        })
    });
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut paused=None;
    for _ in 0..100{
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let state=store.get(id).await.unwrap();
        if state.status==RunStatus::AwaitingApproval{paused=Some(state);break;}
    }
    let state=paused.expect("GitHub write did not stop at approval boundary");
    assert_eq!(state.pending_approval.as_ref().unwrap().name,"github_commit_file");
    assert!(state.in_flight.is_none());
    assert_eq!(state.events.iter().filter(|ev|ev.kind=="tool_started").count(),0);
    engine.cancel(id).await.unwrap();
    assert_eq!(store.get(id).await.unwrap().status,RunStatus::Cancelled);
    for _ in 0..100{
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    drop(engine);drop(store);
    tokio::fs::remove_dir_all(root).await.unwrap();
}


#[tokio::test]
async fn github_pr_and_google_docs_mutations_require_approval_before_external_network(){
 for (name,args) in [
  ("github_create_pull_request",json!({
   "owner":"demo","repo":"demo","title":"Proposed change",
   "head":"fix/case","base":"main","body":"Evidence"
  })),
  ("google_docs_create",json!({"title":"New document"}))
 ]{
  let root=std::env::temp_dir().join(format!("dominion-connection-approval-{}",Uuid::new_v4()));
  let mut settings=Settings::from_env().unwrap();
  settings.data_dir=root.join("data");settings.workspace_dir=root.join("workspace");
  let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
  let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
  let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
  let mut run=Run::new(CreateRun{
   instruction:format!("External account tool {name}"),budget_krw:Some(50.0),max_steps:Some(4)
  },4).unwrap();
  run.plan=Some("Use only the queued account tool".into());
  let id=run.id;
  let call=ToolCall{id:"external-approved-1".into(),name:name.into(),arguments:args};
  run.advertised_tools=crate::connected_tools::definitions(&engine.settings);
  // No OAuth account is configured, and the advertised snapshot is empty:
  // durable approval must be requested before any attempt at dispatch.
  run.advertised_tools.clear();
  run.queued_calls.push(call);
  store.put(run).await.unwrap();
  engine.launch(id).await.unwrap();
  let state=tokio::time::timeout(std::time::Duration::from_secs(3),async{
   loop{
    let state=store.get(id).await.unwrap();
    if state.status==RunStatus::AwaitingApproval{break state;}
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
   }
  }).await.unwrap();
  assert_eq!(state.pending_approval.as_ref().unwrap().name,name);
  assert!(state.in_flight.is_none());
  assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),0);
  engine.cancel(id).await.unwrap();
  drop(engine);drop(store);
  tokio::fs::remove_dir_all(root).await.unwrap();
 }
}
