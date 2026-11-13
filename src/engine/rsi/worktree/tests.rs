    use super::*;
    #[test]
    fn rsi_resolves_cargo_even_with_a_minimal_service_path() {
        let cargo = cargo_executable();
        assert!(
            cargo.is_file() || cargo == Path::new("cargo"),
            "unexpected cargo path: {cargo:?}"
        );
        assert!(!npm_executable().as_os_str().is_empty());
        let node = node_executable();
        assert!(
            node.is_file() || node == Path::new("node"),
            "unexpected node path: {node:?}"
        );
    }
    #[test]
    fn only_known_work_branches_can_receive_a_verified_campaign() {
        for branch in ["dominion-sub", "dominion-sub1", "dominion-sub2",
            "dominion-sub3", "dominion-sub4", "dominion-champion"] {
            assert!(is_work_branch(branch), "{branch}");
        }
        for branch in ["main", "master", "dominion-sub1-other",
            "dominion-sub5", "dominion-champion-old", "refs/heads/main", ""] {
            assert!(!is_work_branch(branch), "{branch}");
        }
    }

    #[test]
    fn later_generations_see_prior_compile_failures_without_fabricated_gains(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!("rsi-feedback-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("job",&json!({"id":"j1","input":{"rsiObjective":"correct Rust type failure"}}));
        state.store.put("rsi_run",&json!({"id":"rsi-run-j1","jobId":"j1",
            "candidateId":"rsi-one","createdAt":"2026-11-09T01:00:00Z",
            "checksPassed":false,"checks":[{"label":"cargo check --all-targets","passed":false}],
            "modelError":"","captureError":""}));
        let prompt=recent_feedback(&state);
        assert!(prompt.contains("correct Rust type failure"));
        assert!(prompt.contains("정적 검사 실패"));
        assert!(prompt.contains("cargo check --all-targets"));
        assert!(!prompt.contains("실제 성능 향상 검증"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn classification_covers_the_improver_harness_and_algorithm(){
        assert_eq!(classify("diff --git a/src/engine/rsi.rs b/src/engine/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/api/rsi.rs b/src/api/rsi.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/rsi_evolve.rs b/src/engine/rsi_evolve.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/room_state.rs b/src/engine/room_state.rs\n"),"algorithm");
        assert_eq!(classify("diff --git a/web/src/pages/Home.tsx b/web/src/pages/Home.tsx\n"),"harness");
        assert_eq!(classify("diff --git a/src/engine/rsi/meta_controller/selection.rs b/src/engine/rsi/meta_controller/selection.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/rsi/system/agenda.rs b/src/engine/rsi/system/agenda.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/rsi_recursive_laboratory.rs b/src/engine/rsi_recursive_laboratory.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/algorithms.rs b/src/engine/algorithms.rs\ndiff --git a/src/engine/rsi/meta_controller/transfer.rs b/src/engine/rsi/meta_controller/transfer.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/algorithms.rs b/src/engine/algorithms.rs\ndiff --git a/web/src/pages/Home.tsx b/web/src/pages/Home.tsx\n"),"harness");
        assert_eq!(classify("diff --git a/src/engine/memory.rs b/src/engine/memory.rs\n"),"algorithm");
        assert_eq!(classify(""),"harness");
    }
    #[test]
    fn failing_verification_never_completes_and_only_valid_failures_retry(){
        assert_eq!(run_status(true,false,false,true,"",""),("needs_review","build_or_typecheck_failed"));
        assert_eq!(run_status(true,false,true,true,"",""),("needs_review","unsafe_safety_failed"));
        assert_eq!(run_status(true,true,false,true,"",""),("completed","candidate_compiles_not_performance_verified"));
        assert_eq!(run_status(true,true,false,false,"","").0,"needs_review");
        assert_eq!(run_status(true,true,false,true,"provider timeout",""),
            ("completed","candidate_compiles_not_performance_verified"),
            "A verifiably compiled source patch must reach paired evaluation despite a late model stream error");
        assert_eq!(run_status(false,true,false,true,"","invalid diff").0,"needs_review");
        let failing=json!({"status":"needs_review","reason":"build_or_typecheck_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"cargo check --all-targets","passed":false,"stderr":"error[E0308]: wrong type"}]}});
        assert!(repair_feedback(&failing).unwrap().contains("error[E0308]"));
        let unsafe_failure=json!({"status":"needs_review","reason":"unsafe_safety_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"unsafe_safety","passed":false,"error":"unsafe 블록에 SAFETY 근거가 없습니다."}]}});
        assert!(repair_feedback(&unsafe_failure).unwrap().contains("SAFETY"));
        let mut late_transport_failure=failing;
        late_transport_failure["rsi"]["modelError"]=json!("provider stream closed after edit");
        assert!(repair_feedback(&late_transport_failure).is_some(),
            "A compiler-rejected real patch must still be repairable after a late stream error");
        assert_eq!(run_status(true,false,false,true,"stream closed",""),
            ("needs_review","build_or_typecheck_failed"));
    }
    #[test]
    fn sub1_compiler_errors_keep_first_location_after_long_cargo_output() {
        let raw=format!(
            "error[E0308]: mismatched types\n --> src/engine/decompose.rs:44:9\n{}\nerror: could not compile",
            "note: dependency warning\n".repeat(350));
        let reduced=compiler_diagnostic_excerpt(&raw);
        assert!(reduced.contains("src/engine/decompose.rs:44:9"));
        assert!(reduced.contains("error: could not compile"));
        assert!(reduced.len()<raw.len());
        assert_eq!(compiler_diagnostic_excerpt("short compiler failure"),"short compiler failure");
    }
    #[test]
    fn compiler_repair_rehydrates_current_source_from_ansi_rustc_location() {
        let root=std::env::temp_dir().join(format!("sub1-compiler-repair-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        let source="fn left() {\n    let x = 2;\n    println!(\"{}\", x);\n}\n";
        std::fs::write(root.join("src/engine/example.rs"),source).unwrap();
        let failure=json!({"rsi":{"checks":[{"passed":false,
            "stderr":"error: expected expression\n\u{1b}[1m  --> \u{1b}[0msrc/engine/example.rs:3:5\n"}]}});
        let repair=compiler_repair_context(&root,&failure).unwrap();
        let digest=format!("{:x}",Sha256::digest(source.as_bytes()));
        assert!(repair.contains("src/engine/example.rs"));
        assert!(repair.contains("    println!(\"{}\", x);"));
        assert!(!repair.contains("3 |     println!"),
            "Repair input must be raw source, never numbered source");
        assert!(repair.contains(&digest));
        assert!(repair.contains("workspace_replace_range"));
        let spoof=json!({"rsi":{"checks":[{"passed":false,
            "stderr":"  --> ../secret.rs:2:1\n"}]}});
        assert!(compiler_repair_context(&root,&spoof).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn requires_complete_sha(){
        assert!(valid_sha("1234567890abcdef1234567890abcdef12345678"));
        assert!(!valid_sha("bad"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn detached_worktree_collects_a_real_patch_without_touching_source(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        use std::process::Command as SyncCommand;
        fn git_local(path:&Path,args:&[&str])->String {
            let out=SyncCommand::new("git").current_dir(path).args(args).output().unwrap();
            assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        let root=std::env::temp_dir().join(format!("rsi-proof-{}",uuid::Uuid::new_v4()));
        let source=root.join("checkout");
        let data=root.join("data");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::create_dir_all(source.join("web/src/pages")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(source.join("Cargo.toml"),"[package]\nname=\"sample\"\nversion=\"0.1.0\"\n").unwrap();
        std::fs::write(source.join("src/engine/rsi.rs"),"// source\n").unwrap();
        std::fs::write(source.join("web/src/pages/Demo.tsx"),"export const version = 1\n").unwrap();
        git_local(&source,&["init","-q"]);
        git_local(&source,&["checkout","-b","dominion-sub"]);
        git_local(&source,&["add","."]);
        git_local(&source,&["-c","user.email=ci@example.com","-c","user.name=CI","commit","-qm","base"]);
        let base=git_local(&source,&["rev-parse","HEAD"]);
        let cfg=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),workspace:data.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source.to_str().unwrap()}));
        let input=json!({
            "rsiBaseCommit":base,"rsiSourceDir":source.to_str().unwrap(),
            "rsiObjective":"DOMINION 화면 렌더링 검증 대상 소스를 수정한다",
            "model":"test-model",
            "rsiFrozenCases":[
                {"id":"memory","domain":"memory","holdout":true,"filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
                {"id":"algorithm","domain":"algorithm","holdout":true,"filter":"engine::algorithms::tests::generation_uses_evidence_and_returns_executable_kernels"}
            ]
        });
        let job=state.store.put("job",&json!({"kind":"prompt","status":"running","input":input})).unwrap();
        let context=JobContext::new(state.clone(),job["id"].as_str().unwrap().to_string());
        let worktree=prepare(&context,&job["input"]).await.unwrap();
        assert_ne!(worktree.path,source);
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        std::fs::write(worktree.path.join("web/src/pages/Demo.tsx"),"export const version = 2\n").unwrap();
        let outcome=finish(&context,&job,&worktree,Ok(Some(json!({"id":"m1"}))),"session-rsi").await.unwrap();
        assert_eq!(outcome["status"],"needs_review");
        assert_eq!(outcome["reason"],"build_or_typecheck_failed");
        let candidate_id=outcome["rsi"]["candidateId"].as_str().unwrap();
        let candidate=state.store.get("rsi_candidate",candidate_id).unwrap();
        assert_eq!(candidate["status"],"proposed");
        assert_eq!(candidate["pairedComparisonComplete"],false);
        assert!(candidate["patch"].as_str().unwrap().contains("version = 2"));
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 1\n");
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        assert!(git_local(&source,&["status","--porcelain"]).is_empty());
        let request=json!({"confirmation":"APPLY_UNVERIFIED_TO_SUB",
            "baseCommit":base,"patchSha256":candidate["patchSha256"]});
        // Typecheck could not run without installed web dependencies. The
        // manual apply gate must refuse even an explicit user confirmation.
        assert!(apply_to_sub(&state,candidate_id,&request).await.is_err());
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 1\n");
        // Simulate a separately completed successful verifier receipt to
        // exercise the Git apply path. This is NOT a measured model benchmark.
        let record_id=format!("rsi-run-{}",context.job_id);
        let mut proof=state.store.get("rsi_run",&record_id).unwrap();
        proof["checksPassed"]=json!(true);
        proof["pairedComparisonComplete"]=json!(true);
        state.store.put("rsi_run",&proof);
        let mut compared=state.store.get("rsi_candidate",candidate_id).unwrap();
        compared["status"]=json!("inconclusive");
        compared["evaluation"]=json!({"metrics":{"regressions":0}});
        state.store.put("rsi_candidate",&compared);
        let applied=apply_to_sub(&state,candidate_id,&request).await.unwrap();
        assert_eq!(applied["status"],"applied_to_sub");
        assert_eq!(std::fs::read_to_string(source.join("web/src/pages/Demo.tsx")).unwrap(),"export const version = 2\n");
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        assert!(!git_local(&source,&["status","--porcelain"]).is_empty());
        cleanup(&worktree).await.unwrap();
        assert!(!worktree.path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn independently_measured_plateau_restores_real_patch_into_descendant_only() {
        use crate::{config::Config, db::Store, state::AppState, vault::Vault};
        use std::process::Command as SyncCommand;
        fn git_result(path: &Path, args: &[&str]) -> String {
            let output=SyncCommand::new("git").current_dir(path).args(args)
                .output().unwrap();
            assert!(output.status.success(),
                "git {:?}: {}",args,String::from_utf8_lossy(&output.stderr));
            String::from_utf8(output.stdout).unwrap()
        }
        let root=std::env::temp_dir().join(format!("rsi-plateau-worktree-{}",uuid::Uuid::new_v4()));
        let source=root.join("source");
        let data=root.join("data");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(source.join("Cargo.toml"),
            "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").unwrap();
        std::fs::write(source.join("src/engine/rsi.rs"),"// system source\n").unwrap();
        let original="pub fn sample() -> u8 { 1 }\n";
        let changed="pub fn sample() -> u8 { 2 }\n";
        let path="src/engine/sample.rs";
        std::fs::write(source.join(path),original).unwrap();
        git_result(&source,&["init","-q"]);
        git_result(&source,&["checkout","-b","dominion-sub1"]);
        git_result(&source,&["add","."]);
        git_result(&source,&["-c","user.name=CI","-c","user.email=ci@example.com",
            "commit","-qm","baseline"]);
        let base=git_result(&source,&["rev-parse","HEAD"]).trim().to_string();
        std::fs::write(source.join(path),changed).unwrap();
        let patch=git_result(&source,&["diff","--",path]);
        std::fs::write(source.join(path),original).unwrap();
        assert!(!patch.trim().is_empty());
        let cfg=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),
            workspace:data.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()));
        let plan=(0..8).map(|i|json!({
            "id":format!("case-{i}"),
            "domain":if i==1 {"other"} else {"harness"},
            "holdout":i<3,
            "filter":format!("engine::rsi::tests::case_{i}")
        })).collect::<Vec<_>>();
        let mut candidate=crate::engine::rsi::propose(&state,&json!({
            "target":"harness","baseCommit":base,
            "patch":patch,"hypothesis":"Improve real source",
            "cases":plan,"oracleVersion":"frozen-v1",
            "generatorModel":"real-model"
        })).unwrap();
        let id=candidate["id"].as_str().unwrap().to_string();
        candidate["rsiJobId"]=json!("parent-gen");
        candidate["origin"]=json!("model_driven_detached_worktree");
        state.store.put("rsi_candidate",&candidate);
        let raw=json!({
            "baseCommit":base,"patchSha256":candidate["patchSha256"],
            "oracleVersion":"frozen-v1","runner":"local_gitrepo_not_os_sandbox",
            "reportSha256":"b".repeat(64),
            "baselineBuildOk":true,"candidateBuildOk":true,
            "cases":(0..8).map(|i|json!({
                "id":format!("case-{i}"),"baselinePass":true,"candidatePass":true
            })).collect::<Vec<_>>()
        });
        candidate=crate::engine::rsi::evaluate(&state,&id,&raw).unwrap();
        assert_eq!(candidate["status"],"inconclusive");
        candidate["pairedComparisonComplete"]=json!(true);
        candidate["evaluation"]["source"]=json!("server_invoked_local_paired_git_worktrees");
        candidate["evaluation"]["evidenceSha256"]=raw["reportSha256"].clone();
        state.store.put("rsi_candidate",&candidate);
        state.store.put("rsi_pair",&json!({
            "id":"measured-neutral-pair","candidateId":id,
            "evaluationId":candidate["evaluation"]["id"],
            "report":raw,"verdict":"inconclusive",
            "metrics":candidate["evaluation"]["metrics"],
            "evidenceSha256":candidate["evaluation"]["evidenceSha256"],
            "runner":"local_gitrepo_not_os_sandbox"
        }));
        state.store.put("rsi_run",&json!({
            "id":"rsi-run-parent-gen","candidateId":id,
            "baseCommit":base,"checksPassed":true,
            "checks":[{"label":"cargo check --all-targets","ran":true,"passed":true}]
        }));
        state.store.put("job",&json!({
            "id":"child-gen","kind":"rsi_generation","status":"running"
        }));
        let context=JobContext::new(state.clone(),"child-gen".into());
        let child_path=root.join("descendant");
        git_result(&source,&["worktree","add","--detach",
            child_path.to_str().unwrap(),&base]);
        let isolated=Worktree{source:source.clone(),path:child_path.clone(),base:base.clone()};
        let input=json!({"rsiRepairCandidateId":id,"rsiRepairParentJobId":"parent-gen"});
        let repaired=restore_failed_candidate(&context,&input,&isolated).await.unwrap().unwrap();
        assert_eq!(repaired["repairKind"],"quality_plateau");
        assert_eq!(repaired["qualityMetrics"]["improvedHoldouts"],0);
        assert_eq!(std::fs::read_to_string(child_path.join(path)).unwrap(),changed);
        assert_eq!(std::fs::read_to_string(source.join(path)).unwrap(),original);
        assert_eq!(git_result(&source,&["rev-parse","HEAD"]).trim(),base);
        assert!(git_result(&source,&["status","--porcelain"]).trim().is_empty());
        cleanup(&isolated).await.unwrap();
        assert!(!child_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rsi_model_cannot_use_absolute_write_or_shell_tools(){
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!("rsi-permission-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg=Config{port:0,data_dir:root.clone(),web_dir:root.join("web"),workspace:root.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"rsi-session","rsiRun":true}));
        let ctx=tools::ToolContext{
            workspace:&root,store:&state.store,state:&state,session_id:"rsi-session",
            agent_id:None,tools_enabled:true,tools_execution_enabled:true
        };
        let outside=root.join("outside.txt");
        assert!(tools::execute(&ctx,"file_write",&json!({"path":outside.to_str().unwrap(),"content":"forbidden"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"shell_exec",&json!({"command":"touch outside.txt"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"workspace_write",&json!({"path":".git","content":"tamper"}),"run").await.is_err());
        assert!(tools::execute(&ctx,"workspace_write",&json!({"path":"Cargo.toml","content":"tamper"}),"run").await.is_err());
        assert!(!outside.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_patch_is_audited_and_unrelated_patch_is_not(){
        let root=std::env::temp_dir().join(format!("rsi-unsafe-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"),"unsafe { call(); }\n").unwrap();
        std::fs::write(root.join("src/unrelated.rs"),"unsafe fn unrelated() {}\n").unwrap();
        let unsafe_patch="diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-unsafe { call(); }\n+unsafe { call_again(); }\n";
        let audit=unsafe_audit_for_patch(&root,unsafe_patch).expect("unsafe patch must be audited");
        assert_eq!(audit["passed"],false);
        let check=unsafe_audit_check(&audit);
        assert_eq!(check["label"],"unsafe_safety");
        assert_eq!(check["passed"],false);
        assert!(check["findings"].as_array().unwrap()[0].as_str().unwrap().ends_with("src/lib.rs:1"));
        assert!(check["findings"].as_array().unwrap().iter().all(|row|!row.as_str().unwrap().contains("unrelated.rs")),
            "unrelated pre-existing files must not block this patch");
        assert!(unsafe_audit_for_patch(&root,"diff --git a/src/lib.rs b/src/lib.rs\n+let value = 1;\n").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
