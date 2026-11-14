//! Owner revisions are staged and applied at safe goal-turn boundaries.
//! No mutation of acceptance criteria, tool permissions, task identity or in-flight tool args.
use crate::state::AppState;
use serde_json::{json,Value};

#[derive(Debug,PartialEq,Eq)]
pub enum SteeringError{Missing,Invalid,Immutable,Conflict}
pub fn submit(state:&AppState,id:&str,expected:u64,plan:&str)->Result<Value,SteeringError>{
    let plan=plan.trim();
    if plan.is_empty()||plan.chars().count()>6000||plan.contains('\0'){
        return Err(SteeringError::Invalid);
    }
    let goal=state.store.get("goal",id).ok_or(SteeringError::Missing)?;
    if goal["status"]!="running"{return Err(SteeringError::Immutable);}
    let prior=goal["planRevision"].as_u64().unwrap_or(0);
    if prior!=expected{return Err(SteeringError::Conflict);}
    let revision=prior.checked_add(1).ok_or(SteeringError::Invalid)?;
    // Compare BOTH status and revision under the same SQLite lock.
    // A terminal transition racing this API cannot accept a phantom change.
    let old=json!({"status":goal["status"],"planRevision":goal["planRevision"]});
    let patch=json!({"planRevision":revision,
        "ownerPlan":{"revision":revision,"plan":plan,
            "submittedAt":chrono::Utc::now().to_rfc3339()}});
    let saved=state.store.patch_if_fields("goal",id,&old,&patch)
        .ok_or(SteeringError::Conflict)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_steering_requested",
        json!({"goalId":id,"revision":revision,"characters":plan.chars().count()}));
    Ok(json!({"id":id,"revision":revision,"status":"pending_turn_boundary"}))
}

pub fn pending(state:&AppState,id:&str,applied:u64)->Option<(u64,String)>{
    let goal=state.store.get("goal",id)?;
    if goal["status"]!="running"{return None;}
    let owner=&goal["ownerPlan"];
    let revision=owner["revision"].as_u64()?;
    let plan=owner["plan"].as_str()?.trim();
    (revision>applied&&!plan.is_empty()).then(||(revision,plan.to_string()))
}

/// Install the latest owner revision strictly at an executor turn boundary.
/// Each decision is an optimistic transaction; a newer owner request, a
/// terminal goal or a previously adopted revision invalidates the candidate.
pub fn apply_at_boundary(state:&AppState,id:&str,applied:u64,
    history:&[Value],objective:&str,next_iteration:usize)
    ->Option<(u64,String,Vec<Value>)>
{
    let (revision,plan)=pending(state,id,applied)?;
    let goal=state.store.get("goal",id)?;
    if goal["status"]!="running"
        || goal["ownerPlan"]["revision"].as_u64()!=Some(revision)
        || goal["appliedOwnerPlanRevision"].as_u64().unwrap_or(0)>=revision
    {return None;}
    let version=history.iter().filter_map(|row|row["version"].as_u64())
        .max().unwrap_or(0).saturating_add(1);
    let mut next_history=history.to_vec();
    next_history.push(json!({"version":version,"plan":plan,
        "reason":"owner_steering","ownerRevision":revision,
        "at":chrono::Utc::now().to_rfc3339()}));
    if next_history.len()>40{next_history.drain(..next_history.len()-40);}
    let old=json!({"status":goal["status"],"ownerPlan":goal["ownerPlan"],
        "appliedOwnerPlanRevision":goal["appliedOwnerPlanRevision"]});
    let patch=json!({"plan":plan,"planHistory":next_history,
        "appliedOwnerPlanRevision":revision,
        "planScore":crate::engine::plan_quality::score(&plan,objective)});
    let saved=state.store.patch_if_fields("goal",id,&old,&patch)?;
    state.bus.record("goal",&saved);
    state.store.audit("goal.owner_steering_applied",
        json!({"goalId":id,"revision":revision,"beforeIteration":next_iteration}));
    Some((revision,plan,next_history))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn running_goal_stages_only_plan_and_rejects_stale_revisions(){
        let dir=std::env::temp_dir().join(format!("goal-steer-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("goal",&json!({"id":"g1","status":"running",
            "plan":"old","acceptanceChecks":[{"type":"file_exists","path":"a.txt"}],
            "executor":{"agentId":"a1"},"planRevision":0})).unwrap();
        assert_eq!(submit(&state,"g1",0,"improved safe plan").unwrap()["revision"],1);
        assert_eq!(pending(&state,"g1",0),Some((1,"improved safe plan".into())));
        assert_eq!(pending(&state,"g1",1),None);
        assert_eq!(submit(&state,"g1",0,"stale"),Err(SteeringError::Conflict));
        assert_eq!(submit(&state,"g1",1," "),Err(SteeringError::Invalid));
        let row=state.store.get("goal","g1").unwrap();
        assert_eq!(row["plan"],"old");
        assert_eq!(row["acceptanceChecks"][0]["path"],"a.txt");
        assert_eq!(row["executor"]["agentId"],"a1");
        let (rev,applied_plan,history)=apply_at_boundary(
            &state,"g1",0,&[],"Build a reliable app",2).unwrap();
        assert_eq!(rev,1);
        assert_eq!(applied_plan,"improved safe plan");
        assert_eq!(history[0]["reason"],"owner_steering");
        assert_eq!(state.store.get("goal","g1").unwrap()["plan"],"improved safe plan");
        assert_eq!(state.store.get("goal","g1").unwrap()["appliedOwnerPlanRevision"],1);
        assert!(apply_at_boundary(&state,"g1",1,&history,"Build a reliable app",3).is_none());
        assert_eq!(state.store.get("goal","g1").unwrap()["acceptanceChecks"][0]["path"],"a.txt");
        state.store.patch_if("goal","g1","status",&json!("running"),&json!({"status":"completed"})).unwrap();
        assert_eq!(submit(&state,"g1",1,"late"),Err(SteeringError::Immutable));
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
}
