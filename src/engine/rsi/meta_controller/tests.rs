//! Contract tests for the real compiled meta-controller stages.
    use super::*;

    #[test]
    fn reciprocal_evolution_chooses_second_real_role_and_reaches_edit_focus(){
        let row=|role:&str,path:&str|json!({
            "role":role,"domain":"improver","path":path,
            "sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "attempts":0,"verifiedRedesigns":0
        });
        let frontier=vec![
            row("harness","src/engine/jobs.rs"),
            row("improvement_methods","src/engine/rsi_meta_controller.rs"),
            row("memory","src/engine/memory.rs")
        ];
        let plan=synthesize(&[],&frontier,&["improver"],None);
        assert!(plan["reciprocalExperiment"]["partner"].is_object());
        assert_ne!(plan["target"]["role"],
            plan["reciprocalExperiment"]["partner"]["role"]);
        let mut agenda=json!({"target":plan["target"],
            "frontier":frontier,"focus":[plan["target"]],"inheritedStage":null});
        attach_reciprocal_focus(&mut agenda,&plan);
        assert_eq!(agenda["focus"].as_array().unwrap().len(),2);
        assert_eq!(agenda["focus"][1],plan["reciprocalExperiment"]["partner"]);
        agenda["inheritedStage"]=json!({"source":"model_authored_stage"});
        agenda["focus"]=json!([plan["target"]]);
        attach_reciprocal_focus(&mut agenda,&plan);
        assert_eq!(agenda["focus"].as_array().unwrap().len(),1);
    }
    #[test]
    fn reciprocal_evolution_uses_only_real_adopted_cross_role_descendants(){
        let adopted=|id:&str,base:&str,commit:String,role:&str|json!({
            "jobId":id,"generatorCommit":base,
            "accepted":true,"pairedRevalidated":true,"selfControllerVerified":true,
            "evaluation":{"metrics":{"eligible":true}},
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":commit,
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":base,"generationJobId":id,
                "component":"system","components":[role],
                "targetFiles":["src/engine/jobs.rs"]},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"b".repeat(64)}
        });
        let a="a".repeat(40);let b="b".repeat(40);
        let c="c".repeat(40);let d="d".repeat(40);
        let history=vec![
            adopted("meta-start","original",a.clone(),"improvement_methods"),
            adopted("harness-next",&a,b,"harness"),
            adopted("harness-start","independent",c.clone(),"harness"),
            adopted("meta-next",&c,d,"improvement_methods")
        ];
        let role=|role:&str|json!({"role":role,"domain":"improver",
            "path":if role=="harness"{"src/engine/jobs.rs"}
                else{"src/engine/rsi_meta_controller.rs"},
            "sourceSha256":"a".repeat(64),"sourceBytes":100});
        let primary=role("harness");
        let experiment=reciprocal_evolution_contract(
            &history,&primary,&[primary.clone(),role("improvement_methods")]);
        assert_eq!(experiment["observedForwardTransfers"],1);
        assert_eq!(experiment["observedReverseTransfers"],1);
        assert_eq!(experiment["observedJointAdoptions"],0);
        assert_eq!(experiment["twoWayTemporalEvidence"],true);
        assert_eq!(experiment["causalImprovementProven"],false);
        let mut forged=history;
        forged[3]["pairedRevalidated"]=json!(false);
        let weakened=reciprocal_evolution_contract(
            &forged,&primary,&[primary.clone(),role("improvement_methods")]);
        assert_eq!(weakened["observedForwardTransfers"],0);
        assert_eq!(weakened["observedReverseTransfers"],1);
    }
    #[test]
    fn source_mapped_holdout_edges_separate_isolated_and_joint_changes(){
        let single=json!({"jobId":"j","generatorCommit":"base",
            "accepted":true,"pairedRevalidated":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":"a".repeat(40),
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"base","generationJobId":"j",
                "component":"harness","targetFiles":["src/engine/jobs.rs"]},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"a".repeat(64)},
            "evaluation":{"metrics":{"eligible":true,"holdoutEvidence":[
                {"domain":"memory","baselinePass":false,"candidatePass":true,
                    "metricGains":0,"metricRegressions":0}]}}});
        let edges=measured_interaction_edges(&[single.clone()]);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].0,1);
        let mut joint=single;
        joint["systemDesign"]["component"]="system".into();
        joint["systemDesign"]["components"]=json!(["harness","learning"]);
        joint["systemDesign"]["componentFiles"]=json!({
            "harness":["src/engine/jobs.rs"],
            "learning":["src/engine/learning.rs"]});
        joint["systemDesign"]["targetFiles"]=json!([
            "src/engine/jobs.rs","src/engine/learning.rs"]);
        joint["acceptedSourceHashes"]["src/engine/learning.rs"]=
            json!("b".repeat(64));
        let edges=measured_interaction_edges(&[joint]);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].0,0);
        assert_eq!(edges[&("harness".to_string(),"memory".to_string())].2,1);
    }
    #[test]
    fn reverse_redesign_requires_both_accepted_component_source_receipts(){
        let source=|role:&str,path:&str|json!({
            "role":role,"path":path,"domain":"improver",
            "sourceSha256":"c".repeat(64),"sourceBytes":100,
            "verifiedRedesigns":0});
        let harness=source("harness","src/engine/jobs.rs");
        let learning=source("learning","src/engine/learning.rs");
        let ancestor=json!({
            "jobId":"j","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"accepted":true,
            "pairedRevalidated":true,"selfControllerVerified":true,
            "candidateOrigin":"model_driven_detached_worktree",
            "evaluation":{"metrics":{"eligible":true}},
            "systemAgenda":{"reciprocalExperiment":{
                "source":"compiled_descendant_reciprocal_evolution_v1",
                "primary":harness,"partner":learning}},
            "systemDesign":{"source":"model_authored_system_design",
                "component":"system","components":["harness","learning"],
                "generatorCommit":"a".repeat(40),"generationJobId":"j",
                "targetFiles":["src/engine/jobs.rs","src/engine/learning.rs"],
                "componentFiles":{"harness":["src/engine/jobs.rs"],
                    "learning":["src/engine/learning.rs"]}},
            "acceptedSourceHashes":{"src/engine/jobs.rs":"c".repeat(64),
                "src/engine/learning.rs":"d".repeat(64)}
        });
        let frontier=vec![source("harness","src/engine/jobs.rs"),
            source("learning","src/engine/learning.rs")];
        let result=adopted_reciprocal_reversal(&[ancestor.clone()],&frontier);
        assert_eq!(result.unwrap().0["role"],"learning");
        let mut invalid=ancestor;
        invalid["acceptedSourceHashes"]["src/engine/learning.rs"]=Value::Null;
        assert!(adopted_reciprocal_reversal(&[invalid],&frontier).is_none());
    }
    #[test]
    fn cross_system_influence_model_is_revision_bound_and_unproven(){
        let empty=role_influence_model(&[]);
        assert_eq!(empty["edges"],json!([]));
        assert_eq!(empty["causalLoopProven"],false);
        assert_eq!(empty["sha256"].as_str().unwrap().len(),64);
        let plan=synthesize(&[],&[json!({"role":"harness",
            "domain":"context","path":"src/engine/jobs.rs",
            "sourceSha256":"a".repeat(64),"sourceBytes":800})],
            &["context"],None);
        assert_eq!(plan["interactionDynamics"]["sha256"],empty["sha256"]);
        assert_eq!(plan["interactionDynamics"]["rewardOrigin"],
            "independently_frozen_paired_holdouts");
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

    #[test]
    fn accepted_meta_stage_rewrite_is_counted_but_test_only_rewrite_is_not() {
        let mut generation=json!({
            "accepted":true,"pairedRevalidated":true,
            "evaluation":{"metrics":{"eligible":true}},
            "candidateOrigin":"model_driven_detached_worktree",
            "acceptedCommit":"a".repeat(40),
            "generatorCommit":"b".repeat(40),"jobId":"child",
            "systemDesign":{
                "source":"model_authored_system_design",
                "generatorCommit":"b".repeat(40),"generationJobId":"child",
                "component":"improvement_methods",
                "targetFiles":["src/engine/rsi/meta_controller/selection.rs"]
            },
            "acceptedSourceHashes":{
                "src/engine/rsi/meta_controller/selection.rs":"c".repeat(64)
            }
        });
        assert!(meta_source_changed(&generation));
        generation["systemDesign"]["targetFiles"]=json!(["src/engine/rsi/meta_controller/tests.rs"]);
        generation["acceptedSourceHashes"]=json!({
            "src/engine/rsi/meta_controller/tests.rs":"c".repeat(64)
        });
        assert!(!meta_source_changed(&generation));
        generation["accepted"]=json!(false);
        assert!(!meta_source_changed(&generation));
    }
