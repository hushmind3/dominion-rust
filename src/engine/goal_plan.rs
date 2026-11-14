//! Owner plan intervention at a safe iteration boundary.
/// Unlike changing a prompt in the middle of a tool call, this consumes an
/// explicit new plan only before the next side-effecting executor turn.
/// Success criteria, task identity, permissions and reviewer remain unchanged.
use crate::state::AppState;
use serde_json::{json, Value};

pub(crate) fn apply_owner_override(
    state:&AppState,goal_id:&str,plan:&mut String,history:&mut Vec<Value>,
) -> bool {
    let Some(goal)=state.store.get("goal",goal_id) else {return false};
    if !matches!(goal["status"].as_str(),Some("queued"|"running")) {
        return false;
    }
    let Some(next)=goal["planOverride"].as_str()
        .map(str::trim).filter(|s|!s.is_empty()&&s.len()<=6000) else {return false};
    let fresh=next.to_string();
    let mut patch=json!({"planOverride":Value::Null});
    let changed=fresh!=*plan;
    if changed {
        let mut updated=history.clone();
        updated.push(json!({"version":updated.len(),"plan":fresh,
            "reason":"owner_override","at":chrono::Utc::now().to_rfc3339()}));
        patch["plan"]=json!(fresh);
        patch["planHistory"]=json!(updated);
    }
    // CAS prevents a stale plan consumer from deleting a newer submitted
    // owner correction. Keep the pending plan if another update won the race.
    let Some(saved)=state.store.patch_if("goal",goal_id,"planOverride",
        &goal["planOverride"],&patch) else {return false};
    if changed {
        *plan=fresh;
        *history=saved["planHistory"].as_array().cloned().unwrap_or_default();
    }
    state.bus.record("goal",&saved);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn owner_override_changes_only_plan_and_records_provenance() {
        let dir=std::env::temp_dir().join(format!("goal-plan-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("goal",&json!({"id":"g","status":"running","plan":"old",
            "planOverride":"new","acceptanceChecks":[{"type":"file_exists","path":"a"}],
            "executor":{"agentId":"alpha"}}));
        let mut current="old".to_string();
        let mut history=vec![json!({"plan":"old"})];
        assert!(apply_owner_override(&state,"g",&mut current,&mut history));
        assert_eq!(current,"new");
        assert_eq!(history[1]["reason"],"owner_override");
        let stored=state.store.get("goal","g").unwrap();
        assert_eq!(stored["acceptanceChecks"][0]["path"],"a");
        assert_eq!(stored["executor"]["agentId"],"alpha");
        assert!(stored["planOverride"].is_null());
        assert!(!apply_owner_override(&state,"g",&mut current,&mut history));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
