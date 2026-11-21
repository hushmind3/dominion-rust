use super::*;
    #[test]
    fn tiny_room_context_stays_bounded_without_trusting_peer_notes(){
        let participant=Participant{name:"critic".into(),role:"review".into(),economy:false};
        let mut room=Room{id:Uuid::new_v4(),topic:"tiny room".into(),
            participants:vec![participant.clone()],messages:vec![],status:RoomStatus::Open,
            rounds:0,max_rounds:100,cost_krw:0.0,budget_krw:1000.0,
            created_at_ms:0,updated_at_ms:0,revision:0,used_tokens:0,
            usage_receipts:vec![],remote_continuations:HashMap::new(),read_cursors:HashMap::new(),finish_reason:String::new(),last_provider_failure:None,
            frontend:None,client_messages:HashMap::new(),recipients:vec![]};
        for i in 0..50{
            room.messages.push(Utterance{speaker:if i%2==0{"user"}else{"peer"}.into(),
                content:format!("Turn #{i}: {}", "new data ".repeat(120)),
                at_ms:i as u64,model:None});
        }
        let context=project_room_context(&room,&participant,600);
        assert!(context.projected_chars<=600);
        assert!(context.messages.iter().all(|m|m["role"]=="user"));
        assert!(context.omitted_messages>0);
    }
    async fn fake_responses(
        axum::extract::State(requests):axum::extract::State<
            Arc<Mutex<Vec<Value>>>>,
        axum::Json(body):axum::Json<Value>,
    )->axum::Json<Value>{
        let mut observed=requests.lock().await;
        observed.push(body);
        let index=observed.len();
        axum::Json(json!({
            "id":format!("resp-test-{index}"),
            "status":"completed",
            "output":[{"type":"message","role":"assistant",
                "content":[{"type":"output_text",
                    "text":format!("Agent analysis {index}")}]}],
            "usage":{"input_tokens":4000,"output_tokens":12,
                "input_tokens_details":{
                    "cached_tokens":if index>1{3200}else{0}}}
        }))
    }
    #[tokio::test]
    async fn nine_remote_agents_reuse_each_original_persona_across_three_owner_turns(){
        use axum::{routing::post,Router};
        use crate::config::Settings;
        let seen=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.unwrap();
        let address=listener.local_addr().unwrap();
        let server={
            let seen=seen.clone();
            tokio::spawn(async move{
                axum::serve(listener,Router::new()
                    .route("/responses",post(fake_responses))
                    .with_state(seen)).await.unwrap();
            })
        };
        let mut cfg=Settings::from_env().unwrap();
        cfg.main.base_url=format!("http://{address}");
        cfg.main.name="mock-remote-stateful".into();
        cfg.main.api_key.clear();
        cfg.economy=cfg.main.clone();
        let model=ModelClient::new(&cfg).unwrap()
            .with_primary_protocol(cfg.main.clone(),"responses").unwrap();
        let root=std::env::temp_dir().join(format!(
            "dominion-9by3-remote-{}",Uuid::new_v4()));
        let store=Groups::open(root.clone()).await.unwrap();
        let members=(0..9).map(|i|Participant{
            name:format!("agent-{i}"),
            role:format!("private skill for agent {i}"),economy:false
        }).collect::<Vec<_>>();
        let room=store.create(NewRoom{
            topic:"Shared test task".into(),participants:members.clone(),
            max_rounds:Some(50),budget_krw:Some(10000.0)
        }).await.unwrap();
        for owner in 0..3{
            store.say(room.id,format!("Owner message {}",owner+1)).await.unwrap();
            for member in &members{
                store.next_for(&model,room.id,Some(&member.name))
                    .await.unwrap();
            }
        }
        let room=store.get(room.id).await.unwrap();
        assert_eq!(room.messages.len(),30);
        assert_eq!(room.rounds,27);
        assert_eq!(room.usage_receipts.len(),27);
        assert_eq!(room.remote_continuations.len(),9);
        assert_eq!(room.usage_receipts.iter()
            .filter(|r|r.reused_server_continuation).count(),18);
        assert!(room.usage_receipts[9].wire_prompt_chars
            <room.usage_receipts[0].wire_prompt_chars,
            "second owner turn must upload less than the first full persona");
        let bodies=seen.lock().await.clone();
        assert_eq!(bodies.len(),27);
        for i in 0..9{
            assert_eq!(bodies[i]["previous_response_id"],Value::Null);
            let first=bodies[i]["input"].to_string();
            assert!(first.contains(&format!("private skill for agent {i}")));
        }
        for i in 9..27{
            let prev=i-8;
            assert_eq!(bodies[i]["previous_response_id"],
                format!("resp-test-{prev}"));
            let input=bodies[i]["input"].to_string();
            assert!(!input.contains("private skill for agent"),
                "repeat group calls must send delta, not a full persona");
            assert!(!input.contains("Shared test task"),
                "common first-turn instructions remain in provider-side history");
            assert!(!bodies[i]["input"].as_array().unwrap().is_empty());
        }
        server.abort();drop(store);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    async fn responses_with_dropped_remote_pointer(
        axum::extract::State(requests):axum::extract::State<
            Arc<Mutex<Vec<Value>>>>,
        axum::Json(body):axum::Json<Value>,
    )->(axum::http::StatusCode,axum::Json<Value>){
        let mut seen=requests.lock().await;
        seen.push(body.clone());
        let ordinal=seen.len();
        if body["previous_response_id"]=="resp-stale-1"{
            return (axum::http::StatusCode::NOT_FOUND,axum::Json(json!({
                "error":{"param":"previous_response_id",
                    "code":"previous_response_not_found",
                    "message":"Previous response not found"}
            })));
        }
        (axum::http::StatusCode::OK,axum::Json(json!({
            "id":format!("resp-stale-{ordinal}"),"status":"completed",
            "output_text":format!("Completed analysis {ordinal}"),
            "usage":{"input_tokens":100,"output_tokens":14,
                "input_tokens_details":{"cached_tokens":20}}
        })))
    }
    #[tokio::test]
    async fn stale_remote_history_rebases_once_from_durable_room_and_resumes_delta(){
        use axum::{routing::post,Router};
        use crate::config::Settings;
        let observed=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.unwrap();
        let address=listener.local_addr().unwrap();
        let server={
            let observed=observed.clone();
            tokio::spawn(async move{
                axum::serve(listener,Router::new()
                    .route("/responses",post(responses_with_dropped_remote_pointer))
                    .with_state(observed)).await.unwrap();
            })
        };
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{address}");
        settings.main.api_key.clear();
        settings.main.name="mock-stale-remote".into();
        settings.economy=settings.main.clone();
        let model=ModelClient::new(&settings).unwrap()
            .with_primary_protocol(settings.main.clone(),"responses").unwrap();
        let root=std::env::temp_dir().join(format!(
            "dominion-stale-chain-{}",Uuid::new_v4()));
        let group=Groups::open(root.clone()).await.unwrap();
        let room=group.create(NewRoom{
            topic:"Continuity after remote expiry".into(),
            participants:vec![Participant{
                name:"reviewer".into(),
                role:"quality auditor of durable decisions".into(),
                economy:false
            }],
            max_rounds:Some(12),budget_krw:Some(10000.0)
        }).await.unwrap();
        group.say(room.id,"First decision: keep the journal".into())
            .await.unwrap();
        group.next_for(&model,room.id,Some("reviewer")).await.unwrap();
        group.say(room.id,"Second decision: keep the journal".into())
            .await.unwrap();
        group.next_for(&model,room.id,Some("reviewer")).await.unwrap();
        let partial=group.get(room.id).await.unwrap();
        assert_eq!(partial.messages.len(),4);
        assert_eq!(partial.usage_receipts.len(),2,
            "upstream rejected the stale pointer before inference");
        assert_eq!(partial.usage_receipts.iter()
            .filter(|r|r.remote_history_rebased).count(),1);
        assert!(!partial.usage_receipts[1].reused_server_continuation);
        assert!(partial.usage_receipts[1].wire_prompt_chars
            >partial.usage_receipts[0].wire_prompt_chars/2,
            "recovery must send reconstructed full history, not stale delta");
        assert_eq!(partial.remote_continuations["reviewer"].previous_response_id,
            "resp-stale-3");
        group.say(room.id,"Third owner: continue from last decision".into())
            .await.unwrap();
        group.next_for(&model,room.id,Some("reviewer")).await.unwrap();
        let all=observed.lock().await.clone();
        assert_eq!(all.len(),4,"1 full, 1 rejected delta, 1 safe full, 1 delta");
        assert_eq!(all[0]["previous_response_id"],Value::Null);
        assert_eq!(all[1]["previous_response_id"],"resp-stale-1");
        assert_eq!(all[2]["previous_response_id"],Value::Null);
        assert!(all[2]["input"].to_string().contains(
            "quality auditor of durable decisions"));
        assert_eq!(all[3]["previous_response_id"],"resp-stale-3");
        assert!(!all[3]["input"].to_string().contains(
            "quality auditor of durable decisions"));
        let finished=group.get(room.id).await.unwrap();
        assert_eq!(finished.messages.len(),6);
        assert_eq!(finished.usage_receipts.len(),3);
        assert_eq!(finished.rounds,3);
        assert!(finished.usage_receipts[2].reused_server_continuation);
        assert_eq!(finished.used_tokens,3*114);
        let restored:Room=serde_json::from_value(
            serde_json::to_value(&finished).unwrap()).unwrap();
        assert!(restored.usage_receipts[1].remote_history_rebased);
        server.abort();drop(group);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn remote_responses_chain_sends_persona_once_then_only_delta_across_restart(){
        use axum::{routing::post,Router};
        use crate::config::Settings;
        let requests=Arc::new(Mutex::new(Vec::<Value>::new()));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0")
            .await.unwrap();
        let port=listener.local_addr().unwrap();
        let server={
            let requests=requests.clone();
            tokio::spawn(async move{
                axum::serve(listener,Router::new().route("/responses",
                    post(fake_responses)).with_state(requests))
                    .await.unwrap();
            })
        };
        let mut settings=Settings::from_env().unwrap();
        settings.main.base_url=format!("http://{port}");
        settings.main.name="fake-responses-model".into();
        settings.main.api_key.clear();
        settings.economy=settings.main.clone();
        let model=ModelClient::new(&settings).unwrap()
            .with_primary_protocol(settings.main.clone(),"responses").unwrap();
        let root=std::env::temp_dir()
            .join(format!("dominion-room-response-chain-{}",Uuid::new_v4()));
        let group=Groups::open(root.clone()).await.unwrap();
        let member=Participant{
            name:"analyst".into(),role:"original durable persona".into(),economy:false
        };
        let room=group.create(NewRoom{
            topic:"test lasting instructions".into(),participants:vec![member],
            max_rounds:Some(12),budget_krw:Some(10000.0)
        }).await.unwrap();
        group.say(room.id,"First owner message".into()).await.unwrap();
        group.next_for(&model,room.id,Some("analyst")).await.unwrap();
        let stored=group.get(room.id).await.unwrap();
        assert_eq!(stored.read_cursors["analyst"].next_unread_index,2,
            "first accepted reply must atomically commit the participant cursor");
        assert_eq!(stored.read_cursors["analyst"].last_owner_index,Some(0));
        assert_eq!(stored.remote_continuations["analyst"].previous_response_id,
            "resp-test-1");
        assert_eq!(stored.messages.len(),2);
        drop(group);
        let reopened=Groups::open(root.clone()).await.unwrap();
        assert_eq!(reopened.get(room.id).await.unwrap().remote_continuations
            ["analyst"].previous_response_id,"resp-test-1");
        reopened.say(room.id,"Second owner message".into()).await.unwrap();
        reopened.next_for(&model,room.id,Some("analyst")).await.unwrap();
        reopened.say(room.id,"Third owner message".into()).await.unwrap();
        reopened.next_for(&model,room.id,Some("analyst")).await.unwrap();
        let bodies=requests.lock().await.clone();
        assert_eq!(bodies.len(),3);
        assert_eq!(bodies[0]["previous_response_id"],Value::Null);
        assert!(bodies[0]["input"].as_array().unwrap().iter().any(|m|
            m["role"]=="developer" && m["content"].as_str().unwrap()
                .contains("original durable persona")));
        for (i,prior) in [(1,"resp-test-1"),(2,"resp-test-2")]{
            assert_eq!(bodies[i]["previous_response_id"],prior);
            let payload=bodies[i]["input"].to_string();
            assert!(!payload.contains("original durable persona"),
                "remote server already holds the persona; no second full prompt");
            assert!(!payload.contains("test lasting instructions"),
                "remote server already holds the shared group system header");
            assert!(payload.contains(&format!(
                "{} owner message",if i==1{"Second"}else{"Third"})));
            assert_eq!(bodies[i]["input"].as_array().unwrap().len(),1,
                "previous own reply is already part of the remote response chain");
        }
        let room=reopened.get(room.id).await.unwrap();
        assert_eq!(room.messages.len(),6);
        assert_eq!(room.usage_receipts.len(),3);
        assert_eq!(room.read_cursors["analyst"].next_unread_index,6);
        assert_eq!(room.read_cursors["analyst"].last_reply_index,Some(5));
        assert_eq!(room.unread_range("analyst"),6..6);
        assert_eq!(room.usage_receipts.iter()
            .map(|r|r.input_tokens).sum::<u64>(),12000,
            "remote continuation reduces wire payload, not necessarily billed context");
        assert_eq!(room.usage_receipts.iter()
            .map(|r|r.cached_input_tokens).sum::<u64>(),6400);
        assert_eq!(room.usage_receipts.iter()
            .filter(|r|r.reused_server_continuation).count(),2);
        assert!(room.usage_receipts[1].wire_prompt_chars
            <room.usage_receipts[0].wire_prompt_chars);
        assert_eq!(room.remote_continuations["analyst"].previous_response_id,
            "resp-test-3");
        server.abort();
        drop(reopened);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn read_cursors_recover_from_legacy_and_preserve_independent_speaker_positions(){
        let root=std::env::temp_dir().join(format!("dominion-cursor-{}",Uuid::new_v4()));
        let groups=Groups::open(root.clone()).await.unwrap();
        let mut room=groups.create(NewRoom{topic:"read cursor".into(),participants:vec![
            Participant{name:"architect".into(),role:"design".into(),economy:false},
            Participant{name:"critic".into(),role:"review".into(),economy:false},
        ],max_rounds:Some(20),budget_krw:Some(2000.0)}).await.unwrap();
        let note=|speaker:&str,content:&str|Utterance{
            speaker:speaker.into(),content:content.into(),at_ms:now_ms(),model:None};
        room.messages.push(note("user","first task"));
        room.messages.push(note("architect","first answer"));
        assert_eq!(room.unread_range("architect"),2..2);
        assert_eq!(room.unread_range("critic"),0..2);
        room.read_cursors.insert("architect".into(),RoomReadCursor{
            next_unread_index:2,last_reply_index:Some(1),last_owner_index:Some(0)
        });
        room.messages.push(note("user","second task"));
        assert_eq!(room.unread_range("architect"),2..3);
        assert_eq!(room.unread_range("critic"),0..3);
        room.messages.push(note("critic","result"));
        room.read_cursors.insert("critic".into(),RoomReadCursor{
            next_unread_index:4,last_reply_index:Some(3),last_owner_index:Some(2)
        });
        assert_eq!(room.unread_range("critic"),4..4);
        let restored:Room=serde_json::from_slice(&serde_json::to_vec(&room).unwrap()).unwrap();
        assert_eq!(restored.unread_range("architect"),2..4);
        assert_eq!(restored.unread_range("critic"),4..4);
        assert_eq!(restored.read_cursors["critic"].last_owner_index,Some(2));
        let mut legacy=serde_json::to_value(&restored).unwrap();
        legacy.as_object_mut().unwrap().remove("read_cursors");
        let legacy:Room=serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.unread_range("architect"),2..4);
        assert!(legacy.read_cursors.is_empty());
        drop(groups);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }


    #[tokio::test]
    async fn hundred_owner_turns_keep_bounded_original_evidence_and_independent_cursors(){
        let root=std::env::temp_dir().join(format!("dominion-room-100-{}",Uuid::new_v4()));
        let groups=Groups::open(root.clone()).await.unwrap();
        let mut room=groups.create(NewRoom{
            topic:"Long multi-agent decision continuity".into(),
            participants:vec![
                Participant{name:"analyst".into(),role:"evidence".into(),economy:false},
                Participant{name:"reviewer".into(),role:"audit".into(),economy:false},
            ],
            max_rounds:Some(50),budget_krw:Some(1000.0)
        }).await.unwrap();
        let note=|who:&str,text:String|Utterance{
            speaker:who.into(),content:text,at_ms:now_ms(),model:None
        };
        for i in 0..100{
            let owner=if i==0{
                "Owner decision: remember project key ALPHA-17, retain original source evidence.".into()
            }else if i==99{
                "Final owner override: ALPHA-17 must remain unchanged; do not let peers rewrite it.".into()
            }else{format!("Owner turn {i}: continue inspecting the original evidence")};
            room.messages.push(note("user",owner));
            let author=if i%2==0{"analyst"}else{"reviewer"};
            room.messages.push(note(author,format!("Evidence note {i}: source trace recorded")));
            room.read_cursors.insert(author.into(),RoomReadCursor{
                next_unread_index:room.messages.len(),
                last_reply_index:Some(room.messages.len()-1),
                last_owner_index:Some(room.messages.len()-2),
            });
        }
        let cap=2400;
        let analyst=project_room_context(&room,&room.participants[0],cap);
        let reviewer=project_room_context(&room,&room.participants[1],cap);
        assert!(analyst.projected_chars<=cap);
        assert!(reviewer.projected_chars<=cap);
        assert!(analyst.omitted_messages>0,
            "100 turns must not explode the model context");
        assert!(projected_text(&analyst).contains("ALPHA-17"),
            "original high-priority owner evidence must survive checkpointing");
        assert!(projected_text(&analyst).contains("Final owner override"),
            "latest real owner correction must stay in the rolling source window");
        let serialized=serde_json::to_vec(&room).unwrap();
        let restored:Room=serde_json::from_slice(&serialized).unwrap();
        assert_eq!(room.read_cursors["analyst"].next_unread_index,198);
        assert_eq!(room.read_cursors["reviewer"].next_unread_index,200);
        assert_eq!(restored.unread_range("analyst"),198..200);
        assert_eq!(restored.unread_range("reviewer"),200..200);
        let after=project_room_context(&restored,&restored.participants[0],cap);
        assert_eq!(projected_text(&analyst),projected_text(&after),
            "the same original room source must project identical context after restart");
        assert_eq!(analyst.checkpoint_end,after.checkpoint_end);
        drop(groups);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn mention_selects_correct_participant() {
        let root=std::env::temp_dir().join(format!("dominion-room-{}",Uuid::new_v4()));
        let store=Groups::open(root.clone()).await.unwrap();
        let mut room=store.create(NewRoom{topic:"review".into(),participants:vec![
            Participant{name:"architect".into(),role:"design".into(),economy:false},
            Participant{name:"critic".into(),role:"testing".into(),economy:true}
        ],max_rounds:Some(6),budget_krw:Some(0.0)}).await.unwrap();
        room.messages.push(Utterance{speaker:"user".into(),content:"@critic assess this".into(),at_ms:now_ms(),model:None});
        assert_eq!(Groups::select(&room),1);
        fs::remove_dir_all(root).await.unwrap();
    }
    #[tokio::test]
    async fn same_owner_message_id_cannot_switch_target_agents_even_after_restart(){
        let root=std::env::temp_dir()
            .join(format!("dominion-room-target-dedup-{}",Uuid::new_v4()));
        let groups=Groups::open(root.clone()).await.unwrap();
        let room=groups.create(NewRoom{topic:"route owner correction".into(),
            participants:vec![
                Participant{name:"architect".into(),role:"design".into(),economy:false},
                Participant{name:"reviewer".into(),role:"review".into(),economy:false}
            ],max_rounds:Some(20),budget_krw:Some(100.0)}).await.unwrap();
        let first=groups.say_once_scoped(room.id,"Review this code".into(),
            "owner-1",vec!["architect".into()],vec!["architect".into()])
            .await.unwrap();
        assert!(first.1);
        let same=groups.say_once_scoped(room.id,"Review this code".into(),
            "owner-1",vec!["architect".into()],vec!["architect".into()])
            .await.unwrap();
        assert!(!same.1);
        assert_eq!(same.2,first.2);
        assert!(groups.say_once_scoped(room.id,"Review this code".into(),
            "owner-1",vec!["reviewer".into()],vec!["reviewer".into()])
            .await.unwrap_err().contains("audience"));
        drop(groups);
        let reopened=Groups::open(root.clone()).await.unwrap();
        assert!(reopened.say_once_scoped(room.id,"Review this code".into(),
            "owner-1",vec!["reviewer".into()],vec!["reviewer".into()])
            .await.unwrap_err().contains("audience"));
        let accepted=reopened.say_once_scoped(room.id,"Review this code".into(),
            "owner-1",vec!["architect".into()],vec!["architect".into()])
            .await.unwrap();
        assert!(!accepted.1);
        assert_eq!(accepted.0.messages.iter().filter(|m|m.speaker=="user").count(),1);
        let original=serde_json::to_value(&accepted.0).unwrap();
        assert_eq!(original["client_messages"]["owner-1"]["requested_agents"],
            json!(["architect"]));
        let mut legacy=original;
        legacy["client_messages"]["owner-1"]=json!(0);
        let restored:Room=serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.client_messages["owner-1"].index(),0);
        assert!(restored.client_messages["owner-1"].matches_agents(&["reviewer".into()]),
            "old snapshots without target metadata must remain readable");
        drop(reopened);
        fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn round_robin_recipient_is_frozen_in_the_durable_owner_turn(){
        let root=std::env::temp_dir().join(format!("dominion-room-rr-pin-{}",Uuid::new_v4()));
        let groups=Groups::open(root.clone()).await.unwrap();
        let participants=vec![
            Participant{name:"coder".into(),role:"code".into(),economy:false},
            Participant{name:"reviewer".into(),role:"verify".into(),economy:false},
        ];
        let mut room=groups.create(NewRoom{topic:"single owner turn".into(),
            participants,max_rounds:Some(20),budget_krw:Some(0.0)}).await.unwrap();
        room.frontend=Some(FrontendRoom{
            name:"rr".into(),topic:"single owner turn".into(),owner_name:"user".into(),
            agent_ids:vec!["coder".into(),"reviewer".into()],
            routing_mode:"round_robin".into(),rounds:1,max_turns:1,
            lifetime_max_responses:20,token_budget:0,context_budget_tokens:2000,
            tools_enabled:false,auto_learning:false,auto_models:true,models:vec![],
            reasoning_effort:String::new(),learned_context:String::new(),
        });
        groups.configure(room.id,room.frontend.clone().unwrap(),room.participants.clone())
            .await.unwrap();
        let (first,inserted,index)=groups.say_once(room.id,"fix it".into(),
            "rr-idempotent-1",Vec::new()).await.unwrap();
        assert!(inserted);
        assert_eq!(index,0);
        assert_eq!(first.recipients,vec!["coder"]);
        let (retry,inserted_again,again)=groups.say_once(room.id,"fix it".into(),
            "rr-idempotent-1",Vec::new()).await.unwrap();
        assert!(!inserted_again);
        assert_eq!(again,index);
        assert_eq!(retry.recipients,first.recipients);
        drop(groups);
        let restored=Groups::open(root.clone()).await.unwrap().get(room.id).await.unwrap();
        assert_eq!(restored.recipients,vec!["coder"],
            "the owner-to-agent assignment must survive a process restart");
        let mut after_reply=restored;
        after_reply.rounds+=1;
        after_reply.messages.push(Utterance{speaker:"coder".into(),
            content:"result".into(),at_ms:now_ms(),model:None});
        assert_eq!(Groups::select(&after_reply),0,
            "reply accounting cannot silently redirect the same owner turn");
        fs::remove_dir_all(root).await.unwrap();
    }

