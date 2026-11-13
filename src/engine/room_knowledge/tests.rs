//! Split from src/engine/room_knowledge.rs; regression coverage only.
    use super::*;
    use crate::config::Config;
    use crate::db::Store;
    use crate::vault::Vault;

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-room-knowledge-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn message(id: &str, role: &str, agent: Option<&str>, content: &str) -> Value {
        json!({
            "id": id,
            "session_id": "room:r1",
            "role": role,
            "agent_id": agent,
            "content": content,
            "metadata": {"authorKind": if role == "user" { "owner" } else { "agent" }},
        })
    }

    #[test]
    fn batch_starts_with_the_oldest_unprocessed_messages() {
        let messages = (0..120)
            .map(|index| {
                json!({
                    "id": format!("m{index}"),
                    "content": format!("message {index}"),
                })
            })
            .collect::<Vec<_>>();
        let batch = select_batch(&messages);
        assert_eq!(batch.len(), MAX_MESSAGES);
        assert_eq!(batch.first().unwrap()["id"], "m0");
        assert_eq!(batch.last().unwrap()["id"], "m79");
    }

    #[test]
    fn batch_respects_the_prompt_budget() {
        let long = "x".repeat(MAX_CONTENT_CHARS);
        let messages = (0..30)
            .map(|index| json!({"id": format!("m{index}"), "content": long}))
            .collect::<Vec<_>>();
        let batch = select_batch(&messages);
        let per_message = MAX_CONTENT_CHARS + 128;
        assert_eq!(batch.len(), MAX_PROMPT_CHARS / per_message);
        assert_eq!(batch.first().unwrap()["id"], "m0");
    }

    #[test]
    fn questions_do_not_count_as_owner_confirmation() {
        assert!(is_question("이 방식이 맞아?"));
        assert!(is_question("정말？ "));
        assert!(!is_question("이 방식이 맞습니다."));
        assert!(!is_question("좋아, 그걸로 확정해"));
    }

    #[test]
    fn relevance_prefers_the_question_topic() {
        let corpus = json!({"text":"database rollback runbook","topicKey":"database","type":"conclusion"});
        let other = json!({"text":"캐시 만료 정책","topicKey":"cache","type":"claim"});
        assert!(relevance(&corpus, "database rollback") > relevance(&other, "database rollback"));
        assert!(relevance(&other, "캐시 정책") > relevance(&corpus, "캐시 정책"));
    }

    #[test]
    fn extraction_keeps_provenance_and_supersedes_a_correction_once() {
        let (state, dir) = state();
        state.store.put("agent", &json!({"id":"a","name":"A"}));
        state.store.put("agent", &json!({"id":"b","name":"B"}));
        state.store.put(
            "session",
            &json!({"id":"room:r1","roomId":"r1","agentId":null}),
        );
        let owner = message("m1", "user", None, "배포 방식 정하자");
        let claim = message("m2", "assistant", Some("a"), "배포는 X 방식으로 하자");
        let correction = message(
            "m3",
            "assistant",
            Some("b"),
            "정정: X는 실패했다. 배포는 Y 방식으로 해야 한다.",
        );
        let confirm = message("m4", "user", None, "Y가 맞다");
        for item in [&owner, &claim, &correction, &confirm] {
            state.store.write_message(item);
        }
        let batch = vec![owner, claim, correction, confirm];
        let extraction = json!({"knowledge":[
            {
                "type":"conclusion","topicKey":"deploy","text":"배포는 X 방식",
                "sourceMessageIds":["m2"],"evidenceQuotes":["배포는 X 방식으로 하자"],
                "confidence":0.6,"verification":"debate"
            },
            {
                "type":"correction","topicKey":"deploy","text":"배포는 Y 방식",
                "sourceMessageIds":["m3","m4"],
                "evidenceQuotes":["배포는 Y 방식으로 해야 한다.","Y가 맞다"],
                "correctsMessageIds":["m2"],"confidence":0.95,
                "verification":"owner_confirmed"
            }
        ]});
        let first = apply_extraction(&state, "r1", "room:r1", &batch, &extraction).unwrap();
        assert_eq!(first["inserted"], 2, "{first}");
        let second = apply_extraction(&state, "r1", "room:r1", &batch, &extraction).unwrap();
        assert_eq!(second["inserted"], 0, "{second}");
        let items = list(&state, "r1");
        assert_eq!(items.len(), 2, "{items:?}");
        let correction = items
            .iter()
            .find(|item| item["type"] == "correction")
            .expect("correction");
        assert_eq!(correction["verification"], "owner_confirmed");
        let promoted = state.store.all("experience_principle");
        assert_eq!(promoted.len(), 1, "{promoted:?}");
        assert_eq!(promoted[0]["origin"], "room_knowledge");
        assert_eq!(promoted[0]["scope"], "global");
        assert_eq!(promoted[0]["principle"], "배포는 Y 방식");
        assert_eq!(correction["status"], "active");
        let claim = items
            .iter()
            .find(|item| item["type"] == "claim")
            .expect("claim");
        assert_eq!(claim["status"], "superseded", "{claim}");
        assert_eq!(claim["supersededBy"], correction["id"]);
        let sources = correction["sources"].as_array().unwrap();
        assert!(sources.iter().any(|source| source["agentName"] == "B"));
        assert!(sources.iter().any(|source| source["messageId"] == "m3"));
        let block = context_block(&state, "r1", "").unwrap();
        assert!(block.contains("배포는 Y 방식"), "{block}");
        assert!(block.contains("owner_confirmed"), "{block}");
        assert!(block.contains("B/"), "{block}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unsupported_quotes_and_single_agent_conclusions_are_rejected_or_demoted() {
        let (state, dir) = state();
        let owner = message("m1", "user", None, "질문");
        let claim = message("m2", "assistant", Some("a"), "내 주장은 X다");
        state.store.write_message(&owner);
        state.store.write_message(&claim);
        let extraction = json!({"knowledge":[
            {
                "type":"conclusion","topicKey":"x","text":"X",
                "sourceMessageIds":["m2"],"evidenceQuotes":["내 주장은 X다"],
                "verification":"debate"
            },
            {
                "type":"conclusion","topicKey":"bad","text":"없는 인용",
                "sourceMessageIds":["m2"],"evidenceQuotes":["이 문장은 원문에 없다"],
                "verification":"debate"
            }
        ]});
        let report = apply_extraction(&state, "r1", "room:r1", &[owner, claim], &extraction).unwrap();
        assert_eq!(report["rejected"], 1, "{report}");
        let items = list(&state, "r1");
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0]["type"], "claim", "{items:?}");
    }

    #[test]
    fn social_greetings_do_not_request_semantic_learning() {
        let (state, dir) = state();
        state
            .store
            .write_message(&message("m1", "user", None, "다시 인사해봐"));
        state.store.write_message(&message(
            "m2",
            "assistant",
            Some("a"),
            "안녕하세요, 주인님. 오늘도 평화롭습니다.",
        ));
        assert!(!should_learn_automatically(&state, "room:r1", "m2"));
        assert!(enqueue_automatic(&state, "r1", "room:r1", "m2").is_none());
        assert!(state.store.all("job").is_empty());
        let skipped = state
            .store
            .get("room_learning_state", &state_id("r1"))
            .expect("skip state");
        assert_eq!(skipped["status"], "skipped");
        assert_eq!(skipped["modelCalls"], 0);
        assert_eq!(skipped["checkpointAdvanced"], true);
        assert_eq!(
            state
                .store
                .get("room_learning_checkpoint", &checkpoint_id("r1"))
                .expect("checkpoint")["throughMessageId"],
            "m2"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_greeting_does_not_advance_past_unprocessed_work() {
        let (state, dir) = state();
        state
            .store
            .write_message(&message("m1", "user", None, "배포 방식을 정하자"));
        state.store.write_message(&message(
            "m2",
            "assistant",
            Some("a"),
            "배포는 X 방식으로 하자.",
        ));
        state
            .store
            .write_message(&message("m3", "user", None, "다시 인사해봐"));
        state.store.write_message(&message(
            "m4",
            "assistant",
            Some("a"),
            "안녕하세요, 주인님.",
        ));
        assert!(!should_learn_automatically(&state, "room:r1", "m4"));
        assert!(enqueue_automatic(&state, "r1", "room:r1", "m4").is_none());
        let skipped = state
            .store
            .get("room_learning_state", &state_id("r1"))
            .expect("skip state");
        assert_eq!(skipped["checkpointAdvanced"], false);
        assert_eq!(skipped["pendingBacklog"], true);
        assert!(state
            .store
            .get("room_learning_checkpoint", &checkpoint_id("r1"))
            .is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn topic_exchange_still_requests_semantic_learning() {
        let (state, dir) = state();
        state
            .store
            .write_message(&message("m1", "user", None, "배포 방식을 정하자"));
        state.store.write_message(&message(
            "m2",
            "assistant",
            Some("a"),
            "배포는 X 방식으로 하자.",
        ));
        assert!(should_learn_automatically(&state, "room:r1", "m2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_evidence_overrides_a_social_owner_turn() {
        let (state, dir) = state();
        state.store.write_message(&message("m1", "user", None, "안녕"));
        state.store.write_message(&json!({
            "id":"m2","session_id":"room:r1","role":"assistant","agent_id":"a",
            "content":"확인했습니다.","metadata":{
                "authorKind":"agent","executionId":"run-1"
            }
        }));
        state.store.put(
            "execution_operation",
            &json!({
                "id":"op-1","runId":"run-1","tool":"workspace_write",
                "status":"completed","opSeq":1
            }),
        );
        assert!(should_learn_automatically(&state, "room:r1", "m2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_correction_reply_overrides_a_social_owner_turn() {
        let (state, dir) = state();
        state.store.write_message(&message("m1", "user", None, "또 인사해봐"));
        state.store.write_message(&message(
            "m2",
            "assistant",
            Some("a"),
            "정정: X는 실패했다. 배포는 Y 방식으로 해야 한다.",
        ));
        assert!(should_learn_automatically(&state, "room:r1", "m2"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn room_knowledge_is_shared_without_a_model_or_agent_filter() {
        let (state, dir) = state();
        state.store.put(
            "room_knowledge",
            &json!({
                "id":"k1","roomId":"r1","type":"conclusion","topicKey":"t",
                "text":"공유 결론","status":"active","verification":"debate",
                "confidence":0.6,"sources":[{"agentName":"A","model":"local-4b","messageId":"m1"}],
                "updatedAt":"2026-01-01T00:00:00Z"
            }),
        );
        let block = context_block(&state, "r1", "").unwrap();
        assert!(block.contains("공유 결론"), "{block}");
        assert!(block.contains("local-4b"), "{block}");
        assert!(context_block(&state, "r2", "").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
