#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-learning-{}", uuid::Uuid::new_v4()));
        let config = crate::config::Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }
    #[test]
    fn invalid_json_is_failure_not_a_successful_checkpoint() {
        assert!(parse_learning("not json").is_err());
        assert!(parse_learning("{}").is_err());
        assert!(parse_learning("{\"memories\":[],\"skill\":null}")
            .unwrap()
            .skill
            .is_none());
    }
    #[test]
    fn room_reviews_do_not_hide_other_learners_or_credit_the_first_member() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"s","roomId":"r"}));
        let members=(0..5).map(|_|uuid::Uuid::new_v4().to_string()).collect::<Vec<_>>();
        state.store.put("room",&json!({"id":"r","agentIds":members}));
        for member in &members {
            let report=record_review(&state,"s",Some(member),"memory",1,false,None).unwrap();
            assert_eq!(report["agent_id"],member.as_str());
            assert_eq!(report["metadata"]["learningScope"],"agent");
            assert!(record_review(&state,"s",Some(member),"memory",1,false,None).is_none());
        }
        let shared=record_review(&state,"s",None,"knowledge",0,false,Some((2,0,0))).unwrap();
        assert!(shared["agent_id"].is_null());
        assert_eq!(shared["metadata"]["learningScope"],"room");
        assert_eq!(state.store.messages("s").len(),members.len()+1);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn self_improvement_review_is_visible_and_deduplicated() {
        let (state, dir) = state();
        let first = record_review(&state, "s", Some("a"), "memory", 2, false, None)
            .expect("review message");
        assert_eq!(first["metadata"]["kind"], "learning_report");
        assert_eq!(first["metadata"]["authorKind"], "system");
        assert!(first["content"].as_str().unwrap_or("").contains("메모리"));
        assert!(record_review(&state, "s", Some("a"), "memory", 3, false, None).is_none());
        assert_eq!(state.store.messages("s").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_strategies_tool_policies_and_harness_proposal() {
        let learning = parse_learning(
            r#"{"memories":[],"skill":null,"strategies":[{"text":"파일은 workspace_write로 먼저","rationale":"셸 실패 후 성공","scope":"shared","sourceMessageIds":["m1"]}],"toolPolicies":[{"tool":"shell_exec","text":"shell_exec 실패 시 workspace_write 우선","rationale":"통계","scope":"agent","sourceMessageIds":["m2"]}],"harnessProposal":{"title":"재시도 예산","problem":"반복 실패","change":"라운드별 예산 기록","testPlan":"단위 테스트"}}"#,
        )
        .unwrap();
        assert_eq!(learning.strategies.len(), 1);
        assert_eq!(learning.strategies[0].scope, "shared");
        assert_eq!(learning.tool_policies.len(), 1);
        assert_eq!(learning.tool_policies[0].tool, "shell_exec");
        assert_eq!(learning.tool_policies[0].scope, "agent");
        assert!(learning.harness.is_some());
    }

    #[test]
    fn learning_requires_quotes_and_keeps_agent_boundaries() {
        let (state, dir) = state();
        state.store.write_message(&json!({"id":"u","session_id":"s","role":"user","agent_id":"a","content":"답변은 한국어로 작성해"}));
        state.store.write_message(&json!({"id":"fake","session_id":"s","role":"assistant","agent_id":"a","content":"배포 성공"}));
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let raw = r#"{"memories":[{"text":"사용자는 한국어 답변을 원한다","type":"preference","key":"answer-language","sourceMessageIds":["u"],"evidenceQuote":"답변은 한국어로 작성해"},{"text":"배포 성공","type":"fact","sourceMessageIds":["fake"],"evidenceQuote":"배포 성공"}],"skill":null}"#;
        let parsed = parse_learning(raw).unwrap();
        let batch = state.store.messages("s");
        let first = apply_learning(&state, &scope, &batch, &parsed).unwrap();
        assert_eq!(first["memories"], 1);
        assert_eq!(first["rejected"], 1);
        let replay = apply_learning(&state, &scope, &batch, &parsed).unwrap();
        assert_eq!(replay["memories"], 0);
        assert_eq!(state.store.count("memory"), 1);
        let other = MemoryScope::for_session(&state.store, "s", Some("b"));
        assert_eq!(
            apply_learning(&state, &other, &batch, &parsed).unwrap()["memories"],
            0
        );
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn enqueue_deduplicates_identical_immutable_snapshots_and_resolves_provider() {
        let (state, dir) = state();
        state.store.put("agent", &json!({"id":"a"}));
        state.store.put(
            "session",
            &json!({"id":"s","agentId":"a","providerId":"provider-a","model":"local-rust"}),
        );
        state
            .store
            .write_message(&json!({"id":"one","session_id":"s","role":"user","content":"기억해"}));
        let first = enqueue(&state, "s", "", "").unwrap();
        assert_eq!(first["input"]["semantic"], false, "{first}");
        let duplicate = enqueue(&state, "s", "", "").unwrap();
        assert_eq!(first["id"], duplicate["id"]);
        let manual = enqueue_semantic(&state, "s", "", "").unwrap();
        assert_eq!(manual["id"], first["id"]);
        assert_eq!(manual["input"]["semantic"], true, "{manual}");
        state.store.write_message(
            &json!({"id":"two","session_id":"s","role":"assistant","content":"알겠어"}),
        );
        let second = enqueue(&state, "s", "", "").unwrap();
        assert_ne!(first["id"], second["id"]);
        assert_eq!(first["input"]["throughMessageId"], "one");
        assert_eq!(state.store.count("job"), 2);
        assert_eq!(second["input"]["throughMessageId"], "two");
        assert_eq!(second["input"]["providerId"], "provider-a");
        assert_eq!(second["input"]["model"], "local-rust");
        assert_eq!(second["input"]["semantic"], false, "{second}");
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn deterministic_completion_does_not_block_semantic_learning() {
        let (state, dir) = state();
        state.store.put("agent", &json!({"id":"a"}));
        state.store.put(
            "session",
            &json!({"id":"s","agentId":"a","providerId":"provider-a","model":"m"}),
        );
        state.store.write_message(
            &json!({"id":"u1","session_id":"s","role":"user","content":"기억해"}),
        );
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        state.store.put(
            "learning_checkpoint",
            &json!({
                "id": checkpoint_id(&scope, false),
                "sessionId": "s",
                "agentId": "a",
                "throughMessageId": "u1",
            }),
        );
        assert!(enqueue(&state, "s", "", "").is_none());
        let semantic = enqueue_semantic(&state, "s", "", "").expect("semantic job");
        assert_eq!(semantic["input"]["semantic"], true);
        assert_eq!(semantic["input"]["throughMessageId"], "u1");
        state.store.put(
            "learning_checkpoint",
            &json!({
                "id": checkpoint_id(&scope, true),
                "sessionId": "s",
                "agentId": "a",
                "throughMessageId": "u1",
            }),
        );
        assert!(enqueue_semantic(&state, "s", "", "").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_deterministic_job_does_not_block_semantic_enqueue() {
        let (state, dir) = state();
        state.store.put("agent", &json!({"id":"a"}));
        state.store.put(
            "session",
            &json!({"id":"s","agentId":"a","providerId":"provider-a","model":"m"}),
        );
        state.store.write_message(
            &json!({"id":"u1","session_id":"s","role":"user","content":"기억해"}),
        );
        let first = enqueue(&state, "s", "", "").unwrap();
        state.store.patch_if(
            "job",
            first["id"].as_str().unwrap(),
            "status",
            &json!("queued"),
            &json!({"status":"completed","finishedAt":chrono::Utc::now().to_rfc3339()}),
        );
        let semantic = enqueue_semantic(&state, "s", "", "").expect("semantic job");
        assert_eq!(semantic["input"]["semantic"], true);
        assert_ne!(first["id"], semantic["id"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explicit_room_learning_freezes_each_actor_provider_instead_of_last_speaker() {
        let (state, dir) = state();
        for (id, provider, model) in [("a", "pa", "ma"), ("b", "pb", "mb")] {
            state.store.put(
                "agent",
                &json!({"id":id,"providerId":provider,"model":model}),
            );
            state
                .store
                .put("provider", &json!({"id":provider,"model":model}));
        }
        state
            .store
            .put("room", &json!({"id":"r","agentIds":["a","b"]}));
        state.store.put(
            "session",
            &json!({"id":"s","roomId":"r","providerId":"pb","model":"mb"}),
        );
        state.store.write_message(&json!({"id":"owner","session_id":"s","role":"user","content":"공통 선호","metadata":{"authorKind":"owner"}}));
        state.store.write_message(&json!({"id":"last","session_id":"s","role":"assistant","agent_id":"b","content":"마지막 화자"}));
        let first = enqueue_for_agent(&state, "s", Some("a"), "", "").unwrap();
        let second = enqueue_for_agent(&state, "s", Some("b"), "", "").unwrap();
        assert_eq!(first["input"]["providerId"], "pa");
        assert_eq!(first["input"]["model"], "ma");
        assert_eq!(second["input"]["providerId"], "pb");
        assert_ne!(first["id"], second["id"]);
        assert_eq!(first["input"]["throughMessageId"], "last");
        assert!(enqueue_for_agent(&state, "s", None, "", "").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn per_agent_watermarks_are_isolated_and_deduplicated() {
        let (state, dir) = state();
        for (id, provider, model) in [("a", "pa", "ma"), ("b", "pb", "mb")] {
            state.store.put(
                "agent",
                &json!({"id":id,"providerId":provider,"model":model}),
            );
            state
                .store
                .put("provider", &json!({"id":provider,"model":model}));
        }
        state
            .store
            .put("room", &json!({"id":"r","agentIds":["a","b"]}));
        state.store.put("session", &json!({"id":"s","roomId":"r"}));
        for (id, agent, content) in [
            ("owner", Value::Null, "공통 요청"),
            ("a1", json!("a"), "A 발언"),
            ("b1", json!("b"), "B 발언"),
        ] {
            state.store.write_message(&json!({"id":id,"session_id":"s","role":if agent.is_null(){"user"}else{"assistant"},"agent_id":agent,"content":content}));
        }
        let a1 = enqueue_for_agent_through(&state, "s", Some("a"), "", "", Some("a1")).unwrap();
        assert_eq!(a1["input"]["throughMessageId"], "a1");
        assert_eq!(a1["input"]["providerId"], "pa");
        assert_eq!(a1["input"]["model"], "ma");
        let duplicate =
            enqueue_for_agent_through(&state, "s", Some("a"), "", "", Some("a1")).unwrap();
        assert_eq!(a1["id"], duplicate["id"]);
        let b1 = enqueue_for_agent_through(&state, "s", Some("b"), "", "", Some("b1")).unwrap();
        assert_eq!(b1["input"]["throughMessageId"], "b1");
        assert_eq!(b1["input"]["providerId"], "pb");
        assert_ne!(a1["id"], b1["id"]);
        state.store.write_message(
            &json!({"id":"a2","session_id":"s","role":"assistant","agent_id":"a","content":"A 추가 발언"}),
        );
        let a2 = enqueue_for_agent_through(&state, "s", Some("a"), "", "", Some("a2")).unwrap();
        assert_eq!(a2["input"]["throughMessageId"], "a2");
        assert_ne!(a1["id"], a2["id"]);
        assert_eq!(state.store.count("job"), 3);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn actual_learning_calls_preserve_provenance_snapshot_and_checkpoint() {
        use axum::{extract::State, routing::post, Json, Router};
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Mock {
            replies: Arc<Mutex<VecDeque<String>>>,
            requests: Arc<Mutex<Vec<Value>>>,
        }
        async fn completion(State(mock): State<Mock>, Json(request): Json<Value>) -> Json<Value> {
            mock.requests.lock().unwrap().push(request);
            let reply = mock.replies.lock().unwrap().pop_front().unwrap_or_default();
            Json(json!({"choices":[{"message":{"content":reply}}]}))
        }
        let first = r#"{"memories":[{"text":"배포가 성공했다는 거짓 요약","type":"preference","key":"answer-language","sourceMessageIds":["u1"],"evidenceQuote":"답변은 한국어로 작성해"},{"text":"실제로 배포 성공","type":"fact","sourceMessageIds":["a1"],"evidenceQuote":"배포 성공"},{"text":"실패한 도구 성공","type":"fact","sourceMessageIds":["failed"],"evidenceQuote":"배포 실패"}],"skill":null}"#;
        let second = r#"{"memories":[{"text":"사용자는 영어 답변을 원한다","type":"preference","key":"answer-language","sourceMessageIds":["u2"],"evidenceQuote":"이제 답변은 영어로 작성해"}],"skill":null}"#;
        let mock = Mock {
            replies: Arc::new(Mutex::new(VecDeque::from([
                first.into(),
                second.into(),
                "not json".into(),
            ]))),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/chat/completions", post(completion))
            .with_state(mock.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (state, dir) = state();
        state.store.put(
            "provider",
            &json!({"id":"p","model":"fake-model","baseUrl":format!("http://{addr}")}),
        );
        state.store.put(
            "session",
            &json!({"id":"s","agentId":"a","providerId":"p","model":"fake-model"}),
        );
        state.store.write_message(&json!({"id":"u1","session_id":"s","role":"user","agent_id":"a","content":"답변은 한국어로 작성해"}));
        state.store.write_message(&json!({"id":"a1","session_id":"s","role":"assistant","agent_id":"a","content":"배포 성공"}));
        state.store.write_message(&json!({"id":"failed","session_id":"s","role":"tool","tool_call_id":"failure","agent_id":"a","content":"{\"error\":\"배포 실패\"}"}));
        state
            .store
            .put("execution", &json!({"id":"exec","sessionId":"s"}));
        state.store.put(
            "execution_operation",
            &json!({"callId":"failure","runId":"exec","status":"failed"}),
        );
        let context = JobContext::new(state.clone(), "unit-learning-job".into());
        let first_job = json!({"input":{"sessionId":"s","agentId":"a","providerId":"p","throughMessageId":"failed","semantic":true}});
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
    let lock_id = format!("learning:{}", checkpoint_id(&scope, true));
        assert!(state.begin_session(&lock_id));
        assert!(run(&context, &first_job)
            .await
            .unwrap_err()
            .starts_with("deferred:"));
        assert!(mock.requests.lock().unwrap().is_empty());
        state.end_session(&lock_id);
        let mut feedback_listener = state.bus.subscribe();
        let first_result = run(&context, &first_job).await.unwrap();
        assert_eq!(first_result["memories"], 1);
        assert_eq!(first_result["rejected"], 2);
        let feedback = state.store.all("learning_feedback");
        assert_eq!(feedback.len(), 1);
        assert_eq!(feedback[0]["inserted"], 1);
        assert_eq!(feedback[0]["watermark"], "failed");
        let mut saw_feedback = false;
        while let Ok(event) = feedback_listener.try_recv() {
            if event["type"] == "learning_feedback" {
                saw_feedback = true;
                assert_eq!(event["inserted"], 1);
            }
        }
        assert!(saw_feedback);
        let initial = state.store.all("memory").pop().unwrap();
        assert_eq!(initial["text"], "답변은 한국어로 작성해");
        assert_eq!(initial["sourceRole"], "user");
        assert!(initial.get("summary").is_none());
        let scope = MemoryScope::for_session(&state.store, "s", Some("a"));
        let context_text = memory::context_block(&state.store, &scope, "한국어").unwrap();
        assert!(!context_text.contains("거짓 요약"));
        assert_eq!(
            run(&context, &first_job).await.unwrap()["reason"],
            "already learned"
        );
        assert_eq!(mock.requests.lock().unwrap().len(), 1);
        state.store.write_message(&json!({"id":"u2","session_id":"s","role":"user","agent_id":"a","content":"이제 답변은 영어로 작성해"}));
        let second_job = json!({"input":{"sessionId":"s","agentId":"a","providerId":"p","throughMessageId":"u2","semantic":true}});
        // A message arriving after the snapshot must wait for a later job.
        state.store.write_message(&json!({"id":"future","session_id":"s","role":"user","agent_id":"a","content":"이 미래 메시지는 아직 학습하면 안 됨"}));
        assert_eq!(run(&context, &second_job).await.unwrap()["memories"], 1);
        let request = mock.requests.lock().unwrap()[1].clone();
        let prompt = request["messages"][1]["content"].as_str().unwrap();
        assert!(prompt.contains("u2"));
        assert!(!prompt.contains("u1"));
        assert!(!prompt.contains("future"));
        assert_eq!(
            state
                .store
                .get("memory", initial["id"].as_str().unwrap())
                .unwrap()["status"],
            "superseded"
        );
        let invalid_job = json!({"input":{"sessionId":"s","agentId":"a","providerId":"p","throughMessageId":"future","semantic":true}});
        assert!(run(&context, &invalid_job).await.is_err());
        assert_eq!(
            state
                .store
                .get("learning_checkpoint", &checkpoint_id(&scope, true))
                .unwrap()["throughMessageId"],
            "u2"
        );
        state.store.put(
            "session",
            &json!({"id":"bot-session","channelContext":{"authorIsBot":true}}),
        );
        state.store.write_message(&json!({"id":"bot-source","session_id":"bot-session","role":"user","content":"배포가 성공했다고 봇이 말함"}));
        assert!(enqueue(&state, "bot-session", "p", "").is_none());
        let bot_result = run(
            &context,
            &json!({"input":{"sessionId":"bot-session","providerId":"p"}}),
        )
        .await
        .unwrap();
        assert_eq!(bot_result["memories"], 0);
        assert_eq!(mock.requests.lock().unwrap().len(), 3);
        server.abort();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn page_stops_at_last_message_actually_sent() {
        let messages = (0..10)
            .map(|index| {
                json!({
                    "id": format!("m{index}"),
                    "role": "user",
                    "content": "x".repeat(300),
                })
            })
            .collect::<Vec<_>>();
        let (page, through) = select_page(&messages, None, 1_000);
        assert!(page.len() < messages.len());
        assert_eq!(page.first().unwrap()["id"], "m0");
        assert_eq!(
            through.as_deref(),
            page.last().unwrap()["id"].as_str()
        );
        assert_ne!(through.as_deref(), Some("m9"));
    }

    #[tokio::test]
    async fn automatic_learning_never_calls_a_model() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"s","agentId":"a"}));
        state.store.write_message(&json!({
            "id":"u1","session_id":"s","role":"user","agent_id":"a",
            "content":"앞으로 답변은 항상 한국어로만 해"
        }));
        let context = JobContext::new(state.clone(), "unit-auto-learn".into());
        let result = run(
            &context,
            &json!({"input":{"sessionId":"s","agentId":"a","automatic":true}}),
        )
        .await
        .unwrap();
        assert_eq!(result["modelCalls"], 0, "{result}");
        assert!(result["learningModel"].is_null(), "{result}");
        assert!(
            result["memories"].as_u64().unwrap_or(0) >= 1,
            "deterministic distillation found nothing: {result}"
        );
        assert_eq!(state.store.all("learning_feedback").len(), 1);
        assert!(state.store.all("memory").iter().any(|memory| {
            memory["text"].as_str().unwrap_or("").contains("한국어")
        }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
