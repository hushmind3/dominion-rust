//! Attribution of executed editable methods from original paired
//! experiment history. Separate from campaign orchestration and the evaluator.
use super::*;

pub(super) fn assess(
    context:&JobContext,record:&Value,generation:&mut Value
) {
        // Inspect the exact code bytes AFTER actual program/rule execution;
        // editing the method generator is merely a proposal until a separate
        // adopted source improvement is independently recorded.
        let method_reward=sub2_edit_policy_reward(&generation);
        generation["editMethodExecutionProof"]=json!({
            "source":"ordered_tool_stage_to_adopted_source_receipt_v1",
            "revision":generation["editPolicy"]["revision"],
            "parentRevision":generation["editPolicy"]["parentRevision"],
            "rewriteProgram":generation["editPolicy"]["rewriteProgram"],
            "executedStages":generation["programStages"].as_array()
                .map(Vec::len).unwrap_or(0),
            "firedRules":generation["ruleExecutions"].as_array()
                .map(Vec::len).unwrap_or(0),
            "matchedAdoptedSourceWrite":method_reward>=1.0,
            "qualityEvidence":if method_reward>=1.0 {
                "independently_adopted_source_with_prior_executed_policy"
            }else{"not_proven"},
            "causalPolicySuperiorityProven":false,
            "independentlyMeasuredAdoption":generation["accepted"]==true
                && generation["pairedRevalidated"]==true
        });
        generation["metaLearning"]=crate::engine::rsi_meta_controller::feedback(
            &generation["metaPolicy"],&generation);
        // The inherited, compiled method is not scored by its own presence.
        // Publish whether its REAL successor was independently assessed,
        // adopted and authored a further change to executable meta source.
        generation["metaMethodOutcome"]=
            crate::engine::rsi_system::observed_method_outcome(&generation);
        // The independent old-binary control never joins accepted genealogy.
        // Replay BOTH immutable candidate receipts before recording any
        // attributed benefit for this method's real downstream execution.
        let verified_ancestors=crate::engine::rsi_system::revalidated_method_history(
            &context.state,record["history"].as_array()
                .map(Vec::as_slice).unwrap_or(&[]));
        let method_control_outcome=verified_ancestors.last()
            .map(|parent|super::super::method_control::measured_advantage(
                &context.state,parent,&generation))
            .unwrap_or(Value::Null);
        generation["methodControlOutcome"]=method_control_outcome;
        // The immediate quality receipt and the future descendant's
        // productivity are SEPARATE learning targets. Future descendants
        // reconstruct the latter only after real independent evaluations.
        generation["principleObservation"]=
            crate::engine::rsi_principle_lab::observation(&generation);
        generation["modelRouteObservation"]=
            crate::engine::rsi_model_ecology::observation(&generation);
        // Persist the real outcome of the newly executed recovery policy.
        // A ranking score or a source write is not a validated improvement;
        // this is the observed action utility that the next built generator
        // may learn from. Keep the exact policy revision and actual outcome.
        generation["actionPolicyOutcome"]=json!({
            "revision":generation["actionDirective"]["policy"]["revision"],
            "mode":generation["actionDirective"]["mode"],
            "trainingEvidence":generation["actionDirective"]["policy"]["trainingGenerationIds"],
            "policyStatus":generation["actionDirective"]["policy"]["status"],
            "focusApplied":generation["actionExecution"]["focusApplied"],
            "editCheckpointReached":generation["actionExecution"]["editCheckpointReached"],
            "observedUtility":observed_action_value(&generation),
            "actualSourceDiffBytes":generation["actionObservation"]["sourceDiffBytes"],
            "actualCandidateId":generation["candidateId"],
            "accepted":generation["accepted"],
            "metric":"observed_action_utility_not_independent_quality_gain",
            "qualityGain":Value::Null
        });
}
