use super::*;
#[test]
fn policy_observation_uses_real_outcome_instead_of_stored_claim(){
    let p="src/engine/rsi/experiment_policy.rs";
    let row=json!({"path":p,"role":"experiment_design","domain":"improver",
        "sourceBytes":3000});
    let failed=json!({"focusFiles":[p],"accepted":false,
        "evaluation":{"metrics":{"regressions":1}}});
    let adopted=json!({"experimentDesign":{"target":{"path":p}},
        "experimentOutcome":{"verifiedQualityGain":true},
        "accepted":true,"pairedRevalidated":true,
        "evaluation":{"metrics":{"eligible":true}}});
    assert!(score(&row,&[adopted],&Value::Null)
        >score(&row,&[failed],&Value::Null));
}
