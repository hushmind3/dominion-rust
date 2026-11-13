use super::*;

fn source_row() -> Value {
    json!({"path":"src/engine/rsi/experiment_policy.rs",
        "role":"experiment_design","domain":"improver","sourceBytes":3000})
}

fn generation(verified:bool)->Value {
    json!({
        "focusFiles":["src/engine/rsi/experiment_policy.rs"],
        "experimentDesign":{"policyRevision":"a".repeat(64),
            "target":{"path":"src/engine/rsi/experiment_policy.rs"}},
        "experimentOutcome":{"sourceEditObserved":true,
            "actualWorkerExecuted":true,"verifiedQualityGain":verified,
            "policyRevision":"a".repeat(64)},
        "accepted":true,"pairedRevalidated":true,
        "evaluation":{"verdict":"improved","metrics":{
            "eligible":true,"regressions":0}}
    })
}

#[test]
fn independent_adoption_and_executed_policy_revision_increase_score() {
    let row=source_row();
    let verified=generation(true);
    let unproven=generation(false);
    assert!(score(&row,&[verified],&Value::Null)
        >score(&row,&[unproven],&Value::Null));
}

#[test]
fn model_claim_without_replayed_receipt_does_not_earn_gain() {
    let row=source_row();
    let mut forged=generation(true);
    forged["pairedRevalidated"]=json!(false);
    let rejected=generation(false);
    assert_eq!(score(&row,&[forged],&Value::Null),
        score(&row,&[rejected],&Value::Null));
}

#[test]
fn mismatched_revision_does_not_earn_policy_credit() {
    let row=source_row();
    let mut misattributed=generation(true);
    misattributed["experimentOutcome"]["policyRevision"]=json!("b".repeat(64));
    let rejected=generation(false);
    assert_eq!(score(&row,&[misattributed],&Value::Null),
        score(&row,&[rejected],&Value::Null));
}

#[test]
fn rejected_outcome_does_not_become_parent_experiment_revision() {
    let mut failed=generation(true);
    failed["accepted"]=json!(false);
    assert!(parent_revision(&[failed]).is_none());
    assert_eq!(parent_revision(&[generation(true)]),Some("a".repeat(64)));
}
