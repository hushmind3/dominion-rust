#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_file_is_not_meta_improvement() {
        assert!(meta_source("src/engine/rsi/planner/descendant.rs"));
        assert!(meta_source("src/engine/rsi_system/lineage.rs"));
        assert!(!meta_source("src/engine/memory.rs"));
        assert!(!meta_source("README.md"));
        assert!(!meta_source("src/engine/rsi/proof.rs"));
        assert!(!meta_source("src/engine/rsi.rs"));
        assert!(!meta_source("src/engine/rsi_evolve.rs"));
        assert!(!meta_source("src/engine/rsi_system.rs"));
        assert!(meta_source("src/engine/rsi/planner/domain_selection.rs"));
        assert!(!meta_source("src/engine/rsi_system/handoff.rs"));
        assert!(!meta_source("src/engine/rsi/execution/compiled_planning.rs"));
        assert!(!meta_source("src/engine/rsi/execution/generation_evidence.rs"));
        assert!(!meta_source("src/engine/rsi/execution/generation_finalization.rs"));
        assert!(!meta_source("src/engine/rsi/execution/method_ablation.rs"));
        assert!(meta_source("src/engine/rsi/learning/generation_feedback.rs"));
        assert!(meta_source("src/engine/rsi_system/guidance.rs"));
    }
    #[test]
    fn forged_action_flag_does_not_pass_handoff(){
        let p=json!({"generatorCommit":"old","acceptedCommit":"new","generatorBinarySha256":"a"});
        let c=json!({"generatorCommit":"new","generatorBinarySha256":"b",
            "behaviorCounterfactual":{"status":"behavior_changed"}});
        assert!(!handoff(&p,&c,true));
    }
}


#[cfg(test)]
mod recursive_method_feedback_contract {
    use super::*;

    fn observed_method(name:&str,id:&str,revision:&str)->Value {
        json!({"name":name,"methodId":id,"revision":revision,"domain":"memory"})
    }

    #[test]
    fn changed_method_identity_without_measured_child_feedback_is_not_a_closure(){
        let original=observed_method("remember source paths","method-a","rev-a");
        let revised=observed_method("remember source paths","method-b","rev-b");
        let child=json!({"id":"child-b","candidateId":"patch-b"});
        let no_feedback=json!({"improvementDirective":{
            "sourceGenerationId":"unrelated","previousAccepted":true,
            "previousCandidateId":"patch-b"}});
        assert!(!revised_method_after_feedback(&original,&revised,&child,&no_feedback));
        let feedback=json!({"improvementDirective":{
            "sourceGenerationId":"child-b","previousAccepted":true,
            "previousCandidateId":"patch-b"}});
        assert!(revised_method_after_feedback(&original,&revised,&child,&feedback));
        assert!(!revised_method_after_feedback(&original,&original,&child,&feedback));
    }

    #[test]
    fn frozen_case_improvement_must_be_quality_not_a_claimed_counter(){
        let scores=json!({"evaluation":{"metrics":{
            "improvedHoldoutDomains":["memory"],
            "measurements":[{"domain":"memory","holdout":true,
                "baselinePass":true,"candidatePass":true}]
        }}});
        assert!(!independent_quality_gain(&scores,"memory"));
        let improved=json!({"evaluation":{"metrics":{
            "measurements":[{"domain":"memory","holdout":true,
                "baselinePass":false,"candidatePass":true}]
        }}});
        assert!(independent_quality_gain(&improved,"memory"));
        assert!(!independent_quality_gain(&improved,"improver"));
        let quality=json!({"evaluation":{"metrics":{"measurements":[{
            "domain":"improver","holdout":true,"baselinePass":true,
            "candidatePass":true,"metrics":[{"key":"quality","baseline":0.6,
                "candidate":0.8,"improved":true,"regressed":false}]
        }]}}});
        assert!(independent_quality_gain(&quality,"improver"));
        quality_check_does_not_credit_cost_only();
    }

    fn quality_check_does_not_credit_cost_only(){
        let cost=json!({"evaluation":{"metrics":{"measurements":[{
            "domain":"improver","holdout":true,"baselinePass":true,
            "candidatePass":true,"metrics":[{"key":"elapsedMs","baseline":10.0,
                "candidate":5.0,"improved":true,"regressed":false}]
        }]}}});
        assert!(!independent_quality_gain(&cost,"improver"));
    }
}
