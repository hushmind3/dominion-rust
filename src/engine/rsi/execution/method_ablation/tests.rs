use super::*;
use crate::{config::Config,db::Store,vault::Vault};

fn state()->(AppState,std::path::PathBuf){
    let root=std::env::temp_dir().join(format!(
        "rsi-method-control-test-{}",uuid::Uuid::new_v4()));
    let config=Config{port:0,data_dir:root.join("data"),
        web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(config.clone(),
        Store::open(&config.db_path()).unwrap(),
        Vault::open(&config.key_path()));
    (state,root)
}

#[test]
fn only_real_compiled_method_selection_qualifies_for_control(){
    assert!(!method_selected(&json!({"rsiSystemEvolution":false,
        "rsiImprovementAlgebra":{"selected":{"rule":{"method":{
            "path":"src/engine/memory.rs"}}}}})));
    assert!(!method_selected(&json!({"rsiSystemEvolution":true,
        "rsiImprovementAlgebra":{"selected":{"rule":{"method":{
            "path":"../../etc/passwd"}}}}})));
    assert!(method_selected(&json!({"rsiSystemEvolution":true,
        "rsiImprovementAlgebra":{"selected":{"rule":{"method":{
            "path":"src/engine/memory.rs"}}}}})));
    assert!(!method_selected(&json!({"rsiSystemEvolution":true,
        "rsiImprovementAlgebra":{"selected":{
            "target":{"path":"src/engine/memory.rs"},
            "rule":{"method":{"path":"src/engine/memory.rs"}}}}})),
        "rewriting A2 itself must not masquerade as a B control experiment");
}

#[test]
fn a_stored_control_claim_without_original_worker_never_earns_gain(){
    let (state,root)=state();
    let generation=json!({"jobId":"treatment","accepted":true,
        "methodAblation":{"qualityProven":true,"methodAssociationObserved":true}});
    assert!(measure(&state,&generation).is_null());
    state.store.put("job",&json!({"id":"treatment","kind":"rsi_generation",
        "input":{"rsiMethodAblationControlId":"forged"}})).unwrap();
    assert!(measure(&state,&generation).is_null());
    state.store.put("rsi_method_control",&json!({
        "id":"forged","source":SOURCE,
        "controlJobId":"missing","controlMethodDisabled":true
    })).unwrap();
    assert!(measure(&state,&generation).is_null(),
        "a control row without the real OFF worker is not an intervention");
    let _=std::fs::remove_dir_all(root);
}

#[test]
fn measured_quality_uses_identical_frozen_dimensions(){
    let c=json!({"metrics":[{"key":"quality","direction":"higher",
        "baseline":0.2,"candidate":0.4}]});
    let improved=json!({"metrics":[{"key":"quality","direction":"higher",
        "baseline":0.2,"candidate":0.8}]});
    assert_eq!(quality_delta(&c,&improved),(1,0));
    let unpaired=json!({"metrics":[{"key":"quality","direction":"higher",
        "baseline":0.5,"candidate":0.8}]});
    assert_eq!(quality_delta(&c,&unpaired),(0,0));
    assert_eq!(quality_delta(&improved,&c),(0,1));
}
