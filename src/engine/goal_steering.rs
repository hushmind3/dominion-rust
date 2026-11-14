//! A concurrent owner plan revision is applied only at a safe turn boundary.
//! The owner cannot edit acceptance checks, tool approvals or root identity.
use crate::state::AppState;
use serde_json::{json,Value};
#[derive(Debug)]
pub enum SteeringError { Missing, Immutable, Conflict, Invalid }
pub fn submit(state:&AppState,id:&str,expected:u64,plan:&str)->Result<Value,SteeringError> {
    let plan=plan.trim();
    if plan.is_empty()||plan.chars().count()>6000||plan.contains('\0'){
        return Err(SteeringError::Invalid);
    }
    let goal=state.store.get("goal",id).ok_or(SteeringError::Missing)?;
    if !matches!(goal["status"].as_str(),Some("queued"|"running")){
        return Err(SteeringError::Immutable);
    }
    let revision=goal["planRevision"].as_u64().unwrap_or(0);
    if revision!=expected{return Err(SteeringError::Conflict);}
    let next=revision.checked_add(1).ok_or(SteeringError::Invalid)?;
    let saved=state.store.patch_if("goal",id,"planRevision",
        goal.get("planRevision").unwrap_or(&Value::Null),
        &json!({"planRevision":next,"ownerPlan":{"revision":next,"plan":plan,
            "submittedAt":chrono::Utc::now().to_rfc3339()}}))
        .ok_or(SteeringError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_plan_revision",json!({"goalId":id,"revision":next}));
    Ok(json!({"id":id,"planRevision":next,"status":"pending_application"}))
}
pub fn pending(state:&AppState,id:&str,applied:u64)->Option<(u64,String)> {
    let row=state.store.get("goal",id)?;
    let candidate=&row["ownerPlan"];
    let revision=candidate["revision"].as_u64()?;
    let text=candidate["plan"].as_str()?.trim();
    (revision>applied&&!text.is_empty()).then(||(revision,text.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn input_is_bounded(){
        assert!(String::from("a").chars().count()<=6000);
        assert!(matches!(SteeringError::Conflict,SteeringError::Conflict));
    }
}

#[cfg(test)]
#[path = "../../tests/unit/goal_steering_tests.rs"]
mod integration_tests;
