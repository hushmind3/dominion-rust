// DOMINION executable RSI regression suite. Test items are grouped by
// responsibility without changing the parent module, import scope, or test
// function names. Includes remain in the SAME directory as the original so
// include_str! fixture paths retain their existing relative locations.
    use super::*;

    include!("evolve-policy-action.rs");
    include!("evolve-autonomous-search.rs");
    include!("evolve-source-targeting.rs");
    include!("evolve-research-budget.rs");
    include!("evolve-paired-adoption.rs");
    include!("evolve-generation-continuity.rs");
    include!("evolve-tool-repair.rs");
    include!("evolve-method-feedback.rs");
    include!("evolve-meta-transfer.rs");
    include!("evolve-method-programs.rs");
    include!("evolve-trial-learning.rs");
    include!("evolve-trial-controls.rs");
