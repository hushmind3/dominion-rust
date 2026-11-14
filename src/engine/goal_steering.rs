//! Owner commands are durable and take effect at a safe executor turn boundary.
//! Model-generated plans never replace a later authenticated owner instruction.
//! Tool execution rights and acceptance evidence are kept separate from intent.
use crate::state::AppState;
use serde_json::{json,Value};

#[derive(Debug)]
pub enum SteeringError { Missing, Immutable, Conflict, Invalid }

#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum SteeringKind { Plan, Instruction }

pub fn submit(state:&AppState,id:&str,expected:u64,plan:&str)->Result<Value,SteeringError> {
    submit_with_kind(state,id,expected,plan,SteeringKind::Plan)
}

/// A direct user instruction is not a model plan. It is preserved verbatim
/// and presented as a higher-priority owner revision on the next executor turn.
pub fn submit_instruction(state:&AppState,id:&str,expected:u64,instruction:&str)
    ->Result<Value,SteeringError>{
    submit_with_kind(state,id,expected,instruction,SteeringKind::Instruction)
}

fn submit_with_kind(state:&AppState,id:&str,expected:u64,content:&str,kind:SteeringKind)
    ->Result<Value,SteeringError>{
    let content=content.trim();
    if content.is_empty()||content.chars().count()>6000||content.contains('\0'){
        return Err(SteeringError::Invalid);
    }
    let goal=state.store.get("goal",id).ok_or(SteeringError::Missing)?;
    if !matches!(goal["status"].as_str(),Some("queued"|"running")){
        return Err(SteeringError::Immutable);
    }
    let revision=goal["planRevision"].as_u64().unwrap_or(0);
    if revision!=expected{return Err(SteeringError::Conflict);}
    let next=revision.checked_add(1).ok_or(SteeringError::Invalid)?;
    let kind_name=match kind {SteeringKind::Plan=>"plan",SteeringKind::Instruction=>"instruction"};
    let saved=state.store.patch_if("goal",id,"planRevision",
        goal.get("planRevision").unwrap_or(&Value::Null),
        &json!({"planRevision":next,"ownerPlan":{"revision":next,"plan":content,
            "kind":kind_name,"submittedAt":chrono::Utc::now().to_rfc3339()}}))
        .ok_or(SteeringError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_revision",json!({"goalId":id,"revision":next,"kind":kind_name}));
    Ok(json!({"id":id,"planRevision":next,"kind":kind_name,"status":"pending_application"}))
}

pub fn pending(state:&AppState,id:&str,applied:u64)->Option<(u64,String)> {
    pending_command(state,id,applied).map(|(revision,text,_)|(revision,text))
}

pub fn pending_command(state:&AppState,id:&str,applied:u64)
    ->Option<(u64,String,SteeringKind)> {
    let row=state.store.get("goal",id)?;
    let candidate=&row["ownerPlan"];
    let revision=candidate["revision"].as_u64()?;
    let text=candidate["plan"].as_str()?.trim();
    let kind=if candidate["kind"]=="instruction" {
        SteeringKind::Instruction
    } else {
        SteeringKind::Plan
    };
    (revision>applied&&!text.is_empty()).then(||(revision,text.to_string(),kind))
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
