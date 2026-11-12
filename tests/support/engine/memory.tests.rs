#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod memory_algorithm_tests {
    use super::*;

    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }
    fn scope(store: &Store, session: &str, agent: &str) -> MemoryScope {
        MemoryScope::for_session(store, session, Some(agent))
    }
    fn algorithm(store: &Store, id: &str, kind: &str, implementation: &str, parameters: Value) {
        store.put(
            "algorithm",
            &json!({
                "id": id, "kind": kind, "implementation": implementation,
                "variant": implementation, "status": "active", "parameters": parameters
            }),
        );
    }

    #[test]
    fn importance_algorithm_changes_retrieval_ranking() {
        let db = store();
        let scope = scope(&db, "s", "a");
        algorithm(&db, "imp-fixed", "memory.importance", "fixed", json!({}));
        db.put("memory", &json!({
            "id":"m-old","text":"Rust 절차 A","type":"lesson","importance":0.9,
            "scope":"agent","agentId":"a","partitionKey":scope.partition_key,
            "status":"active","createdAt":"2020-01-01T00:00:00Z","uses":0
        }));
        db.put("memory", &json!({
            "id":"m-new","text":"Rust 절차 B","type":"lesson","importance":0.2,
            "scope":"agent","agentId":"a","partitionKey":scope.partition_key,
            "status":"active","createdAt":chrono::Utc::now().to_rfc3339(),"uses":6,
            "lastUsedAt":chrono::Utc::now().to_rfc3339()
        }));
        let fixed = retrieve(&db, &scope, "Rust 절차", 5);
        assert_eq!(fixed[0]["id"], "m-old", "{fixed:?}");
        // Switch the active kernel.
        let mut fixed_record = db.get("algorithm", "imp-fixed").unwrap();
        fixed_record["status"] = json!("available");
        db.put("algorithm", &fixed_record);
        algorithm(
            &db,
            "imp-recency",
            "memory.importance",
            "recency-uses",
            json!({"usesWeight":0.3,"recencyDays":7}),
        );
        let ranked = retrieve(&db, &scope, "Rust 절차", 5);
        assert_eq!(ranked[0]["id"], "m-new", "{ranked:?}");
    }

    #[test]
    fn prompt_delivery_updates_memory_usage_without_changing_content_revision() {
        let db = store();
        let scope = scope(&db, "s", "a");
        db.put("memory", &json!({
            "id":"used-memory","text":"항상 한국어로 답변","type":"preference",
            "importance":0.9,"scope":"agent","agentId":"a",
            "partitionKey":scope.partition_key,"status":"active","uses":0,
            "createdAt":"2026-01-01T00:00:00Z"
        }));
        let before = db.get("memory", "used-memory").unwrap();
        assert_eq!(retrieve(&db, &scope, "다른 질문", 6).len(), 1);
        assert_eq!(db.get("memory", "used-memory").unwrap()["uses"], 0,
            "a search alone must not count as context delivery");
        let (context, manifest) = context_block_with_manifest(&db, &scope, "다른 질문");
        assert!(context.is_some());
        assert_eq!(manifest.len(), 1);
        let after = db.get("memory", "used-memory").unwrap();
        assert_eq!(after["uses"], 1);
        assert!(after["lastUsedAt"].is_string());
        assert_eq!(content_revision(&before), content_revision(&after));
        context_block_with_manifest(&db, &scope, "다른 질문");
        assert_eq!(db.get("memory", "used-memory").unwrap()["uses"], 2);
    }


    #[test]
    fn exact_memory_compaction_preserves_the_strongest_and_keeps_evidence() {
        let db = store();
        let scope = scope(&db, "s", "a");
        algorithm(&db, "compact-exact", "memory.compaction", "exact", json!({}));
        for (id, importance, count) in [("weak", 0.1, 1), ("strong", 0.9, 2)] {
            db.put("memory", &json!({
                "id":id, "text":"파일 오류를 먼저 확인", "type":"lesson",
                "scope":"agent","agentId":"a","sessionId":"s",
                "partitionKey":scope.partition_key,
                "importance":importance,"status":"active",
                "sourceMessageIds":vec!["evidence";count],
                "createdAt":"2026-01-01T00:00:00Z"
            }));
        }
        db.put("memory", &json!({
            "id":"different","text":"파일 오류 확인 후 재시도", "type":"lesson",
            "scope":"agent","agentId":"a","sessionId":"s",
            "partitionKey":scope.partition_key,"importance":0.5,"status":"active"
        }));
        assert_eq!(compact_sweep(&db), 1);
        assert_eq!(compact_sweep(&db), 0);
        assert_eq!(db.get("memory", "strong").unwrap()["status"], "active");
        let weak = db.get("memory", "weak").unwrap();
        assert_eq!(weak["status"], "archived");
        assert_eq!(weak["compactedInto"], "strong");
        assert!(weak["sourceMessageIds"].is_array(), "source evidence remains stored");
        assert!(retrieve(&db, &scope, "파일 오류를 먼저 확인", 10)
            .iter().all(|row| row["id"] != "weak"));
        assert_eq!(db.get("memory", "different").unwrap()["status"], "active");
        let mut active = db.get("algorithm", "compact-exact").unwrap();
        active["implementation"] = json!("never");
        db.put("algorithm", &active);
        assert_eq!(compact_sweep(&db), 0);
    }

    #[test]
    fn forgetting_archives_only_old_low_importance_facts() {
        let db = store();
        let scope = scope(&db, "s", "a");
        algorithm(&db, "forget-decay", "memory.forgetting", "decay", json!({"minImportance":0.3,"maxAgeDays":30}));
        for (id, kind, importance, created) in [
            ("old-low", "fact", 0.1, "2020-01-01T00:00:00Z"),
            ("old-high", "fact", 0.9, "2020-01-01T00:00:00Z"),
            ("old-pref", "preference", 0.1, "2020-01-01T00:00:00Z"),
            ("new-low", "fact", 0.1, chrono::Utc::now().to_rfc3339().as_str()),
        ] {
            db.put("memory", &json!({
                "id":id,"text":format!("{id} 내용"),"type":kind,"importance":importance,
                "scope":"agent","agentId":"a","partitionKey":scope.partition_key,
                "status":"active","createdAt":created,"lastUsedAt":created
            }));
        }
        let archived = forget_sweep(&db);
        assert_eq!(archived, 1, "{:?}", db.all("memory"));
        assert_eq!(db.get("memory", "old-low").unwrap()["status"], "archived");
        assert!(
            !retrieve(&db, &scope, "old-low 내용", 10)
                .iter()
                .any(|item| item["id"] == "old-low"),
            "archived memory must not be retrieved or injected into chat"
        );

        assert_eq!(db.get("memory", "old-high").unwrap()["status"], "active");
        assert_eq!(db.get("memory", "old-pref").unwrap()["status"], "active");
        assert_eq!(db.get("memory", "new-low").unwrap()["status"], "active");
    }
}

#[cfg(test)]
mod per_agent_memory_learning_tests {
    use super::*;
    use crate::{config::Config, state::AppState, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir()
            .join(format!("dominion-memory-learn-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(),
            web_dir: dir.join("web"), workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn only_kernel(state: &AppState, kind: &str, implementation: &str) {
        crate::engine::algorithms::seed(state);
        for mut item in state.store.all("algorithm") {
            if item["kind"].as_str() != Some(kind) { continue; }
            item["status"] = json!(if item["implementation"].as_str()
                == Some(implementation) { "active" } else { "disabled" });
            state.store.put("algorithm", &item);
        }
    }

    #[test]
    fn cached_cross_room_recall_never_promotes_peer_or_external_evidence() {
        let (state, dir) = state();
        let db = &state.store;
        db.put("session", &json!({"id":"origin","roomId":"r1"}));
        db.put("session", &json!({"id":"dest","roomId":"r2"}));
        db.put("session", &json!({"id":"remote","channelContext":{"transport":"telegram"}}));
        db.write_message(&json!({"id":"owner-msg","session_id":"origin","role":"user",
            "content":"검증된 owner 내용","metadata":{"authorKind":"owner"}}));
        db.write_message(&json!({"id":"peer-msg","session_id":"origin","role":"user",
            "content":"봇 주장","metadata":{"authorKind":"agent"}}));
        for (id, evidence) in [("allowed","owner-msg"),("blocked","peer-msg")] {
            db.put("memory", &json!({"id":id,"text":"호출 증거 회상",
                "scope":"agent","agentId":"a","sessionId":"origin",
                "partitionKey":"[\"room\",\"r1\"]",
                "status":"active","sourceMessageIds":[evidence]}));
        }
        let local = MemoryScope::for_session(db, "dest", Some("a"));
        let rows = retrieve(db, &local, "호출 증거", 10);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0]["id"], "allowed");
        let remote = MemoryScope::for_session(db, "remote", Some("a"));
        assert!(retrieve(db, &remote, "호출 증거", 10).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retrieval_honors_the_requested_candidate_pool_above_twenty() {
        let (state, dir) = state();
        state
            .store
            .put("session", &json!({"id":"s","agentId":"agent-a"}));
        only_kernel(&state, "memory.rerank", "bm25");
        let scope = MemoryScope::for_session(&state.store, "s", Some("agent-a"));
        for index in 0..30 {
            state.store.put(
                "memory",
                &json!({
                    "id":format!("pool-{index}"),
                    "text":format!("database migration rollback note {index}"),
                    "type":"lesson",
                    "status":"active",
                    "importance":0.5,
                    "scope":"agent",
                    "agentId":"agent-a",
                    "partitionKey":scope.partition_key,
                    "createdAt":format!("2026-01-01T00:00:{index:02}Z")
                }),
            );
        }
        let pool = retrieve(&state.store, &scope, "database migration rollback", 30);
        assert_eq!(pool.len(), 30, "{:?}", pool.iter().map(|row| row["id"].clone()).collect::<Vec<_>>());
        let capped = retrieve(&state.store, &scope, "database migration rollback", 500);
        assert_eq!(capped.len(), 30);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn importance_is_chosen_per_agent_and_graded_independently_from_task_outcome() {
        let (state, dir) = state();
        state.store.put("session", &json!({"id":"s","agentId":"agent-a"}));
        only_kernel(&state, "memory.importance", "evidence-weighted");
        let scope = MemoryScope::for_session(&state.store, "s", Some("agent-a"));
        state.store.put("memory", &json!({
            "id":"evidence-memory","text":"배포 전 파일을 검증한다",
            "type":"lesson","status":"active","importance":0.7,
            "scope":"agent","agentId":"agent-a","partitionKey":scope.partition_key,
            "sourceMessageIds":["m1","m2"],"evidenceMessageIds":["m1","m2"]
        }));
        let (_context, manifest) = context_block_with_manifest_at(
            &state, &scope, "파일 검증", Some("run-importance"),
        );
        assert_eq!(manifest.len(), 1);
        let usage_id = format!("algorithm-usage-{}",
            stable_id(&["run-importance", "memory.importance"]));
        let usage = state.store.get("algorithm_usage", &usage_id).unwrap();
        assert_eq!(usage["agentId"], "agent-a");
        assert_eq!(usage["signal"]["successes"], 1);
        // An intentionally failed task cannot override this independent signal.
        crate::engine::algorithms::finalize_execution(
            &state, &json!({"id":"run-importance","taskOutcome":"failed"}),
        );
        let stats = state.store.all("algorithm_stat");
        let stat = stats.iter().find(|item| item["agentId"] == "agent-a"
            && item["kind"] == "memory.importance").unwrap();
        assert_eq!(stat["successes"], 1);
        assert_eq!(stat["failures"], 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offline_forgetting_and_compaction_learn_separately_for_each_bot() {
        let (state, dir) = state();
        only_kernel(&state, "memory.forgetting", "decay");
        only_kernel(&state, "memory.compaction", "exact");
        for agent in ["agent-a", "agent-b"] {
            state.store.put("memory", &json!({
                "id":format!("old-{agent}"), "agentId":agent, "partitionKey":"local",
                "scope":"agent", "type":"fact", "text":format!("old {agent}"),
                "importance":0.1, "status":"active",
                "createdAt":"2020-01-01T00:00:00Z",
                "learnedAt":"2020-01-01T00:00:00Z",
                "lastUsedAt":"2020-01-01T00:00:00Z"
            }));
        }
        for (id, importance) in [("copy-low",0.2),("copy-high",0.9)] {
            state.store.put("memory", &json!({
                "id":id,"agentId":"agent-a","partitionKey":"local",
                "scope":"agent","sessionId":"s","type":"lesson",
                "text":"같은 교훈 원문","importance":importance,"status":"active",
                "createdAt":chrono::Utc::now().to_rfc3339()
            }));
        }
        state.store.put("memory", &json!({
            "id":"protected","agentId":"agent-a","partitionKey":"local",
            "scope":"agent","type":"preference","text":"소중한 선호",
            "importance":0.1,"status":"active",
            "createdAt":"2020-01-01T00:00:00Z",
            "learnedAt":"2020-01-01T00:00:00Z",
            "lastUsedAt":"2020-01-01T00:00:00Z"
        }));
        assert_eq!(learning_sweep(&state), 3);
        assert_eq!(learning_sweep(&state), 0, "no idle UCB score inflation");
        assert_eq!(state.store.get("memory","old-agent-a").unwrap()["status"], "archived");
        assert_eq!(state.store.get("memory","old-agent-b").unwrap()["status"], "archived");
        assert_eq!(state.store.get("memory","protected").unwrap()["status"], "active");
        assert_eq!(state.store.get("memory","copy-low").unwrap()["status"], "archived");
        assert_eq!(state.store.get("memory","copy-high").unwrap()["status"], "active");
        let stats = state.store.all("algorithm_stat");
        for (agent,kind) in [
            ("agent-a","memory.forgetting"),("agent-b","memory.forgetting"),
            ("agent-a","memory.compaction")
        ] {
            let stat=stats.iter().find(|stat| stat["agentId"] == agent
                && stat["kind"] == kind).expect("missing bot-specific UCB stat");
            assert_eq!(stat["uses"],1);
            assert_eq!(stat["successes"],1);
        }
        assert!(state.store.all("memory_maintenance_observation").len() >= 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn never_policy_is_evaluated_once_per_unchanged_evidence_snapshot() {
        let (state, dir) = state();
        only_kernel(&state, "memory.forgetting", "never");
        state.store.put("memory", &json!({
            "id":"old-one","agentId":"bot","scope":"agent","partitionKey":"local",
            "text":"stale fact","type":"fact","importance":0.1,
            "status":"active","createdAt":"2020-01-01T00:00:00Z",
            "learnedAt":"2020-01-01T00:00:00Z",
            "lastUsedAt":"2020-01-01T00:00:00Z"
        }));
        assert_eq!(learning_sweep(&state),1);
        assert_eq!(learning_sweep(&state),0);
        assert_eq!(state.store.get("memory","old-one").unwrap()["status"],"active");
        assert!(state.store.all("algorithm_stat").into_iter().all(|record| {
            record["agentId"] != "bot" || record["kind"] != "memory.forgetting"
        }), "preservation policy must not be punished for abstaining");
        assert_eq!(state.store.all("memory_maintenance_observation")[0]["status"], "abstained");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bm25_rerank_scores_documents_by_term_evidence() {
        let mut apple = BTreeSet::new();
        apple.insert("apple".to_string());
        let mut banana = BTreeSet::new();
        banana.insert("banana".to_string());
        let mut query = BTreeSet::new();
        query.insert("apple".to_string());
        let scores = bm25_scores(&query, &[apple, banana], 1.2, 0.75);
        assert!(scores[0] > scores[1], "{scores:?}");

        let (state, dir) = state();
        let db = state.store.clone();
        let scope = MemoryScope::for_session(&db, "s", Some("a"));
        db.put("algorithm", &json!({
            "id":"rerank-bm25","kind":"memory.rerank","implementation":"bm25",
            "variant":"bm25","status":"active","parameters":{"k1":1.2,"b":0.75}
        }));
        db.put("memory", &json!({
            "id":"apple-note","text":"apple deployment note","type":"lesson",
            "importance":0.5,"scope":"agent","agentId":"a",
            "partitionKey":scope.partition_key,"status":"active"
        }));
        db.put("memory", &json!({
            "id":"banana-note","text":"banana deployment note","type":"lesson",
            "importance":0.5,"scope":"agent","agentId":"a",
            "partitionKey":scope.partition_key,"status":"active"
        }));
        let ranked = retrieve(&db, &scope, "apple", 5);
        assert_eq!(ranked[0]["id"], "apple-note", "{ranked:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn standing_preferences_survive_bm25_rerank_limit() {
        let db = Store::open(std::path::Path::new(":memory:")).unwrap();
        let scope = MemoryScope::for_session(&db, "s", Some("a"));
        let importance = json!({"implementation":"fixed"});
        let rerank = json!({"implementation":"bm25","parameters":{"k1":1.2,"b":0.75}});
        db.put("memory", &json!({
            "id":"standing","text":"답변은 짧고 정확하게 유지한다","type":"preference",
            "importance":0.95,"scope":"agent","agentId":"a",
            "partitionKey":scope.partition_key,"status":"active",
            "createdAt":"2026-01-01T00:00:00Z"
        }));
        for index in 0..20 {
            db.put("memory", &json!({
                "id":format!("lexical-{index}"),
                "text":if index % 2 == 0 {
                    format!("database note {index}")
                } else {
                    format!("rollback note {index}")
                },
                "type":"lesson","importance":0.5,"scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"status":"active",
                "createdAt":format!("2026-01-01T00:00:{index:02}Z")
            }));
        }
        let ranked = retrieve_ranked_with_rerank(
            &db, &scope, "database rollback", 3, &importance, &rerank,
        );
        assert!(
            ranked.iter().any(|record| record["id"] == "standing"),
            "standing preference was dropped by the BM25 limit: {:?}",
            ranked.iter().map(|record| record["id"].clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn compression_summarizes_old_memories_and_preserves_originals() {
        let (state, dir) = state();
        only_kernel(&state, "memory.compression", "extractive");
        for (id, text, evidence) in [
            ("old-a", "예전 배포 절차 A", "m1"),
            ("old-b", "예전 배포 절차 B", "m2"),
        ] {
            state.store.put("memory", &json!({
                "id":id,"agentId":"agent-a","partitionKey":"local",
                "scope":"agent","type":"fact","text":text,"importance":0.32,
                "status":"active","createdAt":"2020-01-01T00:00:00Z",
                "learnedAt":"2020-01-01T00:00:00Z",
                "lastUsedAt":"2020-01-01T00:00:00Z",
                "evidenceMessageIds":[evidence]
            }));
        }
        let evaluated = learning_sweep(&state);
        assert!(evaluated >= 1, "compression was not evaluated");
        let memories = state.store.all("memory");
        let summary = memories.iter().find(|record| {
            record["source"].as_str() == Some("compression")
        }).expect("compressed summary");
        assert_eq!(summary["status"], "active");
        assert_eq!(summary["compressedFrom"].as_array().unwrap().len(), 2);
        assert_eq!(summary["evidenceMessageIds"].as_array().unwrap().len(), 2);
        for id in ["old-a", "old-b"] {
            let original = state.store.get("memory", id).unwrap();
            assert_eq!(original["status"], "archived");
            assert_eq!(original["compressedInto"], summary["id"]);
            assert!(original["evidenceMessageIds"].is_array(), "evidence lost");
        }
        assert_eq!(learning_sweep(&state), 0, "idle sweep re-compressed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compressed_archive_recall_recovers_omitted_detail_without_unarchiving() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "archive-reader", Some("agent-a"));
        let old = "2020-01-01T00:00:00Z";
        state.store.put("memory", &json!({
            "id":"compression-summary","source":"compression","scope":"global",
            "partitionKey":"local","agentId":"agent-a","sourceAuthorKey":"owner",
            "sessionId":null,"type":"fact","text":"배포 관련 요약",
            "compressedFrom":["original-detail"],"status":"active","importance":0.35,
            "createdAt":old,"learnedAt":old
        }));
        state.store.put("memory", &json!({
            "id":"original-detail","scope":"global","partitionKey":"local",
            "agentId":"agent-a","sourceAuthorKey":"owner","sessionId":null,
            "type":"fact","text":"cerulean comet checksum: 7F22",
            "status":"archived","compressedInto":"compression-summary",
            "archiveReason":"extractive compression; original evidence preserved",
            "importance":0.32,"createdAt":old,"learnedAt":old,
            "evidenceMessageIds":["original-message"]
        }));
        let retrieved = retrieve(&state.store, &scope, "cerulean comet checksum", 6);
        assert_eq!(retrieved.first().map(|m|m["id"].as_str()),Some(Some("original-detail")),
            "archived exact evidence must be reachable when a lossy summary omitted the answer");
        assert_eq!(retrieved[0]["retrievalSource"],"compressed_archive");
        assert_eq!(state.store.get("memory","original-detail").unwrap()["status"],"archived",
            "retrieval must never silently unarchive or mutate evidence");
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compressed_archive_recall_never_crosses_provenance_or_visibility() {
        let (state, dir) = state();
        let scope = MemoryScope::for_session(&state.store, "archive-reader", Some("agent-a"));
        let old = "2020-01-01T00:00:00Z";
        state.store.put("memory",&json!({
            "id":"public-summary","source":"compression","scope":"global",
            "partitionKey":"local","agentId":"agent-a","sourceAuthorKey":"owner",
            "sessionId":null,"type":"fact","text":"일반 공개 요약",
            "compressedFrom":["wrong-partition","disabled-original","unlinked-original"],
            "status":"active","createdAt":old
        }));
        for (id, partition, enabled, compression_id) in [
            ("wrong-partition","private-other-room",true,"public-summary"),
            ("disabled-original","local",false,"public-summary"),
            ("unlinked-original","local",true,"another-summary"),
        ] {
            state.store.put("memory",&json!({
                "id":id,"scope":"global","partitionKey":partition,
                "agentId":"agent-a","sourceAuthorKey":"owner","sessionId":null,
                "type":"fact","text":"private-archive-needle",
                "status":"archived","enabled":enabled,"compressedInto":compression_id,
                "archiveReason":"extractive compression; original evidence preserved",
                "createdAt":old
            }));
        }
        assert!(retrieve(&state.store,&scope,"private-archive-needle",6).is_empty(),
            "an accessible summary must not launder an inaccessible archived record");
        let _=std::fs::remove_dir_all(dir);
    }

    fn set_rerank(store: &Store, implementation: &str) {
        store.put("algorithm", &json!({
            "id":"measurement-rerank","kind":"memory.rerank",
            "implementation":implementation,"variant":implementation,
            "status":"active","parameters":{"k1":1.2,"b":0.75}
        }));
    }

    fn measurement_corpus(store: &Store, scope: &MemoryScope) {
        for (id, text) in [
            ("m1-gold", "database migration rollback runbook notes archive"),
            ("m1-short", "database migration"),
            ("m2-gold", "token expiration audit trail notes archive"),
            ("m2-short", "token expiration"),
            ("m3-gold", "cache invalidation policy notes archive log"),
            ("m3-short", "cache invalidation"),
            ("filler-1", "database backup"),
            ("filler-2", "token rotation"),
            ("filler-3", "cache warmup"),
            ("filler-4", "migration report"),
            ("filler-5", "expiration warning"),
        ] {
            store.put("memory", &json!({
                "id":id,"text":text,"type":"lesson","importance":0.5,
                "scope":"agent","agentId":"a","partitionKey":scope.partition_key,
                "status":"active","learnedAt":"2026-01-01T00:00:00Z",
                "lastUsedAt":"2026-01-01T00:00:00Z"
            }));
        }
    }

    #[test]
    fn measurement_bm25_improves_top_one_accuracy_on_length_skewed_corpus() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let scope = MemoryScope::for_session(&store, "s", Some("a"));
        measurement_corpus(&store, &scope);
        // Query order differs from the gold text so the phrase bonus cannot
        // rescue the baseline; only IDF + length normalization can.
        let queries = [
            ("rollback database migration", "m1-gold"),
            ("audit token expiration", "m2-gold"),
            ("policy cache invalidation", "m3-gold"),
        ];
        let hit = |implementation: &str| {
            set_rerank(&store, implementation);
            queries
                .iter()
                .filter(|(query, gold)| {
                    retrieve(&store, &scope, query, 3)
                        .first()
                        .and_then(|record| record["id"].as_str())
                        == Some(*gold)
                })
                .count()
        };
        let baseline = hit("none");
        let reranked = hit("bm25");
        println!(
            "bm25 measurement: top1 none={baseline}/3 bm25={reranked}/3"
        );
        assert!(reranked > baseline, "bm25 did not improve: {baseline} -> {reranked}");
        assert_eq!(reranked, 3, "bm25 top-1 accuracy below fixture target");
    }

    #[test]
    fn compression_does_not_mix_unrelated_topics() {
        let (state, dir) = state();
        only_kernel(&state, "memory.compression", "extractive");
        only_kernel(&state, "memory.compaction", "never");
        for (id, text) in [
            ("deploy-1", "배포 절차는 서버에서 파일을 복사하고 서비스를 재시작한 뒤 로그를 확인한다"),
            ("deploy-2", "배포 절차는 롤백 전에 서버 백업을 복사하고 로그를 확인한다"),
            ("cook-1", "김치는 배추를 소금에 절이고 양념을 만들어 하루 동안 숙성한다"),
            ("cook-2", "된장찌개는 된장을 풀고 두부와 채소를 넣어 끓인다"),
        ] {
            state.store.put("memory", &json!({
                "id":id,"agentId":"agent-a","partitionKey":"local","scope":"agent",
                "type":"fact","text":text,"importance":0.32,"status":"active",
                "createdAt":"2020-01-01T00:00:00Z",
                "learnedAt":"2020-01-01T00:00:00Z",
                "lastUsedAt":"2020-01-01T00:00:00Z",
                "evidenceMessageIds":[format!("{id}-m")]
            }));
        }
        assert!(learning_sweep(&state) >= 1);
        let summaries = state.store.all("memory").into_iter()
            .filter(|record| record["source"].as_str() == Some("compression"))
            .collect::<Vec<_>>();
        let deploy = summaries.iter().find(|summary| {
            summary["compressedFrom"].as_array().is_some_and(|ids| {
                ids.iter().any(|id| id == "deploy-1")
            })
        }).expect("deployment summary");
        assert_eq!(deploy["compressedFrom"].as_array().unwrap().len(), 2, "{deploy}");
        assert!(deploy["text"].as_str().unwrap_or("").contains("배포"), "{deploy}");
        for summary in &summaries {
            let ids = summary["compressedFrom"].as_array().cloned().unwrap_or_default();
            let has_deploy = ids.iter().any(|id| id.as_str().is_some_and(|id| id.starts_with("deploy-")));
            let has_cook = ids.iter().any(|id| id.as_str().is_some_and(|id| id.starts_with("cook-")));
            assert!(!(has_deploy && has_cook), "unrelated topics were merged: {summary}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn measurement_compression_reduces_active_memory_text() {
        let (state, dir) = state();
        only_kernel(&state, "memory.compression", "extractive");
        for (id, text) in [
            ("old-1", "예전 배포 절차는 서버에서 파일을 복사하고 서비스를 재시작한 뒤 로그를 확인했다"),
            ("old-2", "예전 배포 절차는 서버마다 파일을 복사하고 서비스를 재시작한 뒤 로그를 확인했다"),
            ("old-3", "예전 배포 절차는 서버에서 파일을 복사하고 서비스를 재시작한 뒤 로그만 확인했다"),
            ("old-4", "예전 배포 절차는 서버에서 파일을 복사하고 서비스를 재시작한 뒤 롤백했다"),
        ] {
            state.store.put("memory", &json!({
                "id":id,"agentId":"agent-a","partitionKey":"local","scope":"agent",
                "type":"fact","text":text,"importance":0.32,"status":"active",
                "createdAt":"2020-01-01T00:00:00Z",
                "learnedAt":"2020-01-01T00:00:00Z",
                "lastUsedAt":"2020-01-01T00:00:00Z",
                "evidenceMessageIds":[format!("{id}-m")]
            }));
        }
        let before_records = state.store.all("memory").iter()
            .filter(|record| {
                record["agentId"].as_str() == Some("agent-a")
                    && record["id"].as_str().is_some_and(|id| id.starts_with("old-"))
                    && record["status"].as_str() == Some("active")
            }).count();
        let before = state.store.all("memory").iter()
            .filter(|record| {
                record["agentId"].as_str() == Some("agent-a")
                    && record["id"].as_str().is_some_and(|id| id.starts_with("old-"))
                    && record["status"].as_str() == Some("active")
            })
            .filter_map(|record| record["text"].as_str().map(|text| text.chars().count()))
            .sum::<usize>();
        assert!(learning_sweep(&state) >= 1);
        let after_records = state.store.all("memory").iter()
            .filter(|record| {
                record["agentId"].as_str() == Some("agent-a")
                    && (record["source"].as_str() == Some("compression")
                        || record["id"].as_str().is_some_and(|id| id.starts_with("old-")))
                    && record["status"].as_str() == Some("active")
            }).count();
        let after = state.store.all("memory").iter()
            .filter(|record| {
                record["agentId"].as_str() == Some("agent-a")
                    && (record["source"].as_str() == Some("compression")
                        || record["id"].as_str().is_some_and(|id| id.starts_with("old-")))
                    && record["status"].as_str() == Some("active")
            })
            .filter_map(|record| record["text"].as_str().map(|text| text.chars().count()))
            .sum::<usize>();
        println!(
            "compression measurement: activeRecords {before_records} -> {after_records}, activeChars {before} -> {after} ({:.0}% smaller)",
            (1.0 - after as f64 / before.max(1) as f64) * 100.0
        );
        assert!(after_records < before_records, "compression kept every record active");
        let summary = state.store.all("memory").into_iter()
            .find(|record| record["source"].as_str() == Some("compression"))
            .expect("compressed summary");
        let summary_text = summary["text"].as_str().unwrap_or("");
        assert!(summary_text.contains("로그"), "summary lost the log fact: {summary}");
        assert!(summary_text.contains("롤백"), "summary lost the rollback fact: {summary}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod regression_memory_visibility_tests {
    use super::*;
    use crate::{config::Config, state::AppState, vault::Vault};

    fn state() -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-mem-scope-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0, data_dir: dir.clone(), web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        (AppState::new(config, store, vault), dir)
    }

    fn only_kernel(state: &AppState, kind: &str, implementation: &str) {
        crate::engine::algorithms::seed(state);
        for mut algorithm in state.store.all("algorithm") {
            if algorithm["kind"].as_str() != Some(kind) { continue; }
            algorithm["status"] = json!(if algorithm["implementation"].as_str()
                == Some(implementation) { "active" } else { "disabled" });
            state.store.put("algorithm", &algorithm);
        }
    }

    #[test]
    fn compression_keeps_shared_global_and_partition_visibility() {
        let (state, dir) = state();
        only_kernel(&state, "memory.compression", "extractive");
        only_kernel(&state, "memory.compaction", "never");
        let room_one = json!(["room", "one"]).to_string();
        let room_two = json!(["room", "two"]).to_string();
        state.store.put("session", &json!({"id":"s1","roomId":"one"}));
        state.store.put("session", &json!({"id":"s2","roomId":"two"}));
        let scope_one = MemoryScope::for_session(&state.store, "s1", Some("bot-b"));
        let scope_two = MemoryScope::for_session(&state.store, "s2", Some("bot-b"));

        let old = "2020-01-01T00:00:00Z";
        for (id, scope, partition, text, source_author) in [
            ("shared-one-a","shared",room_one.as_str(),"Shared room-one alpha note","owner"),
            ("shared-one-b","shared",room_one.as_str(),"Shared room-one beta note","owner"),
            ("shared-two","shared",room_two.as_str(),"A private second room fact","owner"),
            ("shared-other-author","shared",room_one.as_str(),"Different speaker's fact","guest"),
            ("global-one","global","local","Cross-room global alpha","owner"),
            ("global-two","global","local","Cross-room global beta","owner"),
        ] {
            state.store.put("memory", &json!({
                "id":id,"scope":scope,"agentId":"bot-a",
                "partitionKey":partition,"sourceAuthorKey":source_author,
                "type":"fact","text":text,"importance":0.32,"status":"active",
                "createdAt":old,"learnedAt":old,"lastUsedAt":old
            }));
        }
        assert!(learning_sweep(&state) >= 1);
        let summaries = state.store.all("memory").into_iter()
            .filter(|record| record["source"].as_str() == Some("compression"))
            .collect::<Vec<_>>();
        assert_eq!(summaries.len(), 2, "{summaries:?}");
        let room_summary = summaries.iter()
            .find(|record| record["scope"] == "shared").unwrap();
        assert_eq!(room_summary["partitionKey"], room_one);
        assert_eq!(room_summary["sourceAuthorKey"], "owner");
        assert!(visible(room_summary, &scope_one));
        assert!(!visible(room_summary, &scope_two));
        let global_summary = summaries.iter()
            .find(|record| record["scope"] == "global").unwrap();
        assert!(visible(global_summary, &scope_one));
        assert!(visible(global_summary, &scope_two));
        assert_eq!(global_summary["scope"], "global");
        assert_eq!(state.store.get("memory", "shared-one-a").unwrap()["status"], "archived");
        for id in ["shared-two", "shared-other-author"] {
            assert_eq!(state.store.get("memory", id).unwrap()["status"], "active");
        }
        assert_eq!(learning_sweep(&state), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_compressed_global_memories_regain_original_visibility() {
        let (state, dir) = state();
        for id in ["old-global-a", "old-global-b"] {
            state.store.put("memory", &json!({
                "id":id,"scope":"global","agentId":"original-bot",
                "partitionKey":"local","type":"fact",
                "status":"archived","compressedInto":"legacy-global",
                "text":format!("global note {id}")
            }));
        }
        state.store.put("memory", &json!({
            "id":"legacy-global","source":"compression",
            "scope":"agent","agentId":"global","partitionKey":"local",
            "type":"lesson","status":"active",
            "compressedFrom":["old-global-a","old-global-b"],
            "text":"압축된 기억"
        }));
        assert_eq!(repair_legacy_compressions(&state.store), 1);
        assert_eq!(repair_legacy_compressions(&state.store), 0);
        let repaired = state.store.get("memory", "legacy-global").unwrap();
        assert_eq!(repaired["scope"], "global");
        assert_eq!(repaired["agentId"], "original-bot");
        assert_eq!(repaired["type"], "fact");
        assert!(visible(&repaired, &MemoryScope::for_session(
            &state.store, "room-2", Some("other-bot")
        )));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_mixed_scope_compression_restores_records_instead_of_leaking_them() {
        let (state, dir) = state();
        let room_one = json!(["room", "one"]).to_string();
        let room_two = json!(["room", "two"]).to_string();
        for (id, partition) in [("one-old", room_one.as_str()),
                                ("two-old", room_two.as_str())] {
            state.store.put("memory", &json!({
                "id":id,"scope":"shared","agentId":"bot-a",
                "partitionKey":partition,"type":"fact","importance":0.32,
                "status":"archived","compressedInto":"legacy-mixed",
                "text":format!("private memory {id}")
            }));
        }
        state.store.put("memory", &json!({
            "id":"legacy-mixed","source":"compression",
            "scope":"agent","agentId":"global","partitionKey":"local",
            "status":"active","compressedFrom":["one-old","two-old"],
            "text":"unsafe shared summary"
        }));
        assert_eq!(repair_legacy_compressions(&state.store), 1);
        assert_eq!(state.store.get("memory","legacy-mixed").unwrap()["status"], "archived");
        for id in ["one-old","two-old"] {
            let restored = state.store.get("memory", id).unwrap();
            assert_eq!(restored["status"], "active");
            assert!(restored["lastUsedAt"].is_string());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bm25_context_has_its_own_outcome_signal_even_when_turn_fails() {
        let (state, dir) = state();
        only_kernel(&state, "memory.rerank", "bm25");
        let scope = MemoryScope::for_session(&state.store, "s", Some("bot"));
        state.store.put("memory", &json!({
            "id":"correct","scope":"agent","agentId":"bot",
            "partitionKey":scope.partition_key,"status":"active",
            "type":"fact","text":"database rollback procedure",
            "importance":0.5
        }));
        let (text, manifest) = context_block_with_manifest_at(
            &state, &scope, "database rollback", Some("rerank-run"),
        );
        assert!(text.is_some());
        assert_eq!(manifest.len(), 1);
        let id = format!("algorithm-usage-{}",
            stable_id(&["rerank-run", "memory.rerank"]));
        let usage = state.store.get("algorithm_usage", &id).unwrap();
        assert_eq!(usage["signalBasis"], "lexical_ndcg_proxy");
        assert_eq!(usage["signal"]["successes"], 1);
        crate::engine::algorithms::finalize_execution(
            &state, &json!({"id":"rerank-run","taskOutcome":"failed"}),
        );
        let stat = state.store.all("algorithm_stat").into_iter()
            .find(|value| value["agentId"] == "bot" && value["kind"] == "memory.rerank")
            .unwrap();
        assert_eq!(stat["successes"], 1);
        let _ = context_block_with_manifest_at(&state, &scope, "unknownword", Some("empty-run"));
        let empty_id = format!("algorithm-usage-{}",
            stable_id(&["empty-run", "memory.rerank"]));
        assert!(state.store.get("algorithm_usage", &empty_id).is_none(),
            "no graded evidence should mean no UCB usage");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bm25_retrieval_is_unchanged_by_archived_and_invisible_corpus_records() {
        let db = Store::open(std::path::Path::new(":memory:")).unwrap();
        let scope = MemoryScope::for_session(&db, "s", Some("a"));
        let importance = json!({"implementation":"fixed"});
        let rerank = json!({"implementation":"bm25","parameters":{"k1":1.2,"b":0.75}});
        for (id, text) in [
            ("one", "database rollback procedure"),
            ("two", "database archive"),
            ("three", "database policy"),
        ] {
            db.put("memory", &json!({
                "id":id,"text":text,"status":"active",
                "scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"type":"fact"
            }));
        }
        let first = retrieve_ranked_with_rerank(&db, &scope,
            "database rollback", 3, &importance, &rerank);
        for n in 0..60 {
            db.put("memory", &json!({
                "id":format!("archived-{n}"),"text":"database rollback irrelevant long text",
                "status":"archived","scope":"agent","agentId":"a",
                "partitionKey":scope.partition_key,"type":"fact"
            }));
            db.put("memory", &json!({
                "id":format!("private-{n}"),"text":"database rollback irrelevant",
                "status":"active","scope":"agent","agentId":"private-bot",
                "partitionKey":scope.partition_key,"type":"fact"
            }));
        }
        let second = retrieve_ranked_with_rerank(&db, &scope,
            "database rollback", 3, &importance, &rerank);
        let ids = |results: &[Value]| results.iter()
            .map(|item| item["id"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids(&first), ids(&second));
    }
}
