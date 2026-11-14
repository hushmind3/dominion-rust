//! Owner plan intervention at a safe iteration boundary.
/// Unlike changing a prompt in the middle of a tool call, this consumes an
/// explicit new plan only before the next side-effecting executor turn.
/// Success criteria, task identity, permissions and reviewer remain unchanged.
use crate::state::AppState;
use serde_json::{json, Value};

/// A direct owner plan edit supersedes stale tool proposals. Match both the
/// active goal's session and its root task ID so other sessions, model text
/// and untrusted tool output cannot change which run is interrupted.
pub(crate) fn pending_owner_steer(
    state: &AppState, session_id: &str, task_id: Option<&str>,
) -> Option<String> {
    let task_id = task_id?;
    state.store.all("goal").into_iter().find(|goal| {
        goal["status"] == "running"
            && goal["sessionId"].as_str() == Some(session_id)
            && goal["taskId"].as_str() == Some(task_id)
            && goal["planOverride"].as_str()
                .is_some_and(|plan| !plan.trim().is_empty())
    }).and_then(|goal| goal["id"].as_str().map(str::to_string))
}

/// Complete every unexecuted assistant tool call with an explicit superseded
/// result. Strict providers require 1 tool response per proposed call even
/// when the owner changes instructions while the agent is executing.
pub(crate) fn record_superseded_tool_results(
    state: &AppState, session_id: &str, execution_id: &str,
    agent_id: Option<&str>, calls: &[(String, String)],
) -> Result<(), String> {
    for (id, name) in calls {
        let saved = state.store.write_message(&json!({
            "session_id": session_id,
            "role": "tool",
            "tool_call_id": id,
            "content": json!({"error":"Superseded by a new direct owner plan",
                "ownerSteerSuperseded":true,"tool":name}).to_string(),
            "agent_id": agent_id,
            "metadata": {"authorKind":"tool","executionId":execution_id,
                "ownerSteerSuperseded":true}
        })).ok_or("사용자 변경으로 취소한 도구 호출 결과를 저장하지 못했습니다.")?;
        state.bus.message(&saved);
    }
    Ok(())
}

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
    fn only_current_owner_goal_steer_interrupts_matching_session_and_task() {
        let dir=std::env::temp_dir().join(format!("goal-steer-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("goal",&json!({"id":"g","status":"running",
            "sessionId":"s1","taskId":"t1","planOverride":"new owner instruction"}));
        assert_eq!(pending_owner_steer(&state,"s1",Some("t1")).as_deref(),Some("g"));
        assert_eq!(pending_owner_steer(&state,"s2",Some("t1")),None);
        assert_eq!(pending_owner_steer(&state,"s1",Some("t2")),None);
        assert_eq!(pending_owner_steer(&state,"s1",None),None);
        state.store.patch_if("goal","g","status",&json!("running"),&json!({"planOverride":null}));
        assert_eq!(pending_owner_steer(&state,"s1",Some("t1")),None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn owner_superseded_tool_proposals_receive_pairing_messages() {
        let dir=std::env::temp_dir().join(format!("goal-tools-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        let skipped=vec![("call-1".to_string(),"workspace_write".to_string()),
            ("call-2".to_string(),"shell_exec".to_string())];
        record_superseded_tool_results(&state,"s1","run-1",Some("agent-1"),&skipped).unwrap();
        let results=state.store.messages("s1");
        assert_eq!(results.len(),2);
        assert_eq!(results[0]["tool_call_id"],"call-1");
        assert_eq!(results[1]["tool_call_id"],"call-2");
        assert_eq!(results[0]["metadata"]["ownerSteerSuperseded"],true);
        assert_eq!(results[1]["role"],"tool");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
