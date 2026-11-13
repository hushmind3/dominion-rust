//! Source-level tests for the detached RSI worktree.
use super::*;

use super::*;
    #[tokio::test]
    async fn descendant_restores_only_same_campaign_failed_patch_in_its_own_tree() {
        use crate::{config::Config,db::Store,state::AppState,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "rsi-recover-{}",uuid::Uuid::new_v4()));
        let source=root.join("source");
        let child=root.join("child");
        let data=root.join("data");
        for location in [&source,&child] {
            std::fs::create_dir_all(location.join("src/engine")).unwrap();
            std::fs::write(location.join("src/engine/memory.rs"),
                "fn old() {}\n").unwrap();
        }
        std::fs::create_dir_all(&data).unwrap();
        git(&child,&["init","-q"]).await.unwrap();
        let cfg=Config{port:0,data_dir:data.clone(),
            web_dir:data.join("web"),workspace:child.clone()};
        let state=AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("job",&json!({
            "id":"restore-job","kind":"rsi_generation","status":"running"
        })).unwrap();
        let ctx=JobContext::new(state.clone(),"restore-job".into());
        let patch=concat!(
            "diff --git a/src/engine/memory.rs b/src/engine/memory.rs\n",
            "--- a/src/engine/memory.rs\n",
            "+++ b/src/engine/memory.rs\n",
            "@@ -1 +1 @@\n",
            "-fn old() {}\n",
            "+fn broken( {}\n"
        );
        let base="a".repeat(40);
        state.store.put("rsi_candidate",&json!({
            "id":"failed-patch","baseCommit":base,
            "patch":patch,"patchSha256":digest(patch),
            "paths":["src/engine/memory.rs"],
            "experimentCampaignId":"campaign-1"
        })).unwrap();
        let input=json!({
            "rsiCampaignId":"campaign-1",
            "rsiFailedPatchRecovery":{
                "candidateId":"failed-patch","baseCommit":base,
                "failedGenerationJobId":"prior"
            }
        });
        let worktree=Worktree{source:source.clone(),
            path:child.clone(),base:base.clone()};
        let restored=restore_failed_patch(&ctx,&input,&worktree)
            .await.unwrap();
        assert_eq!(restored["status"],"restored_unverified");
        assert!(restored["independentGain"].is_null());
        assert_eq!(std::fs::read_to_string(
            child.join("src/engine/memory.rs")).unwrap(),"fn broken( {}\n");
        assert_eq!(std::fs::read_to_string(
            source.join("src/engine/memory.rs")).unwrap(),"fn old() {}\n");
        let mut foreign=input.clone();
        foreign["rsiCampaignId"]=json!("different-campaign");
        assert!(restore_failed_patch(&ctx,&foreign,&worktree).await.is_err());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_algorithm_mutation_is_classified_as_self_improver() {
        let patch="diff --git a/src/engine/algorithms.rs b/src/engine/algorithms.rs\n--- a/src/engine/algorithms.rs\n+++ b/src/engine/algorithms.rs\n@@ -1 +1 @@\n-a\n+b\n";
        assert_eq!(classify(patch),"improver");
        assert!(is_rsi_work_branch("dominion-sub2"));
        assert!(is_rsi_work_branch("dominion-sub3"));
        assert!(!is_rsi_work_branch("dominion-champion"));
        assert!(!is_rsi_work_branch("main"));
        assert!(!is_rsi_work_branch("dominion-sub02"));
    }

    #[test]
    fn exact_branch_ownership_blocks_cross_branch_promotion() {
        assert!(owns_application_branch("dominion-sub2",Some("dominion-sub2")));
        assert!(!owns_application_branch("dominion-sub2",Some("main")));
        assert!(!owns_application_branch("dominion-sub2",Some("dominion-sub1")));
        assert!(!owns_application_branch("dominion-sub2",None));
        assert!(!owns_application_branch("dominion-sub2",Some("")));
        assert!(!owns_application_branch("main",Some("main")));
    }

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
    }
    #[test]
    fn observed_stale_tool_error_recovers_with_new_current_source_not_blind_retry(){
        let result=json!({"status":"needs_review","reason":"no_valid_candidate",
            "rsi":{"sourceClean":true,"diffBytes":0,"modelError":""}});
        let session=json!({"id":"s","rsiRun":true,"rsiExplicitFocus":true,
            "rsiFocusFiles":["src/engine/memory.rs"]});
        let failed=json!({"sessionId":"s","status":"completed",
            "tool":"workspace_replace","arguments":{
                "path":"src/engine/memory.rs","startLine":4,"endLine":4,
                "newText":"fn revised() {}"},
            "result":{"written":false,"staleVersion":true,
                "error":"읽은 뒤 소스가 변경되었습니다."}});
        let feedback=failed_edit_feedback(&result,&session,&[failed.clone()]).unwrap();
        assert!(feedback.contains("workspace_read_range"));
        assert!(feedback.contains("fn revised() {}"));
        let mut different=failed.clone();
        different["sessionId"]=json!("someone-else");
        assert!(failed_edit_feedback(&result,&session,&[different]).is_none());
        different=failed.clone();
        different["result"]["written"]=json!(true);
        assert!(failed_edit_feedback(&result,&session,&[different]).is_none());
        different=failed.clone();
        different["result"]["staleVersion"]=json!(false);
        assert!(failed_edit_feedback(&result,&session,&[different]).is_none());
        different=failed.clone();
        different["tool"]=json!("workspace_write");
        different["result"]["staleVersion"]=json!(false);
        different["result"]["error"]=json!("workspace_write는 새 파일 생성 전용입니다");
        let instruction=failed_edit_feedback(&result,&session,&[different]).unwrap();
        assert!(instruction.contains("workspace_replace"));
    }

    #[test]
    fn no_edit_recovery_runs_only_for_grounded_source_and_once_per_job(){
        let result=json!({"status":"needs_review","reason":"no_valid_candidate",
            "rsi":{"sourceClean":true,"diffBytes":0,
                "captureError":"모델이 실제 소스 파일을 변경하지 않았습니다.","modelError":""}});
        let session=json!({"id":"model-session","rsiRun":true,
            "rsiExplicitFocus":true,"rsiFocusContextReady":true,
            "rsiPolicyFocus":true,"rsiCodeIndexId":"frozen-index",
            "rsiFocusFiles":["src/engine/memory.rs"]});
        let target="src/engine/memory.rs";
        let plan=no_edit_feedback(&result,&session,&[]).unwrap();
        assert!(plan.contains(target));
        assert!(plan.contains("workspace_replace"));
        let read=json!({"sessionId":"model-session","tool":"workspace_read_range",
            "status":"completed","arguments":{"path":target},
            "result":{"sourceSha256":"a".repeat(64),"content":"1 fn before() {}"}});
        let mut no_index=session.clone();
        no_index["rsiPolicyFocus"]=json!(false);
        assert!(no_edit_feedback(&result,&no_index,&[read.clone()])
            .unwrap().contains("sourceSha256"));
        assert!(no_edit_feedback(&result,&no_index,&[]).is_none());
        let mut owner_focus=no_index.clone();
        owner_focus["rsiNoEditGroundedTarget"]=json!(true);
        assert!(no_edit_feedback(&result,&owner_focus,&[]).is_some(),
            "an owner-selected target with indexed source already in context can retry once");
        let wrong=json!({"sessionId":"foreign","tool":"workspace_read_range",
            "status":"completed","arguments":{"path":target},
            "result":{"sourceSha256":"a".repeat(64)}});
        assert!(no_edit_feedback(&result,&no_index,&[wrong]).is_none());
        let edited=json!({"sessionId":"model-session","tool":"workspace_replace",
            "status":"failed","arguments":{"path":target},
            "result":{"staleVersion":true}});
        assert!(no_edit_feedback(&result,&session,&[edited]).is_none(),
            "a failed edit belongs to the method repair algorithm");
        let mut no_focus=session.clone();
        no_focus["rsiFocusContextReady"]=json!(false);
        assert!(no_edit_feedback(&result,&no_focus,&[]).is_none());
        assert!(no_edit_feedback(&result,&no_focus,&[read.clone()]).is_some(),
            "a successful on-demand source read is stronger evidence than an absent prompt excerpt");
        let mut no_model=result.clone();
        no_model["rsi"]["modelError"]=json!("provider timeout");
        assert!(no_edit_feedback(&no_model,&session,&[]).is_none());
        let mut diff=result.clone();
        diff["rsi"]["diffBytes"]=json!(1);
        assert!(no_edit_feedback(&diff,&session,&[]).is_none());
    }

    #[test]
    fn failing_verification_never_completes_and_only_valid_failures_retry(){
        assert_eq!(run_status(true,false,false,true,"",""),("needs_review","build_or_typecheck_failed"));
        assert_eq!(run_status(true,false,true,true,"",""),("needs_review","unsafe_safety_failed"));
        assert_eq!(run_status(true,true,false,true,"",""),("completed","candidate_compiles_not_performance_verified"));
        assert_eq!(run_status(true,true,false,false,"","").0,"needs_review");
        assert_eq!(run_status(true,true,false,true,"provider timeout","").0,"needs_review");
        assert_eq!(run_status(false,true,false,true,"","invalid diff").0,"needs_review");
        let failing=json!({"status":"needs_review","reason":"build_or_typecheck_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"cargo check --all-targets","passed":false,"stderr":"error[E0308]: wrong type"}]}});
        assert!(repair_feedback(&failing).unwrap().contains("error[E0308]"));
        let unsafe_failure=json!({"status":"needs_review","reason":"unsafe_safety_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"unsafe_safety","passed":false,"error":"unsafe 블록에 SAFETY 근거가 없습니다."}]}});
        assert!(repair_feedback(&unsafe_failure).unwrap().contains("SAFETY"));
        let mut no_retry=failing;
        no_retry["rsi"]["modelError"]=json!("provider unavailable");
        assert!(repair_feedback(&no_retry).is_none());
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

