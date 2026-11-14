use super::*;
    use std::sync::{Arc, Barrier};

    fn session_job(parent: &str) -> (Value, Value) {
        let session_id = uuid::Uuid::new_v4().to_string();
        let job_id = uuid::Uuid::new_v4().to_string();
        (
            json!({"id":session_id,"parentSessionId":parent,"delegated":true}),
            json!({"id":job_id,"kind":"prompt","status":"queued","input":{
                "sessionId":session_id,"parentSessionId":parent,"delegation":true
            }})
        )
    }

    fn temporary_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "dominion-delegation-reservation-{}", uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (Store::open(&dir.join("jobs.sqlite")).unwrap(), dir)
    }

    #[test]
    fn concurrent_independent_connections_cannot_overbook_or_orphan_sessions() {
        let (primary, dir) = temporary_store();
        let secondary = Store::open(&dir.join("jobs.sqlite")).unwrap();
        primary.put("session", &json!({"id":"root"})).unwrap();
        let barrier = Arc::new(Barrier::new(12));
        let threads: Vec<_> = (0..12).map(|index| {
            let store = if index % 2 == 0 {primary.clone()} else {secondary.clone()};
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let (session, job) = session_job("root");
                barrier.wait();
                store.reserve_delegation("root", &session, &job, 24, 4)
            })
        }).collect();

        let results: Vec<_> = threads.into_iter().map(|worker| worker.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 4);
        assert_eq!(primary.all("job").len(), 4);
        assert_eq!(primary.all("session").len(), 5);
        for job in primary.all("job") {
            let session_id = job["input"]["sessionId"].as_str().unwrap();
            let session = primary.get("session", session_id).unwrap();
            assert_eq!(session["parentSessionId"], "root");
            assert_eq!(session["delegated"], true);
        }
        drop(secondary);
        drop(primary);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn terminal_jobs_release_active_capacity_but_not_lifetime_capacity() {
        let (store, dir) = temporary_store();
        let (first_session, first_job) = session_job("root");
        store.reserve_delegation("root", &first_session, &first_job, 2, 1).unwrap();
        let (next_session, next_job) = session_job("root");
        assert!(store.reserve_delegation("root", &next_session, &next_job, 2, 1)
            .unwrap_err().contains("동시 위임"));
        assert!(store.get("session", next_session["id"].as_str().unwrap()).is_none());
        let mut finished = first_job;
        finished["status"] = json!("completed");
        store.put("job", &finished).unwrap();
        store.reserve_delegation("root", &next_session, &next_job, 2, 1).unwrap();
        let (last_session, last_job) = session_job("root");
        assert!(store.reserve_delegation("root", &last_session, &last_job, 2, 1)
            .unwrap_err().contains("위임 작업 한도"));
        assert_eq!(store.all("job").len(), 2);
        assert_eq!(store.all("session").len(), 2);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelled_parent_and_forged_metadata_never_create_workers() {
        let (store, dir) = temporary_store();
        let (session, mut job) = session_job("root");
        job["input"]["parentSessionId"] = json!("other");
        assert!(store.reserve_delegation("root", &session, &job, 24, 4).is_err());
        assert!(store.all("job").is_empty());
        assert!(store.all("session").is_empty());
        job["input"]["parentSessionId"] = json!("root");
        store.cancel_session_epoch("root").unwrap();
        assert!(store.reserve_delegation("root", &session, &job, 24, 4)
            .unwrap_err().contains("중단"));
        assert!(store.all("job").is_empty());
        assert!(store.all("session").is_empty());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
