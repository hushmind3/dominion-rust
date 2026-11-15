//! Extracted isolated regression tests for src/engine/rsi/learning/edit_genome.rs.
use super::*;
    fn reward(v:&Value)->f64{
        if v["accepted"]==true && v["evidenceVerified"]==true {1.0}
        else if v["candidateId"].is_string() {0.1}
        else {0.0}
    }
    fn row(job:&str,policy:Value)->Value{
        json!({"jobId":job,"tokens":12000,"editPolicy":policy,
            "candidateId":null,"accepted":false,
            "editEvidence":{"verifiedWrites":0,"attempts":1}})
    }

    #[test]
    fn synthesis_mutates_real_failures_into_new_executable_controllers(){
        let initial=evolve(&[],reward);
        assert_eq!(initial["checkpointAfter"],3);
        let next=evolve(&[row("first",initial.clone())],reward);
        assert_ne!(next["revision"],initial["revision"]);
        assert!(next["method"].as_str().unwrap().starts_with("composed-"));
        assert!(next["checkpointAfter"].as_u64().unwrap()>=2);
        assert!(next["checkpointAfter"].as_u64().unwrap()<=5);
        assert!(next["repairLimit"].as_u64().unwrap()<=3);
        assert!(next["parentRevision"].is_string());
    }

    #[test]
    fn verified_patch_without_candidate_evolves_repair_algorithm(){
        let initial=evolve(&[],reward);
        let mut failure=row("real-write",initial);
        failure["editEvidence"]["verifiedWrites"]=json!(1);
        let next=evolve(&[failure],reward);
        assert_eq!(next["mutation"],"increase_verifier_guided_repair_after_real_patch");
        assert_eq!(next["repairLimit"],3);
    }

    #[test]
    fn exhausted_budget_shrinks_repair_and_changes_next_algorithm(){
        let initial=evolve(&[],reward);
        let mut failure=row("token-exhausted",initial);
        failure["error"]=json!("rsi_token_budget: exhausted");
        failure["editEvidence"]["verifiedWrites"]=json!(1);
        let next=evolve(&[failure],reward);
        assert_eq!(next["mutation"],"shorten_discovery_and_repair_from_token_exhaustion");
        assert_eq!(next["repairLimit"],1);
    }

    #[test]
    fn cannot_learn_from_claims_that_never_executed(){
        let initial=evolve(&[],reward);
        let fake=json!({"jobId":"fake","tokens":0,"editPolicy":initial,
            "accepted":true,"evidenceVerified":true});
        assert_eq!(evolve(&[fake],reward)["method"],"balanced");
    }

    #[test]
    fn repeated_independent_success_retains_a_proven_controller(){
        let policy=evolve(&[],reward);
        let mut rows=Vec::new();
        for id in ["one","two","three"]{
            let mut item=row(id,policy.clone());
            item["accepted"]=json!(true);
            item["evidenceVerified"]=json!(true);
            rows.push(item);
        }
        let selected=evolve(&rows,reward);
        assert_eq!(selected["revision"],policy["revision"]);
        assert_eq!(selected["mutation"],"reuse_measured_successful_method");
    }

    #[test]
    fn generated_source_program_inherits_failure_recovery_nodes() {
        let initial=evolve(&[],reward);
        assert!(initial["program"].as_array().unwrap().iter().any(|s|s=="ground"));
        let mut failure=row("hash-failure",initial.clone());
        failure["editEvidence"]["recentFailures"]=json!(["workspace_replace_range: sha mismatch"]);
        let repaired=evolve(&[failure],reward);
        assert!(repaired["program"].as_array().unwrap().iter().any(|s|s=="reground"));
        assert_ne!(initial["revision"],repaired["revision"],
            "changing executable stage topology must change revision");
        let mut neutral=row("neutral-holdout",repaired.clone());
        neutral["candidateId"]=json!("candidate");
        neutral["evaluation"]=json!({"verdict":"unchanged"});
        let again=evolve(&[row("hash-failure",initial),neutral],reward);
        assert!(again["program"].as_array().unwrap().iter().any(|s|s=="contrast"));
        assert!(again["program"].as_array().unwrap().iter().any(|s|s=="reground"));
        assert_eq!(again["improvementProven"],false);
    }

    #[test]
    fn three_descendant_generations_rewrite_their_own_execution_procedure() {
        let mut history=Vec::new();
        let base=evolve(&history,reward);
        let mut first=row("generation-1",base);
        first["editEvidence"]["recentFailures"]=json!(["source sha mismatch"]);
        history.push(first);
        let second=evolve(&history,reward);
        assert!(second["program"].as_array().unwrap().contains(&json!("reground")));

        let mut generated=row("generation-2",second.clone());
        generated["candidateId"]=json!("candidate-2");
        generated["evaluation"]=json!({"verdict":"unchanged"});
        history.push(generated);
        let third=evolve(&history,reward);
        assert!(third["program"].as_array().unwrap().contains(&json!("contrast")));
        assert!(third["program"].as_array().unwrap().contains(&json!("reground")));

        let mut stalled=row("generation-3",third);
        stalled["programStages"]=json!([
            {"phase":"ground","toolCalls":1},{"phase":"contrast","toolCalls":3}
        ]);
        history.push(stalled);
        let fourth=evolve(&history,reward);
        assert!(!fourth["program"].as_array().unwrap().contains(&json!("contrast")),
            "the real stage stall must not be overwritten by duplicate-genome exploration");
        assert!(fourth["program"].as_array().unwrap().contains(&json!("reground")));
        assert_eq!(fourth["improvementProven"],false);
    }

    #[test]
    fn descendants_generate_novel_predicates_not_only_stage_labels(){
        let baseline=evolve(&[],reward);
        let mut generation=row("g1",baseline);
        generation["toolTimeline"]=json!([
            {"tool":"workspace_read_range","status":"completed"},
            {"tool":"code_search","status":"completed"},
            {"tool":"code_search","status":"completed"},
            {"tool":"code_search","status":"completed"}
        ]);
        let evolved=evolve(&[generation],reward);
        assert_eq!(evolved["rules"][0]["guard"]["all"][2]["value"],3);
        assert_eq!(evolved["improvementProven"],false);
        assert!(evolved["revision"]!=evolved["parentRevision"]);
    }

    #[test]
    fn verified_success_spawns_a_shorter_executable_child_instead_of_only_reusing_it() {
        let base=evolve(&[],reward);
        let successful_tool=json!({"runId":"test-run","tool":"workspace_replace_range","status":"completed",
            "path":"src/engine/test.rs",
            "result":{"written":true,"postSha256":"a".repeat(64)},
            "toolReceipt":{"verified":true}});
        let mut outcomes=Vec::new();
        for id in ["adopted-1","adopted-2"] {
            let mut row=row(id,base.clone());
            row["accepted"]=json!(true);
            row["evidenceVerified"]=json!(true);
            row["evaluation"]=json!({"metrics":{"eligible":true}});
            row["acceptedSourceHashes"]=json!({"src/engine/test.rs":"a".repeat(64)});
            row["toolTimeline"]=json!([
                {"runId":"test-run","tool":"workspace_read_range","status":"completed"},
                {"runId":"test-run","tool":"code_search","status":"completed"},
                successful_tool
            ]);
            outcomes.push(row);
        }
        let trial=evolve(&outcomes,reward);
        assert_eq!(trial["mutation"],"compress_successful_source_edit_strategy");
        assert!(trial["rules"].as_array().is_some_and(|rules|!rules.is_empty()));
        assert_eq!(trial["parentRevision"],base["revision"]);
        assert_eq!(trial["improvementProven"],false);
        let mut failed=row("failed-trial",trial.clone());
        failed["candidateId"]=Value::Null;
        outcomes.push(failed);
        let resumed=evolve(&outcomes,reward);
        assert_ne!(resumed["revision"],trial["revision"]);
    }

    #[test]
    fn rule_provenance_cannot_fragment_an_identical_executable_algorithm() {
        let mut genome=Genome::baseline();
        let base=serde_json::json!([{"guard":{"metric":"grounded","cmp":"eq","value":1},
            "allow":["workspace_replace_range"],"sourceJobId":"first-worker"}]);
        genome.rules=crate::engine::rsi_decision_rules::normalize(&base);
        let first=genome.revision();
        let other=serde_json::json!([{"guard":{"metric":"grounded","cmp":"eq","value":1},
            "allow":["workspace_replace_range"],"sourceJobId":"other-worker"}]);
        genome.rules=crate::engine::rsi_decision_rules::normalize(&other);
        assert_eq!(genome.revision(),first);
        genome.rules=crate::engine::rsi_decision_rules::normalize(&serde_json::json!([
            {"guard":{"metric":"grounded","cmp":"eq","value":1},
             "allow":["workspace_replace"],"sourceJobId":"other-worker"}
        ]));
        assert_ne!(genome.revision(),first);
    }

    #[test]
    fn descendant_rewrites_the_algorithm_that_generates_its_guards(){
        let baseline=evolve(&[],reward);
        assert!(baseline["rewriteProgram"].as_array().unwrap().is_empty());
        let mut first=row("worker-failure",baseline.clone());
        first["toolTimeline"]=json!([
            {"tool":"workspace_read_range","status":"completed"},
            {"tool":"workspace_replace_range","status":"failed"}]);
        let child=evolve(&[first],reward);
        assert_eq!(child["rewriteProgram"][0]["kind"],"switch_edit");
        assert_ne!(child["revision"],baseline["revision"]);
        let original=crate::engine::rsi_decision_rules::normalize(&json!([
            {"guard":{"metric":"grounded","cmp":"eq","value":1},
            "allow":["workspace_replace_range"]}]));
        let rewrite=crate::engine::rsi_rule_rewrite::apply(
            &child["rewriteProgram"].as_array().unwrap().clone(),&original);
        assert_eq!(rewrite[0]["allow"],json!(["workspace_replace"]));
        assert_eq!(child["improvementProven"],false);
    }

    #[test]
    fn independently_promoted_policy_can_mutate_again_after_a_real_failure(){
        let control=evolve(&[],reward);
        let mut challenger=Genome::from_policy(&control);
        challenger.checkpoint_after=4;
        let winner=serialize(challenger,"challenger",&control["revision"].to_string(),
            "trial",0,0.0,None);
        let make=|job:&str,policy:Value,head:&str,success:bool|{
            let mut item=row(job,policy);
            item["generatorCommit"]=json!(head);
            item["generatorModel"]=json!("model-1");
            item["generatorProviderId"]=json!("provider-1");
            item["objectiveId"]=json!("goal-1");
            item["knowledgePartition"]=json!("partition-1");
            item["candidateId"]=json!(format!("candidate-{job}"));
            item["accepted"]=json!(success);
            item["evidenceVerified"]=json!(success);
            item["evaluation"]=json!({"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":success}});
            item
        };
        let mut rows=vec![
            make("a-control",control.clone(),"sha-a",false),
            make("a-winner",winner.clone(),"sha-a",true),
            make("b-control",control,"sha-b",false),
            make("b-winner",winner.clone(),"sha-b",true)
        ];
        // Both versions also have enough source-receipted examples for
        // counterexample induction. Independent paired wins still take
        // precedence over generating yet another speculative guard.
        for (index,item) in rows.iter_mut().enumerate() {
            let mut operations=vec![json!({"runId":format!("run-{index}"),
                "tool":"workspace_read_range","status":"completed",
                "path":"src/engine/test.rs"})];
            if index%2==0 {
                for _ in 0..3 {
                    operations.push(json!({"runId":format!("run-{index}"),
                        "tool":"code_search","status":"completed"}));
                }
            }
            operations.push(json!({"runId":format!("run-{index}"),
                "tool":"workspace_replace_range","status":"completed",
                "path":"src/engine/test.rs",
                "result":{"written":true,"postSha256":"a".repeat(64)},
                "toolReceipt":{"verified":true}}));
            item["toolTimeline"]=json!(operations);
            item["acceptedSourceHashes"]=json!({"src/engine/test.rs":"a".repeat(64)});
        }
        let mut failed=row("winner-regression",winner.clone());
        failed["editEvidence"]["attempts"]=json!(2);
        rows.push(failed);
        let next=evolve(&rows,reward);
        assert_eq!(next["mutation"],"rechallenge_independently_promoted_controller");
        assert_eq!(next["parentRevision"],winner["revision"]);
        assert_ne!(next["revision"],winner["revision"]);
        assert_eq!(next["improvementProven"],false);
    }

    #[test]
    fn generated_methods_can_compose_beyond_three_fixed_choices(){
        let mut policy=evolve(&[],reward);
        let mut revisions=BTreeSet::new();
        let mut history=Vec::new();
        revisions.insert(policy["revision"].as_str().unwrap().to_owned());
        for i in 0..9 {
            let mut outcome=row(&format!("failure-{i}"),policy);
            outcome["editEvidence"]["recentFailures"]=json!(["workspace_replace_range: unchanged"]);
            history.push(outcome);
            policy=evolve(&history,reward);
            revisions.insert(policy["revision"].as_str().unwrap().to_owned());
        }
        assert!(revisions.len()>3,"an executable method generator must go beyond a fixed three-name catalog");
    }
