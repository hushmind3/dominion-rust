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
#[path = "rsi/execution/campaign.rs"]
mod execution_campaign;
use execution_campaign::{run_loop};
#[path = "rsi/evaluation/adoption.rs"]
mod evaluation_adoption;
use evaluation_adoption::{proof_goal_reached, generation_model_error};
pub(crate) use evaluation_adoption::{adopt};
#[path = "rsi/planner/descendant.rs"]
mod planner_descendant;
use planner_descendant::{generation_target_plan, reconcile_descendant_target, select_generation_strategy, paired_descendant_target, system_evolution_target_hint};
pub use planner_descendant::{plan_generation_from_file};
#[path = "rsi/execution/worker.rs"]
mod execution_worker;
pub use execution_worker::{worker};

#[cfg(test)]
#[path = "rsi_evolve_tests.rs"]
mod tests;


// Source template for the native domain-selection mutation. This file is
// compiled in differential tests; production uses it only after a real patch
// inserts it into the descendant's own RSI selector and A/B accepts it.
pub(super) fn fast_failure_domain(
    history: &[serde_json::Value],
    candidates: &[&str],
    objective_matches: impl Fn(&str) -> bool,
) -> String {
    #[derive(Clone, Copy, Default)]
    struct Stats {
        attempts: u64,
        accepted: u64,
        tokens: u64,
        verified: u64,
        gains: u64,
        verified_tokens: u64,
    }
    let mut stats = vec![Stats::default(); candidates.len()];
    for row in history {
        let Some(index) = candidates
            .iter()
            .position(|domain| row["domain"] == *domain)
        else {
            continue;
        };
        let item = &mut stats[index];
        item.attempts += 1;
        item.accepted += u64::from(row["accepted"] == true);
        item.tokens += row["tokens"].as_u64().unwrap_or(0);
        if row["evidenceVerified"] == true {
            item.verified += 1;
            item.gains += row["verifiedHoldoutGains"].as_u64().unwrap_or(0).min(64);
            item.verified_tokens += row["tokens"].as_u64().unwrap_or(0).max(1000);
        }
    }
    let mut best = (f64::NEG_INFINITY, candidates[0]);
    for (domain, item) in candidates.iter().zip(stats) {
        let attempts = item.attempts as f64;
        let token_cost = if attempts == 0.0 {
            0.0
        } else {
            (item.tokens as f64 / attempts) / 10_000.0
        };
        let count = attempts.max(1.0);
        let utility = item.accepted as f64 / (1.0 + token_cost * count) / count;
        let exploration = if attempts == 0.0 {
            3.0
        } else {
            (2.0 * ((history.len() as f64) + 1.0).ln() / attempts)
                .sqrt()
                .min(3.0)
        };
        let gain_efficiency = if item.verified_tokens > 0 {
            (item.gains as f64 * 10_000.0 / item.verified_tokens as f64).min(8.0)
        } else {
            0.0
        };
        let payoff = if item.verified > 0 {
            (item.gains as f64 / (item.verified as f64 + 1.0)).min(4.0) * 0.75
                + gain_efficiency.min(4.0) * 0.30
        } else {
            0.0
        };
        let objective_match = if objective_matches(domain) { 1.0 } else { 0.0 };
        let score = utility + payoff + exploration + objective_match * 2.0;
        if score > best.0 {
            best = (score, domain);
        }
    }
    best.1.to_string()
}
