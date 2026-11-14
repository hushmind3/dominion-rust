//! Real SQLite usage receipts across delegated executor sessions.
use super::*;
use crate::{config::Config, db::Store, vault::Vault};
#[test]
fn shared_goal_budget_includes_descendants_and_caches_but_excludes_other_sessions(){
    let root=std::env::temp_dir().join(format!("dominion-shared-budget-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(config,Store::open(&root.join("db.sqlite")).unwrap(),
        Vault::open(&root.join("key")));
    for (id,parent) in [("root",None),("child",Some("root")),
        ("grandchild",Some("child")),("other",None)]{
        state.store.put("session",&json!({"id":id,"goalId":"g",
            "parentSessionId":parent,"delegated":parent.is_some()})).unwrap();
    }
    state.store.put("goal",&json!({"id":"g","status":"running",
        "sessionId":"root","tokenBudget":100})).unwrap();
    for (session,tokens) in [("root",10),("child",20),
        ("grandchild",45),("other",200)]{
        state.store.audit("model.usage",json!({
            "sessionId":session,"promptTokens":tokens,
            "completionTokens":0,"totalTokens":tokens
        }));
    }
    state.store.audit("model.cache_usage",json!({
        "sessionId":"child","promptTokens":20,"cachedPromptTokens":10
    }));
    let usage=state.store.session_tree_usage("root").unwrap();
    assert_eq!(usage["totalTokens"],75);
    assert_eq!(usage["calls"],3);
    assert_eq!(usage["cacheReportedCalls"],1);
    assert_eq!(usage["cachedPromptTokens"],10);
    assert_eq!(usage["cacheHitRate"],0.5);
    let child=state.store.get("session","child").unwrap();
    assert!(goal_token_budget_error(&state,&child).is_none());
    state.store.audit("model.usage",json!({
        "sessionId":"grandchild","promptTokens":30,
        "completionTokens":0,"totalTokens":30
    }));
    assert_eq!(state.store.session_tree_usage("root").unwrap()["totalTokens"],105);
    assert!(goal_token_budget_error(&state,&child).is_some());
    assert!(goal_token_budget_error(&state,
        &state.store.get("session","root").unwrap()).is_some());
    // A non-delegated peer with a parentSessionId is NOT charged to this goal.
    state.store.put("session",&json!({"id":"not-delegated",
        "parentSessionId":"root","delegated":false,"goalId":"g"})).unwrap();
    state.store.audit("model.usage",json!({"sessionId":"not-delegated",
        "totalTokens":500}));
    assert_eq!(state.store.session_tree_usage("root").unwrap()["totalTokens"],105);
    // UNIX recursion terminates even with a legacy/session-corruption cycle.
    state.store.patch_if("session","root","id",&json!("root"),
        &json!({"delegated":true,"parentSessionId":"grandchild"})).unwrap();
    assert_eq!(state.store.session_tree_usage("root").unwrap()["calls"],4);
    state.store.patch_if("goal","g","id",&json!("g"),
        &json!({"tokenBudget":0})).unwrap();
    assert!(goal_token_budget_error(&state,&child).is_none());
    drop(state);
    let _=std::fs::remove_dir_all(root);
}
