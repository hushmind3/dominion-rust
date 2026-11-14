//! Owner-directed **objective** revision for autonomous goals.
//!
//! Unlike plan steering, this changes the *goal itself*. A pending revision is
//! staged durably and installed exactly at a new executor turn boundary.
//! Frozen acceptance baselines are re-established for that revision, and old
//! tool receipts are archived as history, NOT accepted as proof of the new job.
//! No model-generated text is authorized to submit an owner correction.
use crate::{
    engine::verification::{baseline_checks_for_workspace, infer_criteria, validate_criteria},
    state::AppState,
};
use serde_json::{json,Value};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum RevisionError { Missing, Invalid, Immutable, Conflict }

#[derive(Debug)]
pub struct AppliedObjective {
    pub revision: u64,
    pub objective: String,
    pub acceptance: Value,
}

/// Stage exact owner text and new acceptance requirements. An empty criteria
/// array requests the native deterministic inference on the *new* objective.
pub fn submit(
    state:&AppState, id:&str, expected:u64, objective:&str, criteria:&Value,
)->Result<Value,RevisionError>{
    if objective.trim().is_empty() || objective.len()>64_000
        || objective.contains('\0') || validate_criteria(criteria).is_err()
    {return Err(RevisionError::Invalid);}
    let goal=state.store.get("goal",id).ok_or(RevisionError::Missing)?;
    if goal["status"]!="running"{return Err(RevisionError::Immutable);}
    let current=goal["ownerObjectiveRevision"].as_u64().unwrap_or(0);
    if current!=expected{return Err(RevisionError::Conflict);}
    let revision=current.checked_add(1).ok_or(RevisionError::Invalid)?;
    let payload=json!({
        "revision":revision, "goal":objective,
        "successCriteria":criteria, "source":"owner",
        "submittedAt":chrono::Utc::now().to_rfc3339(),
    });
    let expected=json!({
        "status":goal["status"],
        "ownerObjectiveRevision":goal["ownerObjectiveRevision"],
    });
    let saved=state.store.patch_if_fields(
        "goal",id,&expected,&json!({
            "ownerObjectiveRevision":revision,
            "ownerObjective":payload,
        }),
    ).ok_or(RevisionError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_objective_queued",json!({
        "goalId":id, "revision":revision,
        "bytes":objective.len(), "criteria":criteria.as_array().map(Vec::len),
    }));
    Ok(json!({"id":id,"revision":revision,"status":"pending_turn_boundary"}))
}

/// True when an authenticated owner revision has not yet been applied.
/// The goal cannot finalize old work as "completed" while this is true.
pub fn pending(state:&AppState,id:&str,applied:u64)->bool{
    state.store.get("goal",id).is_some_and(|goal|
        goal["status"]=="running"
        && goal["ownerObjective"]["revision"].as_u64().is_some_and(|rev|rev>applied)
        && goal["ownerObjectiveRevision"]==goal["ownerObjective"]["revision"])
}

/// Snapshot the CURRENT workspace before new actions, archive old evidence
/// provenance and replace the objective + acceptance contract atomically.
pub fn adopt(
    state:&AppState,id:&str,applied:u64,workspace:&Path,iteration:usize,
)->Result<Option<AppliedObjective>,String>{
    let Some(goal)=state.store.get("goal",id) else{return Err("목표를 찾을 수 없습니다.".into());};
    if goal["status"]!="running"{return Ok(None);}
    let owner=&goal["ownerObjective"];
    let Some(revision)=owner["revision"].as_u64() else{return Ok(None)};
    if revision<=applied || goal["appliedOwnerObjectiveRevision"].as_u64().unwrap_or(0)>=revision
        || goal["ownerObjectiveRevision"]!=owner["revision"]
    {return Ok(None);}
    let Some(objective)=owner["goal"].as_str() else{return Err("새 목표의 원문이 유실됐습니다.".into())};
    let criteria=owner["successCriteria"].clone();
    validate_criteria(&criteria)?;
    let checks=criteria.as_array().cloned().unwrap_or_default();
    let checks=if checks.is_empty(){infer_criteria(objective)}else{checks};
    let acceptance=baseline_checks_for_workspace(state,workspace,checks);

    let mut history=goal["objectiveHistory"].as_array().cloned().unwrap_or_default();
    history.push(json!({
        "revision":revision,
        "previousGoal":goal["goal"],
        "goal":objective,
        "previousTaskId":goal["taskId"],
        "priorEvidenceCount":goal["evidence"].as_array().map(Vec::len).unwrap_or(0),
        "previousAcceptance":goal["acceptanceChecks"],
        "acceptedAtIteration":iteration,
        "at":chrono::Utc::now().to_rfc3339(),
    }));
    if history.len()>20{history.drain(..history.len()-20);}
    let old=json!({
        "status":goal["status"],"ownerObjective":goal["ownerObjective"],
        "appliedOwnerObjectiveRevision":goal["appliedOwnerObjectiveRevision"],
    });
    let updated=state.store.patch_if_fields("goal",id,&old,&json!({
        "goal":objective,
        "successCriteria":criteria,
        "acceptanceChecks":acceptance,
        "appliedOwnerObjectiveRevision":revision,
        "objectiveHistory":history,
        "evidence":[],
        "gaps":[],
        "stalled":0,
        "plan":"",
        "ownerPlan":null,
        "appliedOwnerPlanRevision":goal["planRevision"],
        "phase":"plan",
        "executionReceipt":null,
        "deliverables":[],
        "finalSummary":null,
    }));
    let Some(saved)=updated else{
        // A newer revision or cancellation won the CAS, so do NOT execute
        // stale instructions or consume the pending correction.
        return Ok(None);
    };
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_objective_applied",json!({
        "goalId":id,"revision":revision,"atIteration":iteration,
        "acceptanceChecks":acceptance.as_array().map(Vec::len).unwrap_or(0),
        "oldEvidenceInvalidated":true,
    }));
    Ok(Some(AppliedObjective{revision,objective:objective.to_string(),acceptance}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    fn fixture()->(AppState,std::path::PathBuf){
        let dir=std::env::temp_dir().join(format!("dominion-objective-revision-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        state.store.put("goal",&json!({
            "id":"g","status":"running","taskId":"root-old",
            "goal":"edit old.txt","planRevision":3,
            "ownerPlan":{"revision":3,"plan":"stale user plan"},
            "plan":"old plan","acceptanceChecks":[{"type":"file_exists","path":"old.txt"}],
            "evidence":[{"tool":"file_write","result":{"written":true}}],
        })).unwrap();
        (state,dir)
    }

    #[test]
    fn owner_changes_goal_and_verifier_without_laundering_old_actions(){
        let (state,dir)=fixture();
        std::fs::write(dir.join("new.txt"),"before").unwrap();
        let original="  새 파일 new.txt를 수정하세요.\n기존 목표는 중단합니다.  ";
        assert_eq!(submit(&state,"g",0,original,&json!([])).unwrap()["revision"],1);
        assert!(pending(&state,"g",0));
        assert_eq!(submit(&state,"g",0,"stale",&json!([])),Err(RevisionError::Conflict));
        let adoption=adopt(&state,"g",0,&dir,2).unwrap().unwrap();
        assert_eq!(adoption.objective,original);
        assert_eq!(adoption.revision,1);
        assert!(adoption.acceptance.as_array().is_some());
        let goal=state.store.get("goal","g").unwrap();
        assert_eq!(goal["goal"],original);
        assert_eq!(goal["evidence"],json!([]));
        assert_eq!(goal["plan"],"");
        assert!(goal["ownerPlan"].is_null());
        assert_eq!(goal["appliedOwnerPlanRevision"],3);
        assert_eq!(goal["objectiveHistory"][0]["previousGoal"],"edit old.txt");
        assert_eq!(goal["objectiveHistory"][0]["priorEvidenceCount"],1);
        assert_eq!(goal["acceptanceChecks"],adoption.acceptance);
        assert!(!pending(&state,"g",1));
        assert!(adopt(&state,"g",1,&dir,3).unwrap().is_none());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn owner_revision_is_cancel_aware_and_never_replaces_another_revision(){
        let (state,dir)=fixture();
        assert_eq!(submit(&state,"g",0," ",&json!([])),Err(RevisionError::Invalid));
        assert_eq!(submit(&state,"missing",0,"new",&json!([])),Err(RevisionError::Missing));
        assert_eq!(submit(&state,"g",0,"new",&json!([{"type":"imaginary"}])),Err(RevisionError::Invalid));
        submit(&state,"g",0,"first",&json!([])).unwrap();
        submit(&state,"g",1,"second",&json!([])).unwrap();
        let adopted=adopt(&state,"g",0,&dir,1).unwrap().unwrap();
        assert_eq!(adopted.objective,"second");
        state.store.patch_if("goal","g","status",&json!("running"),&json!({"status":"cancelled"})).unwrap();
        assert_eq!(submit(&state,"g",2,"third",&json!([])),Err(RevisionError::Immutable));
        assert!(adopt(&state,"g",2,&dir,3).unwrap().is_none());
        drop(state);let _=std::fs::remove_dir_all(dir);
    }
}
