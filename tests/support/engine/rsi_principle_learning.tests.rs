#[cfg(test)]
mod tests {
    use super::*;
    fn trial(id:&str,mechanism:&str,source:&str,target_role:&str,
        before:bool,after:bool,accepted:bool)->Value {
        let mut hashes=serde_json::Map::new();
        hashes.insert(source.into(),json!("b".repeat(64)));
        let generator="a".repeat(40);
        let mut assignments=serde_json::Map::new();
        assignments.insert(target_role.into(),json!([source]));
        let revision="d".repeat(64);
        json!({
            "id":id,"jobId":format!("job-{id}"),"candidateId":format!("candidate-{id}"),
            "generatorCommit":generator,"acceptedCommit":if accepted{json!("c".repeat(40))}else{Value::Null},
            "accepted":accepted,"pairedMeasured":true,"pairedRevalidated":accepted,
            "evaluationProfileSha256":"d".repeat(64),
            "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                "metrics":{"eligible":accepted,"verdict":"paired_measured",
                    "measurements":[{"id":"case-a","domain":"memory","holdout":true,
                        "baselinePass":before,"candidatePass":after}]}},
            "changedSourcePaths":[source],
            "acceptedSourceHashes":Value::Object(hashes),
            "compiledPlanExecuted":{"source":"running_compiled_descendant_binary",
                "generatorCommit":generator,"metaRevision":"m",
                "openImprovementRevision":revision},
            "metaPolicy":{"revision":"m","selectionMode":"authored_open_improvement_program",
                "openImprovementRevision":revision.clone(),
                "target":{"role":target_role,"path":source}},
            "openImprovementProgram":{"source":"compiled_descendant_open_improvement_program_v1",
                "revision":revision,"target":{"role":target_role,"path":source},
                "node":{"mechanism":mechanism}},
            "systemDesign":{"source":"model_authored_system_design",
                "generatorCommit":generator,"generationJobId":format!("job-{id}"),
                "components":[target_role],
                "componentFiles":Value::Object(assignments),
                "targetFiles":[source]}
        })
    }
    #[test]
    fn independent_task_gains_train_priority_while_failed_ideas_lose_rank(){
        let good=trial("good","executable retrieval repair","src/engine/memory.rs",
            "memory",false,true,true);
        let bad=trial("bad","repeated no-op search","src/engine/memory.rs",
            "memory",true,false,false);
        let ledger=study(&[good,bad]);
        assert!(valid(&ledger,&[] )==false);
        let yes=estimate(&ledger,&json!({"mechanism":"executable retrieval repair"}));
        let no=estimate(&ledger,&json!({"mechanism":"repeated no-op search"}));
        assert!(yes["estimatedPriority"].as_f64()>no["estimatedPriority"].as_f64());
        assert_eq!(ledger["principles"].as_array().unwrap().len(),2);
    }
    #[test]
    fn a_different_child_method_can_credit_cross_role_task_improvement(){
        let parent=trial("parent","adapt retrieval by observing independent task failures",
            "src/engine/memory.rs","memory",false,true,true);
        let mut child=trial("child","child uses a different improvement program",
            "src/engine/models.rs","models",false,true,true);
        child["generatorCommit"]=parent["acceptedCommit"].clone();
        child["acceptedCommit"]=json!("e".repeat(40));
        child["compiledPlanExecuted"]["generatorCommit"]=child["generatorCommit"].clone();
        child["systemDesign"]["generatorCommit"]=child["generatorCommit"].clone();
        child["systemDesign"]["components"]=json!(["models"]);
        child["openImprovementProgram"]=Value::Null;
        child["metaPolicy"]=json!({"revision":"m"});
        child["evaluation"]["metrics"]["measurements"][0]["domain"]=json!("learning");
        let history=vec![parent,child];
        let report=study(&history);
        let row=&report["principles"][0];
        assert_eq!(row["mechanism"],
            "adapt retrieval by observing independent task failures");
        assert_eq!(row["nextGenerationCases"],1);
        assert_eq!(row["nextGenerationImprovements"],1);
        assert_eq!(row["descendantRoles"][0],"models");
        let mut altered=history;
        altered[1]["pairedRevalidated"]=json!(false);
        altered[1]["pairedMeasured"]=json!(false);
        assert_eq!(study(&altered)["principles"][0]["nextGenerationImprovements"],0);
    }
    #[test]
    fn accepted_source_without_quality_gain_is_not_a_learned_positive_example(){
        let g=trial("neutral","source edit without task improvement",
            "src/engine/learning.rs","learning",false,false,true);
        let row=study(&[g])["principles"][0].clone();
        assert_eq!(row["improvedCases"],0);
        assert_eq!(row["independentSourceAdoptions"],1);
        assert_eq!(row["posteriorCaseGain"].as_f64(),Some(1.0/3.0));
    }
    #[test]
    fn legacy_model_authored_system_change_teaches_the_next_open_learner(){
        let mut g=trial("legacy","placeholder","src/engine/models.rs",
            "models",false,true,true);
        g["openImprovementProgram"]=Value::Null;
        g["metaPolicy"]=json!({"revision":"m"});
        g["systemDesign"]["hypothesis"]=json!(
            "Replace the model selection and context assembly strategy so the frozen downstream tasks improve");
        g["systemDesign"]["component"]=json!("models");
        let ledger=study(&[g.clone()]);
        assert_eq!(ledger["principles"].as_array().unwrap().len(),1);
        assert_eq!(ledger["principles"][0]["improvedCases"],1);
        g["systemDesign"]["generatorCommit"]=json!("forged-parent");
        assert_eq!(study(&[g])["principles"].as_array().unwrap().len(),0);
    }
    #[test]
    fn two_different_mechanisms_train_a_transferable_factor_for_a_novel_third(){
        let mut first=trial("first-key","repair memory index using task traces",
            "src/engine/memory.rs","memory",false,true,true);
        let mut second=trial("second-key","adapt model context based on source outcomes",
            "src/engine/models.rs","models",false,true,true);
        first["openImprovementProgram"]["node"]["principleKeys"]
            =json!(["evidence_guided_context"]);
        second["openImprovementProgram"]["node"]["principleKeys"]
            =json!(["evidence_guided_context"]);
        second["evaluation"]["metrics"]["measurements"][0]["domain"]
            =json!("learning");
        let bank=study(&[first,second]);
        let factor=&bank["transferFeatures"][0];
        assert_eq!(factor["independentMechanisms"],2);
        assert_eq!(factor["transferEligible"],true);
        assert_eq!(factor["domains"].as_array().unwrap().len(),2);
        let novel=json!({"mechanism":"new adaptive learning algorithm combining two subsystems",
            "principleKeys":["evidence_guided_context"]});
        let general=estimate(&bank,&novel);
        let no_key=estimate(&bank,&json!({"mechanism":"unseen arbitrary change"}));
        assert_eq!(general["learnedEvidenceSource"],
            "cross_mechanism_transferable_principle");
        assert_eq!(general["caseQualityPosterior"].as_f64(),Some(0.75));
        assert!(general["estimatedPriority"].as_f64().unwrap()
            >no_key["estimatedPriority"].as_f64().unwrap());
        assert_eq!(no_key["caseQualityPosterior"].as_f64(),Some(0.5));
    }
    #[test]
    fn bad_heldout_forecasts_trigger_a_source_rewrite_of_the_meta_learner(){
        let samples=(0..3).map(|i|{
            let mut g=trial(&format!("badprediction-{i}"),
                "overconfident domain-specific technique",
                "src/engine/models.rs","models",false,false,false);
            g["openImprovementProgram"]["principleSelectionEvidence"]=json!({
                "caseQualityPosterior":0.95});
            g["openImprovementProgram"]["principleLearningSha256"]
                =json!("f".repeat(64));
            g["principleLearning"]=json!({"sha256":"f".repeat(64)});
            g
        }).collect::<Vec<_>>();
        let ledger=study(&samples);
        assert_eq!(ledger["predictiveCalibration"]["measuredCases"],3);
        assert_eq!(ledger["predictiveCalibration"]["needsLearnerSourceRevision"],true);
        assert!(ledger["predictiveCalibration"]["meanBrierError"]
            .as_f64().is_some_and(|e|e>0.8));
        let mut forged=samples;
        forged[0]["pairedMeasured"]=json!(false);
        assert_eq!(study(&forged)["predictiveCalibration"]["measuredCases"],2);
        assert_eq!(study(&forged)["predictiveCalibration"]["needsLearnerSourceRevision"],false);
    }
    #[test]
    fn model_prose_without_independent_paired_evidence_never_trains_principle(){
        let mut g=trial("false","invented gain","src/engine/memory.rs",
            "memory",false,true,true);
        g["evaluation"]["source"]=json!("model_self_report");
        assert_eq!(study(&[g])["principles"].as_array().unwrap().len(),0);
    }
}
