//! Opt-in experimental evolution. The coordinator and its oracle stay frozen;
//! each accepted source revision is rebuilt and actually generates the next patch.
//! Only private detached worktrees are committed. Production refs never move.
use crate::{
    engine::{
        experience,
        jobs::{self, JobContext},
        memory::MemoryScope,
        rsi_worktree, tools,
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

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "models", "ui", "improver"];


#[path = "rsi/planner/search.rs"]
mod planner_search;
use planner_search::{search_features, score_domain, search_strategy};
pub(crate) use planner_search::{objective_matches, candidate_domains, scored_domains, select_domain};
pub use planner_search::{limits};
#[path = "rsi/learning/edit_policy.rs"]
mod learning_edit_policy;
use learning_edit_policy::{executed_policy_rule, edit_method_reward, synthesize_edit_method, cross_domain_stage_donor, learned_edit_method, history_with_verified_payoffs, select_domain_for_state};
pub use learning_edit_policy::{evaluate_search_algorithm};
#[path = "rsi/execution/source.rs"]
mod execution_source;
use execution_source::{git, save, elapsed, lineage_fingerprint, snapshot_source, select};
pub(crate) use execution_source::{command};
pub use execution_source::{auto_write_allowed, campaign_source, campaign_model_tokens, remaining_output_tokens, lineage_fingerprint_valid, choose, run};
#[path = "rsi/planner/context.rs"]
mod planner_context;
use planner_context::{verified_candidate_id, identical_failed_patch, matching_prior_campaign, research_digest, target_focus_objective, choose_focus_files, focus_file_context, paired_behavioral_feedback, generation_context, research_scope, focus_domain_hint, selection_objective};
#[path = "rsi/learning/methods.rs"]
mod learning_methods;
use learning_methods::{method_session_running, applicable_methods, method_reuse_decision, record_method_feedback, research_artifacts};
pub(crate) use learning_methods::{propose_method, configure_method_session, method_strategies, collect_method_executions, preserve_failed_research, remember_research};
#[path = "rsi/learning/trials.rs"]
mod learning_trials;
use learning_trials::{proposed_algorithms, adaptive_search_expression, recovery_search_expression, queue_adaptive_search_trial, queue_search_trial, pending_trial_candidate, comparable_trial_baseline, trial_verdict, record_trial_outcome};
#[path = "rsi/learning/research.rs"]
mod learning_research;
use learning_research::{numbered_source_range, unavailable_research_receipt, session_research_fingerprint};
pub(crate) use learning_research::{reuse_research};
#[path = "rsi/execution/guards.rs"]
mod execution_guards;
use execution_guards::{generation_stop_reason, generation_fits_remaining_budget, unresolved_quality_case, fit_worker_prompt, post_adoption_objective, should_stop_no_progress, new_research_findings, session_tool_usage, generation_prompt_variants, bind_inherited_target};
pub use execution_guards::{diagnose_capabilities};
#[path = "rsi/execution/method_ablation.rs"]
mod method_ablation;
use method_ablation::run_control;
pub(crate) use method_ablation::measure as measured_method_ablation;
#[path = "rsi/execution/campaign.rs"]
mod execution_campaign;
use execution_campaign::{run_loop};
#[path = "rsi/evaluation/adoption.rs"]
mod evaluation_adoption;
use evaluation_adoption::{proof_goal_reached, generation_model_error};
pub(crate) use evaluation_adoption::{adopt};
#[path = "rsi/meta/experiment_policy.rs"]
pub(crate) mod meta_experiment_policy;
#[path = "rsi/planner/descendant.rs"]
mod planner_descendant;
use planner_descendant::{generation_target_plan, reconcile_descendant_target, select_generation_strategy, paired_descendant_target, system_evolution_target_hint};
pub use planner_descendant::{plan_generation_from_file};
#[path = "rsi/execution/worker.rs"]
mod execution_worker;
pub use execution_worker::{worker};

#[cfg(test)]
#[path = "rsi/tests/evolve.rs"]
mod tests;

// Exactly one native search rule: the same template is compiled for
// differential trials and included here for ordinary execution.
include!("rsi_domain_accumulator.rs");
