use super::*;
use crate::{config::Config,db::Store,state::AppState,vault::Vault};
fn fixture()->(AppState,std::path::PathBuf){
    let dir=std::env::temp_dir().join(format!("dominion-plan-steering-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
    let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
    (state,dir)
}
#[test]
fn concurrent_owner_revisions_require_fresh_compare_and_swap(){
    let (state,dir)=fixture();
    state.store.put("goal",&json!({"id":"goal","status":"running",
        "plan":"initial","planRevision":0,"appliedPlanRevision":0})).unwrap();
    let submitted=submit(&state,"goal",0,"inspect and repair").unwrap();
    assert_eq!(submitted["planRevision"],1);
    assert!(matches!(submit(&state,"goal",0,"stale"),Err(SteeringError::Conflict)));
    let pending_plan=pending(&state,"goal",0).unwrap();
    assert_eq!(pending_plan,(1,"inspect and repair".to_string()));
    assert!(pending(&state,"goal",1).is_none());
    assert_eq!(state.store.get("goal","goal").unwrap()["plan"],"initial",
        "owner steering must not rewrite an in-flight executor plan");
    let next=submit(&state,"goal",1,"new plan").unwrap();
    assert_eq!(next["planRevision"],2);
    assert_eq!(pending(&state,"goal",1).unwrap().1,"new plan");
    let row=state.store.get("goal","goal").unwrap();
    state.store.patch_if("goal","goal","status",&row["status"],
        &json!({"status":"completed"})).unwrap();
    assert!(matches!(submit(&state,"goal",2,"late"),Err(SteeringError::Immutable)));
    assert!(matches!(submit(&state,"missing",0,"late"),Err(SteeringError::Missing)));
    assert!(matches!(submit(&state,"goal",2,""),Err(SteeringError::Invalid)));
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn direct_instruction_has_its_own_durable_kind_and_supersedes_earlier_plan() {
    let (state,dir)=fixture();
    state.store.put("goal",&json!({"id":"owner-task","status":"running",
        "plan":"old model plan","planRevision":0,"appliedPlanRevision":0})).unwrap();
    let plan=submit(&state,"owner-task",0,"model plan").unwrap();
    assert_eq!(plan["kind"],"plan");
    let edit=submit_instruction(&state,"owner-task",1,"사용자 최신 지시를 먼저 수행").unwrap();
    assert_eq!(edit["kind"],"instruction");
    let (revision,content,kind)=pending_command(&state,"owner-task",0).unwrap();
    assert_eq!(revision,2);
    assert_eq!(content,"사용자 최신 지시를 먼저 수행");
    assert_eq!(kind,SteeringKind::Instruction);
    assert!(pending_command(&state,"owner-task",2).is_none());
    assert!(matches!(submit_instruction(&state,"owner-task",1,"old"),Err(SteeringError::Conflict)));
    assert_eq!(state.store.get("goal","owner-task").unwrap()["plan"],
        "old model plan","an in-flight model plan is never silently overwritten");
    drop(state); std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn owner_revision_and_goal_terminal_status_are_one_atomic_cas() {
    let (state,dir)=fixture();
    let initial=state.store.put("goal",&json!({
        "id":"sealing-goal","status":"running","planRevision":0,
        "appliedPlanRevision":0,"plan":"old model plan"
    })).unwrap();
    let original_status=initial["status"].clone();
    let original_rev=initial["planRevision"].clone();
    let instruction=submit_instruction(&state,"sealing-goal",0,
        "주인의 최신 지시를 우선 수행").unwrap();
    assert_eq!(instruction["planRevision"],1);
    // A completed report produced under revision 0 may NOT seal revision 1.
    assert!(state.store.patch_if_fields("goal","sealing-goal",
        &[("status",&original_status),("planRevision",&original_rev)],
        &json!({"status":"completed","phase":"complete"})).is_none());
    let after=state.store.get("goal","sealing-goal").unwrap();
    assert_eq!(after["status"],"running");
    assert_eq!(after["ownerPlan"]["plan"],"주인의 최신 지시를 우선 수행");
    assert_eq!(after["planRevision"],1);
    // A reviewer may close to review, but cannot claim the unexecuted
    // direct owner correction as completed work.
    let reviewed=state.store.patch_if_fields("goal","sealing-goal",
        &[("status",&json!("running")),("planRevision",&json!(1))],
        &json!({"status":"needs_review"})).unwrap();
    assert_eq!(reviewed["status"],"needs_review");
    assert!(matches!(submit_instruction(&state,"sealing-goal",1,"too late"),
        Err(SteeringError::Immutable)));
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn terminal_goal_cannot_be_reopened_by_stale_owner_revision() {
    let (state,dir)=fixture();
    state.store.put("goal",&json!({"id":"done","status":"running",
        "planRevision":0})).unwrap();
    state.store.patch_if_fields("goal","done",
        &[("status",&json!("running")),("planRevision",&json!(0))],
        &json!({"status":"completed"})).unwrap();
    assert!(matches!(submit_instruction(&state,"done",0,"execute again"),
        Err(SteeringError::Immutable)));
    assert_eq!(state.store.get("goal","done").unwrap()["planRevision"],0);
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn only_durable_goal_sessions_can_yield_for_owner_instructions() {
    let (state,dir)=fixture();
    state.store.put("goal",&json!({
        "id":"goal-z","status":"running","planRevision":0,"appliedPlanRevision":0
    })).unwrap();
    let real_session=json!({"id":"session-z","goalId":"goal-z"});
    let unrelated=json!({"id":"chat","roomId":"room","goalId":null});
    assert!(!pending_for_session(&state,&real_session));
    assert!(!pending_for_session(&state,&unrelated));
    submit_instruction(&state,"goal-z",0,"새로운 사용자 지시").unwrap();
    assert!(pending_for_session(&state,&real_session));
    assert!(!pending_for_session(&state,&unrelated));
    state.store.patch_if_fields("goal","goal-z",
        &[("status",&json!("running")),("planRevision",&json!(1))],
        &json!({"appliedPlanRevision":1})).unwrap();
    assert!(!pending_for_session(&state,&real_session));
    drop(state);std::fs::remove_dir_all(dir).unwrap();
}
