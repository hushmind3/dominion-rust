//! Regression tests: src/engine/jobs.rs (rsi_native_preflight_tests)
use super::*;
    #[test]
    fn opted_in_source_search_can_start_natively_without_spending_a_model_turn() {
        let input=json!({"rsiNativeInduction":true,
            "rsiSourceScopeExpanded":true,"rsiAutonomous":true,
            "rsiAutoFocus":true,"rsiInheritedCandidateId":null});
        assert!(rsi_native_preflight(&input));
        for key in ["rsiNativeInduction","rsiAutoFocus"] {
            let mut blocked=input.clone();
            blocked[key]=json!(false);
            assert!(!rsi_native_preflight(&blocked));
        }
        let mut initial=input.clone();
        initial["rsiSourceScopeExpanded"]=json!(false);
        initial["rsiAutonomous"]=json!(false);
        assert!(rsi_native_preflight(&initial));
        initial["rsiFailedPatchRecovery"]=json!({"candidateId":"compiler-failure"});
        assert!(!rsi_native_preflight(&initial));
        let mut inherited=input;
        inherited["rsiInheritedCandidateId"]=json!("neutral-patch");
        assert!(!rsi_native_preflight(&inherited));
    }
