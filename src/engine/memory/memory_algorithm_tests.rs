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
