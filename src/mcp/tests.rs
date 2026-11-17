//! Extracted regression tests; production modules stay compact.
use super::*;
    use axum::{http::HeaderMap, routing::post, Json,Router};

    use std::{convert::Infallible,sync::{Arc,atomic::{AtomicUsize,Ordering}}};
    use axum::{body::{Body,Bytes},extract::State,response::IntoResponse};
    use tokio::sync::Notify;

    #[derive(Clone)]
    struct StreamFixture{
        started:Arc<Notify>,
        notifications:Arc<AtomicUsize>,
    }
    async fn streamable_rpc(State(fixture):State<StreamFixture>,Json(body):Json<Value>)
        ->axum::response::Response{
        let id=body["id"].clone();
        let method=body["method"].as_str().unwrap_or("");
        if method=="tools/list"{
            return Json(json!({"jsonrpc":"2.0","id":id,"result":{
                "tools":[{"name":"run","description":"Streaming test tool","inputSchema":{
                    "type":"object","properties":{"mode":{"type":"string"}},
                    "required":["mode"]}}
                ]
            }})).into_response();
        }
        if method!="tools/call"{
            fixture.notifications.fetch_add(1,Ordering::SeqCst);
            return (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":"unsupported"}))).into_response();
        }
        let mode=body["params"]["arguments"]["mode"].as_str().unwrap_or("");
        if mode=="http-500"{
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let hold=mode=="hold";
        let incomplete=mode=="incomplete";
        let started=Arc::clone(&fixture.started);
        let token=body["params"]["_meta"]["progressToken"].clone();
        let response=async_stream::stream!{
            let progress=json!({"jsonrpc":"2.0","method":"notifications/progress",
                "params":{"progressToken":token,"progress":1,"total":4,"message":"started"}});
            let unrelated=json!({"jsonrpc":"2.0","method":"notifications/progress",
                "params":{"progressToken":"other-request","progress":999}});
            yield Ok::<Bytes,Infallible>(Bytes::from(format!(
                "event: message\r\ndata: {}\r\n\r\n",unrelated
            )));
            yield Ok::<Bytes,Infallible>(Bytes::from(format!(
                "event: message\r\ndata: {}\r\n\r\n",progress
            )));
            started.notify_one();
            if hold {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            if incomplete{return;}
            let completed=json!({"jsonrpc":"2.0","id":id,
                "result":{"content":[{"type":"text","text":"completed"}]}});
            // Split final event across chunks so parsing cannot rely on
            // frame alignment with network reads.
            let whole=format!("event: message\ndata: {}\n\n",completed);
            let split=whole.len()/2;
            yield Ok::<Bytes,Infallible>(Bytes::from(whole[..split].to_string()));
            yield Ok::<Bytes,Infallible>(Bytes::from(whole[split..].to_string()));
            // A conforming SSE server may keep a stream alive after its final
            // reply. The client must return on the matching response frame.
            tokio::time::sleep(Duration::from_secs(10)).await;
        };
        ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(response)).into_response()
    }
    async fn fixture_server()->(Mcp,StreamFixture,tokio::task::JoinHandle<()>){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let fixture=StreamFixture{
            started:Arc::new(Notify::new()),notifications:Arc::new(AtomicUsize::new(0))
        };
        let state=fixture.clone();
        let handle=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/mcp",post(streamable_rpc))
                .with_state(state)).await.unwrap();
        });
        let client=Mcp::new(vec![Server{name:"test".into(),url,bearer_token:String::new()}]).unwrap();
        (client,fixture,handle)
    }
    fn test_call(mode:&str)->ToolCall{
        ToolCall{id:"client-tool-1".into(),name:"mcp__test__run".into(),
            arguments:json!({"mode":mode})}
    }
    #[tokio::test]
    async fn sse_stream_completes_on_matching_frame_without_waiting_for_close(){
        let (client,fixture,server)=fixture_server().await;
        let response=tokio::time::timeout(Duration::from_secs(2),client.call(&test_call("fast")))
            .await.expect("SSE frame was not processed before socket close")
            .unwrap();
        assert!(response.contains("completed"));
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0);
        server.abort();
    }

    #[tokio::test]
    async fn forwards_only_matching_sse_progress_with_bounded_content(){
        let (client,_fixture,server)=fixture_server().await;
        let (_tx,rx)=watch::channel(false);
        let (progress_tx,mut progress_rx)=mpsc::channel(8);
        let run=Uuid::new_v4();
        let result=client.call_with_progress(run,&test_call("fast"),rx,progress_tx).await.unwrap();
        assert!(matches!(result,McpCallOutcome::Settled(Ok(_))));
        let progress=progress_rx.recv().await.expect("matching progress not delivered");
        assert_eq!(progress.progress,1.0);
        assert_eq!(progress.total,Some(4.0));
        assert_eq!(progress.message.as_deref(),Some("started"));
        assert_eq!(progress.request_id,request_identity(run,&test_call("fast")));
        assert!(progress_rx.try_recv().is_err(),
            "unrelated request token must not enter this progress channel");
        server.abort();
    }

    #[tokio::test]
    async fn http_stream_cancel_is_unconfirmed_not_success_or_user_error(){
        let (client,fixture,server)=fixture_server().await;
        let (tx,rx)=watch::channel(false);
        let task=tokio::spawn(async move{client.call_cooperative(&test_call("hold"),rx).await});
        tokio::time::timeout(Duration::from_secs(2),fixture.started.notified())
            .await.expect("remote MCP stream did not start");
        tx.send_replace(true);
        let outcome=tokio::time::timeout(Duration::from_secs(2),task).await
            .expect("cancelled SSE did not close promptly").unwrap().unwrap();
        assert!(matches!(outcome,McpCallOutcome::Uncertain(_)),
            "closing a remote response never proves the side effect stopped");
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0,
            "MCP 2026 HTTP must not send a cancelled notification POST");
        server.abort();
    }

    #[tokio::test]
    async fn incomplete_or_server_failed_remote_mcp_call_never_auto_retries(){
        let (client,fixture,server)=fixture_server().await;
        let (_tx,rx)=watch::channel(false);
        let incomplete=tokio::time::timeout(Duration::from_secs(2),
            client.call_cooperative(&test_call("incomplete"),rx.clone()))
            .await.expect("incomplete response should close")
            .unwrap();
        assert!(matches!(incomplete,McpCallOutcome::Uncertain(_)));
        let failed=client.call_cooperative(&test_call("http-500"),rx).await.unwrap();
        assert!(matches!(failed,McpCallOutcome::Uncertain(_)));
        assert_eq!(fixture.notifications.load(Ordering::SeqCst),0);
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_before_remote_dispatch_is_definitively_non_mutating(){
        let (client,_fixture,server)=fixture_server().await;
        let (tx,rx)=watch::channel(true);
        // Pre-cancelled tool invocation may fetch no remote result but
        // cannot dispatch tools/call.
        let outcome=client.call_cooperative(&test_call("hold"),rx).await.unwrap();
        assert!(matches!(outcome,McpCallOutcome::Settled(Err(_))));
        drop(tx);
        server.abort();
    }


    /// Real public 2026 Streamable HTTP endpoint, intentionally opt-in.
    /// Read-only metadata lookup only; never invokes paid or modifying tools.
    /// Run:
    /// cargo test --lib live_public_streamable_http_interop -- --ignored --nocapture
    #[tokio::test]
    #[ignore = "network-bound public MCP compatibility gate"]
    async fn live_public_streamable_http_interop(){
        let url=std::env::var("DOMINION_LIVE_MCP_URL")
            .unwrap_or_else(|_|"https://mcp.specification.website/mcp".into());
        let server=Mcp::new(vec![Server{
            name:"live".into(),url:url.clone(),bearer_token:String::new()
        }]).expect("public live MCP server configuration invalid");
        let before=Instant::now();
        let discovered=tokio::time::timeout(Duration::from_secs(30),server.list("live"))
            .await.expect("public MCP discovery timeout")
            .unwrap_or_else(|error|panic!("live discovery {url}: {error}"));
        assert!(!discovered.is_empty(),"public MCP returned zero usable tools");
        let selected=discovered.iter().find_map(|tool|{
            match tool.name.as_str(){
                "list_topics"=>Some((tool.name.clone(),json!({"limit":3}))),
                "search"=>Some((tool.name.clone(),json!({"query":"protocol","limit":3}))),
                "read_wiki_structure"=>Some((tool.name.clone(),
                    json!({"repoName":"modelcontextprotocol/modelcontextprotocol"}))),
                _=>None
            }
        }).expect("no known read-only public MCP tool in discovered catalog");
        let call=ToolCall{
            id:"interop-public-read".into(),name:format!("mcp__live__{}",selected.0),
            arguments:selected.1
        };
        let (sender,signal)=watch::channel(false);
        let (progress,mut receiver)=mpsc::channel(8);
        let start=Instant::now();
        let result=tokio::time::timeout(Duration::from_secs(45),
            server.call_with_progress(Uuid::new_v4(),&call,signal,progress))
            .await.expect("public MCP tool call timed out")
            .unwrap_or_else(|error|panic!("live tool transport error: {error}"));
        let response=match result{
            McpCallOutcome::Settled(Ok(value))=>value,
            McpCallOutcome::Settled(Err(error))=>panic!("live tool rejected call: {error}"),
            McpCallOutcome::Uncertain(error)=>panic!("public read-only call outcome unknown: {error}")
        };
        assert!(!response.is_empty(),"live MCP returned empty content");
        assert!(response.len()<=16000,"server output cap was not enforced");
        let mut notifications=0usize;
        while receiver.try_recv().is_ok(){notifications+=1;}
        println!("{}",json!({
            "live_mcp":url,
            "server_tools":discovered.len(),
            "tool":call.name,
            "discovery_elapsed_ms":before.elapsed().as_millis(),
            "call_elapsed_ms":start.elapsed().as_millis(),
            "response_chars":response.chars().count(),
            "progress_events":notifications,
            "external_api_cost_krw":0,
            "cost_note":"Unauthenticated public documentation request; provider billing not measured"
        }));
        drop(sender);
    }


    #[test]
    fn honors_2026_cache_hints_without_fabricating_freshness(){
        assert!(announced_ttl(&json!({"ttlMs":0,"cacheScope":"private"})).is_zero());
        assert!(announced_ttl(&json!({"ttlMs":10000})).is_zero());
        assert!(announced_ttl(&json!({"cacheScope":"public"})).is_zero());
        assert!(announced_ttl(&json!({"ttlMs":5000,"cacheScope":"invalid"})).is_zero());
        assert_eq!(announced_ttl(&json!({"ttlMs":9999999,"cacheScope":"public"})),
            Duration::from_millis(MAX_CACHE_MS));
    }

    async fn paginated_tools_fixture(State(calls):State<Arc<AtomicUsize>>,Json(body):Json<Value>)
        ->Json<Value>{
        calls.fetch_add(1,Ordering::SeqCst);
        let cursor=body["params"]["cursor"].as_str();
        let (name,next,ttl)=match cursor{
            None=>("first",Some("page-2"),5000),
            Some("page-2")=>("second",None,1000),
            _=>("unexpected",Some("page-2"),0)
        };
        let mut result=json!({"resultType":"complete","ttlMs":ttl,
            "cacheScope":"private","tools":[{
                "name":name,"description":"paged tool",
                "inputSchema":{"type":"object","properties":{}}
            }]});
        if let Some(next)=next{result["nextCursor"]=json!(next);}
        Json(json!({"jsonrpc":"2.0","id":body["id"],"result":result}))
    }

    #[tokio::test]
    async fn discovers_every_page_and_refetches_when_shortest_ttl_expires(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let calls=Arc::new(AtomicUsize::new(0));
        let state=Arc::clone(&calls);
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new().route("/mcp",post(paginated_tools_fixture))
                .with_state(state)).await.unwrap();
        });
        let client=Mcp::new(vec![Server{name:"paged".into(),url,bearer_token:String::new()}]).unwrap();
        let definitions=client.definitions().await;
        assert_eq!(definitions.len(),2);
        assert_eq!(definitions[0]["function"]["name"],"mcp__paged__first");
        assert_eq!(definitions[1]["function"]["name"],"mcp__paged__second");
        assert_eq!(calls.load(Ordering::SeqCst),2);
        assert_eq!(client.definitions().await.len(),2);
        assert_eq!(calls.load(Ordering::SeqCst),2,"fresh listing should hit client cache");
        {
            let mut cache=client.cached.lock().await;
            cache.get_mut("paged").unwrap().0=Instant::now()-Duration::from_secs(5);
        }
        assert_eq!(client.definitions().await.len(),2);
        assert_eq!(calls.load(Ordering::SeqCst),4,"expired listing must fetch both pages");
        server.abort();
    }

    #[test]
    fn validates_endpoints_and_header_schema() {
        assert!(validate_endpoint("http://127.0.0.1:8080/mcp").is_ok());
        assert!(validate_endpoint("http://external.example.com/mcp").is_err());
        let schema=json!({"type":"object","properties":{"city":{"type":"string","x-mcp-header":"City"}}});
        assert_eq!(collect_headers(&schema).unwrap().len(),1);
        assert!(collect_headers(&json!({"type":"object","anyOf":[{"x-mcp-header":"City"}]})).is_err());
        assert_eq!(encode_header("서울"),"=?base64?7ISc7Jq4?=");
    }
    async fn endpoint(headers:HeaderMap,Json(body):Json<Value>)->Json<Value>{
        assert_eq!(headers.get("MCP-Protocol-Version").unwrap(),"2026-07-28");
        let method=body["method"].as_str().unwrap();
        assert_eq!(headers.get("Mcp-Method").unwrap(),method);
        let id=body["id"].clone();
        let result=if method=="tools/list"{
            json!({"tools":[{"name":"echo","description":"Echo value","inputSchema":{
                "type":"object","properties":{"word":{"type":"string","x-mcp-header":"Word"}},"required":["word"]}}]})
        }else{
            assert_eq!(headers.get("Mcp-Name").unwrap(),"echo");
            assert_eq!(headers.get("Mcp-Param-Word").unwrap(),"test");
            assert_eq!(body["params"]["arguments"]["word"],"test");
            json!({"content":[{"type":"text","text":"echo successful"}]})
        };
        Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    #[tokio::test]
    async fn lists_and_invokes_2026_stateless_http_tools(){
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/mcp",listener.local_addr().unwrap());
        let server=tokio::spawn(async move{axum::serve(listener,Router::new().route("/mcp",post(endpoint))).await.unwrap();});
        let mcp=Mcp::new(vec![Server{name:"local".into(),url,bearer_token:"".into()}]).unwrap();
        let tools=mcp.definitions().await;
        assert_eq!(tools[0]["function"]["name"],"mcp__local__echo");
        let answer=mcp.call(&ToolCall{id:"id".into(),name:"mcp__local__echo".into(),arguments:json!({"word":"test"})}).await.unwrap();
        assert!(answer.contains("echo successful"));
        server.abort();
    }
