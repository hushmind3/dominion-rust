//! Opt-in experimental evolution. The coordinator and its oracle stay frozen;
//! each accepted source revision is rebuilt and actually generates the next patch.
//! Only private detached worktrees are committed. Production refs never move.
use crate::{
    engine::{
        experience,
        jobs::{self, JobContext},
        memory::MemoryScope,
        rsi, rsi_worktree, tools,
    },
    state::AppState,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::process::Command;

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "ui", "improver"];
// Responsibility boundaries. The top-level coordinator retains its existing
// Rust API; each subsystem owns the same implementation, not a copy.
#[path = "rsi/context.rs"]
mod context;
#[path = "rsi/research.rs"]
mod research;
#[path = "rsi/lineage.rs"]
mod lineage;
#[path = "rsi/worker.rs"]
mod worker;
#[path = "rsi/search.rs"]
mod search;
#[path = "rsi/source.rs"]
mod source;
#[path = "rsi/campaign.rs"]
mod campaign;
#[path = "rsi/learning.rs"]
mod learning;
#[path = "rsi/evaluation.rs"]
mod evaluation;
#[path = "rsi/contract.rs"]
pub(crate) mod contract;
#[path = "rsi/method_control.rs"]
mod method_control;
use method_control::run_compiled_method_control;
#[path = "rsi/execution.rs"]
mod execution;
#[path = "rsi/planner.rs"]
mod planner;
// Closure is owned by the ACTUAL native evolution coordinator, not by a
// parallel candidate-ledger scoring path.
#[path = "rsi/closure.rs"]
mod recursive_closure;
pub(crate) fn recursive_closure_evidence(
    state:&AppState,history:&[Value]
)->Value {
    recursive_closure::evidence(state,history)
}

use search::{
    objective_matches, search_features, score_domain, eligible_domains, valid_domain_scores,
    search_strategy, select_domain_for_state,
};
use source::{
    git, save, elapsed, lineage_fingerprint, snapshot_source,
};
use campaign::{
    select, verified_candidate_id, matching_prior_campaign, allowed_trial_campaigns, trial_owned_by,
};
use context::{
    research_digest, focus_excerpt_has_source, bind_action_to_source, choose_focus_files, append_bounded_excerpt,
    focus_file_context, grounded_next_experiment, generation_context, research_scope, focus_domain_hint,
    selection_objective,
};
use research::{
    research_artifacts, session_research_fingerprint, generation_stop_reason, generation_fits_remaining_budget, should_stop_no_progress,
    new_research_findings, session_tool_usage, previous_autonomous_failures,
};
use lineage::{
    retry_learned_model_failure, generation_model_error,
};
use learning::{
    adopted_structural_descendant, method_selection_feedback, method_observed_selection_score, method_reuse_decision, is_synthesized_source_repair,
    synthesize_stale_edit_repair, synthesize_indexed_edit_repair, synthesize_existing_file_write_repair, synthesize_indexed_path_repair, synthesize_successful_dag_ablation,
    synthesize_observed_dag_pruning, synthesize_failed_model_edit, synthesize_failed_reversed_span, record_method_feedback,
};
use evaluation::{
    proposed_algorithms, queue_search_trial, autonomous_search_hypothesis, propose_autonomous_search_trial, pending_trial_candidate,
    comparable_trial_baseline, trial_verdict, record_trial_outcome,
};
use execution::{
    run_loop,
};
use planner::{
    actual_edit_observation, observed_edit_outcome, action_learning_context, action_choices, observed_action_value,
    evolved_execution_action, next_execution_action, execution_action_contract, research_continuation, generation_target_plan,
    pending_failed_patch_domain, pending_method_recovery_domain, select_generation_strategy, reconcile_descendant_target, sub2_edit_policy_reward,
    evidence_based_meta_repair_focus, compiled_system_generation_plan,
};

// Stable public facade: callers keep crate::engine::rsi_evolve::* paths.
pub use search::{limits,evaluate_search_algorithm};
pub(crate) use search::select_domain;
pub use source::{
    auto_write_allowed,auto_write_denial_reason,campaign_source,
    remaining_output_tokens,lineage_fingerprint_valid,
};
pub(crate) use source::command;
pub use campaign::{choose,run};
pub(crate) use context::policy_requests_source_edit;
pub(crate) use research::{remember_research,reuse_research};
pub(crate) use lineage::adopt;
pub use worker::{plan_generation,plan_counterfactual,worker};

// Preserve external call sites in tools, policy and descendant workers.
pub(crate) use learning::{
    propose_method, configure_method_session,
    method_strategies, collect_method_executions,
};


#[cfg(test)]
#[path = "rsi_oracle_fixture.rs"]
mod oracle_tests;

#[cfg(test)]
#[path = "rsi/tests/evolve.rs"]
mod tests;

#[cfg(test)]
#[path = "rsi/tests/inheritance.rs"]
mod sub2_inherited_main_capabilities_tests;
