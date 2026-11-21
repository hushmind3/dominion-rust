use super::*;
use super::*;
    #[test]
    fn run_reasoning_model_matrix_rejects_unsupported_requests_before_billing(){
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.name="gpt-5.6".into();
        cfg.main.base_url="https://api.openai.com/v1".into();
        let model=ModelClient::new(&cfg).unwrap();
        let selected=model.with_run_reasoning(Some("high"),None).unwrap();
        let mut openai=json!({"model":"gpt-5.6","messages":[]});
        selected.apply_run_reasoning(&mut openai).unwrap();
        assert_eq!(openai["reasoning_effort"],"high");
        assert!(openai.get("think").is_none());
        assert!(model.with_run_reasoning(None,Some(false)).is_err());
        assert!(model.with_run_reasoning(Some("xhigh"),None).is_err());
        cfg.main.base_url="https://proxy.example/v1".into();
        assert!(ModelClient::new(&cfg).unwrap()
            .with_run_reasoning(Some("high"),None).is_err(),
            "unverified compatible gateways cannot receive first-party fields");
        cfg.main.name="qwen3:4b".into();
        cfg.main.base_url="http://127.0.0.1:11434".into();
        let ollama=ModelClient::new(&cfg).unwrap()
            .with_primary_protocol(cfg.main.clone(),"ollama").unwrap();
        let disabled=ollama.with_run_reasoning(None,Some(false)).unwrap();
        let mut body=json!({"model":"qwen3:4b","messages":[]});
        disabled.apply_run_reasoning(&mut body).unwrap();
        assert_eq!(body["think"],false);
        assert!(ollama.with_run_reasoning(Some("high"),None).is_err());
        cfg.main.name="gpt-oss:20b".into();
        let gpt_oss=ModelClient::new(&cfg).unwrap()
            .with_primary_protocol(cfg.main.clone(),"ollama").unwrap();
        assert!(gpt_oss.with_run_reasoning(None,Some(false)).is_err());
        let mut level_body=json!({"model":"gpt-oss:20b"});
        gpt_oss.with_run_reasoning(Some("low"),None).unwrap()
            .apply_run_reasoning(&mut level_body).unwrap();
        assert_eq!(level_body["think"],"low");
    }
    #[tokio::test]
    async fn native_ollama_final_wire_request_contains_verified_thinking_disable(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        async fn capture(State(received):State<Arc<Mutex<Vec<Value>>>>,
            Json(body):Json<Value>)->Json<Value>{
            received.lock().unwrap().push(body);
            Json(json!({"done":true,"message":{"role":"assistant",
                "content":"No-thinking response"},
                "prompt_eval_count":13,"eval_count":3}))
        }
        let requests=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/api/chat",post(capture))
            .with_state(requests.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.name="qwen3:4b".into();
        cfg.main.api_key.clear();
        let selected=ModelClient::new(&cfg).unwrap()
            .with_primary_protocol(cfg.main.clone(),"ollama").unwrap()
            .with_run_reasoning(None,Some(false)).unwrap();
        let reply=selected.complete(&[json!({"role":"user",
            "content":"안녕하세요"})],&[],false).await.unwrap();
        assert_eq!(reply.text,"No-thinking response");
        let received=requests.lock().unwrap();
        assert_eq!(received.len(),1,"never silently retry a paid model call");
        assert_eq!(received[0]["think"],false);
        assert!(received[0].get("reasoning_effort").is_none());
        assert!(received[0].get("tools").is_none());
        drop(received);
        server.abort();
    }
    #[test]
    fn native_ollama_history_maps_tool_names_and_object_arguments(){
        let original=vec![
            json!({"role":"system","content":"tools"}),
            json!({"role":"user","content":"inspect"}),
            json!({"role":"assistant","content":null,"tool_calls":[{
                "id":"call-x","type":"function","function":{
                    "name":"read_file","arguments":r#"{"path":"main.rs"}"#}}]}),
            json!({"role":"tool","tool_call_id":"call-x","content":"rust"}),
        ];
        let normalized=ollama_messages(&original).unwrap();
        assert_eq!(normalized[2]["tool_calls"][0]["function"]["arguments"]["path"],"main.rs");
        assert_eq!(normalized[3]["tool_name"],"read_file");
        assert!(normalized[2]["tool_calls"][0].get("id").is_none());
        let mut invalid=original.clone();
        invalid[3]["tool_call_id"]=json!("invented");
        assert!(ollama_messages(&invalid).unwrap_err().contains("orphan"));
    }
    #[tokio::test]
    async fn native_ollama_calls_real_api_chat_normalizes_calls_and_bills_tokens(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        async fn server(State(received):State<Arc<Mutex<Vec<Value>>>>,
            Json(body):Json<Value>)->Json<Value>{
            let mut requests=received.lock().unwrap();
            requests.push(body);
            if requests.len()==1{
                Json(json!({"model":"qwen-local","done":true,
                    "message":{"role":"assistant","content":"",
                        "tool_calls":[{"function":{"name":"read_file",
                            "arguments":{"path":"main.rs"}}}]},
                    "prompt_eval_count":120,"eval_count":13}))
            }else{
                Json(json!({"model":"qwen-local","done":true,
                    "message":{"role":"assistant","content":"Artifact inspected"},
                    "prompt_eval_count":155,"prompt_eval_cached_count":115,
                    "eval_count":8}))
            }
        }
        let received=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/api/chat",post(server)).with_state(received.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut settings=Settings::from_env().unwrap();
        settings.main.api_key.clear();
        settings.main.base_url=format!("http://{address}");
        settings.main.name="qwen-local".into();
        settings.main.input_krw_per_million=1000.0;
        settings.main.output_krw_per_million=2000.0;
        let client=ModelClient::new(&settings).unwrap()
            .with_primary_protocol(settings.main.clone(),"ollama").unwrap();
        let tool=json!({"type":"function","function":{"name":"read_file",
            "description":"inspect file","parameters":{"type":"object",
                "properties":{"path":{"type":"string"}},"required":["path"]}}});
        let mut history=vec![json!({"role":"user","content":"inspect main.rs"})];
        let result=client.complete(&history,&[tool],false).await.unwrap();
        assert_eq!(result.calls.len(),1);
        assert_eq!(result.calls[0].arguments["path"],"main.rs");
        assert_eq!(result.input_tokens,120);
        assert_eq!(result.output_tokens,13);
        assert!((result.cost_krw-0.146).abs()<0.000001);
        history.push(result.message);
        history.push(json!({"role":"tool","tool_call_id":result.calls[0].id,
            "content":"file text"}));
        let final_reply=client.complete(&history,&[],false).await.unwrap();
        assert_eq!(final_reply.text,"Artifact inspected");
        assert_eq!(final_reply.input_tokens,155);
        assert_eq!(final_reply.cached_input_tokens,115,
            "cached prefix must come from Ollama KV-cache usage receipt");
        assert_eq!(final_reply.input_tokens-final_reply.cached_input_tokens,40);
        assert_eq!(received.lock().unwrap()[1]["messages"][2]["tool_name"],"read_file");
        assert_eq!(received.lock().unwrap()[0]["tools"][0]["function"]["name"],"read_file");
        server.abort();
    }
    #[test]
    fn explicit_anthropic_group_anchor_is_first_party_only(){
        let history=vec![
            json!({"role":"system","content":"shared"}),
            json!({"role":"user","content":"indexed owner decisions",
                "dominion_cache_anchor":true}),
            json!({"role":"user","content":"operator-selected identity"}),
        ];
        let (system,native)=anthropic_messages(&history,true).unwrap();
        assert_eq!(system,"shared");
        assert_eq!(native[0]["content"][0]["cache_control"]["type"],"ephemeral");
        assert!(native[1]["content"][0].get("cache_control").is_none());
        let (_,proxy)=anthropic_messages(&history,false).unwrap();
        assert!(proxy[0]["content"][0].get("cache_control").is_none());
    }
    #[test]
    fn modern_explicit_cache_receipt_requires_both_read_and_write_counters(){
        let mut cfg=Settings::from_env().unwrap().main;
        cfg.name="gpt-6".into();
        cfg.base_url="https://api.openai.com/v1".into();
        let partial=json!({"usage":{"prompt_tokens":120,"completion_tokens":9,
            "prompt_tokens_details":{"cached_tokens":60}}});
        assert!(!reliable_cache_read_write_receipt(&partial,&cfg,true));
        let complete=json!({"usage":{"prompt_tokens":120,"completion_tokens":9,
            "prompt_tokens_details":{"cached_tokens":60,"cache_write_tokens":0}}});
        assert!(reliable_cache_read_write_receipt(&complete,&cfg,true));
        cfg.name="gpt-5.5".into();
        assert!(reliable_cache_read_write_receipt(&partial,&cfg,true),
            "legacy provider read-cache accounting does not require a write counter");
        cfg.name="gpt-6".into();
        cfg.base_url="http://127.0.0.1:11434/v1".into();
        assert!(reliable_cache_read_write_receipt(&partial,&cfg,true),
            "local compatibles must not inherit first-party billing claims");
    }
    #[test]
    fn latest_openai_cache_breakpoints_pin_stable_chat_prefix_and_strip_annotations(){
        let mut settings=Settings::from_env().unwrap().main;
        settings.name="gpt-6.1-sol".into();
        settings.base_url="https://api.openai.com/v1".into();
        settings.cache_ttl=Some("12h".into()); // unsupported on GPT-5.6+
        let mut request=json!({"model":settings.name,"messages":[
            {"role":"system","content":"stable agent rules","dominion_cache_anchor":true},
            {"role":"user","content":"old owner evidence","dominion_cache_anchor":true},
            {"role":"user","content":"new owner question"}
        ]});
        configure_openai_cache(&mut request,&settings,Some("chat-session"),"messages");
        assert_eq!(request["prompt_cache_options"]["mode"],"explicit");
        assert_eq!(request["prompt_cache_options"]["ttl"],"30m");
        assert_eq!(request["prompt_cache_key"],"chat-session");
        assert_eq!(request["messages"][0]["content"][0]["type"],"text");
        assert_eq!(request["messages"][1]["content"][0]["prompt_cache_breakpoint"]["mode"],"explicit");
        assert_eq!(request["messages"][2]["content"],"new owner question");
        let mut delta=json!({"input":[{"role":"user","content":"changed continuation"}]});
        configure_openai_cache(&mut delta,&settings,Some("chat-session"),"input");
        assert_eq!(delta["prompt_cache_options"]["mode"],"explicit");
        assert_eq!(delta["input"][0]["content"],"changed continuation");
        assert!(delta["input"][0].get("prompt_cache_breakpoint").is_none());
        assert!(!request.to_string().contains("dominion_cache_anchor"));
        assert!(!request.to_string().contains("12h"));
    }
    #[test]
    fn responses_cache_uses_input_text_and_other_providers_keep_legacy_schema(){
        let mut settings=Settings::from_env().unwrap().main;
        settings.name="gpt-5.6".into();
        settings.base_url="https://api.openai.com/v1".into();
        let history=json!([{"role":"developer","content":"stable instructions",
            "dominion_cache_anchor":true},{"role":"user","content":"new task"}]);
        let mut request=json!({"input":history});
        configure_openai_cache(&mut request,&settings,Some("run-1"),"input");
        assert_eq!(request["input"][0]["content"][0]["type"],"input_text");
        assert_eq!(request["input"][1]["content"],"new task");
        let mut proxy=json!({"messages":history});
        settings.base_url="https://thirdparty.example/v1".into();
        configure_openai_cache(&mut proxy,&settings,Some("run-1"),"messages");
        assert!(proxy.get("prompt_cache_key").is_none());
        assert!(proxy.get("prompt_cache_options").is_none());
        assert_eq!(proxy["messages"][0]["content"],"stable instructions");
        assert!(!proxy.to_string().contains("dominion_cache_anchor"));
        settings.base_url="https://api.openai.com/v1".into();
        settings.name="gpt-5.5".into();
        let mut older=json!({"messages":history});
        configure_openai_cache(&mut older,&settings,Some("run-1"),"messages");
        assert!(older.get("prompt_cache_options").is_none());
        assert_eq!(older["messages"][0]["content"],"stable instructions");
    }
    #[test]
    fn annotated_group_messages_never_reach_openai_or_ollama_json_schemas(){
        let original=vec![
            json!({"role":"system","content":"common"}),
            json!({"role":"user","content":"facts","dominion_cache_anchor":true}),
            json!({"role":"user","content":"persona"})
        ];
        let mut openai=json!({"model":"mock","messages":original});
        let mut settings=Settings::from_env().unwrap().main;
        settings.base_url="https://thirdparty.example/v1".into();
        configure_openai_cache(&mut openai,&settings,None,"messages");
        assert!(openai["messages"][1].get("dominion_cache_anchor").is_none());
        assert!(ollama_messages(&original).unwrap().iter()
            .all(|entry|entry.get("dominion_cache_anchor").is_none()));
    }
    #[test]
    fn actual_gateway_token_usage_is_accounted_and_missing_usage_is_labelled(){
        let chat=json!({"usage":{"prompt_tokens":270,"completion_tokens":35,
            "prompt_tokens_details":{"cached_tokens":150}}});
        assert_eq!(openai_compatible_usage(&chat),(270,35,150,0,true,true));
        let responses=json!({"usage":{"input_tokens":480,"output_tokens":60,
            "input_tokens_details":{"cached_tokens":240,"cache_write_tokens":30}}});
        assert_eq!(openai_compatible_usage(&responses),(480,60,240,30,true,true));
        assert_eq!(openai_compatible_usage(&json!({"usage":{}})),(0,0,0,0,false,false));
        assert_eq!(openai_compatible_usage(&json!({"usage":{"input_tokens":67}})),
            (67,0,0,0,false,false));
    }
    #[test]
    fn stale_responses_rebase_only_with_explicit_parameter_error(){
        let legitimate=json!({"error":{"param":"previous_response_id",
            "code":"invalid_previous_response_id",
            "message":"Previous response not found"}}).to_string();
        assert!(definitely_missing_previous_response(400,legitimate.as_bytes()));
        assert!(definitely_missing_previous_response(404,legitimate.as_bytes()));
        assert!(!definitely_missing_previous_response(500,legitimate.as_bytes()));
        assert!(!definitely_missing_previous_response(429,legitimate.as_bytes()));
        for payload in [
            json!({"error":{"param":"model","code":"invalid_request_error",
                "message":"model not found"}}),
            json!({"error":{"code":"invalid_request_error",
                "message":"Response not found"}}),
            json!({"error":{"param":"previous_response_id",
                "code":"invalid_request_error","message":"bad function schema"}})
        ]{
            assert!(!definitely_missing_previous_response(400,
                payload.to_string().as_bytes()),
                "never duplicate a potentially billable or unrelated call");
        }
    }
    #[test]
    fn responses_affinity_changes_with_account_credentials(){
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url="https://api.openai.com/v1".into();
        cfg.main.name="gpt-6".into();
        cfg.main.api_key="sk-test-account-one".into();
        let model=ModelClient::new(&cfg).unwrap();
        let first=model.room_stateful_identity(false).unwrap();
        assert!(!first.contains("account-one"));
        let second_cfg=ModelSettings{api_key:"sk-test-account-two".into(),
            ..cfg.main.clone()};
        let other=model.with_primary(second_cfg);
        let second=other.room_stateful_identity(false).unwrap();
        assert_ne!(first,second,
            "reusing a response ID across provider accounts causes 400");
        assert_eq!(model.room_stateful_identity(false),
            model.room_stateful_identity(false));
        let unrelated=ModelSettings{base_url:"https://example.net/v1".into(),
            ..cfg.main.clone()};
        assert!(model.with_primary(unrelated).room_stateful_identity(false)
            .is_none(),"unsupported gateways cannot claim remote state");
    }
    #[test]
    fn first_party_openai_cache_key_inherits_stable_session_or_explicit_operator_key(){
        let mut config=Settings::from_env().unwrap();
        config.main.base_url="https://api.openai.com/v1".into();
        config.main.cache_key=None;
        let base=ModelClient::new(&config).unwrap();
        let selected=base.with_session_affinity("dominion-chat-stable");
        assert_eq!(selected.openai_cache_key_for(&config.main).as_deref(),
            Some("dominion-chat-stable"));
        assert_eq!(base.openai_cache_key_for(&config.main),None);
        let mut proxied=config.main.clone();
        proxied.base_url="https://api.openai.com.attacker.test/v1".into();
        assert_eq!(selected.openai_cache_key_for(&proxied),None);
        proxied.base_url="https://api.openai.com/v1".into();
        proxied.cache_key=Some("operator-owned-key".into());
        let composite=selected.openai_cache_key_for(&proxied).unwrap();
        assert!(composite.starts_with("operator-owned-key:"));
        assert!(composite.len()<=128);
        assert!(!composite.contains("dominion-chat-stable"));
        let unrelated=base.with_session_affinity("another-chat");
        assert_ne!(unrelated.openai_cache_key_for(&proxied),Some(composite),
            "operator key must not collapse unrelated chat routing");
    }
    #[test]
    fn cloud_cache_affinity_is_account_scoped_by_gateway_and_exact_hostname(){
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url="https://dominion-cloud.dominion-hushmind.workers.dev/v1".into();
        let base=ModelClient::new(&cfg).unwrap();
        let session=base.with_session_affinity("dominion-chat-one");
        let hash=session.dominion_cloud_affinity_for(&cfg.main).unwrap();
        assert_eq!(hash.len(),32);
        assert_ne!(hash,"dominion-chat-one");
        assert_eq!(hash,session.dominion_cloud_affinity_for(&cfg.main).unwrap());
        let conversation=vec![
            json!({"role":"system","content":"stable","dominion_cache_anchor":true}),
            json!({"role":"user","content":"historical","dominion_cache_anchor":true}),
            json!({"role":"user","content":"new instruction"})
        ];
        assert_eq!(session.dominion_cloud_cache_boundary_for(&cfg.main,&conversation),Some(1));
        let short=vec![json!({"role":"user","content":"single current request",
            "dominion_cache_anchor":true})];
        assert_eq!(session.dominion_cloud_cache_boundary_for(&cfg.main,&short),None);
        cfg.main.base_url="https://api.deepseek.com/v1".into();
        assert_eq!(session.dominion_cloud_cache_boundary_for(&cfg.main,&conversation),None);
        cfg.main.base_url="https://dominion-cloud.dominion-hushmind.workers.dev/v1".into();
        let other=base.with_session_affinity("dominion-chat-two");
        assert_ne!(hash,other.dominion_cloud_affinity_for(&cfg.main).unwrap());
        cfg.main.base_url="https://dominion-cloud.dominion-hushmind.workers.dev.evil.test/v1".into();
        assert!(session.dominion_cloud_affinity_for(&cfg.main).is_none());
        cfg.main.base_url="https://thirdparty.example/v1".into();
        assert!(session.dominion_cloud_affinity_for(&cfg.main).is_none());
    }
    #[test]
    fn opencode_affinity_uses_durable_run_identity_and_never_leaks_to_other_hosts(){
        let mut config=Settings::from_env().unwrap();
        config.main.base_url="https://opencode.ai/zen/go/v1".into();
        config.main.name="go/test".into();
        let base=ModelClient::new(&config).unwrap();
        let fallback=base.opencode_session_for(&config.main).unwrap().to_string();
        assert_ne!(fallback,"dominion-sub4",
            "branch-wide hard-coded session IDs contaminate unrelated users");
        let run_a=base.with_session_affinity("run-aa11");
        let run_b=base.with_session_affinity("run-bb22");
        assert_eq!(run_a.opencode_session_for(&config.main),Some("run-aa11"));
        assert_eq!(run_b.opencode_session_for(&config.main),Some("run-bb22"));
        assert_eq!(run_a.with_primary(config.main.clone())
            .opencode_session_for(&config.main),Some("run-aa11"));
        assert_eq!(run_a.with_primary_protocol(config.main.clone(),"openai").unwrap()
            .opencode_session_for(&config.main),Some("run-aa11"));
        assert_eq!(base.opencode_session_for(&config.main),Some(fallback.as_str()));
        let unrelated=ModelClient::new(&config).unwrap();
        assert_ne!(unrelated.opencode_session_for(&config.main),Some(fallback.as_str()));
        for origin in ["http://opencode.ai/zen/go/v1",
            "https://opencode.ai.attacker.test/zen/go/v1",
            "https://not-opencode.ai/zen/go/v1",
            "https://example.com/v1","https://api.openai.com/v1"] {
            let mut other=config.main.clone();
            other.base_url=origin.into();
            assert_eq!(run_a.opencode_session_for(&other),None,
                "private affinity must not leak to {origin}");
        }
    }
    #[test]
    fn cloudflare_room_affinity_is_explicit_scoped_and_not_sent_to_other_providers(){
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url="https://api.cloudflare.com/client/v4/accounts/example/ai/v1".into();
        cfg.main.name="@cf/zai-org/glm-4.7-flash".into();
        let client=ModelClient::new(&cfg).unwrap()
            .with_session_affinity("dominion-room-a1");
        assert_eq!(client.affinity_for(&cfg.main),Some("dominion-room-a1"));
        let another=ModelSettings{base_url:"https://api.openai.com/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&another),None);
        let local=ModelSettings{base_url:"http://127.0.0.1:11434/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&local),None);
        let malicious=ModelSettings{base_url:"https://api.cloudflare.com.attacker.test/ai/v1".into(),
            ..cfg.main.clone()};
        assert_eq!(client.affinity_for(&malicious),None);
        let selected=client.with_primary(cfg.main.clone());
        assert_eq!(selected.affinity_for(&cfg.main),Some("dominion-room-a1"));
        let selected=client.with_primary_protocol(cfg.main.clone(),"openai").unwrap();
        assert_eq!(selected.affinity_for(&cfg.main),Some("dominion-room-a1"));
    }
    #[test]
    fn retry_policy_is_precise(){
        assert!(retryable("provider HTTP 429 Too Many Requests: throttled"));
        assert!(!retryable("provider HTTP 503 Service Unavailable: maintenance"));
        assert!(!retryable("provider transport: connection reset"));
        assert!(!safe_fallback("provider body: connection reset"));
        assert!(safe_fallback("provider HTTP 429 Too Many Requests: throttled"));
        assert!(!retryable("provider HTTP 401 Unauthorized: bad token"));
        assert!(!retryable("invalid provider JSON: bad body"));
    }
    #[tokio::test]
    async fn retries_transient_provider_failure_then_accepts_real_response(){
        use axum::{extract::State,response::IntoResponse,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn flaky(State(count):State<Arc<AtomicUsize>>)->impl axum::response::IntoResponse{
            let attempt=count.fetch_add(1,Ordering::SeqCst);
            if attempt==0 {
                (axum::http::StatusCode::TOO_MANY_REQUESTS,Json(json!({"error":"slow down"}))).into_response()
            }else{
                (axum::http::StatusCode::OK,Json(json!({"choices":[{"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}))).into_response()
            }
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let router=Router::new().route("/chat/completions",post(flaky)).with_state(count.clone());
        let handle=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();
        let client=ModelClient::new(&settings).unwrap();
        let reply=client.complete(&[json!({"role":"user","content":"hi"})],&[],false).await.unwrap();
        assert_eq!(reply.text,"ok");
        assert_eq!(count.load(Ordering::SeqCst),2);
        handle.abort();
    }
    #[tokio::test]
    async fn ambiguous_server_failure_is_not_replayed_or_fallback_billed(){
        use axum::{extract::State,response::IntoResponse,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn ambiguous(State(count):State<Arc<AtomicUsize>>)->impl IntoResponse{
            count.fetch_add(1,Ordering::SeqCst);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
             Json(json!({"error":"upstream may have consumed tokens"})))
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let router=Router::new().route("/chat/completions",post(ambiguous)).with_state(count.clone());
        let server=tokio::spawn(async move{axum::serve(listener,router).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.economy.base_url=format!("http://{address}");
        cfg.economy.name="budget-model".into();cfg.economy.api_key.clear();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.name="strong-model".into();cfg.main.api_key.clear();
        let result=ModelClient::new(&cfg).unwrap().complete(
            &[json!({"role":"user","content":"a priced generation"})],&[],true).await;
        assert!(result.err().unwrap().contains("provider HTTP 500"));
        assert_eq!(count.load(Ordering::SeqCst),1,
            "uncertain billed completion must not be retried on any model");
        server.abort();
    }
    #[test]
    fn incomplete_or_filtered_provider_output_never_looks_like_completion(){
        assert!(validate_completion("openai",Some("stop"),true,false).is_ok());
        assert!(validate_completion("openai",Some("tool_calls"),false,true).is_ok());
        assert!(validate_completion("openai",Some("tool_calls"),true,true).is_ok());
        // 2026 agent failure pattern: final + tool calls must not execute
        // side-effects on this turn or leave unmatched calls for the next.
        assert!(validate_completion("openai",Some("stop"),true,true).is_err());
        assert!(validate_completion("openai",Some("stop"),false,true).is_err());
        assert!(validate_completion("anthropic",Some("end_turn"),true,true).is_err());
        assert!(validate_completion("anthropic",Some("stop_sequence"),false,true).is_err());
        assert!(validate_completion("openai",None,true,false).is_ok());
        assert!(validate_completion("openai",Some("length"),true,false).is_err());
        assert!(validate_completion("openai",Some("tool_calls"),false,false).is_err());
        assert!(validate_completion("openai",Some("content_filter"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("max_tokens"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("pause_turn"),true,false).is_err());
        assert!(validate_completion("anthropic",Some("tool_use"),false,true).is_ok());
        assert!(validate_completion("anthropic",Some("tool_use"),false,false).is_err());
        assert!(validate_completion("anthropic",Some("end_turn"),false,false).is_err());
    }
    #[tokio::test]
    async fn terminal_stop_with_tool_calls_is_rejected_before_native_dispatch(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn contradictory(State(count):State<Arc<AtomicUsize>>)->Json<Value>{
            count.fetch_add(1,Ordering::SeqCst);
            Json(json!({"choices":[{"finish_reason":"stop","message":{
                "role":"assistant","content":"Task completed.",
                "tool_calls":[{"id":"call_should_not_run","type":"function",
                    "function":{"name":"write_file","arguments":r#"{"path":"unexpected.txt","content":"unsafe"}"#}}]
            }}],"usage":{"prompt_tokens":100,"completion_tokens":40}}))
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/chat/completions",post(contradictory))
            .with_state(Arc::clone(&count));
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.api_key.clear();
        let error=ModelClient::new(&cfg).unwrap().complete(
            &[json!({"role":"user","content":"Read a file and finish"})],&[],false).await
            .err().expect("contradictory terminal tool call must not reach the harness");
        assert!(error.contains("stop_reason=stop"));
        assert_eq!(count.load(Ordering::SeqCst),1,
            "reject the supplier reply without an implicit retry");
        server.abort();
    }

    #[tokio::test]
    async fn truncated_billable_reply_is_not_retried_or_accepted(){
        use axum::{extract::State,routing::post,Json,Router};
        use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
        async fn cut_off(State(count):State<Arc<AtomicUsize>>)->Json<Value>{
            count.fetch_add(1,Ordering::SeqCst);
            Json(json!({"choices":[{"finish_reason":"length","message":{
                "role":"assistant","content":"I finished the task"
            }}],"usage":{"prompt_tokens":100,"completion_tokens":1000}}))
        }
        let count=Arc::new(AtomicUsize::new(0));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/chat/completions",post(cut_off))
            .with_state(Arc::clone(&count));
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.api_key.clear();
        let error=ModelClient::new(&cfg).unwrap().complete(
            &[json!({"role":"user","content":"return a file"})],&[],false).await
            .err().expect("truncated response must be rejected");
        assert!(error.contains("incomplete"));
        assert!(error.contains("already be billed"));
        assert_eq!(count.load(Ordering::SeqCst),1);
        server.abort();
    }
    #[test]
    fn automatic_anthropic_cache_is_first_party_only(){
        assert!(anthropic_native_cache("https://api.anthropic.com/v1"));
        assert!(!anthropic_native_cache("http://api.anthropic.com/v1"));
        assert!(!anthropic_native_cache("https://api.anthropic.com.evil.test/v1"));
        assert!(!anthropic_native_cache("https://proxy.example.org/anthropic"));
        assert!(!anthropic_native_cache("http://localhost:8111/v1"));
    }
    #[test]
    fn billed_usage_splits_cached_and_uncached_tokens(){
        let mut settings=Settings::from_env().unwrap().main;
        settings.input_krw_per_million=1000.0;
        settings.cached_input_krw_per_million=100.0;
        settings.output_krw_per_million=2000.0;
        settings.cache_write_krw_per_million=2500.0;
        let cost=estimate_cost(&settings,1_000_000,400_000,0,100_000);
        assert!((cost-840.0).abs()<0.00001);
        assert!((estimate_cost(&settings,100,200,0,0)-0.01).abs()<0.00001);
        let with_writes=estimate_cost(&settings,1_000_000,400_000,100_000,100_000);
        assert!((with_writes-990.0).abs()<0.00001);
    }
    #[test]
    fn empty_key_may_connect_to_local_server() {
        let settings = Settings::from_env().unwrap();
        assert!(ModelClient::new(&settings).is_ok());
    }
