//! Pure contract regressions. The real campaign/worker tests exercise
//! creation of the original job and independent frozen paired receipts.
use super::*;
fn commit()->String{"a".repeat(40)}
fn request()->Value{
    json!({"rsiBaseCommit":commit(),"rsiSystemEvolution":true,
        "rsiAutonomous":true,"rsiMethodAblation":true,
        "rsiMetaOperatorActive":true,
        "rsiFrozenCases":[{"id":"frozen-1"},{"id":"frozen-2"}],
        "rsiEvaluationConfig":{},"rsiMetaCases":[],
        "rsiSelectedDomain":"memory",
        "rsiFocusFiles":["src/engine/memory.rs"],
        "rsiArchitectureTarget":{"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64)}})
}
fn algebra()->Value{
    json!({"selected":{
        "target":{"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64)},
        "rule":{"method":{"role":"improvement_methods",
            "path":"src/engine/rsi_meta_controller.rs",
            "sourceSha256":"d".repeat(64)}}}})
}
fn frontier()->Vec<Value>{
    vec![json!({"role":"experimental_design","domain":"improver",
        "path":"src/engine/rsi/meta/experiment_policy.rs",
        "sourceSha256":"c".repeat(64)})]
}
#[test]
fn source_compiled_meta_policy_is_typed_and_stamps_frozen_evaluation(){
    let decision=synthesize(&[],&frontier(),&algebra(),&request(),&commit());
    let parsed=decode(&decision,&commit(),&request())
        .expect("source-compiled decision must satisfy the stable envelope");
    assert_eq!(parsed.selected_protocol,ExperimentProtocol::MatchedMethodControl);
    assert!(!parsed.frozen.amendment_allowed_in_trial);
    assert_eq!(parsed.choices.len(),2);
    assert!(policy_controls_method(&decision,&commit(),&request()));
}
#[test]
fn original_frozen_profile_cannot_be_changed_after_policy_selection(){
    let decision=synthesize(&[],&frontier(),&algebra(),&request(),&commit());
    let mut changed=request();
    changed["rsiFrozenCases"]=json!([{"id":"different"}]);
    assert!(decode(&decision,&commit(),&changed).is_none());
    let mut forged=decision.clone();
    forged["selectedProtocol"]=json!("paired_source");
    assert!(decode(&forged,&commit(),&request()).is_none());
}
#[test]
fn failed_control_design_reselects_protocol_and_source_of_experiment_designer(){
    let mut observations=Vec::new();
    for id in 0..3 {
        observations.push(json!({
            "id":id,"pairedReceiptReplayed":true,"pairedRevalidated":false,
            "tokens":8000,
            "experimentPolicyExecuted":{
                "source":"running_compiled_descendant_experiment_designer_v1",
                "protocol":"matched_method_control","policySha256":"f".repeat(64)},
            "experimentPolicy":{"selectedProtocol":"matched_method_control",
                "signatureSha256":"f".repeat(64)},
            "methodAblation":{"qualityProven":false}
        }));
    }
    let next=synthesize(&observations,&frontier(),&algebra(),&request(),&commit());
    assert_eq!(next["selectedProtocol"],"paired_source",
        "measured failed method experiments should alter the next experiment design");
    assert_eq!(next["selectedTarget"]["path"],
        "src/engine/rsi/meta/experiment_policy.rs",
        "the experiment-selection mechanism itself is a source edit target");
    assert_eq!(next["redesignTrigger"],"measured_method_control_design_failure");
}
#[test]
fn a_source_revision_cannot_reuse_its_predecessor_policy_envelope(){
    let decision=synthesize(&[],&frontier(),&algebra(),&request(),&commit());
    assert!(decode(&decision,&"e".repeat(40),&request()).is_none());
    let mut no_operator=request();
    no_operator["rsiMetaOperatorActive"]=json!(false);
    let disabled=synthesize(&[],&frontier(),&algebra(),&no_operator,&commit());
    assert_eq!(disabled["selectedProtocol"],"paired_source");
}
