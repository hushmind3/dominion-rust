//! Extracted isolated regression tests for src/engine/chat/continuity.rs.
use super::*;
    fn state(root: &std::path::Path) -> AppState {
        let config = crate::config::Config { port:0, data_dir:root.into(), web_dir:root.join("web"), workspace:root.into() };
        AppState::new(config.clone(),crate::db::Store::open(&config.db_path()).unwrap(),crate::vault::Vault::open(&config.key_path()))
    }
    fn route() -> ProviderRef {
        ProviderRef { id:"p".into(),name:"test".into(),base_url:"https://example.test/codex".into(),api_key:"never-store-this".into(),model:"model-a".into(),tool_mode:"native".into(),reasoning_protocol:"openai".into(),local:false,context_tokens:None,max_output_tokens:None,requests_per_minute:None,kind:"codex".into(),account_id:Some("account-a".into()) }
    }
    #[test]
    fn continuity_survives_restart_and_separates_bots_routes_and_prefix_revisions() {
        let root=std::env::temp_dir().join(format!("dominion-continuity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        let request=TurnRequest::from_value(&json!({"input":"first","agentId":"a"})).unwrap();
        let provider=route();
        let first=prepare(&state,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["samePrefixAsPreviousRequest"],false);
        let mut next=request.clone();next.input="second, different content".into();
        let second=prepare(&state,&provider,&next,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],second["cacheKey"]);
        assert_eq!(second["samePrefixAsPreviousRequest"],true);
        commit(&state,&first,&json!({"id":"not-persisted"}));
        let id=first["id"].as_str().unwrap();
        assert!(state.store.get("model_conversation",id).unwrap()["lastReplyMessageId"].is_null());
        let message=state.store.write_message(&json!({"session_id":"room:one","agent_id":"a","role":"assistant","content":"my previous answer","metadata":{"usage":{"cachedPromptTokens":0}}})).unwrap();
        commit(&state,&first,&message);
        let other=|provider:&ProviderRef,request:&TurnRequest,session:&str,prefix:&str|prepare(&state,provider,request,session,prefix,&json!({"effort":"low"})).unwrap();
        let mut peer=request.clone();peer.agent_id=Some("b".into());
        assert_ne!(first["cacheKey"],other(&provider,&peer,"room:one","full persona")["cacheKey"]);
        let another_room = other(&provider,&request,"room:two","another room contract");
        assert_eq!(first["cacheKey"],another_room["cacheKey"]);
        assert_eq!(first["cacheAffinityId"],another_room["cacheAffinityId"]);
        assert_ne!(first["participantId"],another_room["participantId"]);
        assert_ne!(first["transportSessionId"],another_room["transportSessionId"]);
        assert!(another_room["previousReplyMessageId"].is_null());
        let private_chat=other(&provider,&request,"private:one","private chat contract");
        assert_eq!(first["cacheKey"],private_chat["cacheKey"]);
        assert!(private_chat["previousReplyMessageId"].is_null());
        for change in ["model","account","endpoint"] {
            let mut changed=provider.clone();
            match change {"model"=>changed.model="model-b".into(),"account"=>changed.account_id=Some("account-b".into()),_=>changed.base_url="https://other.test".into()}
            assert_ne!(first["cacheKey"],other(&changed,&request,"room:one","full persona")["cacheKey"]);
        }
        let changed_contract=other(&provider,&request,"room:one","edited room contract");
        assert_eq!(first["cacheKey"],changed_contract["cacheKey"]);
        assert_eq!(changed_contract["samePrefixAsPreviousRequest"],false);
        state.store.put("agent",&json!({"id":"a","systemPrompt":"edited persona"}));
        assert_ne!(first["cacheKey"],other(&provider,&request,"room:one","full persona")["cacheKey"]);
        state.store.remove("agent","a");
        assert!(!state.store.get("model_conversation",id).unwrap().to_string().contains("never-store-this"));
        for kind in ["openai","anthropic","ollama","llamacpp","lmstudio"] {
            let mut changed=provider.clone();changed.kind=kind.into();changed.model=format!("{kind}-test");
            let transferred=other(&changed,&request,"room:one","full persona");
            assert_eq!(transferred["participantId"],first["participantId"]);
            assert_eq!(transferred["previousReplyMessageId"],message["id"]);
            assert_eq!(transferred["transport"],kind);
        }
        drop(state);
        let restored=super::tests::state(&root);
        let resumed=prepare(&restored,&provider,&request,"room:one","full persona",&json!({"effort":"low"})).unwrap();
        assert_eq!(first["cacheKey"],resumed["cacheKey"]);
        assert_eq!(resumed["previousReplyMessageId"],message["id"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_cache_rejection_must_identify_the_parameter_and_preserve_other_errors() {
        let bad=reqwest::StatusCode::BAD_REQUEST;
        assert!(rejects_hint(bad,r#"{"error":{"message":"Unrecognized request argument supplied: prompt_cache_key"}}"#,"prompt_cache_key"));
        assert!(rejects_hint(bad,r#"{"error":{"param":"system.0.cache_control","message":"not supported"}}"#,"cache_control"));
        assert!(!rejects_hint(bad,r#"{"error":{"message":"model not supported"}}"#,"prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::UNAUTHORIZED,"prompt_cache_key not supported","prompt_cache_key"));
        assert!(!rejects_hint(reqwest::StatusCode::TOO_MANY_REQUESTS,"prompt_cache_key not supported","prompt_cache_key"));
    }

    #[tokio::test]
    async fn stable_participant_header_survives_cache_hint_negotiation() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        type Seen=Arc<Mutex<Vec<String>>>;
        async fn endpoint(State(seen):State<Seen>,headers:HeaderMap,Json(body):Json<Value>)->(axum::http::StatusCode,Json<Value>){
            seen.lock().unwrap().push(headers["x-dominion-conversation"].to_str().unwrap().to_string());
            if body.get("prompt_cache_key").is_some(){
                (axum::http::StatusCode::BAD_REQUEST,Json(json!({"error":"prompt_cache_key is unsupported"})))
            }else{(axum::http::StatusCode::OK,Json(json!({"ok":true})))}
        }
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/test",post(endpoint)).with_state(seen.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let root=std::env::temp_dir().join(format!("dominion-affinity-{}",uuid::Uuid::new_v4()));
        let state=state(&root);let mut provider=route();provider.base_url=format!("http://{address}");
        provider.kind="openai".into();
        for (agent,session,text) in [("a","room:one","first"),("a","private:two","second"),("b","room:one","second")] {
            let request=TurnRequest::from_value(&json!({"agentId":agent,"input":text})).unwrap();
            let mut plan=prepare(&state,&provider,&request,session,"identity",&json!({})).unwrap();
            let response=send(&state,&provider,reqwest::Client::new().post(format!("http://{address}/test")),
                &json!({"model":"test","messages":[{"role":"user","content":text}]}),&mut plan,CacheWire::Key).await.unwrap();
            assert!(response.status().is_success());
        }
        let seen=seen.lock().unwrap();
        assert_eq!(seen.len(),4,"one negotiation retry, then no further retries");
        assert_eq!(seen[0],seen[1]);assert_eq!(seen[1],seen[2]);
        assert_ne!(seen[2],seen[3],"another bot must have a separate route affinity");
        drop(seen);server.abort();let _=std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn opencode_go_direct_sends_the_stable_session_header() {
        use axum::{extract::State,http::HeaderMap,routing::post,Json,Router};
        use std::sync::{Arc,Mutex};
        type Seen=Arc<Mutex<Vec<(String,Option<String>)>>>;
        async fn endpoint(State(seen):State<Seen>,headers:HeaderMap,Json(_body):Json<Value>)->Json<Value>{
            seen.lock().unwrap().push((
                headers["x-dominion-conversation"].to_str().unwrap_or("").to_string(),
                headers.get("x-opencode-session").and_then(|value|value.to_str().ok()).map(str::to_string),
            ));
            Json(json!({"ok":true}))
        }
        let seen:Seen=Arc::new(Mutex::new(Vec::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Router::new().route("/test",post(endpoint)).with_state(seen.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap()});
        let root=std::env::temp_dir().join(format!("dominion-opencode-go-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("provider",&json!({"id":"p","integration":"opencode-go"})).unwrap();
        let mut provider=route();provider.base_url=format!("http://{address}");provider.kind="openai".into();
        let request=TurnRequest::from_value(&json!({"agentId":"a","input":"hello"})).unwrap();
        let mut plan=prepare(&state,&provider,&request,"room:one","identity",&json!({})).unwrap();
        let response=send(&state,&provider,reqwest::Client::new().post(format!("http://{address}/test")),
            &json!({"model":"test","messages":[{"role":"user","content":"hello"}]}),&mut plan,CacheWire::Key).await.unwrap();
        assert!(response.status().is_success());
        let seen=seen.lock().unwrap();
        let affinity=plan["cacheAffinityId"].as_str().unwrap();
        assert_eq!(seen[0].0,affinity);
        assert_eq!(seen[0].1.as_deref(),Some(affinity),"direct OpenCode Go must receive the session affinity");
        drop(seen);server.abort();let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn committed_working_view_keeps_wire_prefix_when_selection_slides_and_routes_change() {
        let root=std::env::temp_dir().join(format!("dominion-projection-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:test","roomId":"test"}));
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let request=TurnRequest::from_value(&json!({"input":"same words","agentId":"a"})).unwrap();
        let provider=route();
        let frame=|id:&str, text:&str|json!({"role":"user","content":text,"_dominion_frame":id});
        let system=|source:&str|json!({"role":"system","content":"full immutable identity","_dominion_source":source});
        let first=vec![system("owner-1"),frame("older","old correction"),frame("owner-1","same words")];
        let mut plan=prepare(&state,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        let (first_wire,pending)=project(&state,&provider,&first,&[],"room:test",&mut plan);
        assert!(!json!(first_wire).to_string().contains("_dominion_"));
        assert!(state.store.get("participant_conversation",plan["participantId"].as_str().unwrap()).unwrap()["workingView"].is_null(),"uncommitted requests cannot change the working view");
        let saved=state.store.write_message(&json!({"session_id":"room:test","agent_id":"a","role":"assistant","content":"I remember","metadata":{"replyToMessageId":"owner-1"}})).unwrap();
        commit(&state,&plan,&saved);
        commit_projection(&state,&plan,&saved,pending.unwrap());
        let mut changed=provider.clone();changed.kind="ollama".into();changed.model="other-model".into();
        let mut second_plan=prepare(&state,&changed,&request,"room:test","full immutable identity",&json!({})).unwrap();
        // The old correction was omitted by the new selector. Identical text
        // from a NEW owner source must still be appended as a new request.
        let mut own_reply=frame(saved["id"].as_str().unwrap(),"I remember");own_reply["role"]=json!("assistant");
        let second=vec![system("owner-2"),own_reply,frame("owner-2","same words")];
        let (second_wire,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(&second_wire[..first_wire.len()],first_wire.as_slice());
        assert_eq!(second_wire.iter().filter(|row|row["content"]=="same words").count(),2);
        assert_eq!(second_plan["workingView"]["reusedFrames"],3);
        assert_eq!(second_plan["workingView"]["appendedFrames"],1);
        // A changed persona invalidates the prefix without dropping the fresh
        // selected evidence, and budget rotation never cuts mandatory input.
        let mut edited=second.clone();edited[0]["content"]=json!("edited identity");
        let (rebased,_)=project(&state,&changed,&edited,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"contract_changed");
        assert_eq!(rebased.len(),edited.len());
        state.store.put("room",&json!({"id":"test","contextBudgetTokens":1}));
        let (bounded,_)=project(&state,&changed,&second,&[],"room:test",&mut second_plan);
        assert_eq!(second_plan["workingView"]["rebaseReason"],"history_budget");
        assert_eq!(bounded.len(),second.len());
        drop(state);
        let restored=super::tests::state(&root);
        let mut resumed=prepare(&restored,&provider,&request,"room:test","full immutable identity",&json!({})).unwrap();
        restored.store.put("room",&json!({"id":"test","contextBudgetTokens":2000}));
        let (after_restart,_)=project(&restored,&provider,&second,&[],"room:test",&mut resumed);
        assert_eq!(after_restart,second_wire);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replay_does_not_duplicate_tool_pairs_or_retain_revoked_knowledge() {
        let root=std::env::temp_dir().join(format!("dominion-projection-tools-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"s"}));
        state.store.put("skill",&json!({"id":"skill","instructions":"old procedure","enabled":true}));
        let request=TurnRequest::from_value(&json!({"input":"work","agentId":"a"})).unwrap();
        let provider=route();
        let deps=dependencies(&state,&[json!({"kind":"skill","id":"skill"})]);
        let system=json!({"role":"system","content":"identity","_dominion_source":"owner1","_dominion_dependencies":deps});
        let call=json!({"role":"assistant","content":"","tool_calls":[{"id":"call1","type":"function","function":{"name":"read","arguments":"{}"}}]});
        let result=json!({"role":"tool","tool_call_id":"call1","content":"done"});
        let messages=vec![system.clone(),json!({"role":"user","content":"old procedure","_dominion_frame":"owner1"}),call.clone(),result.clone()];
        let mut plan=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (_,pending)=project(&state,&provider,&messages,&[],"s",&mut plan);
        let saved=state.store.write_message(&json!({"session_id":"s","agent_id":"a","role":"assistant","content":"finished","metadata":{"replyToMessageId":"owner1"}})).unwrap();
        commit(&state,&plan,&saved);commit_projection(&state,&plan,&saved,pending.unwrap());
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"lastUsedAt":"new telemetry","useCount":100}));
        let mut replay=vec![system.clone(),call,result,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        replay[0]["_dominion_source"]=json!("owner2");
        replay[1]["_dominion_frame"]=json!("stored-call-row");replay[2]["_dominion_frame"]=json!("stored-result-row");
        let mut next=prepare(&state,&provider,&request,"s","identity",&json!({})).unwrap();
        let (wire,_)=project(&state,&provider,&replay,&[],"s",&mut next);
        assert!(next["workingView"]["rebaseReason"].is_null());
        assert_eq!(wire.iter().filter(|row|row["tool_calls"].is_array()).count(),1);
        assert_eq!(wire.iter().filter(|row|row["role"]=="tool").count(),1);
        state.store.patch_if("skill","skill","enabled",&json!(true),&json!({"enabled":false}));
        let mut fresh=vec![system,json!({"role":"user","content":"next","_dominion_frame":"owner2"})];
        fresh[0]["_dominion_source"]=json!("owner2");fresh[0]["_dominion_dependencies"]=json!([]);
        let (wire,_)=project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"context_evidence_changed");
        assert!(!json!(wire).to_string().contains("old procedure"));
        state.store.clear_messages("s");
        project(&state,&provider,&fresh,&[],"s",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"transcript_reset");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retrieval_batches_and_ranking_updates_do_not_resend_identical_evidence() {
        let root=std::env::temp_dir().join(format!("dominion-evidence-delta-{}",uuid::Uuid::new_v4()));
        let state=state(&root);
        state.store.put("session",&json!({"id":"room:r","roomId":"r"}));
        state.store.put("room",&json!({"id":"r","contextBudgetTokens":8000}));
        for id in ["a","b"] {
            state.store.put("memory",&json!({"id":id,"text":format!("FACT_{id}"),"scope":"shared","shareWithRooms":true,"status":"active","importance":0.8,"confidence":0.6}));
        }
        let request=TurnRequest::from_value(&json!({"input":"question","agentId":"agent"})).unwrap();
        let provider=route();
        let rows=|ids:&[&str]|ids.iter().map(|id|json!({"kind":"memory","id":id,"text":format!("FACT_{id}"),"sourceMessageIds":null})).collect::<Vec<_>>();
        let messages=|source:&str,ids:&[&str]|{
            let deps=dependencies(&state,&rows(ids));
            let mut messages=vec![json!({"role":"system","content":"FULL_PERSONA","_dominion_source":source,"_dominion_dependencies":deps})];
            messages.extend(super::super::room_context::evidence_frames(&rows(ids)));
            messages.push(json!({"role":"user","content":format!("question {source}"),"_dominion_frame":source}));
            messages
        };
        let mut first=prepare(&state,&provider,&request,"room:r","FULL_PERSONA",&json!({})).unwrap();
        let (wire,pending)=project(&state,&provider,&messages("owner1",&["a"]),&[],"room:r",&mut first);
        let reply=state.store.write_message(&json!({"session_id":"room:r","agent_id":"agent","role":"assistant","content":"chosen answer"})).unwrap();
        commit(&state,&first,&reply);commit_projection(&state,&first,&reply,pending.unwrap());
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"importance":0.99,"confidence":0.9,"revision":"ranking-update","supportingCases":["another observation"]}));
        let mut next=prepare(&state,&provider,&request,"room:r","FULL_PERSONA",&json!({})).unwrap();
        let (next_wire,_)=project(&state,&provider,&messages("owner2",&["a","b"]),&[],"room:r",&mut next);
        assert!(next["workingView"]["rebaseReason"].is_null(),"{next}");
        assert_eq!(&next_wire[..wire.len()],wire.as_slice());
        let text=json!(next_wire).to_string();
        assert_eq!(text.matches("FACT_a").count(),1);
        assert_eq!(text.matches("FACT_b").count(),1);
        assert!(!text.contains("sourceMessageIds"),"null fields are not evidence");
        // Changes to actual provenance and revocation still invalidate old data.
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"sourceAuthors":["a different author"]}));
        project(&state,&provider,&messages("owner2",&["a","b"]),&[],"room:r",&mut next);
        assert_eq!(next["workingView"]["rebaseReason"],"context_evidence_changed");
        state.store.patch_if("memory","a","status",&json!("active"),&json!({"status":"archived"}));
        let (revoked,_)=project(&state,&provider,&messages("owner2",&["b"]),&[],"room:r",&mut next);
        assert!(!json!(revoked).to_string().contains("FACT_a"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_websocket_hint_is_also_disabled_for_http_and_expires() {
        let root=std::env::temp_dir().join(format!("dominion-hint-{}",uuid::Uuid::new_v4()));
        let state=state(&root);let provider=route();
        let mut plan=json!({"cacheKey":"stable-participant-key"});
        let original=json!({"input":[{"role":"user","content":"hello"}]});
        let body=hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key);
        assert_eq!(body["prompt_cache_key"],"stable-participant-key");
        note_hint(&state,&provider,CacheWire::Key,false,400);
        let body=hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key);
        assert_eq!(body,original);assert_eq!(plan["cacheHintAcceptance"],false);
        let id=feature_id(&provider,CacheWire::Key);
        state.store.patch_if("provider_wire_feature",&id,"accepted",&json!(false),&json!({"expiresAt":0}));
        assert_eq!(hinted_body(&state,&provider,&original,&mut plan,CacheWire::Key)["prompt_cache_key"],"stable-participant-key");
        std::fs::remove_dir_all(root).unwrap();
    }
