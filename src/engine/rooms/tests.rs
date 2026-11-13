    use super::*;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    fn test_state() -> (AppState, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("dominion-shared-room-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        let state = AppState::new(config, store, vault);
        for (id, name) in [("a", "가람"), ("b", "나루"), ("c", "다온")] {
            state.store.put(
                "agent",
                &json!({"id":id,"name":name,"providerId":"p","model":"test"}),
            );
        }
        state.store.put("room",&json!({"id":"r","name":"우리 대화방","agentIds":["a","b","c"],"routingMode":"discussion","rounds":1,"maxTurns":6,"toolsEnabled":false,"autoLearning":false}));
        (state, dir)
    }
    fn room(state: &AppState) -> Value {
        state.store.get("room", "r").unwrap()
    }

    #[test]
    fn natural_language_vocative_targets_only_the_named_agent() {
        let (state, dir) = test_state();
        for (id, name) in [("s", "샤르티아"), ("a", "아인즈"), ("b", "알베도")] {
            state
                .store
                .put("agent", &json!({"id":id,"name":name}));
        }
        let roster = vec!["s".to_string(), "a".to_string(), "b".to_string()];
        assert_eq!(
            mentioned_agents(&state, &roster, "샤르티아야 니가 결과를 가져와봐라").unwrap(),
            vec!["s".to_string()]
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "샤르티아에게 시켜").unwrap(),
            vec!["s".to_string()]
        );
        assert_eq!(
            mentioned_agents(&state, &roster, "아인즈와 알베도가 확인해").unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn room_evidence_count_includes_completed_tool_operations() {
        let (state, dir) = test_state();
        state
            .store
            .put("execution", &json!({"id":"run-1","sessionId":"s","agentId":"a"}));
        state.store.put(
            "execution_operation",
            &json!({"id":"op-ok","runId":"run-1","tool":"workspace_list","status":"completed"}),
        );
        state.store.put(
            "execution_operation",
            &json!({"id":"op-fail","runId":"run-1","tool":"workspace_write","status":"failed"}),
        );
        assert_eq!(
            message_evidence_count(&state, &json!({"metadata":{"executionId":"run-1"}})),
            1
        );
        assert_eq!(
            message_evidence_count(
                &state,
                &json!({"metadata":{"evidence":[{"id":"e1"},{"id":"e2"}]}})
            ),
            2
        );
        assert_eq!(
            message_evidence_count(
                &state,
                &json!({"metadata":{"executionId":"run-1","evidence":[{"id":"e1"}]}})
            ),
            2
        );
        let _ = std::fs::remove_dir_all(dir);
    }
    fn save_owner_job(state: &AppState, id: &str, text: &str) -> Value {
        let session = ensure_room_session(state, "r").unwrap();
        let sid = session["id"].as_str().unwrap();
        let message=state.store.write_message(&json!({"id":format!("{id}:owner"),"session_id":sid,"role":"user","content":text,"metadata":{"authorKind":"owner","roomId":"r"}})).unwrap();
        state.store.put("job",&json!({"id":id,"kind":"room","status":"running","input":{"roomId":"r","sessionId":sid,"messageId":message["id"],"sourceMessageId":message["id"],"input":text,"generation":state.cancellation_generation(sid),"roomSnapshot":room(state),"explicitAgentIds":false}})).unwrap()
    }
    type Callback = Arc<dyn Fn(usize, &Value) + Send + Sync>;
    async fn provider(
        state: &AppState,
        deltas: Vec<Value>,
        callback: Option<Callback>,
    ) -> (tokio::task::JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
        use axum::{extract::State, routing::post, Json, Router};
        #[derive(Clone)]
        struct Mock {
            deltas: Arc<Vec<Value>>,
            bodies: Arc<Mutex<Vec<Value>>>,
            callback: Option<Callback>,
        }
        async fn handle(
            State(mock): State<Mock>,
            Json(body): Json<Value>,
        ) -> ([(&'static str, &'static str); 1], String) {
            let index = {
                let mut bodies = mock.bodies.lock().unwrap();
                let index = bodies.len();
                bodies.push(body.clone());
                index
            };
            if let Some(callback) = &mock.callback {
                callback(index, &body);
            }
            let delta = mock
                .deltas
                .get(index)
                .cloned()
                .unwrap_or(json!({"content":"좋아, 주인 이야기를 기다릴게."}));
            let stream = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":delta}]})
            );
            ([("content-type", "text/event-stream")], stream)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        state.store.put("provider",&json!({"id":"p","name":"mock","baseUrl":format!("http://127.0.0.1:{port}/v1"),"model":"test","enabled":true,"toolMode":"native"}));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let router = Router::new()
            .route("/v1/chat/completions", post(handle))
            .with_state(Mock {
                deltas: Arc::new(deltas),
                bodies: bodies.clone(),
                callback,
            });
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        (server, bodies)
    }
    fn text_delta(text: &str) -> Value {
        json!({"content":text})
    }
    fn run_context(state: &AppState, id: &str) -> JobContext {
        JobContext::new(state.clone(), id.to_string())
    }

    #[test]
    fn room_session_is_stable_and_cannot_adopt_private_or_other_room_chat() {
        let (state, dir) = test_state();
        let first = ensure_room_session(&state, "r").unwrap();
        let second = ensure_room_session(&state, "r").unwrap();
        assert_eq!(first["id"], second["id"]);
        assert_eq!(state.store.count("session"), 1);
        state
            .store
            .put("session", &json!({"id":"private","title":"private"}));
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"sessionId":"private"}),
        );
        assert!(ensure_room_session(&state, "r").is_err());
        state
            .store
            .put("session", &json!({"id":"other-room","roomId":"other"}));
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"sessionId":"other-room"}),
        );
        assert!(ensure_room_session(&state, "r").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn short_casual_requests_use_a_small_speaker_window() {
        let (state, dir) = test_state();
        for (id, name) in [("d", "라온"), ("e", "마루")] {
            state.store.put(
                "agent",
                &json!({"id":id,"name":name,"providerId":"p","model":"test"}),
            );
        }
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a","b","c","d","e"],"maxTurns":0}),
        );
        let config = room(&state);
        let casual = plan_speakers(&state, &config, "안녕", None).unwrap();
        assert_eq!(casual.len(), 2);
        assert!(casual.contains(&"a".to_string()));
        let everyone = plan_speakers(&state, &config, "다 같이 얘기하자", None).unwrap();
        assert_eq!(everyone, vec!["a", "b", "c", "d", "e"]);
        let mentioned = plan_speakers(&state, &config, "@나루 답해", None).unwrap();
        assert_eq!(mentioned, vec!["b"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pool_slots_are_stable_and_distinct_for_participants() {
        let roster = vec!["a".to_string(), "b".to_string()];
        assert_eq!(pool_slot(&roster, "a", 2), 0);
        assert_eq!(pool_slot(&roster, "b", 2), 1);
        assert_eq!(pool_slot(&roster, "a", 2), pool_slot(&roster, "a", 2));
    }

    #[test]
    fn routing_modes_respect_owner_selection_ids_and_real_mentions() {
        let (state, dir) = test_state();
        let mut config = room(&state);
        assert_eq!(
            plan_speakers(&state, &config, "@나루 너부터", None).unwrap(),
            vec!["b"]
        );
        config["routingMode"] = json!("round_robin");
        config["nextSpeakerIndex"] = json!(2);
        assert_eq!(
            plan_speakers(&state, &config, "안녕", None).unwrap(),
            vec!["c"]
        );
        config["routingMode"] = json!("mentioned");
        assert!(plan_speakers(&state, &config, "안녕", None)
            .unwrap()
            .is_empty());
        assert_eq!(
            plan_speakers(&state, &config, "@b 그리고 @가람", None).unwrap(),
            vec!["b", "a"]
        );
        assert_eq!(
            plan_speakers(&state, &config, "안녕", Some(&json!(["c", "c", "a"]))).unwrap(),
            vec!["c", "a"]
        );
        assert!(plan_speakers(&state, &config, "안녕", Some(&json!(["outsider"]))).is_err());
        assert!(plan_speakers(
            &state,
            &config,
            "예시 `@가람`\n> @나루\n```text\n@다온\n```\nmail@b @가람이야",
            None
        )
        .unwrap()
        .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn shared_conversation_stores_owner_once_and_every_agent_sees_previous_peers() {
        let (state, dir) = test_state();
        let (server, bodies) = provider(
            &state,
            vec![
                text_delta("첫 번째 관점"),
                text_delta("첫 번째 의견에 답할게"),
                text_delta("둘의 의견을 보고 내 생각"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "j", "다 같이 오늘 편하게 얘기하자");
        let context = run_context(&state, "j");
        let result = run(&context, &job).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(result["turns"], 3);
        let messages = state.store.messages("room:r");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        let speakers = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .map(|message| message["agent_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(speakers, vec!["a", "b", "c"]);
        let requests = bodies.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for body in requests.iter() {
            assert_eq!(
                body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "user"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.ends_with("오늘 편하게 얘기하자")))
                    .count(),
                1
            );
        }
        assert!(requests[1]["messages"].to_string().contains("첫 번째 관점"));
        assert!(requests[2]["messages"].to_string().contains("첫 번째 관점"));
        assert!(requests[2]["messages"]
            .to_string()
            .contains("첫 번째 의견에 답할게"));
        assert_eq!(
            state.store.get("job", "j").unwrap()["roomState"]["inFlight"],
            false
        );
        drop(requests);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn mentioned_agents_can_reply_to_each_other_without_fabricated_owner_turns() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"mentioned","maxTurns":5}),
        );
        let (server, _) = provider(
            &state,
            vec![
                text_delta("@나루 너는 어때?"),
                text_delta("@다온 같이 얘기해보자"),
                text_delta("내 생각은 이래"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "j", "@가람 오늘 어땠어?");
        let result = run(&run_context(&state, "j"), &job).await.unwrap();
        assert_eq!(result["turns"], 3);
        assert_eq!(
            result["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|output| output["agentId"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn an_explicit_turn_cap_keeps_all_room_speakers() {
        let (state, dir) = test_state();
        let mut ids = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        for index in 4..=9 {
            let id = format!("a{index}");
            state.store.put(
                "agent",
                &json!({"id":id,"name":format!("참가자{index}"),"providerId":"p","model":"test"}),
            );
            ids.push(id);
        }
        let mut room = json!({
            "id":"r","agentIds":ids,"routingMode":"discussion","rounds":1,"maxTurns":9
        });
        assert_eq!(speaker_window(&state, &room, "다시 인사해봐", &ids).len(), 9);
        assert_eq!(speaker_window(&state, &room, "나머지도 말해라", &ids).len(), 9);
        room["maxTurns"] = json!(0);
        assert_eq!(speaker_window(&state, &room, "짧게 말해", &ids).len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn discussion_follows_mentions_and_leaves_others_silent() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","rounds":1,"maxTurns":3}),
        );
        let (server, bodies) = provider(&state, vec![text_delta("내 생각은 이래")], None).await;
        let job = save_owner_job(&state, "mention-cohort", "@나루 오늘 어땠어?");
        let result = run(&run_context(&state, "mention-cohort"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        assert_eq!(result["outputs"][0]["agentId"], "b");
        assert_eq!(bodies.lock().unwrap().len(), 1);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn pass_marker_is_silence_and_does_not_store_a_message() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","rounds":1,"maxTurns":2}),
        );
        let (server, bodies) = provider(
            &state,
            vec![text_delta("(pass)"), text_delta("나는 말할게")],
            None,
        )
        .await;
        let job = save_owner_job(&state, "pass-silence", "둘이 얘기해봐");
        let result = run(&run_context(&state, "pass-silence"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        let speakers = result["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["agentId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(speakers, vec!["b"]);
        assert_eq!(bodies.lock().unwrap().len(), 2);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn peer_loops_are_bounded_and_owner_interjection_yields_at_boundary() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"mentioned","maxTurns":3}),
        );
        let (server, _) = provider(
            &state,
            vec![
                text_delta("@나루 말해줘"),
                text_delta("@가람 너도"),
                text_delta("@나루 다시"),
            ],
            None,
        )
        .await;
        let job = save_owner_job(&state, "loop", "@가람 얘기해봐");
        let result = run(&run_context(&state, "loop"), &job).await.unwrap();
        assert_eq!(result["turns"], 3);
        assert_eq!(result["pauseReason"], "turn_limit");
        server.abort();
        let state_clone = state.clone();
        let callback: Callback = Arc::new(move |index, _| {
            if index == 0 {
                state_clone.store.write_message(&json!({"id":"new-owner","session_id":"room:r","role":"user","content":"잠깐, 새 주제로 바꿔","metadata":{"authorKind":"owner","roomId":"r"}}));
            }
        });
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"routingMode":"discussion","maxTurns":6}),
        );
        let (server, _) = provider(&state, vec![text_delta("알겠어")], Some(callback)).await;
        let job = save_owner_job(&state, "interject", "이번 주제에 대해 얘기하자");
        let result = run(&run_context(&state, "interject"), &job).await.unwrap();
        assert_eq!(result["turns"], 1);
        assert_eq!(result["yieldedToMessageId"], "new-owner");
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn stopped_generation_and_uncertain_turns_never_replay() {
        let (state, dir) = test_state();
        let job = save_owner_job(&state, "j", "얘기하자");
        state.cancel_session("room:r");
        let result = run(&run_context(&state, "j"), &job).await.unwrap();
        assert_eq!(result["status"], "cancelled");
        assert_eq!(state.store.count("execution"), 0);
        let job=state.store.put("job",&json!({"id":"uncertain","status":"running","input":{"roomId":"r"},"roomState":{"sessionId":"room:r","inFlight":true}})).unwrap();
        let result = run(&run_context(&state, "uncertain"), &job).await.unwrap();
        assert_eq!(result["status"], "needs_review");
        assert_eq!(state.store.count("execution"), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn opted_in_owner_command_executes_real_tools_with_frozen_policy_and_evidence() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a"],"maxTurns":1,"toolsEnabled":true,"contextBudgetTokens":1}),
        );
        state
            .store
            .put("policy", &json!({"id":"default","autoApproveAll":true}));
        let tool = json!({"tool_calls":[{"index":0,"id":"write-proof","type":"function","function":{"name":"workspace_write","arguments":json!({"path":"owner-proof.txt","content":"executed by Rust"}).to_string()}}]});
        let (server, bodies) = provider(
            &state,
            vec![tool, text_delta("파일을 실제로 저장했어")],
            None,
        )
        .await;
        let job = save_owner_job(
            &state,
            "tools",
            "내 컴퓨터에 owner-proof.txt 파일을 만들어줘",
        );
        // A queued message keeps the permission snapshot granted by its owner;
        // later UI changes cannot silently widen this particular job's policy.
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"toolsEnabled":false}),
        );
        let result = run(&run_context(&state, "tools"), &job).await.unwrap();
        assert_eq!(result["status"], "completed");
        assert_eq!(
            std::fs::read_to_string(dir.join("owner-proof.txt")).unwrap(),
            "executed by Rust"
        );
        assert!(bodies.lock().unwrap()[0]["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty()));
        let operations = state.store.all("execution_operation");
        assert_eq!(state.store.all("execution")[0]["contextPolicy"],"native-work-context");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["status"], "completed");
        assert_eq!(operations[0]["tool"], "workspace_write");
        assert_eq!(operations[0]["arguments"]["path"], "owner-proof.txt");
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn completed_checkpoints_and_compatibility_runs_do_not_repeat_owner_or_speakers() {
        let (state, dir) = test_state();
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":["a"],"maxTurns":1}),
        );
        let (server, bodies) = provider(&state, vec![text_delta("안녕")], None).await;
        let job=state.store.put("job",&json!({"id":"legacy","status":"running","input":{"roomId":"r","goal":"안녕","generation":0}})).unwrap();
        let context = run_context(&state, "legacy");
        let first = run(&context, &job).await.unwrap();
        assert_eq!(first["turns"], 1);
        let checkpoint = state.store.get("job", "legacy").unwrap();
        let second = run(&context, &checkpoint).await.unwrap();
        assert_eq!(second["turns"], 1);
        assert_eq!(bodies.lock().unwrap().len(), 1);
        assert_eq!(
            state
                .store
                .messages("room:r")
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn nine_participants_share_one_owner_message_and_all_speak() {
        let (state, dir) = test_state();
        let ids = (1..=9)
            .map(|index| format!("agent-{index}"))
            .collect::<Vec<_>>();
        for id in &ids {
            state.store.put(
                "agent",
                &json!({"id":id,"name":id,"providerId":"p","model":"test"}),
            );
        }
        state.store.patch_if(
            "room",
            "r",
            "id",
            &json!("r"),
            &json!({"agentIds":ids,"maxTurns":9}),
        );
        let deltas = (1..=9)
            .map(|index| text_delta(&format!("{index}번 참가자의 의견")))
            .collect();
        let (server, bodies) = provider(&state, deltas, None).await;
        let job = save_owner_job(&state, "nine", "다 같이 편하게 얘기해보자");
        let result = run(&run_context(&state, "nine"), &job).await.unwrap();
        assert_eq!(result["turns"], 9);
        let messages = state.store.messages("room:r");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["role"] == "assistant")
                .map(|message| message["agent_id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(bodies.lock().unwrap().len(), 9);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn discussion_mentions_cannot_starve_the_remaining_first_speakers() {
        let mut pending = VecDeque::from(vec!["c".to_string()]);
        let spoken = HashSet::from(["a".to_string(), "b".to_string()]);
        route_peers(&mut pending, &["a".to_string()], "b", 2, 6, Some(&spoken));
        assert_eq!(pending, VecDeque::from(vec!["c".to_string()]));
        route_peers(&mut pending, &["a".to_string()], "b", 2, 6, None);
        assert_eq!(
            pending,
            VecDeque::from(vec!["a".to_string(), "c".to_string()])
        );
    }
    #[test]
    fn stable_room_session_reads_do_not_publish_record_refresh_events() {
        let (state, dir) = test_state();
        let mut events = state.bus.subscribe();
        let first = ensure_room_session(&state, "r").unwrap();
        let initial = events.try_recv().unwrap();
        assert_eq!(initial["type"], "record");
        assert_eq!(initial["kind"], "room");
        let record = room(&state);
        for _ in 0..10 {
            assert_eq!(ensure_room_session(&state, "r").unwrap()["id"], first["id"]);
        }
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
        assert_eq!(room(&state), record);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn default_room_routing_uses_the_multiagent_roles_algorithm() {
        let (state, dir) = test_state();
        crate::engine::algorithms::seed(&state);
        for mut algorithm in state.store.all("algorithm") {
            if algorithm["kind"].as_str() != Some("multiagent.roles") {
                continue;
            }
            algorithm["status"] = json!(if algorithm["implementation"].as_str()
                == Some("mentioned") { "active" } else { "disabled" });
            state.store.put("algorithm", &algorithm);
        }
        let room = json!({"agentIds":["a","b","c"]});
        assert_eq!(routing_mode(&state, &room).unwrap(), "mentioned");
        let explicit = json!({"agentIds":["a","b","c"],"routingMode":"round_robin"});
        assert_eq!(routing_mode(&state, &explicit).unwrap(), "round_robin");
        let _ = std::fs::remove_dir_all(dir);
    }
