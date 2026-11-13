#[cfg(test)]
mod rsi_evaluation_classification_tests {
    use super::*;
    #[test]
    fn evaluator_marker_is_not_part_of_the_agent_input() {
        assert!(is_rsi_evaluation_episode(&json!({"rsiEvaluationEpisode":true,"input":{}})));
        assert!(is_rsi_evaluation_episode(&json!({"input":{"rsiMetaEpisode":true}})));
        assert!(!is_rsi_evaluation_episode(&json!({"input":{"rsiSystemEvolution":true}})));
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn capability_requires_three_consecutive_accepts() {
        let partial = json!({"history":[
            {"accepted":true,"evaluation":{}},
            {"accepted":true,"evaluation":{}},
            {"accepted":false,"evaluation":{}},
            {"accepted":true,"evaluation":{}}
        ]});
        assert_eq!(accepted_streak(&partial), (1, 2));
        let full = json!({"history":[
            {"accepted":true},{"accepted":true},{"accepted":true}
        ]});
        assert_eq!(accepted_streak(&full),(3,3));
        let research_break = json!({"history":[
            {"accepted":true},{"accepted":false},{"accepted":true}
        ]});
        assert_eq!(accepted_streak(&research_break),(1,1));
        // Three boolean flags only measure a streak; the independent lineage
        // oracle is enforced by apply_capability and tested in engine::rsi.
    }
}


#[cfg(test)]
mod recursive_completion_projection_tests {
    use super::*;
    use crate::{config::Config,db::Store,vault::Vault};
    #[test]
    fn campaign_api_view_rejects_unproven_adoption_streak(){
        let root=std::env::temp_dir().join(format!("dominion-rsi-proof-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let mut campaign=json!({"id":"missing-campaign", "systemEvolution":true,"history":[
            {"accepted":true},{"accepted":true},{"accepted":true},{"accepted":true}
        ]});
        apply_capability(&state,&mut campaign);
        assert_eq!(campaign["maxAcceptedStreak"],4);
        assert_eq!(campaign["capabilityDemonstrated"],false);
        assert_eq!(campaign["recursiveProof"]["source"],"canonical_recursive_proof_gate_v1");
        std::fs::remove_dir_all(root).unwrap();
    }
}
