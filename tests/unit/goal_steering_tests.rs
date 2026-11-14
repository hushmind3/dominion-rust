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
