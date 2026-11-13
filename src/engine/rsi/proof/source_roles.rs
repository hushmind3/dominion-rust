// Code that changes how the next descendant plans, edits, learns, executes,
// or transfers its improvement method is an improver source. The frozen
// comparator and proof verifier are deliberately NOT improvement targets.
pub(super) fn method_source(path: &str) -> bool {
    matches!(path,
        "src/engine/rsi_evolve.rs"
        | "src/engine/rsi_meta_controller.rs"
        | "src/engine/rsi_prequential.rs"
        | "src/engine/rsi_recursive_rewrite.rs"
        | "src/engine/rsi_principle_learning.rs"
        | "src/engine/rsi_system.rs"
        | "src/engine/algorithms.rs"
    ) || [
        "src/engine/rsi/planner/",
        "src/engine/rsi/learning/",
        "src/engine/rsi/meta/",
        "src/engine/rsi/execution/",
        "src/engine/rsi/lineage/",
    ].iter().any(|prefix| path.starts_with(prefix) && path.ends_with(".rs"))
}

