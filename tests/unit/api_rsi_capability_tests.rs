use super::*;

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
