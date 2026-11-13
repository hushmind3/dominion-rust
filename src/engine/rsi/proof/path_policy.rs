//! Immutable evaluation and proof boundary for model-authored RSI source edits.
//!
//! The independent evaluator must not be editable by an in-flight candidate.
//! Other real method implementations remain revisable and measurable via the
//! original frozen paired worktree evaluator. This is not a blanket ban on
//! changes to experimental comparison policies or new evaluation methods.

pub(crate) fn protected(path: &str) -> bool {
    path.starts_with("tests/")
        || path.starts_with("src/engine/rsi/proof/")
        || matches!(path,
            "src/engine/rsi.rs"
            | "src/engine/rsi_oracle_fixture.rs"
            | "src/engine/rsi/evaluation/worktree.rs"
            | "src/engine/rsi/evaluation/metrics.rs"
            | "src/bin/dominion-benchmark.rs"
        )
}

#[cfg(test)]
#[path = "../../../../tests/unit/engine_rsi_path_policy_tests.rs"]
mod tests;
