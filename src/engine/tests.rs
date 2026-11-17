//! Native execution, cancellation, delegation and recovery regressions.
use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize,Ordering};

    async fn endless_bad_read(State(counter):State<Arc<AtomicUsize>>,
        Json(_request):Json<Value>)->Json<Value>{
        let turn=counter.fetch_add(1,Ordering::SeqCst);
        if turn==0 {
            return Json(json!({"choices":[{"finish_reason":"stop",
                "message":{"role":"assistant","content":"Inspect one missing file"}}],
                "usage":{"prompt_tokens":7,"completion_tokens":6}}));
        }
        Json(json!({"choices":[{"finish_reason":"tool_calls",
            "message":{"role":"assistant","content":null,"tool_calls":[{
                "id":format!("missing-{turn}"),"type":"function",
                "function":{"name":"read_file",
                    "arguments":json!({"path":"never-exists.txt"}).to_string()}
            }]}}],"usage":{"prompt_tokens":7,"completion_tokens":6}}))
    }

    #[tokio::test]
    async fn repetitive_failed_tool_is_rejected_then_paused_without_new_io(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let count=Arc::new(AtomicUsize::new(0));
        let server=tokio::spawn({
            let count=Arc::clone(&count);
            async move {
                axum::serve(listener,
                    Router::new().route("/chat/completions",post(endless_bad_read))
                        .with_state(count)).await.unwrap();
            }
        });
        let root=std::env::temp_dir().join(format!("dominion-loop-guard-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();
        settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,Arc::clone(&store),groups).unwrap();
        let run=Run::new(CreateRun{
            instruction:"Inspect one file and explain the finding".into(),
            budget_krw:Some(500.0),max_steps:Some(15)
        },15).unwrap();
        let id=run.id;store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut observed=None;
        for _ in 0..200{
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let snapshot=store.get(id).await.unwrap();
            if snapshot.events.iter().any(|e|e.kind=="loop_guard_paused"){
                observed=Some(snapshot);break;
            }
        }
        let observed=observed.expect("stuck agent loop was not stopped");
        assert_eq!(observed.status,RunStatus::Paused);
        assert_eq!(observed.events.iter().filter(|e|e.kind=="tool_started").count(),2,
            "third identical failing read must not start");
        assert_eq!(observed.events.iter().filter(|e|e.kind=="tool_error").count(),2);
        assert_eq!(observed.events.iter().filter(|e|e.kind=="loop_guard").count(),2);
        assert_eq!(observed.events.iter().filter(|e|e.kind=="loop_guard_paused").count(),1);
        assert_eq!(observed.events.iter().filter(|e|e.kind=="tool_admission_rejected").count(),3);
        assert_eq!(observed.messages.iter().filter(|m|m["role"]=="tool").count(),5);
        assert_eq!(count.load(Ordering::SeqCst),6,
            "model planning + five bounded tool turns; no runaway polling");
        for _ in 0..100{
            if !engine.active.lock().await.contains(&id){break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!engine.active.lock().await.contains(&id));
        let persisted=Store::open(root.join("data/runs")).await.unwrap().get(id).await.unwrap();
        assert_eq!(persisted.status,RunStatus::Paused);
        assert!(persisted.events.iter().any(|e|e.kind=="loop_guard_paused"));
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn denied_pre_hook_never_dispatches_write_tool(){
        let root=std::env::temp_dir().join(format!("dominion-hook-denied-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        settings.denied_tools=vec!["write_file".into()];
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
        let mut run=Run::new(CreateRun{
            instruction:"write forbidden file".into(),budget_krw:Some(10.0),max_steps:Some(1)
        },1).unwrap();
        run.plan=Some("Run a single write".into());run.steps=1;
        let id=run.id;
        run.queued_calls.push(ToolCall{
            id:"write-1".into(),name:"write_file".into(),
            arguments:json!({"path":"forbidden.txt","content":"should not exist"})
        });
        store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut final_state=None;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let state=store.get(id).await.unwrap();
            if state.terminal(){final_state=Some(state);break;}
        }
        let state=final_state.expect("hook veto not settled");
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_admission_rejected").count(),1);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert_eq!(state.messages.iter().filter(|e|e["role"]=="tool").count(),1);
        assert!(tokio::fs::metadata(settings.workspace_dir.join(id.to_string())
            .join("forbidden.txt")).await.is_err());
        for _ in 0..100 {
            if !engine.active.lock().await.contains(&id){break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    async fn slow_planner(State(started):State<Arc<AtomicUsize>>,
        Json(_request):Json<Value>)->Json<Value>{
        started.fetch_add(1,Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        Json(json!({"choices":[{"message":{"role":"assistant","content":"should not be accepted"}}],
            "usage":{"prompt_tokens":10,"completion_tokens":2}}))
    }

    async fn mocked_long_running_mcp(
        State((called,started)):State<(Arc<AtomicUsize>,Arc<tokio::sync::Notify>)>,
        Json(body):Json<Value>
    )->axum::response::Response{
        use axum::{body::{Body,Bytes},response::IntoResponse};
        use std::convert::Infallible;
        let id=body["id"].clone();
        let method=body["method"].as_str().unwrap_or("");
        if method=="tools/list"{
            return Json(json!({"jsonrpc":"2.0","id":id,"result":{
                "tools":[{"name":"longop","inputSchema":{
                    "type":"object","properties":{"request":{"type":"string"}},
                    "required":["request"]}}
                ]
            }})).into_response();
        }
        if method!="tools/call"{return axum::http::StatusCode::BAD_REQUEST.into_response();}
        called.fetch_add(1,Ordering::SeqCst);
        let progress_token=body["params"]["_meta"]["progressToken"].clone();
        let stream=async_stream::stream!{
            let progress=json!({"jsonrpc":"2.0","method":"notifications/progress",
                "params":{"progressToken":progress_token,"progress":1,"total":10,"message":"remote processing"}});
            yield Ok::<Bytes,Infallible>(Bytes::from(format!("data: {}\n\n",progress)));
            started.notify_one();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let final_response=json!({"jsonrpc":"2.0","id":id,
                "result":{"content":[{"type":"text","text":"remote completed"}]}});
            yield Ok::<Bytes,Infallible>(Bytes::from(format!("data: {}\n\n",final_response)));
        };
        ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],
            Body::from_stream(stream)).into_response()
    }

    #[tokio::test]
    async fn cancelled_mcp_stream_preserves_intent_until_human_observation(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let started=Arc::new(tokio::sync::Notify::new());
        let called=Arc::new(AtomicUsize::new(0));
        let fixture=tokio::spawn({
            let state=(Arc::clone(&called),Arc::clone(&started));
            async move{
                axum::serve(listener,Router::new().route("/mcp",post(mocked_long_running_mcp))
                    .with_state(state)).await.unwrap();
            }
        });
        let root=std::env::temp_dir().join(format!("dominion-mcp-reconcile-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let mcp=Arc::new(crate::mcp::Mcp::new(vec![crate::mcp::Server{
            name:"fixture".into(),url,bearer_token:String::new()
        }]).unwrap());
        let engine=Engine::new_with_mcp(settings,Arc::clone(&store),
            groups,mcp).unwrap();
        let mut run=Run::new(CreateRun{
            instruction:"run a remote operation".into(),budget_krw:Some(100.0),max_steps:Some(1)
        },1).unwrap();
        run.plan=Some("Call approved remote MCP operation".into());
        run.steps=1;
        let id=run.id;
        run.queued_calls.push(ToolCall{
            id:"remote-1".into(),name:"mcp__fixture__longop".into(),
            arguments:json!({"request":"do work"})
        });
        store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut approval=false;
        for _ in 0..100{
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if store.get(id).await.unwrap().status==RunStatus::AwaitingApproval{
                approval=true;break;
            }
        }
        assert!(approval,"external operation was not put behind human approval");
        engine.approve(id,true).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2),started.notified())
            .await.expect("approved MCP tool never entered remote stream");
        let mut progress_saved=false;
        for _ in 0..100{
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if store.get(id).await.unwrap().events.iter().any(|e|e.kind=="mcp_progress"){
                progress_saved=true;break;
            }
        }
        assert!(progress_saved,"MCP progress was not delivered to the native timeline");
        let cancelled=engine.cancel(id).await.unwrap();
        assert_eq!(cancelled.status,RunStatus::Paused);
        assert!(cancelled.cancel_after_tool);
        let mut uncertain=None;
        for _ in 0..120{
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let observed=store.get(id).await.unwrap();
            if observed.events.iter().any(|e|e.kind=="mcp_remote_outcome_unknown")
                && !engine.active.lock().await.contains(&id){
                uncertain=Some(observed);break;
            }
        }
        let observed=uncertain.expect("MCP cancellation failed to retain uncertain result");
        assert_eq!(observed.status,RunStatus::Paused);
        assert_eq!(observed.in_flight.as_ref().unwrap().id,"remote-1");
        let expected_remote=crate::mcp::request_identity(id,observed.in_flight.as_ref().unwrap());
        let intent=observed.events.iter().find(|e|e.kind=="tool_started").unwrap();
        let unknown=observed.events.iter().find(|e|e.kind=="mcp_remote_outcome_unknown").unwrap();
        assert_eq!(intent.detail.as_ref().unwrap()["remote_request_id"],expected_remote);
        assert_eq!(unknown.detail.as_ref().unwrap()["remote_request_id"],expected_remote);
        assert!(observed.cancel_after_tool);
        assert_eq!(observed.events.iter().filter(|e|e.kind=="tool_result").count(),0);
        assert_eq!(called.load(Ordering::SeqCst),1);
        let reconciled=engine.reconcile(id,ReconcileTool{
            succeeded:true,result:"confirmed from the remote service audit".into()
        }).await.unwrap();
        assert_eq!(reconciled.status,RunStatus::Cancelled);
        assert!(reconciled.in_flight.is_none());
        assert_eq!(reconciled.events.iter().filter(|e|e.kind=="tool_reconciled").count(),1);
        assert_eq!(called.load(Ordering::SeqCst),1,
            "remote operation must never be dispatched twice during reconciliation");
        fixture.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_aborts_active_provider_request_without_accepting_stale_plan(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let started=Arc::new(AtomicUsize::new(0));
        let server=tokio::spawn({
            let started=Arc::clone(&started);
            async move{
                axum::serve(listener,Router::new().route("/chat/completions",post(slow_planner))
                    .with_state(started)).await.unwrap();
            }
        });
        let root=std::env::temp_dir().join(format!("dominion-model-cancel-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");
        settings.workspace_dir=root.join("workspace");
        settings.main.base_url=format!("http://{addr}");
        settings.main.api_key.clear();
        settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings,store.clone(),groups).unwrap();
        let run=Run::new(CreateRun{instruction:"create task".into(),
            budget_krw:Some(20.0),max_steps:Some(4)},4).unwrap();
        let id=run.id;
        store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        for _ in 0..100{
            if started.load(Ordering::SeqCst)>0{break;}
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(started.load(Ordering::SeqCst)>0,"test model never received the planning request");
        engine.cancel(id).await.unwrap();
        for _ in 0..100{
            if !engine.active.lock().await.contains(&id){break;}
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!engine.active.lock().await.contains(&id),"cancellation did not stop model wait");
        let state=store.get(id).await.unwrap();
        assert_eq!(state.status,RunStatus::Cancelled);
        assert!(state.plan.is_none(),"stale model response was applied after cancellation");
        assert_eq!(state.events.iter().filter(|event|event.kind=="tool_started").count(),0);
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    async fn fake_model(State(turn):State<Arc<AtomicUsize>>, Json(_request):Json<Value>)->Json<Value>{
        let current=turn.fetch_add(1,Ordering::SeqCst);
        let message=match current {
            0=>json!({"role":"assistant","content":"1. Write output file. 2. Verify."}),
            1=>json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"call-1","type":"function","function":{"name":"write_file","arguments":json!({"path":"result.txt","content":"created by the harness"}).to_string()}
            }]}),
            2=>json!({"role":"assistant","content":"Created result.txt containing the requested artifact."}),
            _=>json!({"role":"assistant","content":"PASS"}),
        };
        Json(json!({"choices":[{"message":message}],"usage":{"prompt_tokens":20,"completion_tokens":10}}))
    }


    async fn invalid_tool_model(State(counter):State<Arc<AtomicUsize>>,Json(_request):Json<Value>)->Json<Value>{
        let turn=counter.fetch_add(1,Ordering::SeqCst);
        let message=match turn {
            0=>json!({"role":"assistant","content":"Check tools before changing files."}),
            1=>json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"unadvertised-1","type":"function","function":{
                    "name":"unadvertised_command",
                    "arguments":"{\"command\":\"touch forbidden.txt\"}"
                }
            }]}),
            2=>json!({"role":"assistant","content":"Unknown tool did not execute."}),
            _=>json!({"role":"assistant","content":"PASS"})
        };
        Json(json!({"choices":[{"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))
    }
    #[tokio::test]
    async fn unadvertised_model_tool_is_rejected_before_execution(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let counter=Arc::new(AtomicUsize::new(0));
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/chat/completions",post(invalid_tool_model))
                .with_state(counter)).await.unwrap();
        });
        let root=std::env::temp_dir().join(format!("dominion-schema-e2e-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=root.join("data");settings.workspace_dir=root.join("workspace");
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
        let run=Run::new(CreateRun{instruction:"Reject unavailable tool invocation".into(),
            budget_krw:Some(1000.0),max_steps:Some(8)},8).unwrap();
        let id=run.id;store.put(run).await.unwrap();engine.launch(id).await.unwrap();
        let mut finished=None;
        for _ in 0..100{
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let run=store.get(id).await.unwrap();
            if run.terminal(){finished=Some(run);break;}
        }
        let state=finished.expect("schema rejection test timed out");
        assert_eq!(state.status,RunStatus::Completed);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_admission_rejected").count(),1);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),0);
        assert_eq!(state.messages.iter().filter(|m|m["role"]=="tool").count(),1);
        assert!(tokio::fs::metadata(settings.workspace_dir.join(id.to_string())
            .join("forbidden.txt")).await.is_err());
        server.abort();tokio::fs::remove_dir_all(root).await.unwrap();
    }

    async fn parallel_mock_model(State(counter):State<Arc<AtomicUsize>>,Json(request):Json<Value>)->Json<Value>{
        let step=counter.fetch_add(1,Ordering::SeqCst);
        if step==1{
            assert_eq!(request["parallel_tool_calls"],true,
                "model did not receive the parallel tool-call contract");
        }
        let message=match step{
            0=>json!({"role":"assistant","content":"1. Inspect inputs. 2. Write report."}),
            1=>json!({"role":"assistant","content":null,"tool_calls":[
                {"id":"parallel-a","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"alpha.txt\"}"}},
                {"id":"parallel-b","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"beta.txt\"}"}},
                {"id":"exclusive-write","type":"function","function":{"name":"write_file","arguments":"{\"path\":\"report.txt\",\"content\":\"Report generated\"}"}}
            ]}),
            2=>json!({"role":"assistant","content":"Inspected both files and wrote report.txt."}),
            _=>json!({"role":"assistant","content":"PASS"})
        };
        Json(json!({"choices":[{"message":message}],"usage":{"prompt_tokens":30,"completion_tokens":12}}))
    }
    #[tokio::test]
    async fn model_multi_call_step_runs_reads_concurrently_then_exclusive_write(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let counter=Arc::new(AtomicUsize::new(0));
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(parallel_mock_model))
                .with_state(counter)).await.unwrap();
        });
        let dir=std::env::temp_dir().join(format!("dominion-parallel-e2e-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();
        settings.main.parallel_tool_calls=true;
        settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
        let run=Run::new(CreateRun{instruction:"Read alpha and beta, write report".into(),
            budget_krw:Some(1000.0),max_steps:Some(8)},8).unwrap();
        let id=run.id;
        let workspace=settings.workspace_dir.join(id.to_string());
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::write(workspace.join("alpha.txt"),"alpha").await.unwrap();
        tokio::fs::write(workspace.join("beta.txt"),"beta").await.unwrap();
        store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut terminal=None;
        for _ in 0..100{
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let run=store.get(id).await.unwrap();
            if run.terminal(){terminal=Some(run);break;}
        }
        let state=terminal.expect("multi-tool execution timed out");
        assert_eq!(state.status,RunStatus::Completed);
        assert_eq!(state.events.iter().filter(|e|e.kind=="read_batch_started").count(),1);
        assert_eq!(state.events.iter().filter(|e|e.kind=="tool_started").count(),1);
        let outcomes=state.messages.iter().filter_map(|v|
            (v["role"]=="tool").then(||v["tool_call_id"].as_str().unwrap().to_owned())
        ).collect::<Vec<_>>();
        assert_eq!(outcomes,vec!["parallel-a","parallel-b","exclusive-write"]);
        assert_eq!(tokio::fs::read_to_string(workspace.join("report.txt")).await.unwrap(),"Report generated");
        for _ in 0..100 {
            if !engine.active.lock().await.contains(&id){break;}
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        server.abort();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn completes_real_plan_tool_verify_cycle() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let count=Arc::new(AtomicUsize::new(0));
        let count_for_server=Arc::clone(&count);
        let server=tokio::spawn(async move { axum::serve(listener,Router::new().route("/chat/completions",post(fake_model)).with_state(count_for_server)).await.unwrap(); });
        let dir=std::env::temp_dir().join(format!("dominion-cycle-{}",Uuid::new_v4()));
        let mut settings=Settings::from_env().unwrap();
        settings.data_dir=dir.join("data");
        settings.workspace_dir=dir.join("workspace");
        settings.main.base_url=format!("http://{addr}");
        settings.main.name="test-model".into();
        settings.main.api_key.clear();
        settings.economy=settings.main.clone();
        let store=Arc::new(Store::open(settings.data_dir.join("runs")).await.unwrap());
        let groups=Arc::new(crate::group::Groups::open(settings.data_dir.join("groups")).await.unwrap());

        let engine=Engine::new(settings.clone(),store.clone(),groups).unwrap();
        let run=Run::new(crate::domain::CreateRun{instruction:"Write result.txt".into(),budget_krw:Some(1000.0),max_steps:Some(8)},8).unwrap();
        let id=run.id;store.put(run).await.unwrap();
        engine.launch(id).await.unwrap();
        let mut result=None;
        for _ in 0..80 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let snapshot=store.get(id).await.unwrap();
            if snapshot.terminal() {result=Some(snapshot);break;}
        }
        let result=result.expect("execution timed out");
        assert_eq!(result.status,RunStatus::Completed,"events: {:?}",result.events);
        assert_eq!(tokio::fs::read_to_string(settings.workspace_dir.join(id.to_string()).join("result.txt")).await.unwrap(),"created by the harness");
        assert!(result.events.iter().any(|e|e.kind=="verification"));
        assert!(count.load(Ordering::SeqCst)>=4);
        server.abort();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

#[cfg(test)]
mod recovery_tests;
