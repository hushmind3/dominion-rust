//! Source-grounded action policy, successor design and compiled descendant planning.
//! Private to the RSI coordinator; no frozen evaluator or model weight changes.
use super::{
    AppState, Value, BTreeMap, BTreeSet, Path,
    PathBuf, json, jobs, rsi, tools,
    DOMAINS, limits, valid_domain_scores, select_domain, search_strategy,
    choose, select, run, focus_excerpt_has_source, bind_action_to_source,
    choose_focus_files, focus_file_context, research_scope, focus_domain_hint, selection_objective,
    generation_fits_remaining_budget, session_tool_usage, worker, comparable_trial_baseline,
};

#[path = "planner/actions.rs"]
mod actions;
pub(super) use actions::{actual_edit_observation,observed_edit_outcome,
    action_learning_context,action_choices,observed_action_value,
    evolved_execution_action,next_execution_action,execution_action_contract};
#[path = "planner/strategy.rs"]
mod strategy;
pub(super) use strategy::{research_continuation,generation_target_plan,
    pending_failed_patch_domain,pending_method_recovery_domain,
    select_generation_strategy,reconcile_descendant_target};
#[path = "planner/edit_policy.rs"]
mod edit_policy;
pub(super) use edit_policy::sub2_edit_policy_reward;
use edit_policy::inherited_edit_policy;
#[path = "planner/operator_policy.rs"]
mod operator_policy;
#[path = "planner/compiled.rs"]
mod compiled;
pub(super) use compiled::{evidence_based_meta_repair_focus,compiled_system_generation_plan};


#[cfg(test)]
mod meta_repair_target_tests {
    use super::*;

    #[test]
    fn observed_transfer_repair_edits_real_selector_not_its_old_facade() {
        let mut plan=json!({
            "metaRepairChosen":true,
            "mode":"repair_own_transfer_method",
            "target":{"path":crate::engine::rsi_meta_controller::TRANSFER_METHOD_SOURCE}
        });
        assert!(evidence_based_meta_repair_focus(&plan,&Value::Null,false));
        assert!(!evidence_based_meta_repair_focus(&plan,&Value::Null,true));
        assert!(!evidence_based_meta_repair_focus(&plan,&json!({"role":"learning"}),false));

        plan["target"]["path"]=json!("src/engine/rsi_meta_controller.rs");
        assert!(!evidence_based_meta_repair_focus(&plan,&Value::Null,false));
        plan["mode"]=json!("repair_own_prediction_mechanism");
        assert!(evidence_based_meta_repair_focus(&plan,&Value::Null,false));
    }
}
