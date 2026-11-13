use super::*;
    #[test]
    fn portable_peer_method_changes_executable_recipient_not_adoption_credit(){
        let target=|role:&str,domain:&str,path:&str|json!({
            "role":role,"domain":domain,"path":path,
            "sourceSha256":"a".repeat(64),"sourceBytes":1200
        });
        let a=target("memory","memory","src/engine/memory.rs");
        let b=target("learning","learning","src/engine/learning.rs");
        let original=json!({
            "source":"compiled_editable_meta_controller_v1",
            "revision":"prior","target":a,
            "controlPolicy":{"role":"memory","domain":"memory",
                "path":"src/engine/memory.rs","sourceSha256":"a".repeat(64)},
            "alternatives":[
                {"role":"memory","score":1.0,"target":a,
                    "outcomeForecast":{"adoption":0.5},
                    "predictedAdoptionProbability":0.5,"uncertainty":0.2},
                {"role":"learning","score":0.95,"target":b,
                    "outcomeForecast":{"adoption":0.25},
                    "predictedAdoptionProbability":0.25,"uncertainty":0.3}
            ],
            "forecast":{"acceptanceProbability":0.5,"uncertainty":0.2}
        });
        let peer=json!({
            "source":"paired_cross_campaign_reference_archive_v1",
            "automaticPatchReuse":false,
            "comparisonProfileSha256":"f".repeat(64),
            "references":[{
                "source":"paired_cross_campaign_design_reference_v1",
                "automaticPatchReuse":false,
                "comparisonProfileSha256":"f".repeat(64),
                "campaignId":"other","candidateId":"verified-1",
                "acceptedCommit":"b".repeat(40),
                "patchSha256":"c".repeat(64),
                "nextDomain":"learning","roles":["memory"],
                "identicalSourceFiles":1,
                "alignedSourceFiles":["src/engine/memory.rs"]
            }]
        });
        let evolved=evolve_from_peer_methods(original.clone(),&peer,&[]);
        assert_eq!(evolved["target"]["role"],"learning");
        assert_eq!(evolved["controlPolicy"]["role"],"learning");
        assert_eq!(evolved["controlPolicy"]["path"],"src/engine/learning.rs");
        assert_eq!(evolved["forecast"]["acceptanceProbability"],0.25);
        assert_eq!(evolved["taskOutcomeForecast"]["adoption"],0.25);
        assert_eq!(evolved["peerMethodTransfer"]["active"],true);
        assert_eq!(evolved["peerMethodTransfer"]["automaticallyApplied"],false);
        assert_eq!(evolved["peerMethodTransfer"]["transferProven"],false);
        let observation=feedback(&evolved,&json!({
            "metaPolicyExecuted":true,
            "comparisonProfileSha256":"f".repeat(64),
            "metaPolicy":evolved.clone(),
            "focusFiles":["src/engine/learning.rs"],
            "candidateId":"recipient-1",
            "accepted":false,
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}}
        }));
        assert_eq!(observation["portableMethodObservation"]
            ["sourceFocusExecuted"],true);
        assert_eq!(observation["portableMethodObservation"]
            ["recipientSourceIndependentlyAdopted"],false);
        assert_eq!(observation["portableMethodObservation"]
            ["portableMethodSuperiorityProven"],false);
        assert!(instruction(&evolved).contains("PORTABLE METHOD HYPOTHESIS"));
        assert!(peer_instruction(&peer,&original).is_empty());
        let single_peer=peer_instruction(&peer,&evolved);
        assert!(single_peer.contains("ISOLATED PEER METHOD REFERENCE"));
        assert!(single_peer.contains("peer=other") || single_peer.contains("donor=other"));
        let mut second_peer=peer.clone();
        second_peer["references"].as_array_mut().unwrap().push(json!({
            "campaignId":"unassigned","candidateId":"other-candidate",
            "comparisonProfileSha256":"f".repeat(64),
            "hypothesis":"must not leak into isolated prompt"
        }));
        assert!(!peer_instruction(&second_peer,&evolved)
            .contains("must not leak into isolated prompt"));
        assert_ne!(evolved["revision"],original["revision"]);
        let mut drift=peer.clone();
        drift["references"][0]["identicalSourceFiles"]=json!(0);
        let ignored=evolve_from_peer_methods(original.clone(),&drift,&[]);
        assert_eq!(ignored["target"],original["target"]);
        let mut wrong_suite=peer.clone();
        wrong_suite["references"][0]["comparisonProfileSha256"]=
            json!("0".repeat(64));
        assert_eq!(evolve_from_peer_methods(original.clone(),&wrong_suite,&[])["target"],
            original["target"]);
        // Four independently assessed non-adoptions lower the donor-method
        // posterior; the child returns to the non-transfer candidate.
        let rejected=(0..4).map(|i|json!({
            "jobId":format!("transfer-{i}"),
            "metaPolicyExecuted":true,
            "comparisonProfileSha256":"f".repeat(64),
            "focusFiles":["src/engine/learning.rs"],
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}},
            "metaPolicy":{
                "target":{"role":"learning","path":"src/engine/learning.rs",
                    "sourceSha256":"a".repeat(64)},
                "peerMethodTransfer":{
                "active":true,"activeDonorCandidateId":"verified-1",
                "comparisonProfileSha256":"f".repeat(64),
                "selectedRecipient":{"role":"learning",
                    "path":"src/engine/learning.rs","sourceSha256":"a".repeat(64)},
                "references":[{"candidateId":"verified-1",
                    "sharedFrozenProfileSha256":"f".repeat(64)}]
            }}
        })).collect::<Vec<_>>();
        let later=evolve_from_peer_methods(original.clone(),&peer,&rejected);
        assert_eq!(later["target"]["role"],"memory");
        let alternative=later["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(alternative["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],4);
        assert_eq!(alternative["transferHypotheses"][0]
            ["recipientSourceAdoptions"],0);
        // An execution referencing two donors may only credit the
        // explicitly assigned one, never each name in the prompt.
        let mut ambiguous=rejected.clone();
        for row in &mut ambiguous{
            row["metaPolicy"]["peerMethodTransfer"]["references"]
                .as_array_mut().unwrap().push(json!({
                    "candidateId":"verified-other",
                    "sharedFrozenProfileSha256":"f".repeat(64)
                }));
            row["metaPolicy"]["peerMethodTransfer"]["activeDonorCandidateId"]=
                json!("verified-other");
        }
        let unmatched=evolve_from_peer_methods(original.clone(),&peer,&ambiguous);
        let unmatched_bridge=unmatched["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(unmatched_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut wrong_source=rejected.clone();
        for row in &mut wrong_source{
            row["metaPolicy"]["target"]["sourceSha256"]=json!("b".repeat(64));
        }
        let source_mismatch=evolve_from_peer_methods(original.clone(),&peer,&wrong_source);
        let source_bridge=source_mismatch["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(source_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut wrong_profile=rejected.clone();
        for row in &mut wrong_profile{
            row["comparisonProfileSha256"]=json!("e".repeat(64));
        }
        let suite_mismatch=evolve_from_peer_methods(original.clone(),&peer,&wrong_profile);
        let suite_bridge=suite_mismatch["alternatives"].as_array().unwrap().iter()
            .find(|row|row["role"]=="learning").unwrap();
        assert_eq!(suite_bridge["transferHypotheses"][0]
            ["independentlyMeasuredTransferTrials"],0);
        let mut failed_receipt=json!({
            "metaPolicyExecuted":true,
            "metaPolicy":evolved.clone(),
            "comparisonProfileSha256":"e".repeat(64),
            "focusFiles":["src/engine/learning.rs"],
            "evaluation":{"verdict":"rejected","metrics":{"eligible":true}}
        });
        assert_eq!(feedback(&evolved,&failed_receipt)["portableMethodObservation"]
            ["independentlyAssessed"],false);
        failed_receipt["comparisonProfileSha256"]=json!("f".repeat(64));
        assert_eq!(feedback(&evolved,&failed_receipt)["portableMethodObservation"]
            ["attributableDonorCandidateId"],"verified-1");
        // Four matched non-transfer adoptions against four donor failures
        // suppress transfer and preserve the negative evidence for method learning.
        let controls=(0..4).map(|i|{
            let job=format!("nontransfer-{i}");
            json!({
                "jobId":job.clone(),"generatorCommit":"d".repeat(40),
                "metaPolicyExecuted":true,
                "comparisonProfileSha256":"f".repeat(64),
                "metaPolicy":{"target":{"role":"learning",
                    "path":"src/engine/learning.rs",
                    "sourceSha256":"a".repeat(64)}},
                "focusFiles":["src/engine/learning.rs"],
                "candidateOrigin":"model_driven_detached_worktree",
                "accepted":true,"pairedRevalidated":true,
                "acceptedCommit":"b".repeat(40),
                "acceptedSourceHashes":{"src/engine/learning.rs":"c".repeat(64)},
                "systemDesign":{"source":"model_authored_system_design",
                    "generatorCommit":"d".repeat(40),
                    "generationJobId":job,"component":"learning",
                    "targetFiles":["src/engine/learning.rs"]},
                "evaluation":{"verdict":"accepted","metrics":{"eligible":true}}
            })
        }).collect::<Vec<_>>();
        assert!(controls.iter().all(independent_adoption));
        let mut matched=rejected.clone();
        matched.extend(controls);
        let suppressed=evolve_from_peer_methods(original.clone(),&peer,&matched);
        assert_eq!(suppressed["target"]["role"],"memory");
        assert_eq!(suppressed["peerMethodLearning"]["observations"][0]
            ["negativeTransferSignal"],true);
        assert!(instruction(&suppressed).contains("OBSERVATIONAL NEGATIVE TRANSFER"));
        assert!(!peer_instruction(&peer,&suppressed).contains("ISOLATED"));
        // Repeated negative transfer now redirects the executable
        // descendant to rewrite the transferable-method selector itself.
        let mut meta_ready=original.clone();
        meta_ready["alternatives"].as_array_mut().unwrap().push(json!({
            "role":"improvement_methods","score":0.40,
            "target":target("improvement_methods","improver",
                "src/engine/rsi_meta_controller/peer_transfer.rs"),
            "outcomeForecast":{"adoption":0.30},
            "predictedAdoptionProbability":0.30,"uncertainty":0.4
        }));
        let method_repair=evolve_from_peer_methods(
            meta_ready.clone(),&peer,&matched);
        assert_eq!(method_repair["mode"],"repair_own_transfer_method");
        assert_eq!(method_repair["metaRepairChosen"],true);
        assert_eq!(method_repair["target"]["path"],
            "src/engine/rsi_meta_controller/peer_transfer.rs");
        assert_eq!(method_repair["controlPolicy"]["path"],
            "src/engine/rsi_meta_controller/peer_transfer.rs");
        assert_eq!(method_repair["peerMethodTransfer"],Value::Null);
        assert!(peer_instruction(&peer,&method_repair).is_empty());
        assert!(instruction(&method_repair).contains(
            "redesign the executable donor-method matching"));
        // A recent pair of real attempts at rewriting this source stops
        // a meta-level feedback loop from consuming the entire campaign.
        let mut cooldown=matched.clone();
        cooldown.extend((0..2).map(|i|json!({
            "jobId":format!("meta-repair-{i}"),
            "focusFiles":["src/engine/rsi_meta_controller/peer_transfer.rs"]
        })));
        let cooled=evolve_from_peer_methods(meta_ready,&peer,&cooldown);
        assert_ne!(cooled["mode"],"repair_own_transfer_method");
        let mut repair=original.clone();
        repair["metaRepairChosen"]=json!(true);
        assert_eq!(evolve_from_peer_methods(repair.clone(),&peer,&[])["target"],
            repair["target"]);
    }

    #[test]
    fn comparison_prioritizes_novelty_then_updates_after_grounded_failures(){
        let target=|role:&str,path:&str|json!({"role":role,"domain":if role=="memory"{"memory"}else{"learning"},
            "path":path,"sourceSha256":"a".repeat(64),"sourceBytes":1200,
            "verifiedRoleImproved":false});
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

