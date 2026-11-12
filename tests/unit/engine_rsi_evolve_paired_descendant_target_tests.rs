use super::*;
    #[test]
    fn immutable_paired_target_outvotes_unmeasured_descendant_exploration() {
        let evidence=json!({"rsiSourceScopeExpanded":true,
            "rsiSelectedDomain":"improver",
            "rsiDecisionInput":{"basis":"independently_measured_post_adoption_target",
                "targetDomain":"improver"}});
        assert!(paired_descendant_target(&evidence));
        let mut mismatched=evidence.clone();
        mismatched["rsiSelectedDomain"]=json!("memory");
        assert!(!paired_descendant_target(&mismatched));
        let mut speculative=evidence;
        speculative["rsiDecisionInput"]["basis"]=json!("parent_next_decision");
        assert!(!paired_descendant_target(&speculative));
    }
