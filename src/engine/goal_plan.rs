//! Owner plan intervention at a safe iteration boundary.
/// Unlike changing a prompt in the middle of a tool call, this consumes an
/// explicit new plan only before the next side-effecting executor turn.
/// Success criteria, task identity, permissions and reviewer remain unchanged.
use crate::state::AppState;
use serde_json::{json, Value};

/// A bounded, durable owner instruction journal; requests and consumption are
/// compared against ONE monotonic revision. No model-generated text, tool
/// response or teammate message can enqueue a directive via this interface.
///
/// `replace` replaces the approach, `append` adds a new owner instruction
/// without discarding the current plan. Neither changes acceptance criteria,
/// execution permissions, or already committed tool effects.
pub(crate) fn enqueue_directive(
    state: &AppState, goal_id: &str, text: &str, mode: &str,
    request_id: Option<&str>,
) -> Result<Value, String> {
    let instruction = text.trim();
    if instruction.len() < 5 || instruction.len() > 6000 {
        return Err("사용자 지시는 5~6000바이트여야 합니다.".into());
    }
    if !matches!(mode, "replace" | "append") {
        return Err("지시 모드는 replace 또는 append입니다.".into());
    }
    let id = match request_id {
        Some(id) if !id.is_empty() && id.len() <= 100
            && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') => id.to_string(),
        Some(_) => return Err("requestId는 영문·숫자·-·_ 1~100자여야 합니다.".into()),
        None => uuid::Uuid::new_v4().to_string(),
    };
    for _ in 0..12 {
        let goal = state.store.get("goal", goal_id)
            .ok_or("목표를 찾을 수 없습니다.")?;
        if !matches!(goal["status"].as_str(), Some("queued" | "running")) {
            return Err("진행 중인 목표에만 지시할 수 있습니다.".into());
        }
        let mut journal = goal["ownerDirectiveJournal"].as_array()
            .cloned().unwrap_or_default();
        if let Some(previous) = journal.iter().find(|item| item["id"] == id) {
            if previous["text"] == instruction && previous["mode"] == mode {
                return Ok(goal); // idempotent retried client request
            }
            return Err("같은 requestId가 다른 지시 내용에 사용됐습니다.".into());
        }
        if journal.len() >= 128 {
            return Err("사용자 지시 기록이 128개에 도달했습니다. 이전 기록을 보존한 채 새 요청을 중단합니다.".into());
        }
        let current_revision = goal["ownerDirectiveRevision"].as_u64().unwrap_or(0);
        let seq = journal.iter().filter_map(|item|item["seq"].as_u64())
            .max().unwrap_or(0).saturating_add(1);
        journal.push(json!({
            "id":id,"seq":seq,"source":"owner","mode":mode,"text":instruction,
            "status":"pending","createdAt":chrono::Utc::now().to_rfc3339()
        }));
        let patch = json!({
            "ownerDirectiveRevision":current_revision.saturating_add(1),
            "ownerDirectiveJournal":journal,
            "planOverride":if mode=="replace" {json!(instruction)} else {Value::Null},
            "planOverrideRequestedAt":chrono::Utc::now().to_rfc3339(),
        });
        // The revision changes on BOTH submissions and consumption; a
        // simultaneous submit/consume cannot lose any owner message.
        if let Some(saved) = state.store.patch_if(
            "goal", goal_id, "ownerDirectiveRevision", &goal["ownerDirectiveRevision"], &patch
        ) {
            state.bus.record("goal",&saved);
            state.store.audit("goal.owner_directive_queued",json!({
                "goalId":goal_id,"directiveId":id,"mode":mode,"seq":seq,
            }));
            return Ok(saved);
        }
    }
    Err("사용자 지시를 동시에 수정 중입니다. 같은 requestId로 다시 요청하세요.".into())
}

fn pending_directives(goal: &Value) -> bool {
    goal["ownerDirectiveJournal"].as_array().is_some_and(|items|
        items.iter().any(|item|item["status"]=="pending" && item["source"]=="owner"))
}

/// Incorporate ALL pending owner messages in sequence at a single
/// pre-tool boundary, avoiding wasted model calls for a burst of edits.
fn apply_journal(
    state:&AppState,goal_id:&str,goal:&Value,
    plan:&mut String,history:&mut Vec<Value>,
) -> Option<bool> {
    let mut journal=goal["ownerDirectiveJournal"].as_array()?.clone();
    if !pending_directives(goal) {return None;}
    let mut revised=plan.clone();
    let mut next_history=history.clone();
    let mut count=0usize;
    let mut last_seq=0u64;
    for item in &mut journal {
        if item["status"]!="pending" || item["source"]!="owner" {continue;}
        let text=item["text"].as_str()?.trim();
        let seq=item["seq"].as_u64()?;
        let mode=item["mode"].as_str()?;
        let proposed=match mode {
            "replace" => text.to_string(),
            "append" => format!("{revised}\n\n[사용자 추가 지시 #{seq}]\n{text}"),
            _ => return None,
        };
        if proposed.len()>32_000 {
            state.store.audit("goal.owner_directive_capacity",json!({
                "goalId":goal_id,"seq":seq,"bytes":proposed.len(),
            }));
            return None; // do not consume a directive we cannot carry
        }
        revised=proposed;
        last_seq=seq;
        count+=1;
        item["status"]=json!("applied");
        item["appliedAt"]=json!(chrono::Utc::now().to_rfc3339());
        next_history.push(json!({
            "version":next_history.len(),"plan":revised,"reason":"owner_directive",
            "directiveId":item["id"],"directiveSeq":seq,"mode":mode,
            "at":chrono::Utc::now().to_rfc3339()
        }));
    }
    if count==0 {return None;}
    let revision=goal["ownerDirectiveRevision"].as_u64().unwrap_or(0);
    let patch=json!({
        "ownerDirectiveRevision":revision.saturating_add(1),
        "ownerDirectiveJournal":journal,
        "ownerAppliedSeq":last_seq,
        "planOverride":Value::Null,
        "plan":revised,"planHistory":next_history,
    });
    let saved=state.store.patch_if("goal",goal_id,"ownerDirectiveRevision",
        &goal["ownerDirectiveRevision"],&patch)?;
    let changed=*plan != revised;
    *plan=revised;
    *history=saved["planHistory"].as_array().cloned().unwrap_or_default();
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_directive_applied",json!({
        "goalId":goal_id,"throughSeq":last_seq,"count":count,
        "changed":changed,
    }));
    Some(true) // even same-text corrections are acknowledged
}

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
            && (pending_directives(goal)
                || goal["planOverride"].as_str()
                    .is_some_and(|plan| !plan.trim().is_empty()))
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
    if let Some(applied)=apply_journal(state,goal_id,&goal,plan,history) {
        return applied;
    }
    // Legacy pre-journal plan edits remain recoverable after migration.
    // A live journal always takes precedence over this compatibility field.
    if pending_directives(&goal) {return false;}
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
    fn durable_owner_queue_preserves_order_identity_and_replay() {
        let dir=std::env::temp_dir().join(format!("owner-inbox-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("goal",&json!({"id":"g","status":"running","sessionId":"s","taskId":"t","plan":"initial"}));
        enqueue_directive(&state,"g","first replacement","replace",Some("first")).unwrap();
        enqueue_directive(&state,"g","then check the actual result","append",Some("second")).unwrap();
        // Retried POSTs must not create a second owner directive.
        enqueue_directive(&state,"g","first replacement","replace",Some("first")).unwrap();
        assert!(enqueue_directive(&state,"g","different instruction","replace",Some("first")).is_err());
        assert_eq!(pending_owner_steer(&state,"s",Some("t")).as_deref(),Some("g"));
        assert_eq!(pending_owner_steer(&state,"s",Some("wrong")),None);
        let mut plan="initial".to_string();
        let mut history=vec![json!({"plan":"initial"})];
        assert!(apply_owner_override(&state,"g",&mut plan,&mut history));
        assert_eq!(plan,"first replacement\n\n[사용자 추가 지시 #2]\nthen check the actual result");
        assert_eq!(history.len(),3);
        assert_eq!(pending_owner_steer(&state,"s",Some("t")),None);
        let goal=state.store.get("goal","g").unwrap();
        assert_eq!(goal["ownerDirectiveJournal"][0]["status"],"applied");
        assert_eq!(goal["ownerDirectiveJournal"][1]["status"],"applied");
        assert_eq!(goal["ownerAppliedSeq"],2);
        assert!(goal["planOverride"].is_null());
        assert!(!apply_owner_override(&state,"g",&mut plan,&mut history));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn owner_queue_rejects_invalid_modes_and_non_owner_records_do_not_steer() {
        let dir=std::env::temp_dir().join(format!("owner-authority-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("goal",&json!({"id":"g","status":"running","sessionId":"s","taskId":"t",
            "ownerDirectiveJournal":[{"source":"tool","status":"pending","text":"replace owner"}]}));
        assert_eq!(pending_owner_steer(&state,"s",Some("t")),None);
        assert!(enqueue_directive(&state,"g","Some owner directive","unknown",None).is_err());
        assert!(enqueue_directive(&state,"g","Some owner directive","append",Some("../bad")).is_err());
        assert!(enqueue_directive(&state,"g","Some owner directive","append",Some("valid")).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

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
