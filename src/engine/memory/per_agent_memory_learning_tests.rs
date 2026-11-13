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
