use super::*;

fn measured(paths: &[&str]) -> Value {
    json!({
        "target":"improver",
        "paths":paths,
        "evaluation":{"metrics":{
            "metaHoldoutGains":1,
            "improvedHoldoutDomains":["improver"],
            "measurements":[{"domain":"improver","holdout":true,
                "baselinePass":false,"candidatePass":true}]
        }}
    })
}

#[test]
fn compiled_method_execution_and_lineage_are_improver_sources() {
    for path in [
        "src/engine/rsi/planner/domain.rs",
        "src/engine/rsi/execution/worker.rs",
        "src/engine/rsi/execution/campaign.rs",
        "src/engine/rsi/lineage/adoption.rs",
        "src/engine/rsi/learning/methods.rs",
        "src/engine/rsi/meta/decision_rules.rs",
    ] {
        assert!(method_source(path), "missing executable method: {path}");
        assert!(method_gain(&measured(&[path])));
    }
}

#[test]
fn frozen_evaluator_proof_and_other_capabilities_are_not_method_credit() {
    for path in [
        "src/engine/rsi/evaluation/worktree.rs",
        "src/engine/rsi/evaluation/metrics.rs",
        "src/engine/rsi/proof/closure.rs",
        "src/engine/memory.rs",
        "src/engine/chat/runtime/turn.rs",
        "tests/e2e.rs",
    ] {
        assert!(!method_source(path), "must not treat as an improver: {path}");
        assert!(!method_gain(&measured(&[path])));
    }
}

#[test]
fn mixed_method_and_capability_patches_cannot_complete_method_only_step() {
    assert!(!method_gain(&measured(&[
        "src/engine/rsi/execution/worker.rs", "src/engine/memory.rs"
    ])));
    assert!(!method_gain(&measured(&[])));
    assert!(method_gain(&measured(&[
        "src/engine/rsi/planner/domain.rs",
        "src/engine/rsi/learning/methods.rs"
    ])));
}

#[test]
fn frozen_improver_transition_is_required_even_for_a_method_only_patch() {
    let mut candidate = measured(&["src/engine/rsi/execution/worker.rs"]);
    candidate["evaluation"]["metrics"]["measurements"][0]["baselinePass"]=json!(true);
    assert!(!method_gain(&candidate));
    candidate["evaluation"]["metrics"]["measurements"][0]["baselinePass"]=json!(false);
    candidate["evaluation"]["metrics"]["metaHoldoutGains"]=json!(0);
    assert!(!method_gain(&candidate));
}
