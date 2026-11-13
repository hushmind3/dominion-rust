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

pub const DOMAINS: &[&str] = &["memory", "tools", "context", "learning", "ui", "improver"];

#[cfg(test)]
#[path = "rsi_oracle_fixture.rs"]
mod oracle_tests;

pub fn limits(body: &Value) -> Result<Value, String> {
    let bound = |key: &str, default: u64, min: u64, max: u64| -> Result<u64, String> {
        let value = body
            .get(key)
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| format!("{key}는 정수여야 합니다."))
            })
            .unwrap_or(Ok(default))?;
        if (min..=max).contains(&value) {
            Ok(value)
        } else {
            Err(format!("{key} 범위는 {min}~{max}입니다."))
        }
    };
    let token_budget = match body.get("tokenBudget") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value.as_u64().ok_or("tokenBudget은 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (1_000..=2_000_000).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "tokenBudget 범위는 1,000~2,000,000입니다. 0 또는 생략은 상한 없음입니다."
                        .into(),
                );
            }
        }
    };
    let time_budget = match body.get("timeBudgetSeconds") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or("timeBudgetSeconds는 정수여야 합니다.")?;
            if number == 0 {
                Value::Null
            } else if (60..=7200).contains(&number) {
                json!(number)
            } else {
                return Err(
                    "timeBudgetSeconds 범위는 60~7200입니다. 0 또는 생략은 상한 없음입니다.".into(),
                );
            }
        }
    };
    Ok(json!({"maxAttempts":bound("maxAttempts",3,1,100)?,
        "tokenBudget":token_budget,
        "timeBudgetSeconds":time_budget}))
}

// Public RSI facade: executable domain selection is an evolvable child.
#[path = "rsi/meta/transition_policy.rs"]
pub(crate) mod meta_transition_policy;
#[path = "rsi/meta/evolution.rs"]
pub(crate) mod meta_evolution;
#[path = "rsi/meta/policy_dispatch.rs"]
pub(crate) mod meta_dispatch;
#[path = "rsi/meta/experiment_dispatch.rs"]
pub(crate) mod experiment_dispatch;
#[path = "rsi/meta/inputs.rs"]
mod meta_inputs;
#[path = "rsi/meta/composition.rs"]
mod meta_composition;
#[path = "rsi/proof/open_evolution.rs"]
pub(crate) mod open_evolution_proof;
#[path = "rsi/evaluation/contract.rs"]
pub(crate) mod experiment_contract;

#[path = "rsi/planner/domain_selection.rs"]
mod domain_selection;
#[cfg(test)]
use domain_selection::{search_features,score_domain};
pub(crate) use domain_selection::{objective_matches,candidate_domains,scored_domains,select_domain};
pub(super) use domain_selection::fast_failure_domain;
// The descendant stages are separate modules with explicit ownership.

#[path = "rsi/planner/policy.rs"]
mod policy_planner;
use policy_planner::{search_strategy, executed_policy_rule, edit_method_reward, synthesize_edit_method, learned_edit_method, history_with_verified_payoffs, select_domain_for_state};
pub use policy_planner::{evaluate_search_algorithm};

#[path = "rsi/lineage/source.rs"]
mod source_lineage;
use source_lineage::{owned_source_origin, git, save, elapsed, lineage_fingerprint, snapshot_source};
pub(crate) use source_lineage::{command};
pub use source_lineage::{auto_write_allowed, campaign_source, campaign_model_tokens, remaining_output_tokens, lineage_fingerprint_valid};

#[path = "rsi/lineage/campaign.rs"]
mod campaign_lineage;
use campaign_lineage::{select};
pub use campaign_lineage::{choose, run};

#[path = "rsi/planner/context.rs"]
mod context_planner;
use context_planner::{verified_candidate_id, identical_failed_patch, matching_prior_campaign, research_digest, target_focus_objective, choose_focus_files, focus_file_context, paired_behavioral_feedback, generation_context, research_scope, focus_domain_hint, selection_objective};

#[path = "rsi/learning/methods.rs"]
mod method_learning;
use method_learning::{method_session_running, applicable_methods, record_method_feedback};
pub(crate) use method_learning::method_reuse_decision;
pub(crate) use method_learning::{propose_method, configure_method_session, method_strategies, collect_method_executions, method_wrote_accepted_candidate};

#[path = "rsi/learning/research.rs"]
mod research_learning;
use research_learning::{research_artifacts, proposed_algorithms, adaptive_search_expression, recovery_search_expression, queue_adaptive_search_trial, queue_search_trial, pending_trial_candidate, numbered_source_range, unavailable_research_receipt, session_research_fingerprint};

#[path = "rsi/evaluation/trials.rs"]
mod evaluation_trials;
use evaluation_trials::{comparable_trial_baseline, trial_verdict, record_trial_outcome};
pub(crate) use research_learning::{preserve_failed_research, remember_research, reuse_research};

#[path = "rsi/execution/generation.rs"]
mod generation_execution;
use generation_execution::{generation_stop_reason, generation_fits_remaining_budget, unresolved_quality_case, fit_worker_prompt, post_adoption_objective, should_stop_no_progress, new_research_findings, session_tool_usage, generation_prompt_variants, bind_inherited_target};
pub use generation_execution::{diagnose_capabilities};

#[path = "rsi/execution/counterfactual.rs"]
mod counterfactual_execution;
use counterfactual_execution::{retain_generator, matched_planner_intervention};
pub(crate) use counterfactual_execution::action_hash;

#[path = "rsi/execution/compiled_planning.rs"]
mod compiled_planning;
use compiled_planning::plan_compiled_descendant;

#[path = "rsi/execution/generation_evidence.rs"]
mod generation_evidence;
use generation_evidence::capture_generation_evidence;

#[path = "rsi/learning/generation_feedback.rs"]
mod generation_feedback;
use generation_feedback::record_generation_feedback;

#[path = "rsi/execution/generation_finalization.rs"]
mod generation_finalization;
use generation_finalization::{finalize_generation,GenerationObservation};

#[path = "rsi/execution/method_ablation.rs"]
mod method_ablation;
use method_ablation::run_method_control;
pub(crate) use method_ablation::measured_method_ablation;

#[path = "rsi/execution/generation_selection.rs"]
mod generation_selection;
use generation_selection::evaluate_and_adopt_generation;

#[path = "rsi/execution/generation_preparation.rs"]
mod generation_preparation;
use generation_preparation::prepare_generation_input;

#[path = "rsi/execution/campaign.rs"]
mod campaign_execution;
use campaign_execution::{run_loop, proof_goal_reached, generation_model_error};
pub(crate) use campaign_execution::{adopt};

#[path = "rsi/planner/descendant.rs"]
mod descendant_planner;
use descendant_planner::{generation_target_plan, reconcile_descendant_target, select_generation_strategy, paired_descendant_target, system_evolution_target_hint, validated_descendant_controller};
pub use descendant_planner::{plan_generation_from_file};

#[path = "rsi/execution/worker.rs"]
mod worker_execution;
pub use worker_execution::{worker};

#[cfg(test)]
include!("../../tests/support/engine/rsi_evolve.tests.rs");



