//! Deterministic coding workload: one missing-file failure, corrected reads,
//! two edited source files, and a final verified artifact. The model is fake;
//! this tests kernel behavior rather than claiming frontier model capability.
use super::*;
use crate::store::Store;
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
            if !saw_failure {
                json!({"role":"assistant","content":"FAIL: durable failed tool receipt must reach model"})
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
    // Regression: real compatible gateways may use Responses-style counters.
    // This must be billed by the same native Run ledger as ordinary tools.
    Json(json!({"choices":[{"message":message}],"usage":{
        "input_tokens":40,"output_tokens":12,
        "input_tokens_details":{"cached_tokens":15}}}))
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
        settings.separate_plan_model_call=true;
        settings.separate_model_audit_enabled=true;
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
    let metered=run.events.iter().filter(|e|e.kind=="model").collect::<Vec<_>>();
    assert!(metered.len()>=6);
    assert!(run.input_tokens>=240,"mock gateway usage must not be recorded as 0");
    assert!(run.output_tokens>=72);
    assert!(run.cached_input_tokens>=90);
    assert!(metered.iter().all(|e|e.detail.as_ref()
        .and_then(|d|d.get("usage_reported")).and_then(Value::as_bool)==Some(true)),
        "every coding-model request must have a real upstream usage receipt");
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
        settings.separate_plan_model_call=true;
        settings.separate_model_audit_enabled=true;
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
        settings.separate_plan_model_call=true;
        settings.separate_model_audit_enabled=true;
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
  // The model-turn schema is frozen; provider credentials are not needed
  // to prove approval happens before any network dispatch.
  run.advertised_tools=if name.starts_with("google_"){
   crate::google_workspace::definitions()
  }else{crate::github_collab::definitions()};
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

/// A complete read-program job uses ONE tool-capable model turn plus ONE final
/// answer turn. Old default path added a separate plan and auditor turn.
/// The mock exposes protocol behavior, not external model quality.
async fn fused_read_program_model(
    State(count):State<Arc<AtomicUsize>>,Json(request):Json<Value>
)->Json<Value>{
    let turn=count.fetch_add(1,Ordering::SeqCst);
    let message=match turn{
        0=>{
            assert!(request["tools"].as_array().is_some_and(|tools|
                tools.iter().any(|t|t.pointer("/function/name").and_then(Value::as_str)
                    ==Some("read_program"))));
            json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"program-inspection-1","type":"function","function":{
                    "name":"read_program",
                    "arguments":json!({"steps":[
                        {"id":"scan","tool":"search_files",
                            "arguments":{"query":"signal=present"}},
                        {"id":"target","tool":"read_file",
                            "arguments":{"path":{"from":"scan","path":"/value/0/file"}},
                            "when":{"step":"scan","status":"ok",
                                "predicate":{"type":"all","predicates":[
                                    {"type":"equals","from":"scan","path":"/status",
                                        "value":"ok"},
                                    {"type":"compare","from":"scan","path":"/value/0/line",
                                        "op":"gte","value":1}
                                ]}}}
                    ]}).to_string()
                }
            }]})
        },
        1=>{
            let observed=request["messages"].as_array().is_some_and(|messages|
                messages.iter().any(|m|m["role"]=="tool"
                    &&m["tool_call_id"]=="program-inspection-1"
                    &&m["content"].as_str().is_some_and(|s|
                        s.contains("signal=present")&&s.contains("nested_read_calls"))));
            assert!(observed,"one durable program tool result must provide real read evidence");
            json!({"role":"assistant","content":"Observed signal=present in target.txt."})
        },
        _=>json!({"role":"assistant","content":"Unexpected additional paid model turn"})
    };
    Json(json!({"choices":[{"message":message}],
        "usage":{"prompt_tokens":26,"completion_tokens":8}}))
}
#[tokio::test]
async fn read_program_fuses_planning_and_skips_duplicate_model_audit(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let turns=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let count=Arc::clone(&turns);
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(fused_read_program_model))
                .with_state(count)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-fused-program-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let run=Run::new(CreateRun{
        instruction:"Inspect target.txt and report observed signal".into(),
        budget_krw:Some(500.0),max_steps:Some(5)
    },5).unwrap();
    let id=run.id;
    let folder=settings.workspace_dir.join(id.to_string());
    tokio::fs::create_dir_all(&folder).await.unwrap();
    tokio::fs::write(folder.join("target.txt"),"signal=present\n").await.unwrap();
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut completed=None;
    for _ in 0..100{
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let state=store.get(id).await.unwrap();
        if state.terminal(){completed=Some(state);break;}
    }
    let result=completed.expect("fused read-program did not finish");
    assert_eq!(result.status,RunStatus::Completed,"events={:?}",result.events);
    assert_eq!(turns.load(Ordering::SeqCst),2,
        "exactly one program decision and one final answer; no plan/auditor model call");
    assert_eq!(result.steps,2);
    assert!(result.events.iter().any(|e|e.kind=="planning_fused"));
    assert!(!result.events.iter().any(|e|e.kind=="verification"));
    assert_eq!(result.events.iter().filter(|e|e.kind=="tool_started").count(),1);
    assert_eq!(result.events.iter().filter(|e|e.kind=="tool_result").count(),1);
    assert!(result.answer.unwrap().contains("signal=present"));
    for _ in 0..100{
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server.abort();
    tokio::fs::remove_dir_all(root).await.unwrap();
}


/// Programmatic Tool Calling integration: ONE model request issues a mixed
/// read/write program. Subsequent steps are executed by the original Rust
/// journal without additional model turns. A second model request finishes.
async fn composed_program_mock(
    State(counter):State<Arc<AtomicUsize>>,Json(request):Json<Value>
)->Json<Value>{
    let round=counter.fetch_add(1,Ordering::SeqCst);
    let message=match round{
        0=>{
            assert!(request["tools"].as_array().is_some_and(|xs|
                xs.iter().any(|x|x.pointer("/function/name").and_then(Value::as_str)
                    ==Some("run_program"))));
            json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"integrated-program-1","type":"function","function":{
                    "name":"run_program",
                    "arguments":json!({"steps":[
                        {"id":"write","tool":"write_file",
                            "arguments":{"path":"result.rs","content":"pub fn old()->i32{ 1 }\n"}},
                        {"id":"parallel_inspection","tool":"read_program",
                            "arguments":{"steps":[
                                {"id":"files","tool":"list_files","arguments":{"path":""}},
                                {"id":"inspect","tool":"read_file","arguments":{"path":"result.rs"}}
                            ]}},
                        {"id":"locate","tool":"search_files",
                            "arguments":{"query":"pub fn old"}},
                        {"id":"inspect","tool":"read_file",
                            "arguments":{"path":{"from":"locate","path":"/value/0/file"}},
                            "when":{"type":"equals","from":"locate",
                                "path":"/status","value":"ok"}},
                        {"id":"edit","tool":"edit_file",
                            "arguments":{"path":{"from":"locate","path":"/value/0/file"},
                                "old_text":"old()->i32{ 1 }",
                                "new_text":"new()->i32{ 42 }"},
                            "when":{"type":"all","predicates":[
                                {"type":"equals","from":"inspect","path":"/status","value":"ok"},
                                {"type":"equals","from":"locate","path":"/status","value":"ok"}
                            ]}},
                        {"id":"verify","tool":"read_file",
                            "arguments":{"path":{"from":"locate","path":"/value/0/file"}}}
                    ]}).to_string()
                }
            }]})
        },
        1=>{
            let messages=request["messages"].as_array().unwrap();
            assert!(messages.iter().any(|m|m["role"]=="tool"
                &&m["tool_call_id"]=="program:integrated-program-1:edit"
                &&m["content"].as_str().is_some_and(|s|s.contains("edited"))));
            assert!(messages.iter().any(|m|m["role"]=="tool"
                &&m["tool_call_id"]=="program:integrated-program-1:parallel_inspection"
                &&m["content"].as_str().is_some_and(|s|
                    s.contains("model_calls_inside")&&s.contains("execution_waves"))));
            assert!(messages.iter().any(|m|m["role"]=="tool"
                &&m["tool_call_id"]=="program:integrated-program-1:verify"
                &&m["content"].as_str().is_some_and(|s|s.contains("new()->i32{ 42 }"))));
            json!({"role":"assistant","content":"Updated result.rs and verified new()->i32{ 42 }."})
        },
        _=>json!({"role":"assistant","content":"Unexpected extra model call"})
    };
    Json(json!({"choices":[{"message":message}],
        "usage":{"prompt_tokens":30,"completion_tokens":11}}))
}
#[tokio::test]
async fn programmatic_tool_calls_write_edit_search_verify_via_existing_journal(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let turns=Arc::new(AtomicUsize::new(0));
    let server=tokio::spawn({
        let turns=Arc::clone(&turns);
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(composed_program_mock))
                .with_state(turns)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-integrated-program-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let run=Run::new(CreateRun{
        instruction:"Create, find, inspect, edit and verify result.rs".into(),
        budget_krw:Some(500.0),max_steps:Some(6)
    },6).unwrap();
    let id=run.id;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut finished=None;
    for _ in 0..160{
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let state=store.get(id).await.unwrap();
        if state.terminal(){finished=Some(state);break;}
    }
    let result=finished.expect("native program did not finish");
    assert_eq!(result.status,RunStatus::Completed,"events={:?}",result.events);
    assert_eq!(turns.load(Ordering::SeqCst),2,
        "six native program steps plus nested parallel reads must use two model requests");
    assert!(result.program_session.is_none());
    assert_eq!(result.steps,2);
    assert_eq!(result.events.iter().filter(|e|e.kind=="program_step_ready").count(),6);
    assert_eq!(result.events.iter().filter(|e|e.kind=="program_observed").count(),6);
    assert_eq!(result.events.iter().filter(|e|e.kind=="tool_result").count(),7);
    assert!(result.events.iter().any(|e|e.kind=="program_completed"));
    assert_eq!(tokio::fs::read_to_string(settings.workspace_dir
        .join(id.to_string()).join("result.rs")).await.unwrap(),
        "pub fn new()->i32{ 42 }\n");
    let persisted=Store::open(root.join("data/runs")).await.unwrap().get(id).await.unwrap();
    assert!(persisted.events.iter().any(|e|e.kind=="program_completed"));
    for _ in 0..100{
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server.abort();
    tokio::fs::remove_dir_all(root).await.unwrap();
}


async fn approval_program_mock(State(counter):State<Arc<AtomicUsize>>,
    Json(_request):Json<Value>)->Json<Value>{
    let round=counter.fetch_add(1,Ordering::SeqCst);
    let message=match round{
        0=>json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"approval-program","type":"function","function":{
                "name":"run_program","arguments":json!({"steps":[
                    {"id":"command","tool":"shell",
                        "arguments":{"command":"touch forbidden-by-approval.txt"}}
                ]}).to_string()
            }
        }]}),
        _=>json!({"role":"assistant","content":
            "Approval was denied; the shell command was not executed."})
    };
    Json(json!({"choices":[{"message":message}],
        "usage":{"prompt_tokens":18,"completion_tokens":5}}))
}
#[tokio::test]
async fn programmatic_shell_step_cannot_skip_existing_human_approval(){
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let turns=Arc::new(AtomicUsize::new(0));
    let fixture=tokio::spawn({
        let count=Arc::clone(&turns);
        async move{
            axum::serve(listener,Router::new()
                .route("/chat/completions",post(approval_program_mock))
                .with_state(count)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-program-approval-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.main.base_url=format!("http://{address}");
    settings.main.api_key.clear();
    settings.economy=settings.main.clone();
    settings.shell_enabled=true;
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let run=Run::new(CreateRun{instruction:"Propose a shell command".into(),
        budget_krw:Some(500.0),max_steps:Some(5)},5).unwrap();
    let id=run.id;
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let mut approval=None;
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let state=store.get(id).await.unwrap();
        if state.status==RunStatus::AwaitingApproval {approval=Some(state);break;}
    }
    let awaiting=approval.expect("shell program step must wait for owner");
    assert_eq!(awaiting.pending_approval.as_ref().unwrap().name,"shell");
    assert!(awaiting.program_session.is_some());
    assert_eq!(awaiting.events.iter()
        .filter(|ev|ev.kind=="tool_started"
            &&ev.detail.as_ref().is_some_and(|d|d["name"]=="shell")).count(),0);
    assert!(tokio::fs::metadata(settings.workspace_dir.join(id.to_string())
        .join("forbidden-by-approval.txt")).await.is_err());
    engine.approve(id,false).await.unwrap();
    let mut completed=None;
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let state=store.get(id).await.unwrap();
        if state.terminal(){completed=Some(state);break;}
    }
    let state=completed.expect("rejected program step did not settle");
    assert_eq!(state.status,RunStatus::Completed);
    assert!(state.events.iter().any(|ev|ev.kind=="tool_denied"));
    assert!(state.events.iter().any(|ev|ev.kind=="program_stopped"));
    assert!(tokio::fs::metadata(settings.workspace_dir.join(id.to_string())
        .join("forbidden-by-approval.txt")).await.is_err());
    for _ in 0..100 {
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    fixture.abort();
    tokio::fs::remove_dir_all(root).await.unwrap();
}


#[tokio::test]
async fn registered_native_ollama_provider_completes_original_agent_tool_loop(){
    use std::sync::Mutex as StdMutex;
    let requests=Arc::new(StdMutex::new(Vec::<Value>::new()));
    async fn ollama(State(requests):State<Arc<StdMutex<Vec<Value>>>>,
        Json(body):Json<Value>)->Json<Value>{
        let mut all=requests.lock().unwrap();
        all.push(body);
        if all.len()==1 {
            Json(json!({"done":true,"model":"qwen-local",
                "message":{"role":"assistant","content":"",
                    "tool_calls":[{"function":{"name":"list_files",
                        "arguments":{"path":""}}}]},
                "prompt_eval_count":75,"eval_count":12}))
        }else {
            Json(json!({"done":true,"model":"qwen-local",
                "message":{"role":"assistant","content":"Workspace inspected"},
                "prompt_eval_count":100,"eval_count":9}))
        }
    }
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let server=tokio::spawn({
        let received=Arc::clone(&requests);
        async move {
            axum::serve(listener,Router::new().route("/api/chat",post(ollama))
                .with_state(received)).await.unwrap();
        }
    });
    let root=std::env::temp_dir().join(format!("dominion-native-ollama-{}",Uuid::new_v4()));
    let mut settings=Settings::from_env().unwrap();
    settings.data_dir=root.join("data");
    settings.workspace_dir=root.join("workspace");
    settings.separate_plan_model_call=false;
    settings.separate_model_audit_enabled=false;
    let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
    let groups=Arc::new(crate::group::Groups::open(
        settings.data_dir.join("groups")).await.unwrap());
    let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
    let provider=engine.providers.upsert(crate::provider_registry::Draft{
        id:None,name:"Local Qwen".into(),
        base_url:format!("http://{address}"),kind:"ollama".into(),
        model:"qwen-local".into(),api_key:None,credential_source_id:None,
        enabled:true,local:true,
        reflection:false,reflection_model:None,requests_per_minute:0,
        tool_calling:Some(true), // this real mock server implements tool calls
        parallel_tool_calls:Some(false),streaming:None,
        prompt_cache:None,vision:None,context_window:None,
    }).unwrap();
    let mut run=Run::new(CreateRun{
        instruction:"List local files and report observed workspace".into(),
        budget_krw:Some(50.0),max_steps:Some(6)
    },6).unwrap();
    run.provider_id=provider.id;
    run.model_name=provider.model;
    let id=run.id;
    let workspace=settings.workspace_dir.join(id.to_string());
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    tokio::fs::write(workspace.join("readme.txt"),"local fixture").await.unwrap();
    store.put(run).await.unwrap();
    engine.launch(id).await.unwrap();
    let terminal=tokio::time::timeout(std::time::Duration::from_secs(8),async{
        loop {
            let state=store.get(id).await.unwrap();
            if state.terminal(){break state;}
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }).await.unwrap();
    assert_eq!(terminal.status,RunStatus::Completed,"events: {:?}",terminal.events);
    assert_eq!(terminal.answer.as_deref(),Some("Workspace inspected"));
    assert_eq!(terminal.input_tokens,175);
    assert_eq!(terminal.output_tokens,21);
    assert_eq!(terminal.events.iter().filter(|e|e.kind=="model").count(),2);
    assert_eq!(terminal.events.iter().filter(|e|e.kind=="tool_started").count(),1);
    assert_eq!(terminal.events.iter().filter(|e|e.kind=="tool_result").count(),1);
    let messages=terminal.messages.iter().filter(|m|m["role"]=="tool")
        .collect::<Vec<_>>();
    assert_eq!(messages.len(),1);
    assert!(messages[0]["content"].as_str().unwrap().contains("readme.txt"));
    let recorded=requests.lock().unwrap().clone();
    assert_eq!(recorded.len(),2);
    assert_eq!(recorded[0]["tools"].as_array().unwrap().iter()
        .filter(|t|t["function"]["name"]=="list_files").count(),1);
    assert!(recorded[1]["messages"].as_array().unwrap().iter().any(|m|
        m["role"]=="tool"&&m["tool_name"]=="list_files"
        &&m["content"].as_str().is_some_and(|s|s.contains("readme.txt"))));
    for _ in 0..120 {
        if !engine.active.lock().await.contains(&id){break;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    server.abort();
    drop(engine);
    tokio::fs::remove_dir_all(root).await.unwrap();
}
