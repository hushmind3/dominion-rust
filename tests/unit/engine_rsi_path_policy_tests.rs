use super::*;

#[test]
fn frozen_source_cannot_be_changed_or_indexed_as_editable() {
    for path in [
        "src/engine/rsi.rs",
        "src/engine/rsi_oracle_fixture.rs",
        "src/engine/rsi/evaluation/worktree.rs",
        "src/engine/rsi/evaluation/metrics.rs",
        "src/engine/rsi/proof/closure.rs",
        "src/engine/rsi/proof/source_roles.rs",
        "src/engine/rsi/proof/path_policy.rs",
        "src/bin/dominion-benchmark.rs",
        "tests/e2e.rs",
        "tests/unit/engine_rsi_evolve_tests.rs",
    ] {
        assert!(protected(path), "verifier was editable: {path}");
        assert!(!crate::engine::rsi::safe_path(path));
    }
}

#[test]
fn legitimate_method_evolution_and_application_source_stay_editable() {
    for path in [
        "src/engine/rsi_evolve.rs",
        "src/engine/rsi/execution/worker.rs",
        "src/engine/rsi/learning/methods.rs",
        "src/engine/rsi/lineage/adoption.rs",
        "src/engine/rsi/evaluation/intervention_lab.rs",
        "src/engine/rsi/evaluation/rule_tournament.rs",
        "src/engine/memory.rs",
        "web/src/pages/Chat.tsx",
    ] {
        assert!(!protected(path), "valid method frozen: {path}");
        assert!(crate::engine::rsi::safe_path(path));
    }
}
