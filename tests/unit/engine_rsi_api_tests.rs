use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn state() -> (AppState, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-rsi-{}", uuid::Uuid::new_v4()));
        let config = Config { port:0, data_dir:root.clone(), web_dir:root.join("web"), workspace:root.clone() };
        let state = AppState::new(config.clone(), Store::open(&config.db_path()).unwrap(), Vault::open(&config.key_path()));
        (state,root)
    }
    fn base() -> String { "a".repeat(40) }
    fn patch() -> String {
        "diff --git a/src/engine/memory.rs b/src/engine/memory.rs\n--- a/src/engine/memory.rs\n+++ b/src/engine/memory.rs\n@@ -1 +1 @@\n-old\n+new\n".into()
    }
    fn cases(domain: &str) -> Vec<Value> {
        (0..8).map(|i|json!({"id":format!("case-{i}"),"domain":if i==1 {"other"} else {domain},"holdout":i<3,
            "filter":format!("engine::rsi::tests::case_{i}")})).collect()
    }
    fn proposal(target: &str) -> Value {
        // A self-improver hypothesis must actually edit the improver.
        let diff=if target=="improver"{
            patch().replace("src/engine/memory.rs","src/engine/rsi_evolve.rs")
        }else{patch()};
        json!({"target":target,"baseCommit":base(),"patch":diff,
            "hypothesis":"반복되는 개선 후보를 보다 정확하게 평가하도록 변경한다.",
            "cases":cases("memory"),"oracleVersion":"frozen-v1","generatorModel":"manual"})
    }
    fn report(record: &Value) -> Value {
        json!({"baseCommit":record["baseCommit"],"patchSha256":record["patchSha256"],
            "oracleVersion":"frozen-v1","runner":"local-worktree","reportSha256":"b".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,
            "cases":(0..8).map(|i|json!({"id":format!("case-{i}"),"baselinePass":i!=0,"candidatePass":true})).collect::<Vec<_>>()})
    }
    #[test]
    fn train_only_improvement_cannot_claim_holdout_generalization() {
        let plan=(0..8).map(|i|json!({
            "id":format!("case-{i}"),"domain":"memory",
            "holdout":i<2
        })).collect::<Vec<_>>();
        let measured=(0..8).map(|i|json!({
            "id":format!("case-{i}"),
            "baselinePass":i!=3,"candidatePass":true
        })).collect::<Vec<_>>();
        let metrics=evaluate_cases(&plan,&measured,"memory").unwrap();
        assert_eq!(metrics["gains"],1);
        assert_eq!(metrics["improvedHoldouts"],0);
        assert_eq!(metrics["eligible"],false);
        assert_eq!(metrics["verdict"],"inconclusive");
        let mut claimed=metrics;
        claimed["improvedHoldoutDomains"]=json!(["memory"]);
        assert!(verified_holdout_domains(&claimed).is_none());
    }

    #[test]
    fn recursive_proof_needs_real_meta_transfer_not_three_adoption_flags() {
        let (state,root)=state();
        let revisions=["a".repeat(40),"b".repeat(40),"c".repeat(40),"d".repeat(40)];
        let mut history=Vec::new();
        for i in 0..3 {
            let id=format!("proof-{i}");
            let improvement_domain=if i==0 {"improver"} else if i==1 {"memory"} else {"context"};
            let target=if i==0 {"improver"} else {"harness"};
            let plan=(0..8).map(|n|json!({
                "id":format!("case-{n}"),"domain":if n==1 {"control"} else {improvement_domain},
                "holdout":n<3,
                "metrics":if i==0 && n==0 {json!([{"key":"quality","direction":"higher",
                    "minRelativeGain":0.05,"minAbsoluteGain":0.0,"maxRelativeRegression":0.0}])}
                    else {json!([])}
            })).collect::<Vec<_>>();
            let raw=(0..8).map(|n|json!({
                "id":format!("case-{n}"),"baselinePass":i==0 || n!=0,
                "candidatePass":true,
                "metrics":if i==0 && n==0 {
                    json!({"quality":{"baseline":0.4,"candidate":0.8}})
                } else {json!({})}
            })).collect::<Vec<_>>();
            let metrics=evaluate_cases(&plan,&raw,target).unwrap();
            assert_eq!(metrics["eligible"],true);
            let evidence_sha="e".repeat(64);
            let report=json!({"baseCommit":revisions[i],"patchSha256":"p".repeat(64),
                "oracleVersion":"frozen-v1","reportSha256":evidence_sha,
                "baselineBuildOk":true,"candidateBuildOk":true,
                "cases":raw});
            let candidate=json!({
                "id":id,"status":"verified","target":target,"cases":plan,
                "baseCommit":revisions[i],"patchSha256":"p".repeat(64),
                "oracleVersion":"frozen-v1",
                "experimentAdoption":{"commit":revisions[i+1]},
                "evaluation":{"source":"server_invoked_local_paired_git_worktrees",
                    "evidenceSha256":evidence_sha,"verdict":metrics["verdict"],
                    "metrics":metrics}
            });
            state.store.put("rsi_candidate",&candidate).unwrap();
            state.store.put("rsi_pair",&json!({
                "id":format!("rsi-pair-{id}"),"candidateId":id,
                "runner":"local_gitrepo_not_os_sandbox",
                "evidenceSha256":evidence_sha,"report":report,
                "verdict":metrics["verdict"],"metrics":metrics
            })).unwrap();
            history.push(json!({"accepted":true,"candidateId":id,
                "generatorCommit":revisions[i],"acceptedCommit":revisions[i+1],
                "evaluation":{"evidenceSha256":"e".repeat(64)}}));
        }
        assert!(capability_proven(&state,&history));
        let mut broken=history.clone();
        broken[1]=json!({"accepted":false});
        assert!(!capability_proven(&state,&broken),
            "failed research generation must break recursive capability proof");
        let mut forged=state.store.get("rsi_candidate","proof-1").unwrap();
        forged["evaluation"]["metrics"]["improvedHoldoutDomains"]=json!(["tools"]);
        state.store.put("rsi_candidate",&forged).unwrap();
        assert!(!capability_proven(&state,&history),
            "summary domain cannot override concrete frozen holdout measurements");
        forged["evaluation"]["metrics"]["improvedHoldoutDomains"]=json!(["memory"]);
        forged["evaluation"]["metrics"]["improvedHoldouts"]=json!(2);
        state.store.put("rsi_candidate",&forged).unwrap();
        assert!(!capability_proven(&state,&history),
            "summary improvement count must match actual measured rows");
        forged["evaluation"]["metrics"]["improvedHoldouts"]=json!(1);
        forged["evaluation"]["metrics"]["measurements"][0]["domain"]=json!("tools");
        state.store.put("rsi_candidate",&forged).unwrap();
        assert!(!capability_proven(&state,&history),
            "holdout domain receipts must be consistent with their published summary");
        forged["evaluation"]["metrics"]["measurements"][0]["domain"]=json!("memory");
        state.store.put("rsi_candidate",&forged).unwrap();
        assert!(capability_proven(&state,&history));
        let mut pair=state.store.get("rsi_pair","rsi-pair-proof-1").unwrap();
        pair["report"]["cases"][0]["candidatePass"]=json!(false);
        state.store.put("rsi_pair",&pair).unwrap();
        assert!(!capability_proven(&state,&history),
            "modified raw measurements cannot support a previously accepted improvement");
        pair["report"]["cases"][0]["candidatePass"]=json!(true);
        state.store.put("rsi_pair",&pair).unwrap();
        assert!(capability_proven(&state,&history));
        pair["candidateId"]=json!("proof-2");
        state.store.put("rsi_pair",&pair).unwrap();
        assert!(!capability_proven(&state,&history),
            "a candidate cannot borrow another candidate's receipt");
        let mut mislabeled=state.store.get("rsi_candidate","proof-0").unwrap();
        mislabeled["evaluation"]["metrics"]["improvedHoldoutDomains"]=json!(["memory"]);
        state.store.put("rsi_candidate",&mislabeled).unwrap();
        assert!(!capability_proven(&state,&history),
            "a meta improvement without an improver holdout is insufficient");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn paired_results_cannot_be_approved_without_evidence_and_frozen_revision() {
        let (state,root)=state();
        let record=propose(&state,&proposal("improver")).unwrap();
        let id=record["id"].as_str().unwrap();
        assert_eq!(propose(&state,&proposal("improver")).unwrap()["id"],id);
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE"})).is_err());
        let mut result=report(&record);
        result["patchSha256"]=json!("changed");
        assert!(evaluate(&state,id,&result).is_err());
        let evaluated=evaluate(&state,id,&report(&record)).unwrap();
        assert_eq!(evaluated["status"],"verified");
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE","baseCommit":base(),"patchSha256":"wrong"})).is_err());
        assert_eq!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE",
            "baseCommit":base(),"patchSha256":record["patchSha256"],"operator":"owner"})).unwrap()["status"],"approved_for_merge");
        assert_eq!(state.store.all("rsi_evaluation").len(),1);
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn evaluation_cannot_switch_its_frozen_oracle_or_test_names() {
        let (state,root)=state();
        let mut bad=proposal("algorithm");
        bad["cases"][0]["filter"]=json!("not-a-full-test-name%");
        assert!(propose(&state,&bad).is_err());
        let record=propose(&state,&proposal("algorithm")).unwrap();
        let mut result=report(&record);
        result["oracleVersion"]=json!("a-different-test");
        assert!(evaluate(&state,record["id"].as_str().unwrap(),&result).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn holdout_regressions_and_unmatched_cases_never_promote() {
        let (state,root)=state();
        let mut p=proposal("harness");
        p["patch"]=json!(patch().replace("new","alternate"));
        let record=propose(&state,&p).unwrap();
        let mut result=report(&record);
        result["cases"][1]["candidatePass"]=json!(false);
        let id=record["id"].as_str().unwrap();
        assert_eq!(evaluate(&state,id,&result).unwrap()["status"],"rejected");
        assert!(decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE"})).is_err());
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn proposal_rejects_unsafe_paths_and_incomplete_improver_evidence() {
        let (state,root)=state();
        let mut p=proposal("algorithm");
        p["patch"]=json!("diff --git a/.github/workflows/ci.yml b/.github/workflows/ci.yml\n@@ -1 +1 @@\n-old\n+new\n");
        assert!(propose(&state,&p).is_err());
        let mut p=proposal("algorithm");
        p["patch"]=json!(patch().replace("src/engine/memory.rs","src/../secrets.env"));
        assert!(propose(&state,&p).is_err());
        let mut p=proposal("improver");
        p["cases"]=json!((0..8).map(|i|json!({"id":format!("case-{i}"),"domain":"only-one","holdout":i<3,"filter":format!("engine::rsi::tests::case_{i}")})).collect::<Vec<_>>());
        let candidate=propose(&state,&p).unwrap();
        assert_eq!(evaluate(&state,candidate["id"].as_str().unwrap(),&report(&candidate)).unwrap()["status"],"inconclusive");
        let _=std::fs::remove_dir_all(root);
    }
    #[test]
    fn only_a_deployed_improver_can_seed_the_next_generation() {
        let (state,root)=state();
        let parent=propose(&state,&proposal("improver")).unwrap();
        let id=parent["id"].as_str().unwrap();
        evaluate(&state,id,&report(&parent)).unwrap();
        decide(&state,id,&json!({"action":"approve","confirmation":"APPROVE",
            "baseCommit":base(),"patchSha256":parent["patchSha256"]})).unwrap();
        let mut child=proposal("improver");
        child["patch"]=json!(patch().replace("+new","+newer"));
        child["parentCandidateId"]=parent["id"].clone();
        assert!(propose(&state,&child).is_err(), "approval is not deployment");
        let deployed = "b".repeat(40);
        assert_eq!(confirm_deployment(&state,id,&json!({
            "commitSha":deployed,"operator":"owner","confirmation":"DEPLOYED"
        })).unwrap()["status"],"reported_deployed");
        child["generatorSourceCommit"]=json!(deployed);
        assert_eq!(propose(&state,&child).unwrap()["lineageGeneration"],2);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn all_passed_guards_can_promote_a_real_metric_gain_and_freeze_its_plan(){
        let(state,root)=state();let mut p=proposal("harness");
        p["cases"][0]["metrics"]=json!([{"key":"quality","direction":"higher","minRelativeGain":0.1}]);
        let candidate=propose(&state,&p).unwrap();
        let mut measured=report(&candidate);
        for row in measured["cases"].as_array_mut().unwrap(){row["baselinePass"]=json!(true);}
        measured["cases"][0]["metrics"]=json!({"quality":{"baseline":0.7,"candidate":0.9}});
        let result=evaluate(&state,candidate["id"].as_str().unwrap(),&measured).unwrap();
        assert_eq!(result["status"],"verified");assert_eq!(result["evaluation"]["metrics"]["gains"],0);
        assert_eq!(result["evaluation"]["metrics"]["metricGains"],1);
        p["cases"][0]["metrics"][0]["minRelativeGain"]=json!(0.01);
        assert_ne!(propose(&state,&p).unwrap()["id"],candidate["id"],"a different oracle cannot reuse a verified candidate identity");
        let _=std::fs::remove_dir_all(root);
    }
