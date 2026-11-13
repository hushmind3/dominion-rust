use super::*;
use crate::{config::Config,db::Store,vault::Vault};

#[test]
fn treatment_control_fingerprint_changes_only_the_method_intervention(){
    let base=json!({"rsiGeneratorCommit":"a".repeat(40),
        "rsiTargetDomain":"memory","providerId":"model-provider",
        "model":"fixed-model","prompt":"read exact source, edit memory"});
    let mut off=base.clone();
    off["rsiMethodAblationArm"]=json!("without_methods");
    let mut on=base.clone();
    on["rsiMethodAblationArm"]=json!("with_methods");
    on["rsiMethodAblationControlId"]=json!("rsi-method-ablation-saved");
    assert_eq!(fingerprint(&off),fingerprint(&on));
    on["prompt"]=json!("another source task");
    assert_ne!(fingerprint(&off),fingerprint(&on));
}

#[test]
fn treatment_cannot_claim_improvement_without_original_control_receipt(){
    let root=std::env::temp_dir().join(format!("dominion-method-ablation-{}",uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
    let state=AppState::new(config.clone(),
        Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
    assert!(measured_method_ablation(&state,&json!({
        "accepted":true,"jobId":"nonexistent-treatment"
    })).is_null());
    std::fs::remove_dir_all(root).unwrap();
}
