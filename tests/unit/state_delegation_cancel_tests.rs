use super::*;
use crate::{config::Config, db::Store, vault::Vault};
use serde_json::json;

#[test]
fn parent_cancellation_reaches_active_children_but_not_independent_jobs() {
    let dir=std::env::temp_dir().join(format!("dominion-delegate-cancel-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    for session in ["root","worker","grandchild","other"] {
        state.store.put("session",&json!({"id":session}));
    }
    for (id,session,parent) in [("child","worker","root"),("grand","grandchild","worker"),("other","other","different")] {
        state.store.put("job",&json!({"id":id,"kind":"prompt","status":"running",
            "input":{"delegation":true,"sessionId":session,"parentSessionId":parent}}));
    }
    let before=state.cancellation_generation("worker");
    state.cancel_session("root");
    assert!(state.is_cancelled("worker"));
    assert!(state.is_cancelled("grandchild"));
    assert!(!state.is_cancelled("other"));
    assert!(state.cancellation_generation("worker")>before);
    for job in ["child","grand"] {
        let record=state.store.get("job",job).unwrap();
        assert_eq!(record["status"],"cancelled");
        assert_eq!(record["cancelRequested"],true);
    }
    assert_eq!(state.store.get("job","other").unwrap()["status"],"running");
    // A fresh process cannot revive an earlier worker invocation.
    let restarted=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    assert!(restarted.is_cancelled("worker"));
    assert!(restarted.is_cancelled("grandchild"));
    drop(restarted);drop(state);
    std::fs::remove_dir_all(dir).unwrap();
}
