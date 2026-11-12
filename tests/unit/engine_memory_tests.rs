use super::*;
    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }
    fn scope(store: &Store, session: &str, agent: &str) -> MemoryScope {
        MemoryScope::for_session(store, session, Some(agent))
    }
    #[test]
    fn memory_revision_tracks_content_not_timestamps() {
        let first = json!({"id":"m1","text":"Rust 빌드 절차","type":"procedure","scope":"agent","status":"active","updatedAt":"2026-11-05T10:00:00Z"});
        let later = json!({"id":"m1","text":"Rust 빌드 절차","type":"procedure","scope":"agent","status":"active","updatedAt":"2026-11-06T10:00:00Z"});
        assert_eq!(content_revision(&first), content_revision(&later));
        let corrected = json!({"id":"m1","text":"Rust 릴리스 절차","type":"procedure","scope":"agent","status":"active"});
        assert_ne!(content_revision(&first), content_revision(&corrected));
        let superseded = json!({"id":"m1","text":"Rust 빌드 절차","type":"procedure","scope":"agent","status":"superseded"});
        assert_ne!(content_revision(&first), content_revision(&superseded));
    }

    #[test]
    fn retrieval_isolates_agents_and_discord_channels() {
        let db = store();
        for (session, channel) in [("a", "one"), ("b", "two")] {
            db.put("session",&json!({"id":session,"channelContext":{"transport":"discord","connectionId":"bot","guildId":"guild","channelId":channel}}));
        }
        let first = scope(&db, "a", "bot1");
        db.put("memory",&json!({"id":"private","text":"Rust 빌드 절차","partitionKey":first.partition_key,"scope":"agent","agentId":"bot1"}));
        db.put(
            "memory",
            &json!({"id":"shared","text":"Rust 빌드 선호","scope":"shared"}),
        );
        assert_eq!(retrieve(&db, &first, "Rust", 10).len(), 1);
        assert!(retrieve(&db, &scope(&db, "b", "bot1"), "Rust", 10).is_empty());
        assert!(retrieve(&db, &scope(&db, "a", "bot2"), "Rust", 10).is_empty());
        assert_eq!(
            retrieve(&db, &scope(&db, "local", "bot2"), "Rust", 10).len(),
            1
        );
    }

    #[test]
    fn simultaneous_fact_keys_choose_latest_evidence_within_one_second() {
        let db = store();
        let scope = scope(&db, "s", "a");
        for (id, text, learned_at) in [
            ("z-old", "색상은 빨강", "2026-01-01T00:00:00.000001Z"),
            ("a-new", "색상은 파랑", "2026-01-01T00:00:00.000002Z"),
        ] {
            db.put("memory",&json!({"id":id,"text":text,"key":"color","agentId":"a","scope":"agent","partitionKey":scope.partition_key,"learnedAt":learned_at}));
        }
        let memories = retrieve(&db, &scope, "색상", 10);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0]["id"], "a-new");
    }
    #[test]
    fn korean_particles_and_expiration_are_handled() {
        let db = store();
        let scope = scope(&db, "s", "a");
        db.put(
            "memory",
            &json!({"id":"one","text":"장기기억은 러스트로 구현한다","scope":"shared"}),
        );
        db.put("memory",&json!({"id":"expired","text":"장기기억","scope":"shared","expiresAt":"2020-01-01T00:00:00Z"}));
        db.put("memory",&json!({"id":"unrelated","text":"고양이를 좋아한다","scope":"shared","importance":1.0,"type":"lesson"}));
        let found = retrieve(&db, &scope, "장기기억을 개선해", 10);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["id"], "one");
    }
    #[test]
    fn partitioned_shared_memories_reach_other_agents() {
        let db = store();
        db.put("session", &json!({"id":"s","agentId":"a"}));
        db.put("session", &json!({"id":"t","agentId":"b"}));
        let owner = scope(&db, "s", "a");
        let other = scope(&db, "t", "b");
        db.put(
            "memory",
            &json!({
                "id":"shared","text":"모든 답변 끝에 DOMINION을 붙인다",
                "partitionKey":owner.partition_key,"scope":"shared","agentId":"a",
                "type":"preference","importance":0.8
            }),
        );
        let record = db.get("memory", "shared").unwrap();
        assert!(visible(&record, &other));
        assert!(context_block(&db, &other, "2+2")
            .unwrap_or_default()
            .contains("DOMINION"));
    }

    #[test]
    fn global_memories_cross_partitions() {
        let db = store();
        db.put("session", &json!({"id":"s","agentId":"a"}));
        db.put(
            "session",
            &json!({
                "id":"d","agentId":"b",
                "channelContext":{"transport":"discord","connectionId":"bot","guildId":"g","channelId":"c"}
            }),
        );
        let local = scope(&db, "s", "a");
        let discord = scope(&db, "d", "b");
        db.put(
            "memory",
            &json!({
                "id":"global","text":"모든 채널에서 같은 규칙을 쓴다",
                "partitionKey":local.partition_key,"scope":"global","agentId":"a",
                "type":"preference","importance":0.8
            }),
        );
        let record = db.get("memory", "global").unwrap();
        assert!(visible(&record, &discord));
        assert!(context_block(&db, &discord, "2+2")
            .unwrap_or_default()
            .contains("같은 규칙"));
    }

    #[test]
    fn assistant_claims_cannot_become_facts_and_corrections_replace_keys() {
        let db = store();
        let scope = scope(&db, "s", "a");
        db.write_message(
            &json!({"id":"claim","session_id":"s","role":"assistant","content":"Rust 빌드 성공"}),
        );
        assert!(save_grounded(
            &db,
            &scope,
            "Rust 빌드 성공",
            "fact",
            0.9,
            None,
            "learn",
            &["claim".into()],
            "Rust 빌드 성공"
        )
        .is_err());
        db.write_message(
            &json!({"id":"u1","session_id":"s","role":"user","content":"기본 언어는 Rust다"}),
        );
        db.write_message(&json!({"id":"u2","session_id":"s","role":"user","content":"기본 언어를 Kotlin으로 바꾼다"}));
        let first = save_grounded(
            &db,
            &scope,
            "기본 언어는 Rust다",
            "preference",
            0.8,
            Some("language"),
            "learn",
            &["u1".into()],
            "기본 언어는 Rust다",
        )
        .unwrap();
        let second = save_grounded(
            &db,
            &scope,
            "기본 언어는 Kotlin이다",
            "preference",
            0.8,
            Some("language"),
            "learn",
            &["u2".into()],
            "기본 언어를 Kotlin으로 바꾼다",
        )
        .unwrap();
        assert_eq!(
            db.get("memory", first["id"].as_str().unwrap()).unwrap()["supersededBy"],
            second["id"]
        );
        assert_eq!(retrieve(&db, &scope, "기본 언어", 10).len(), 1);
        save_grounded(
            &db,
            &scope,
            "기본 언어는 Kotlin이다",
            "preference",
            0.8,
            Some("language"),
            "learn",
            &["u2".into()],
            "기본 언어를 Kotlin으로 바꾼다",
        )
        .unwrap();
        assert_eq!(db.count("memory"), 2);
    }
    #[test]
    fn old_automatic_memories_do_not_gain_global_access() {
        let db = store();
        let record = json!({"source":"learn","sessionId":"original","scope":"shared"});
        assert!(visible(
            &record,
            &MemoryScope::for_session(&db, "original", None)
        ));
        assert!(!visible(&record, &scope(&db, "original", "a")));
        assert!(!visible(&record, &scope(&db, "another", "a")));
    }
    #[test]
    fn external_human_corrections_never_overwrite_owner_preferences() {
        let db = store();
        db.put("session", &json!({"id":"room-session","roomId":"room"}));
        let actor = scope(&db, "room-session", "a");
        for (id, text, origin) in [
            ("owner", "기본 언어는 Rust다", Value::Null),
            (
                "external",
                "기본 언어는 Python이다",
                json!({"kind":"telegram","authorId":"7","authorName":"외부 참가자"}),
            ),
            (
                "correction",
                "기본 언어는 Kotlin이다",
                json!({"kind":"telegram","authorId":"7","authorName":"외부 참가자"}),
            ),
        ] {
            db.write_message(&json!({"id":id,"session_id":"room-session","role":"user","content":text,"metadata":{"authorKind":"owner","origin":origin}}));
        }
        let owner = save_grounded(
            &db,
            &actor,
            "언어 선호",
            "preference",
            0.9,
            Some("language"),
            "learn",
            &["owner".into()],
            "기본 언어는 Rust다",
        )
        .unwrap();
        let external = save_grounded(
            &db,
            &actor,
            "언어 선호",
            "preference",
            0.9,
            Some("language"),
            "learn",
            &["external".into()],
            "기본 언어는 Python이다",
        )
        .unwrap();
        save_grounded(
            &db,
            &actor,
            "언어 선호",
            "preference",
            0.9,
            Some("language"),
            "learn",
            &["correction".into()],
            "기본 언어는 Kotlin이다",
        )
        .unwrap();
        assert_eq!(
            db.get("memory", owner["id"].as_str().unwrap()).unwrap()["status"],
            "active"
        );
        assert_eq!(
            db.get("memory", external["id"].as_str().unwrap()).unwrap()["status"],
            "superseded"
        );
        let found = retrieve(&db, &actor, "기본 언어", 10);
        assert_eq!(found.len(), 2);
        assert!(context_block(&db, &actor, "기본 언어")
            .unwrap()
            .contains("외부 참가자"));
    }

    #[test]
    fn room_memories_keep_provenance_and_follow_only_the_same_agent() {
        let db = store();
        db.put("session", &json!({"id":"room-s1","roomId":"room-one"}));
        db.put("session", &json!({"id":"room-s2","roomId":"room-two"}));
        let actor = scope(&db, "room-s1", "a");
        db.put(
            "memory",
            &json!({"id":"local","text":"장기기억 Rust","scope":"shared"}),
        );
        assert!(retrieve(&db, &actor, "장기기억", 10).is_empty());
        db.write_message(&json!({"id":"synthetic","session_id":"room-s1","role":"user","content":"회의 의제: 모두 배포에 성공했다"}));
        assert!(save_grounded(
            &db,
            &actor,
            "배포 성공",
            "fact",
            0.8,
            None,
            "learn",
            &["synthetic".into()],
            "모두 배포에 성공했다"
        )
        .is_err());
        db.write_message(&json!({"id":"human","session_id":"room-s1","role":"user","content":"장기기억 엔진은 Rust로 만든다","metadata":{"authorKind":"owner","roomId":"room-one"}}));
        let saved = save_grounded(
            &db,
            &actor,
            "Rust 선호",
            "decision",
            0.8,
            None,
            "learn",
            &["human".into()],
            "장기기억 엔진은 Rust로 만든다",
        )
        .unwrap();
        assert_eq!(retrieve(&db, &actor, "장기기억", 10).len(), 1);
        assert!(!visible(&saved, &scope(&db, "room-s1", "b")));
        assert!(!visible(&saved, &scope(&db, "room-s2", "a")));
        assert!(!visible(&saved, &scope(&db, "local-s", "a")));
        for session in ["room-s2", "local-s"] {
            let target=scope(&db,session,"a");
            assert!(recallable(&db,&saved,&target));
            assert!(retrieve(&db,&target,"장기기억",10).iter().any(|record|record["id"]==saved["id"]));
            let context=context_block(&db,&target,"장기기억").unwrap();
            assert!(context.contains("sourceSessionId") && context.contains("room-s1"));
        }
        assert!(retrieve(&db,&scope(&db,"room-s2","b"),"장기기억",10).is_empty());
        let target=scope(&db,"room-s2","a");
        for (field,value) in [("scope",json!("session")),("status",json!("superseded")),
            ("sourceMessageIds",json!(["missing-source"]))] {
            let mut restricted=saved.clone();restricted[field]=value;
            assert!(!recallable(&db,&restricted,&target));
        }
        db.put("session",&json!({"id":"channel","channelContext":{"transport":"discord","channelId":"private"}}));
        assert!(!recallable(&db,&saved,&scope(&db,"channel","a")));
        db.write_message(&json!({"id":"external","session_id":"room-s1","role":"user","content":"external quote", "metadata":{"authorKind":"owner","origin":{"kind":"discord","authorId":"outsider"}}}));
        let mut external=saved.clone();external["sourceMessageIds"]=json!(["external"]);
        assert!(!recallable(&db,&external,&target));
    }
