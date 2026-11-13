    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("dominion.sqlite")).unwrap();
        (store, dir)
    }

    #[test]
    fn run_leases_are_exclusive_and_recover_after_crash() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_lease("session:s", "owner-a", 60_000).is_some());
        assert!(
            store.acquire_lease("session:s", "owner-b", 60_000).is_none(),
            "a live lease must refuse a second owner"
        );
        assert!(store
            .acquire_lease("session:s", "owner-a", 60_000)
            .is_some());
        assert!(store.renew_lease("session:s", "owner-b", 60_000).is_none());
        assert!(store.renew_lease("session:s", "owner-a", 60_000).is_some());

        // A crashed owner stops renewing; a new process releases foreign and
        // expired leases before it resumes work.
        assert!(store.acquire_lease("session:t", "dead", -1).is_some());
        let released = store.release_stale_leases("new-instance");
        assert!(released >= 2, "released {released}");
        assert!(store.active_lease("session:s").is_none());
        assert!(store.acquire_lease("session:s", "owner-c", 60_000).is_some());
    }

    #[test]
    fn usage_summary_aggregates_calls_and_estimates() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":100,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":50,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p2","model":"n","purpose":"helper","promptTokens":10,"completionTokens":5,"totalTokens":15}),
        );
        store.audit("tool.call", json!({"tool":"x"}));
        let summary = store.usage_summary();
        assert_eq!(summary["calls"], 3);
        assert_eq!(summary["totalTokens"], 165);
        assert_eq!(summary["promptTokens"], 10);
        assert_eq!(summary["completionTokens"], 5);
        assert_eq!(summary["estimatedCalls"], 2);
        let groups = summary["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["providerId"], "p1");
        assert_eq!(groups[0]["calls"], 2);
        assert_eq!(groups[1]["estimatedCalls"], 0);
    }

    #[test]
    fn attempt_creation_is_atomic_and_idempotent() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let root = json!({
            "id":"root-1","contract":{"revision":"r1"},
            "attempts":[],"rootOutcome":"running"
        });
        let execution = json!({"id":"exec-1","taskId":"root-1"});
        let attempt = json!({"executionId":"exec-1","status":"running"});
        assert_eq!(
            store.create_attempt(&root, &execution, &attempt).unwrap()["id"],
            "exec-1"
        );
        let episode = store.get("task_episode", "root-1").unwrap();
        assert_eq!(episode["attempts"].as_array().unwrap().len(), 1);
        store.create_attempt(&root, &execution, &attempt).unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "re-linking the same attempt must be idempotent"
        );
        store
            .create_attempt(
                &root,
                &json!({"id":"exec-2","taskId":"root-1"}),
                &json!({"executionId":"exec-2","status":"running"}),
            )
            .unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn attempt_creation_rolls_back_when_the_episode_is_corrupt() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "task_episode",
            &json!({"id":"root-2","attempts":"broken"}),
        );
        assert!(store
            .create_attempt(
                &json!({"id":"root-2"}),
                &json!({"id":"exec-3","taskId":"root-2"}),
                &json!({"executionId":"exec-3"}),
            )
            .is_none());
        assert!(
            store.get("execution", "exec-3").is_none(),
            "a failed attempt link must not leave a detached execution"
        );
    }

    #[test]
    fn records_round_trip_with_client_fields() {
        let (store, dir) = temp_store();
        let saved = store
            .put(
                "provider",
                &json!({ "name": "Local", "baseUrl": "http://127.0.0.1:4199/v1" }),
            )
            .expect("saved");
        let id = saved["id"].as_str().unwrap().to_string();
        assert_eq!(saved["name"], "Local");
        assert!(saved.get("createdAt").is_some());

        let fetched = store.get("provider", &id).unwrap();
        assert_eq!(fetched["baseUrl"], "http://127.0.0.1:4199/v1");

        let updated = store
            .put(
                "provider",
                &json!({ "id": id, "name": "Renamed", "baseUrl": "http://x" }),
            )
            .unwrap();
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(store.all("provider").len(), 1);

        store.remove("provider", &id);
        assert!(store.get("provider", &id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn messages_preserve_reasoning_and_order() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi",
            "reasoning": "thought", "tool_calls": [{ "id": "c1" }]
        }));
        let messages = store.messages("s1");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["reasoning"], "thought");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        store.clear_messages("s1");
        assert!(store.messages("s1").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn try_messages_reads_the_same_rows_with_error_preservation() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi"
        }));
        let strict = store.try_messages("s1").unwrap();
        assert_eq!(strict, store.messages("s1"));
        assert_eq!(strict.len(), 2);
        assert!(store.try_messages("missing").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_workers_claim_a_job_only_once() {
        let (store, dir) = temp_store();
        store
            .put_if_absent("job", &json!({"id":"one","status":"queued"}))
            .unwrap();
        assert!(store
            .put_if_absent("job", &json!({"id":"one","status":"overwritten"}))
            .is_none());
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .patch_if(
                            "job",
                            "one",
                            "status",
                            &json!("queued"),
                            &json!({"status":"running"}),
                        )
                        .is_some()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .filter_map(|worker| worker.join().ok())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
        assert_eq!(store.get("job", "one").unwrap()["status"], "running");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_finalizations_merge_improvement_counters() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "improvement",
            &json!({
                "id":"imp-1","status":"adopted",
                "uses":5,"successes":5,"failures":0,"appliedExecutionIds":[]
            }),
        );
        for (execution_id, status) in [("exec-a", "validated"), ("exec-b", "adopted")] {
            let execution = json!({"id":execution_id});
            let marker = json!({
                "id": format!("execution:{execution_id}"),
                "payloadHash": format!("hash-{execution_id}"),
            });
            let improvement = json!({
                "id":"imp-1","uses":6,"successes":6,"failures":0,"status":status,
                "appliedExecutionIds":[execution_id],
                "__counterDelta":{"uses":1,"successes":1,"failures":0},
            });
            assert_eq!(
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap(),
                FinalizeOutcome::Applied
            );
        }
        let merged = store.get("improvement", "imp-1").unwrap();
        assert_eq!(merged["uses"], 7, "one observation was lost: {merged}");
        assert_eq!(merged["successes"], 7);
        assert_eq!(merged["appliedExecutionIds"].as_array().unwrap().len(), 2);
        assert_eq!(merged["status"], "validated");
        assert!(merged.get("__counterDelta").is_none());
    }

    #[test]
    fn owner_dispatch_is_atomic_and_metadata_is_preserved() {
        let (store, dir) = temp_store();
        let message = json!({"id":"m1","session_id":"room:r","role":"user","content":"내 지시","metadata":{"authorKind":"owner","roomId":"r"}});
        let job = json!({"id":"j1","kind":"room","status":"queued"});
        let (saved, _) = store
            .write_message_with_record(&message, "job", &job)
            .unwrap();
        assert_eq!(saved["metadata"]["authorKind"], "owner");
        assert_eq!(store.message("m1").unwrap()["content"], "내 지시");
        let mut conflicting = message.clone();
        conflicting["id"] = json!("m2");
        assert!(store
            .write_message_with_record(&conflicting, "job", &job)
            .is_none());
        assert!(
            store.message("m2").is_none(),
            "queue failure must roll back owner insertion"
        );
        assert_eq!(store.messages("room:r").len(), 1);
        store.put("agent", &json!({"id":"a","model":"old"}));
        assert!(store
            .patch_records(
                "agent",
                &["a".into(), "missing".into()],
                &json!({"model":"new"})
            )
            .is_none());
        assert_eq!(store.get("agent", "a").unwrap()["model"], "old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finalization_is_idempotent_and_conflict_aware() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-1","status":"completed"});
        let marker = json!({"id":"execution:exec-1","payloadHash":"hash-1"});
        let improvement = json!({"id":"imp-1","uses":0,"status":"adopted"});
        let observation = json!({"id":"obs-1"});
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::Applied
        );
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::AlreadyApplied
        );
        let conflicting = json!({"id":"execution:exec-1","payloadHash":"hash-2"});
        assert_eq!(
            store
                .finalize_once(&execution, &conflicting, &[], &[])
                .unwrap(),
            FinalizeOutcome::Conflict
        );
        assert_eq!(store.get("improvement", "imp-1").unwrap()["uses"], 0);
        assert_eq!(store.try_all("improvement").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_finalizations_apply_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-2","status":"completed"});
        let marker = json!({"id":"execution:exec-2","payloadHash":"hash-2"});
        let improvement = json!({"id":"imp-2","uses":0});
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = store.clone();
            let execution = execution.clone();
            let marker = marker.clone();
            let improvement = improvement.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap()
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.contains(&FinalizeOutcome::Applied));
        assert!(outcomes.contains(&FinalizeOutcome::AlreadyApplied));
        assert_eq!(store.try_all("execution_finalization").unwrap().len(), 1);
    }

    #[test]
    fn try_all_preserves_records_and_returns_them() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("memory", &json!({"id":"m1","text":"a"}));
        store.put("memory", &json!({"id":"m2","text":"b"}));
        let records = store.try_all("memory").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn broken_finalization_marker_is_an_error() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind,id,data) VALUES('execution_finalization','execution:broken','{bad')",
                [],
            )
            .unwrap();
        }
        let execution = json!({"id":"broken","status":"completed"});
        let marker = json!({"id":"execution:broken","payloadHash":"hash"});
        assert!(store.finalize_once(&execution, &marker, &[], &[]).is_err());
    }
