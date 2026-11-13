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
        assert_eq!(classify("diff --git a/src/engine/rsi/planner/search.rs b/src/engine/rsi/planner/search.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/algorithms.rs b/src/engine/algorithms.rs\n"),"improver",
            "a change to how algorithm candidates are selected must be measured as meta improvement");
        assert_eq!(classify("diff --git a/src/engine/algorithm_eval.rs b/src/engine/algorithm_eval.rs\n"),"improver");
        assert_eq!(classify("diff --git a/src/engine/room_state.rs b/src/engine/room_state.rs\n"),"algorithm");
        assert_eq!(classify("diff --git a/web/src/pages/Home.tsx b/web/src/pages/Home.tsx\n"),"harness");
    }
    #[test]
    fn verifier_preserves_first_compiler_error_before_long_warning_tail(){
        let source=format!("warning: setup\\nerror[E0308]: mismatched types\\n --> src/engine/decompose.rs:42:10\\n  |\\n42 | wrong\\n  | expected usize\\n{}",
            "warning: unrelated diagnostics\\n".repeat(250));
        let focused=focused_verifier_stderr(&source.replace("\\n","\n"));
        assert!(focused.contains("error[E0308]"));
        assert!(focused.contains("src/engine/decompose.rs:42:10"));
        assert!(focused.starts_with("warning: setup"));
        assert!(!focused.contains("unrelated diagnostics\\n".repeat(220).as_str()));
    }
    #[test]
    fn failing_verification_never_completes_and_only_valid_failures_retry(){
        assert_eq!(run_status(true,false,false,true,"",""),("needs_review","build_or_typecheck_failed"));
        assert_eq!(run_status(true,false,true,true,"",""),("needs_review","unsafe_safety_failed"));
        assert_eq!(run_status(true,true,false,true,"",""),("completed","candidate_compiles_not_performance_verified"));
        assert_eq!(run_status(true,true,false,false,"","").0,"needs_review");
        assert_eq!(run_status(true,true,false,true,"provider timeout","").0,"completed");
        assert_eq!(run_status(true,true,false,true,"cancelled by user","").0,"needs_review");
        assert_eq!(run_status(false,true,false,true,"","invalid diff").0,"needs_review");
        let failing=json!({"status":"needs_review","reason":"build_or_typecheck_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"cargo check --all-targets","passed":false,"stderr":"error[E0308]: wrong type"}]}});
        assert!(repair_feedback(&failing).unwrap().contains("error[E0308]"));
        let unsafe_failure=json!({"status":"needs_review","reason":"unsafe_safety_failed",
            "rsi":{"sourceClean":true,"captureError":"","modelError":"",
                "checks":[{"label":"unsafe_safety","passed":false,"error":"unsafe 블록에 SAFETY 근거가 없습니다."}]}});
        assert!(repair_feedback(&unsafe_failure).unwrap().contains("SAFETY"));
        let mut after_edit_error=failing;
        after_edit_error["rsi"]["modelError"]=json!("provider unavailable");
        // An independently failed compilation takes precedence over a
        // provider error that happened after an actual source edit.
        assert!(repair_feedback(&after_edit_error).unwrap().contains("error[E0308]"));
    }
    #[test]
    fn failed_real_edit_attempt_receives_grounded_same_generation_repair() {
        let evidence=json!({"status":"needs_review","reason":"no_valid_candidate",
            "rsi":{"sourceClean":true,"modelError":"",
                "captureError":"모델이 실제 소스 파일을 변경하지 않았습니다.",
                "editEvidence":{"attempts":2,"verifiedWrites":0,
                    "recentFailures":["workspace_replace_range: source was not changed"]}}});
        let feedback=repair_feedback(&evidence).unwrap();
        assert!(feedback.contains("workspace_replace_range"));
        assert!(feedback.contains("source was not changed"));
        let mut none=evidence.clone();
        none["rsi"]["editEvidence"]["attempts"]=json!(0);
        assert!(repair_feedback(&none).is_none());
        let mut timeout=evidence;
        timeout["rsi"]["modelError"]=json!("rsi_token_budget: exhausted");
        assert!(repair_feedback(&timeout).is_none());
    }

    #[test]
    fn requires_complete_sha(){
        assert!(valid_sha("1234567890abcdef1234567890abcdef12345678"));
        assert!(!valid_sha("bad"));
        assert!(rsi_work_branch("dominion-champion"));
        assert!(!rsi_work_branch("dominion-sub3"),
            "another agent's branch is not a champion-owned checkout");
        assert!(rsi_work_branch("dominion-sub"));
        assert!(owns_application_branch("dominion-champion",Some("dominion-champion")));
        assert!(!owns_application_branch("dominion-champion",Some("dominion-sub3")));
        assert!(!owns_application_branch("dominion-champion",Some("dominion-sub")));
        assert!(!owns_application_branch("dominion-champion",Some("main")));
        assert!(!owns_application_branch("dominion-champion",None),
            "new champion experiments require an exact recorded source branch");
        assert!(!owns_application_branch("dominion-champion",Some("")),
            "missing campaign provenance cannot authorize a source write");
        assert!(!owns_application_branch("dominion-sub3",Some("dominion-sub3")));
        assert!(!owns_application_branch("dominion-sub3",Some("dominion-sub2")));
        assert!(!owns_application_branch("dominion-sub3",None));
        assert!(!owns_application_branch("dominion-sub3",Some("")));
        assert!(owns_application_branch("dominion-sub",None));
        assert!(!owns_application_branch("main",Some("main")));
        for forbidden in ["main","dominion-sub1","dominion-sub2","dominion-sub3","dominion-sub4","dominion-sub3-other","dominion-champion-other"] {
            assert!(!rsi_work_branch(forbidden), "다른 브랜치에 적용하면 안 됩니다: {forbidden}");
        }
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
        git_local(&source,&["checkout","-b","dominion-champion"]);
        git_local(&source,&["add","."]);
        git_local(&source,&["-c","user.email=ci@example.com","-c","user.name=CI","commit","-qm","base"]);
        let base=git_local(&source,&["rev-parse","HEAD"]);
        let cfg=Config{port:0,data_dir:data.clone(),web_dir:data.join("web"),workspace:data.clone()};
        let state=AppState::new(cfg.clone(),Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        crate::engine::dev::save_config(&state,&json!({"sourceDir":source.to_str().unwrap()}));
        let input=json!({
            "rsiBaseCommit":base,"rsiSourceDir":source.to_str().unwrap(),
            "rsiSourceBranch":"dominion-champion",
            "rsiObjective":"DOMINION 화면 렌더링 검증 대상 소스를 수정한다",
            "model":"test-model","providerId":"test-provider",
            "rsiFrozenCases":[
                {"id":"memory","domain":"memory","holdout":true,"filter":"engine::room_state::tests::korean_owner_correction_replaces_the_previous_assignment"},
                {"id":"algorithm","domain":"algorithm","holdout":true,"filter":"engine::algorithms::tests::generation_uses_evidence_and_returns_executable_kernels"}
            ]
        });
        let job=state.store.put("job",&json!({"kind":"prompt","status":"running","input":input})).unwrap();
        let context=JobContext::new(state.clone(),job["id"].as_str().unwrap().to_string());
        git_local(&source,&["checkout","-b","dominion-sub2"]);
        assert!(prepare(&context,&job["input"]).await.is_err(),
            "same-commit sibling work branches must not inherit a queued source job");
        git_local(&source,&["checkout","dominion-champion"]);
        let mut missing_origin=job["input"].clone();
        missing_origin.as_object_mut().unwrap().remove("rsiSourceBranch");
        assert!(prepare(&context,&missing_origin).await.is_err(),
            "a new champion worktree may not be started without a recorded source branch");
        let worktree=prepare(&context,&job["input"]).await.unwrap();
        assert_ne!(worktree.path,source);
        assert_eq!(git_local(&source,&["rev-parse","HEAD"]),base);
        std::fs::write(worktree.path.join("web/src/pages/Demo.tsx"),"export const version = 2\n").unwrap();
        // Even a real dirty detached worktree cannot be attributed to a
        // model when the model had zero recorded calls.
        let rejected=finish(&context,&job,&worktree,
            Ok(Some(json!({"id":"unproven"}))),"session-rsi").await.unwrap();
        assert!(rejected["rsi"]["candidateId"].is_null());
        assert!(rejected["rsi"]["captureError"].as_str().unwrap()
            .contains("모델 API 호출 기록"));
        // Unit-test fixture: emulate both independent receipt sources after
        // demonstrating the zero-call rejection. Not a real model benchmark.
        state.store.audit("model.usage",json!({
            "sessionId":"session-mixed","providerId":"test-provider",
            "model":"test-model","promptTokens":10,
            "completionTokens":10,"totalTokens":20,"estimated":false
        }));
        state.store.audit("model.usage",json!({
            "sessionId":"session-mixed","providerId":"other-provider",
            "model":"other-model","promptTokens":10,
            "completionTokens":10,"totalTokens":20,"estimated":false
        }));
        let mixed=finish(&context,&job,&worktree,
            Ok(Some(json!({"id":"mixed"}))),"session-mixed").await.unwrap();
        assert!(mixed["rsi"]["candidateId"].is_null(),
            "another model on the same worker session invalidates source attribution");
        assert_eq!(mixed["rsi"]["modelRouteSessionExclusive"],false);
        state.store.audit("model.usage",json!({
            "sessionId":"session-rsi","providerId":"test-provider",
            "model":"test-model","promptTokens":10,
            "completionTokens":10,"totalTokens":20,"estimated":false
        }));
        state.store.put("execution_operation",&json!({
            "id":"fixture-verified-model-write",
            "sessionId":"session-rsi","origin":"model",
            "tool":"workspace_replace","status":"completed",
            "result":{"written":true},"toolReceipt":{"verified":true}
        })).unwrap();
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
