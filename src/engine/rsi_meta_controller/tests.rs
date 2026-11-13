    use super::*;
    #[test]
    fn repeated_measured_brier_drift_selects_own_prediction_source_with_cooldown() {
        let predictors=json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_prequential.rs","sourceSha256":"a".repeat(64),
            "sourceBytes":5000,"verifiedRoleChanged":false});
        let memory=json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceSha256":"b".repeat(64),
            "sourceBytes":3000,"verifiedRoleChanged":false});
        let mut history=(0..4).map(|_|json!({"pairedMeasured":true,
            "metaLearning":{"source":"source_bound_prequential_feedback_v1",
                "nextPolicyCanTrainFromObservation":true,
                "brierError":0.85}})).collect::<Vec<_>>();
        let result=synthesize(&history,&[memory.clone(),predictors.clone()],
            &["memory","improver"],None);
        assert_eq!(result["target"]["path"],"src/engine/rsi_prequential.rs");
        assert_eq!(result["predictionRuleRepair"]["repairRuleSourceSelected"],true);
        assert_eq!(result["mode"],"repair_own_source_bound_prediction_rule");
        history.push(json!({"pairedMeasured":true,
            "focusFiles":["src/engine/rsi_prequential.rs"]}));
        let retry=synthesize(&history,&[memory,predictors],&["memory","improver"],None);
        assert_eq!(retry["predictionRuleRepair"]["cooldown"],true);
        assert_eq!(retry["predictionRuleRepair"]["repairRuleSourceSelected"],false);
    }

    #[test]
    fn adopted_improver_rewrite_links_to_independently_adopted_harness_successor(){
        let origin="a".repeat(40);
        let revised="b".repeat(40);
        let next="c".repeat(40);
        let make=|job:&str,base:&str,accepted:&str,role:&str|json!({
            "jobId":job,"generatorCommit":base,"acceptedCommit":accepted,
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary"},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":job,
                "component":role,"targetFiles":["src/engine/system.rs"]},
            "acceptedSourceHashes":{"src/engine/system.rs":"f".repeat(64)}
        });
        let parent=make("parent",&origin,&revised,"algorithms");
        let child=make("child",&revised,&next,"harness");
        let graph=observed_interaction_graph(&[parent.clone(),child.clone()]);
        let edge=graph["edges"].as_array().unwrap().iter().find(|edge|
            edge["fromRole"]=="algorithms" && edge["toRole"]=="harness").unwrap();
        assert_eq!(edge["compiledSuccessorAttempts"],1);
        assert_eq!(edge["independentlyAdoptedSuccessors"],1);
        assert_eq!(edge["crossRoleCausalityProven"],false);
        let mut unverified=child;
        unverified["pairedRevalidated"]=json!(false);
        let graph=observed_interaction_graph(&[parent,unverified]);
        let edge=&graph["edges"][0];
        assert_eq!(edge["compiledSuccessorAttempts"],1);
        assert_eq!(edge["independentlyAdoptedSuccessors"],0);
    }
    #[test]
    fn comparison_prioritizes_novelty_then_updates_after_grounded_failures(){
        let target=|role:&str,path:&str|json!({"role":role,"domain":if role=="memory"{"memory"}else{"learning"},
            "path":path,"sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "verifiedRoleChanged":false});
        let frontier=vec![target("memory","src/engine/memory.rs"),
            target("learning","src/engine/learning.rs")];
        let first=synthesize(&[],&frontier,&["memory","learning"],None);
        assert!(first["target"].is_object());
        assert_eq!(first["historyCount"],0);
        let mut failures=Vec::new();
        for n in 0..6{
            failures.push(json!({"jobId":format!("job-{n}"),"generatorCommit":"base",
                "accepted":false,"candidateId":null,
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"base","generationJobId":format!("job-{n}"),
                    "component":"memory","targetFiles":["src/engine/memory.rs"]}}));
        }
        let later=synthesize(&failures,&frontier,&["memory","learning"],None);
        assert_eq!(later["target"]["role"],"learning");
        assert!(later["counterexamples"].as_array().unwrap().len()>0);
        assert_eq!(feedback(&later,&failures[0])["independentlyMeasured"],false);
    }
