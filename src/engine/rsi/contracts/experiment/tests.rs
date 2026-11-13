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

#[test]
fn content_identity_changes_when_target_or_intervention_changes(){
    let mut plan=ExperimentDesign{
        schema:"compiled_evolvable_experiment_v1".to_string(),
        generator_commit:"a".repeat(40),history_count:1,
        policy_source_sha256:"b".repeat(64),policy_revision:String::new(),
        target:ExperimentTarget{role:"experiment_design".into(),
            domain:"improver".into(),
            path:"src/engine/rsi/experiment_policy.rs".into(),
            source_sha256:"c".repeat(64)},
        protocol:ExperimentProtocol::FrozenSourcePair,
        question:"test adaptive experiment".into(),
        parent_experiment_revision:None
    };
    plan.seal();
    assert_eq!(plan.policy_revision,plan.revision());
    let stable=plan.policy_revision.clone();
    plan.target.source_sha256="d".repeat(64);
    assert_ne!(stable,plan.revision());
    plan.seal();
    assert_eq!(plan.policy_revision,plan.revision());
}
