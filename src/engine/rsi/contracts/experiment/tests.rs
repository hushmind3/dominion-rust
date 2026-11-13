use super::*;
#[test]
fn immutable_contract_rejects_unknown_protocol_and_unbound_target(){
    let target=json!({"role":"experiment_design","domain":"improver",
        "path":"src/engine/rsi/experiment_policy.rs","sourceSha256":"b".repeat(64)});
    let design=json!({"schema":"compiled_evolvable_experiment_v1",
        "generatorCommit":"a".repeat(40),"historyCount":0,
        "policySourceSha256":"c".repeat(64),"policyRevision":"d".repeat(64),
        "target":target,"protocol":"invented_protocol","question":"test",
        "parentExperimentRevision":null});
    assert!(serde_json::from_value::<ExperimentDesign>(design).is_err());
}
