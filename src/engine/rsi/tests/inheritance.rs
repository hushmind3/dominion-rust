// Kept in the parent's test module for access to stable method receipts.
    use super::*;
    #[test]
    fn an_unevaluated_or_model_claimed_rule_cannot_earn_quality_credit() {
        let fake=json!({"jobId":"worker-1","accepted":true,"pairedRevalidated":true,
            "modelAttributionConsistent":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evidenceVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"r1","rules":[{"id":"learnt-guard"}]},
            "acceptedSourceHashes":{"src/engine/memory.rs":"a".repeat(64)},
            "ruleExecutions":[{"ruleId":"learnt-guard",
                "generationJobId":"worker-1",
                "policyRevision":"r1","runId":"run1","opSeqBefore":1,
                "allow":["workspace_replace"]}],
            "toolTimeline":[]});
        assert_eq!(sub2_edit_policy_reward(&fake),0.0);
        let mut unverified=fake.clone();
        unverified["evidenceVerified"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&unverified),0.0);
        let mut no_receipt=fake.clone();
        no_receipt["toolTimeline"]=json!([{"runId":"run1","opSeq":2,
            "tool":"workspace_replace","status":"completed",
            "path":"src/engine/memory.rs",
            "result":{"written":true,"postSha256":"a".repeat(64)},
            "toolReceipt":{"verified":false}}]);
        assert_eq!(sub2_edit_policy_reward(&no_receipt),0.0);
        let mut real=no_receipt;
        real["toolTimeline"][0]["toolReceipt"]["verified"]=json!(true);
        assert_eq!(sub2_edit_policy_reward(&real),1.0);
        real["modelAttributionConsistent"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&real),0.0,
            "mixed-model sessions cannot promote an edit rule");
    }
    #[test]
    fn rewritten_edit_program_needs_a_later_verified_adopted_source_write() {
        let mut g=json!({
            "jobId":"worker-program","accepted":true,
            "modelAttributionConsistent":true,
            "pairedRevalidated":true,"evidenceVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "editPolicy":{"revision":"rewritten-v2",
                "parentRevision":"original-v1","rules":[]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"a".repeat(64)},
            "programStages":[{"runId":"session-1","generationJobId":"worker-program",
                "policyRevision":"rewritten-v2","phase":"ground","opSeqBefore":1}],
            "toolTimeline":[{"runId":"session-1","opSeq":4,
                "tool":"workspace_replace_function","status":"completed",
                "path":"src/engine/learning.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}]
        });
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "source success cannot be credited to a ground-only policy");
        g["programStages"][0]["phase"]=json!("edit");
        assert_eq!(sub2_edit_policy_reward(&g),1.0);
        g["programStages"][0]["opSeqBefore"]=json!(4);
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "an edit-stage claim after the actual write cannot steal credit");
        g["programStages"][0]["opSeqBefore"]=json!(1);
        g["toolTimeline"][0]["toolReceipt"]["verified"]=json!(false);
        assert_eq!(sub2_edit_policy_reward(&g),0.0,
            "a tool result without independently verified source receipt is not proof");
    }
    #[test]
    fn neutral_source_patch_is_not_edit_program_quality_proof() {
        let neutral=json!({"accepted":false,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "candidateId":"neutral-1",
            "evaluation":{"metrics":{"regressions":0}},
            "editPolicy":{"revision":"r1","rules":[]}});
        assert!(sub2_edit_policy_reward(&neutral)<0.11);
        let native=json!({"accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"native_inductive_source_mutation",
            "evidenceVerified":true,
            "editPolicy":{"revision":"r1","rules":[]}});
        assert_eq!(sub2_edit_policy_reward(&native),0.0);
    }
