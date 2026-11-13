//! Memory learning and visibility tests split from production code.
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
    fn compressed_memory_restores_only_matched_scoped_original_evidence() {
        let db=Store::open(std::path::Path::new(":memory:")).unwrap();
        let scope=MemoryScope::for_session(&db,"s",Some("a"));
        let summary_id="summary-archive";
        let common=json!({"scope":"agent","agentId":"a","partitionKey":scope.partition_key,
            "sessionId":"s","sourceAuthorKey":"owner","type":"fact"});
        let original=json!({"id":"archived-detail","text":"복구 식별자 bluewater7294 보관",
            "status":"archived","compressedInto":summary_id,
            "scope":common["scope"],"agentId":common["agentId"],
            "partitionKey":common["partitionKey"],"sessionId":common["sessionId"],
            "sourceAuthorKey":common["sourceAuthorKey"],"type":common["type"]});
        db.put("memory",&original);
        db.put("memory",&json!({"id":summary_id,"source":"compression",
            "text":"이전 문제의 해결 요약","status":"active",
            "compressedFrom":["archived-detail","wrong-scope"],
            "scope":common["scope"],"agentId":common["agentId"],
            "partitionKey":common["partitionKey"],"sessionId":common["sessionId"],
            "sourceAuthorKey":common["sourceAuthorKey"],"type":common["type"]}));
        db.put("memory",&json!({"id":"wrong-scope","text":"secretcode12345",
            "status":"archived","compressedInto":summary_id,"scope":"agent",
            "agentId":"someone-else","partitionKey":"private-room",
            "sessionId":"private","sourceAuthorKey":"owner","type":"fact"}));
        let restored=retrieve(&db,&scope,"bluewater7294",4);
        assert_eq!(restored.len(),1);
        assert_eq!(restored[0]["id"],summary_id);
        assert_eq!(restored[0]["restoredOriginals"][0]["id"],"archived-detail");
        let context=context_block(&db,&scope,"bluewater7294").unwrap();
        assert!(context.contains("bluewater7294"),
            "matched original details must reach the model context");
        assert!(!context.contains("secretcode12345"));
        assert!(retrieve(&db,&scope,"secretcode12345",4).is_empty(),
            "a mismatched legacy summary must never expose private originals");
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
