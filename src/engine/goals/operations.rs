//! Session lineage and unknown effect admission for autonomous goals.
use super::*;

// Resolve only the goal's own session tree. An unresolved tool from an
// unrelated chat/room must never stop an autonomous goal.
pub(super) fn in_goal_session_tree(state:&AppState,session:&str,root:&str)->bool {
    let mut cursor=session.to_string();
    let mut seen=HashSet::new();
    for _ in 0..=16 {
        if cursor==root {return true;}
        if cursor.is_empty()||!seen.insert(cursor.clone()) {return false;}
        let Some(saved)=state.store.get("session",&cursor) else {return false;};
        cursor=saved["parentSessionId"].as_str().unwrap_or("").to_owned();
    }
    false
}

pub(super) fn unknown_new_goal_operations(state:&AppState,session:&str,previous:&HashSet<String>)->Vec<String>{
    state.store.all("execution_operation").into_iter()
        .filter(|op|op["sessionId"].as_str()
            .is_some_and(|sid|in_goal_session_tree(state,sid,session)))
        .filter(|op|op["id"].as_str().is_some_and(|id|!previous.contains(id)))
        .filter(|op|op["outcomeUnknown"]==true
            || matches!(op["status"].as_str(),Some("running"|"interrupted"))
            || matches!(op["effectState"].as_str(),Some("needs_review"|"unknown")))
        .filter_map(|op|op["id"].as_str().map(str::to_string))
        .take(20).collect()
}
