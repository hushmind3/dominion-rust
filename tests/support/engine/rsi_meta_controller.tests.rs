#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_holdout_loss_changes_the_descendant_search_prior() {
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":1000}),
            json!({"role":"tools","domain":"tools",
                "path":"src/engine/tools.rs","sourceBytes":1000})
        ];
        let baseline=synthesize(&[],&frontier,&["memory","tools"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let rejected=json!({
            "id":"evaluated","jobId":"job-evaluated","candidateId":"candidate-a",
            "generatorCommit":"a".repeat(40),
            "pairedMeasured":true,"pairedRevalidated":false,"accepted":false,
            "evaluationProfileSha256":"e".repeat(64),
            "metaPolicy":{"revision":"d".repeat(64),"target":{"role":"memory"}},
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":"a".repeat(40),"metaRevision":"d".repeat(64)},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":"a".repeat(40),"generationJobId":"job-evaluated",
                "component":"memory","targetFiles":["src/engine/memory.rs"],
                "hypothesis":"Rewrite the memory retrieval algorithm to remove downstream errors"},
            "changedSourcePaths":["src/engine/memory.rs"],
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":false,"verdict":"regressed",
                "regressions":1,"measurements":[
                    {"domain":"tools","holdout":true,"baselinePass":true,
                        "candidatePass":false,"metrics":[]},
                    {"domain":"recall","holdout":true,"baselinePass":true,
                        "candidatePass":false,"metrics":[]},
                    {"domain":"tools","holdout":false,"baselinePass":true,
                        "candidatePass":false,"metrics":[]}
                ]}}
        });
        let after=synthesize(&[rejected],&frontier,&["memory","tools"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let memory=after["alternatives"].as_array().unwrap().iter()
            .find(|row|row["target"]["role"]=="memory").unwrap();
        let evidence=&memory["crossDomainEvidence"];
        assert_eq!(evidence["observedCases"],1);
        assert_eq!(evidence["observedRegressions"],1);
        assert!(memory["crossDomainGeneralizationAdjustment"]
            .as_f64().unwrap()<0.0);
        assert!(valid_policy(&after,1));
        assert!(valid_policy(&baseline,0));
    }
    #[test]
    fn joint_and_unpaired_source_changes_cannot_claim_generalization() {
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":10}),
            json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceBytes":10})
        ];
        let observed=json!({
            "candidateId":"joint","generatorCommit":"b".repeat(40),
            "pairedMeasured":true,
            "changedSourcePaths":["src/engine/memory.rs","src/engine/learning.rs"],
            "evaluation":{"metrics":{"eligible":false,"verdict":"regressed",
                "measurements":[{"domain":"tools","holdout":true,
                    "baselinePass":true,"candidatePass":false}]}}
        });
        let confounded=generalization_evidence(
            &[observed.clone()],&frontier,"memory","src/engine/memory.rs","memory");
        assert_eq!(confounded["observedCases"],0);
        let mut unpaired=observed;
        unpaired["changedSourcePaths"]=json!(["src/engine/memory.rs"]);
        unpaired["pairedMeasured"]=json!(false);
        let untrusted=generalization_evidence(
            &[unpaired],&frontier,"memory","src/engine/memory.rs","memory");
        assert_eq!(untrusted["observedCases"],0);
    }
    #[test]
    fn independent_task_forecast_error_targets_its_own_real_source() {
        let mut history=Vec::new();
        for i in 0..4 {
            let revision="a".repeat(64);
            history.push(json!({
                "id":format!("prequential-{i}"),
                "jobId":format!("job-prequential-{i}"),
                "candidateId":format!("forecast-{i}"),
                "generatorCommit":"b".repeat(40),
                "evaluationProfileSha256":"c".repeat(64),
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"b".repeat(40),
                    "generationJobId":format!("job-prequential-{i}"),
                    "component":"memory","targetFiles":["src/engine/memory.rs"],
                    "hypothesis":"A measured source-level repair should improve heldout task quality"},
                "pairedMeasured":true,"accepted":false,
                "changedSourcePaths":["src/engine/memory.rs"],
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                    "metrics":{"eligible":false,"verdict":"measured",
                        "measurements":[{"id":format!("heldout-{i}"),
                            "domain":"memory","holdout":true,
                            "baselinePass":false,"candidatePass":false}]}},
                "metaPolicy":{"revision":revision,
                    "target":{"role":"memory"},
                    "forecast":{"taskCaseGainProbability":0.95}},
                "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                    "generatorCommit":"b".repeat(40),"metaRevision":revision}
            }));
        }
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":50,
                "sourceSha256":"e".repeat(64)}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi/meta/controller_selection.rs",
                "sourceSha256":"f".repeat(64),"sourceBytes":100})
        ];
        let base=synthesize(&history,&frontier,&["memory","improver"],
            None,&json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&base,history.len()));
        let repaired=steer_quality_predictor_repair(
            base,&history,&frontier,&["memory","improver"]);
        assert_eq!(repaired["selectionMode"],"prequential_task_quality_predictor_repair");
        assert_eq!(repaired["target"]["path"],"src/engine/rsi/meta/controller_selection.rs");
        assert_eq!(repaired["qualityCalibration"]["observations"],4);
        assert_eq!(repaired["qualityCalibration"]["measuredCandidates"],4);
        assert_eq!(repaired["qualityCalibration"]["independentMetaABProven"],false);
        assert!(valid_policy(&repaired,history.len()));
        history[0]["pairedMeasured"]=json!(false);
        let insufficient=quality_forecast_calibration(&history);
        assert_eq!(insufficient["needsPredictorSourceRevision"],false);
        let mut correlated=history[1].clone();
        correlated["evaluation"]["metrics"]["measurements"]=json!([
            {"id":"one","domain":"memory","holdout":true,
                "baselinePass":false,"candidatePass":false},
            {"id":"two","domain":"memory","holdout":true,
                "baselinePass":false,"candidatePass":false},
            {"id":"three","domain":"memory","holdout":true,
                "baselinePass":false,"candidatePass":false},
            {"id":"four","domain":"memory","holdout":true,
                "baselinePass":false,"candidatePass":false}
        ]);
        let same_candidate=quality_forecast_calibration(&[correlated]);
        assert_eq!(same_candidate["observations"],4);
        assert_eq!(same_candidate["measuredCandidates"],1);
        assert_eq!(same_candidate["needsPredictorSourceRevision"],false,
            "four cases from one patch are not four independent policy failures");
    }
    #[test]
    fn failed_unmeasured_edits_do_not_train_fake_success_rates() {
        let frontier=vec![json!({"role":"improvement_methods","domain":"improver",
            "path":"src/engine/rsi_meta_controller.rs","sourceBytes":1200,"verifiedRoleChanged":false})];
        let history=vec![json!({"id":"failed","metaPolicy":{"target":{"role":"improvement_methods"}},
            "accepted":false,"error":"unfinished","candidateId":null})];
        let policy=synthesize(&history,&frontier,&["improver"],None,
            &json!({"acceptedDesigns":[]}),&json!({"references":[]}),
            &json!({"edges":[]}));
        assert!(valid_policy(&policy,history.len()));
        let mut forged=policy.clone();
        forged["forecast"]["probability"]=json!(0.99);
        assert!(!valid_policy(&forged,history.len()),
            "an old supervisor cannot rewrite a compiled descendant forecast");
        let mut diverted=policy.clone();
        diverted["target"]["path"]=json!("src/engine/algorithms.rs");
        assert!(!valid_policy(&diverted,history.len()),
            "a stale controller cannot silently swap the next source target");
        assert_eq!(policy["prediction"]["observedTrials"],0);
        let observation=feedback(&policy,&history[0]);
        assert_eq!(observation["independentlyMeasured"],false);
        assert!(observation["brierError"].is_null());
        let mut measured=history[0].clone();
        measured["evaluation"]["metrics"]["eligible"]=json!(false);
        measured["evaluation"]["metrics"]["verdict"]=json!("regressed");
        measured["pairedMeasured"]=json!(true);
        measured["pairedRevalidated"]=json!(false);
        let measured_feedback=feedback(&policy,&measured);
        assert_eq!(measured_feedback["independentObservation"],false);
        assert!(measured_feedback["brierError"].is_number());
    }
    #[test]
    fn independently_replayed_rejection_lowers_next_success_forecast() {
        let frontier=vec![json!({"role":"memory","domain":"memory",
            "path":"src/engine/memory.rs","sourceBytes":900})];
        let base=synthesize(&[],&frontier,&["memory"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        let failed=json!({"metaPolicy":{"target":{"role":"memory"}},
            "pairedMeasured":true,"pairedRevalidated":false,
            "accepted":false,"candidateId":"replayed-rejected-source",
            "evaluation":{"metrics":{"eligible":false,"verdict":"regressed",
                "regressions":1}}});
        let updated=synthesize(&[failed.clone()],&frontier,&["memory"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert_eq!(updated["prediction"]["observedTrials"],1);
        assert!(updated["forecast"]["probability"].as_f64().unwrap()
            <base["forecast"]["probability"].as_f64().unwrap());
        let feedback=feedback(&updated,&failed);
        assert_eq!(feedback["independentObservation"],false);
        assert!(feedback["brierError"].as_f64().is_some_and(|e|e>0.0));
    }
    #[test]
    fn demonstrated_bad_meta_calibration_reassigns_the_real_learning_source(){
        let frontier=vec![
            json!({"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceBytes":100}),
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_principle_learning.rs",
                "sourceSha256":"a".repeat(64),"sourceBytes":1500})
        ];
        let base=synthesize(&[],&frontier,&["memory","improver"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&base,0));
        let ledger=json!({"sha256":"b".repeat(64),
            "predictiveCalibration":{"needsLearnerSourceRevision":true,
                "measuredCases":4,"meanBrierError":0.81}});
        let revised=steer_learner_repair(base.clone(),&ledger,&frontier,
            &["memory","improver"]);
        assert_eq!(revised["selectionMode"],
            "verified_calibration_driven_learner_rewrite");
        assert_eq!(revised["target"]["path"],
            "src/engine/rsi_principle_learning.rs");
        assert_eq!(revised["principleLearningSha256"],ledger["sha256"]);
        assert!(valid_policy(&revised,0));
        let unchanged=steer_learner_repair(base.clone(),
            &json!({"predictiveCalibration":{"needsLearnerSourceRevision":false}}),
            &frontier,&["memory","improver"]);
        assert_eq!(unchanged,base);
    }
    #[test]
    fn productive_descendant_evidence_changes_target_ranking() {
        let frontier=vec![
            json!({"role":"improvement_methods","domain":"improver",
                "path":"src/engine/rsi_meta_controller.rs","sourceBytes":1000,"verifiedRoleChanged":true}),
            json!({"role":"learning","domain":"learning",
                "path":"src/engine/learning.rs","sourceBytes":1000,"verifiedRoleChanged":true})
        ];
        let before=synthesize(&[],&frontier,&["improver","learning"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(valid_policy(&before,0));
        let eligible=json!({"metrics":{"eligible":true,"verdict":"improved"}});
        let parent=json!({"id":"p","generatorCommit":"a".repeat(40),
            "acceptedCommit":"b".repeat(40),"accepted":true,
            "pairedMeasured":true,"pairedRevalidated":true,
            "metaPolicy":{"target":{"role":"learning"}},
            "evaluation":eligible,"candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "targetFiles":["src/engine/learning.rs"]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"c".repeat(64)}});
        let child=json!({"id":"c","generatorCommit":"b".repeat(40),
            "acceptedCommit":"d".repeat(40),"accepted":true,
            "pairedMeasured":true,"pairedRevalidated":true,
            "controllerPlan":{"source":"compiled_descendant_system_controller_v2"},
            "metaPolicy":{"target":{"role":"learning"}},
            "evaluation":eligible,"candidateOrigin":"model_driven_detached_worktree",
            "systemDesign":{"source":"model_authored_system_design",
                "targetFiles":["src/engine/learning.rs"]},
            "acceptedSourceHashes":{"src/engine/learning.rs":"e".repeat(64)}});
        let after=synthesize(&[parent,child],&frontier,&["improver","learning"],None,
            &json!({}),&json!({}),&json!({"edges":[]}));
        assert!(after["alternatives"].as_array().unwrap().iter().any(|v|
            v["role"]=="learning" && v["productiveOffspring"]==1));
    }
}
