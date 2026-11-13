use super::*;
#[test]
fn input_identity_changes_for_model_cases_and_method_not_control_markers() {
    let mut base=json!({
        "rsiGeneratorCommit":"a".repeat(40),
        "prompt":"same authored task",
        "rsiTargetDomain":"context",
        "providerId":"cloud-one","model":"fixed-model",
        "rsiFrozenCases":[{"id":"task-1"}],
        "rsiMethodSnapshot":{"methodSourceSha256":"b".repeat(64)},
        "rsiNativePlan":{"systemAgenda":{"target":{"path":"src/engine/chat.rs"}}}
    });
    let initial=input_fingerprint(&base);
    base["rsiMethodAblationArm"]=json!("without_methods");
    base["rsiMethodAblationControlId"]=json!("control-1");
    assert_eq!(initial,input_fingerprint(&base));
    base["model"]=json!("other-model");
    assert_ne!(initial,input_fingerprint(&base));
    base["model"]=json!("fixed-model");
    base["rsiFrozenCases"]=json!([{"id":"task-2"}]);
    assert_ne!(initial,input_fingerprint(&base));
}
