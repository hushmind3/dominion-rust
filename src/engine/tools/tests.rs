        use super::{workspace::*,edit::*};
use super::*;

    #[test]
    fn workspace_paths_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("dominion-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        assert!(resolve_workspace(&dir, "sub").is_ok());
        assert!(resolve_workspace(&dir, "../etc/passwd").is_err());
        assert!(resolve_workspace(&dir, "/etc/passwd").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", dir.join("outside")).unwrap();
            assert!(resolve_workspace(&dir, "outside/nonexistent.txt").is_err());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_frozen_version_write_stops_when_the_file_changed() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-frozen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let state = crate::state::AppState::new(
            config.clone(),
            store,
            Vault::open(&config.key_path()),
        );
        let path = dir.join("frozen.txt");
        std::fs::write(&path, "one").unwrap();
        let expected = format!("{:x}", Sha256::digest(b"one"));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let written = checked_write(&ctx, &path, "two", "run", false, Some(&expected))
            .await
            .unwrap();
        assert_eq!(written["written"], true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");

        // The same frozen version must not overwrite a later change.
        std::fs::write(&path, "external change").unwrap();
        let stale = checked_write(&ctx, &path, "three", "run", false, Some(&expected))
            .await
            .unwrap();
        assert_eq!(stale["staleVersion"], true, "{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external change");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn workspace_replace_edits_one_exact_block_without_full_content() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-replace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let path = dir.join("large.rs");
        std::fs::write(&path, "let old_value = 1;\nlet untouched = 2;\n").unwrap();
        let expected = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = replace_text(
            &ctx,
            &path,
            "let old_value = 1;",
            "let new_value = 3;",
            Some(&expected),
            "run",
        )
        .await
        .unwrap();
        assert_eq!(result["replaced"], true, "{result}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 2;\n"
        );
        let ambiguous = replace_text(&ctx, &path, "let", "const", None, "run")
            .await
            .unwrap_err();
        assert!(ambiguous.contains("출현 2회"), "{ambiguous}");
        let unchanged = replace_text(&ctx, &path, "new_value", "new_value", None, "run")
            .await
            .unwrap_err();
        assert!(unchanged.contains("동일"), "{unchanged}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn replace_range_uses_frozen_file_sha_and_does_not_forge_edits() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-range-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let path=dir.join("src/example.rs");
        std::fs::write(&path,"first();\nlet count = 0;\nlast();\n").unwrap();
        let sha=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"range",agent_id:None,tools_enabled:true,
            tools_execution_enabled:true};
        let args=json!({"path":"src/example.rs","startLine":2,"endLine":2,
            "newText":"let count = 1;","expectedSha256":sha});
        let changed=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(changed["written"],true);
        assert_eq!(changed["replacedLines"],1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet count = 1;\nlast();\n");
        let stale=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap_err();
        assert!(stale.contains("소스가 읽은 뒤 변경"),"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet count = 1;\nlast();\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_small_model_edit_contract_recovery_is_unambiguous() {
        let complete="fn target() -> u32 {\n    2\n}";
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":13,"endLine":0}),complete),
            Some(RsiEditIntent::CompleteFunction("target".into())));
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":"13"}),"let score = 2;"),
            Some(RsiEditIntent::SingleLine(13)));
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":3}),"let x = 2;\nlet y = 3;"),None);
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":1,"endLine":3}),complete),None);
    }

    #[tokio::test]
    async fn rsi_workspace_replace_repairs_missing_end_line_with_model_authored_function() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-model-edit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path=dir.join("src/engine/example.rs");
        let original="fn sibling() -> u32 {\n    3\n}\n\nfn target() -> u32 {\n    1\n}\n";
        std::fs::write(&path,original).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        let index=crate::engine::code_index::refresh(&state,&dir).unwrap();
        state.store.put("session",&json!({"id":"model-editor","rsiRun":true,
            "rsiCodeIndexId":index["id"],
            "rsiExplicitFocus":true,"rsiObjective":"Improve target()"})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"model-editor",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true};
        let edit=json!({"path":"src/engine/example.rs",
            "startLine":5,"endLine":0,
            "newText":"fn target() -> u32 {\n    2\n}"});
        let changed=execute(&ctx,"workspace_replace",&edit,"run").await.unwrap();
        assert_eq!(changed["written"],true,"{changed}");
        assert_eq!(changed["editContractRecovered"],"model_authored_complete_function");
        assert_eq!(changed["sourceHashRecoveredFromIndex"],true);
        assert!(std::fs::read_to_string(&path).unwrap().contains("fn sibling() -> u32 {\n    3"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("fn target() -> u32 {\n    2"));
        assert!(execute(&ctx,"workspace_replace",&edit,"run").await.is_err(),
            "a stale indexed SHA must fail even if the model repeats the same edit");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn rsi_workspace_replace_refuses_guessed_multiline_edit() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-refuse-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let path=dir.join("src/example.rs");
        let source="fn original() {\n    foo();\n}\n";
        std::fs::write(&path,source).unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"refuse","rsiRun":true})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"refuse",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true};
        let sha=format!("{:x}",Sha256::digest(source.as_bytes()));
        let ambiguous=json!({"path":"src/example.rs","startLine":1,"endLine":0,
            "newText":"let foo = 1;\nlet bar = 2;","expectedSha256":sha});
        assert!(execute(&ctx,"workspace_replace",&ambiguous,"run").await.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),source);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn isolated_rsi_syntax_failure_never_mutates_live_candidate() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir = std::env::temp_dir()
            .join(format!("sub1-syntax-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path = dir.join("src/engine/decompose.rs");
        let original = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![] }\n";
        std::fs::write(&path, original).unwrap();
        let cfg = Config { port: 0, data_dir: dir.clone(), web_dir: dir.join("web"),
            workspace: dir.clone() };
        let state = crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(), Vault::open(&cfg.key_path()));
        state.store.put("session", &json!({"id":"rsi-syntax", "rsiRun":true,
            "rsiExplicitFocus":true, "rsiObjective":"Improve split_markers() without changing marker_at()"}));
        let ctx = ToolContext { workspace:&dir, store:&state.store, state:&state,
            session_id:"rsi-syntax", agent_id:None, tools_enabled:true,
            tools_execution_enabled:true };
        let sha = format!("{:x}", Sha256::digest(original.as_bytes()));
        let invalid = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![]\n]\n";
        let failure = replace_source_lines(&ctx, &path, 1, 3, invalid, &sha, "run")
            .await.unwrap();
        assert_eq!(failure["preflightSyntaxRejected"],true,"{failure}");
        assert_eq!(failure["syntaxValid"],false,"{failure}");
        assert_eq!(failure["sourceUnchanged"],true,"{failure}");
        assert_eq!(failure["written"],false,"{failure}");
        assert!(failure["error"].as_str().unwrap().contains("rsi_rust_syntax_rejected"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let sibling_drift = "fn marker_at() -> bool { false }\n\nfn split_markers() -> Vec<String> { vec![] }\n";
        let scope_failure = replace_source_lines(&ctx, &path, 1, 3, sibling_drift, &sha, "run")
            .await.unwrap();
        assert_eq!(scope_failure["preflightScopeRejected"],true,"{scope_failure}");
        assert_eq!(scope_failure["written"],false,"{scope_failure}");
        assert_eq!(scope_failure["sourceUnchanged"],true,"{scope_failure}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let valid = "fn marker_at() -> bool { true }\n\nfn split_markers() -> Vec<String> { vec![\"ok\".to_owned()] }\n";
        let result = replace_source_lines(&ctx, &path, 1, 3, valid, &sha, "run")
            .await.unwrap();
        assert_eq!(result["written"], true, "{result}");
        assert_eq!(result["syntaxValid"], true, "{result}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), valid);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn function_editor_preserves_siblings_without_model_line_ranges() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-function-edit-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path=dir.join("src/engine/example.rs");
        let old="fn sibling() -> u32 {\n    5\n}\n\nfn target() -> u32 {\n    1\n}\n\nfn other() -> u32 {\n    7\n}\n";
        let updated="fn target() -> u32 {\n    2\n}";
        std::fs::write(&path,old).unwrap();
        let config=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"sub1-function","rsiRun":true,
            "rsiExplicitFocus":true,"rsiObjective":"Improve target()"}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"sub1-function",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let sha=format!("{:x}",Sha256::digest(old.as_bytes()));
        let change=execute(&ctx,"workspace_replace_function",
            &json!({"path":"src/engine/example.rs","functionName":"target",
                "newText":updated,"expectedSha256":sha}),"run").await.unwrap();
        assert_eq!(change["written"],true,"{change}");
        assert_eq!(change["replacedFunction"],"target");
        assert_eq!(change["syntaxValid"],true,"{change}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            old.replace("fn target() -> u32 {\n    1\n}",updated));
        let rejected=execute(&ctx,"workspace_replace_function",
            &json!({"path":"src/engine/example.rs","functionName":"target",
                "newText":updated,"expectedSha256":sha}),"run").await.unwrap_err();
        assert!(rejected.contains("version changed"),"{rejected}");
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn isolated_rsi_range_read_returns_model_usable_relative_path() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("sub1-rsi-path-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path=dir.join("src/engine/target.rs");
        std::fs::write(&path,"fn target() {\n    good();\n}\n").unwrap();
        let cfg=Config{port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone()};
        let state=crate::state::AppState::new(cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),Vault::open(&cfg.key_path()));
        state.store.put("session",&json!({"id":"rsi","rsiRun":true}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,
            session_id:"rsi",agent_id:None,tools_enabled:true,
            tools_execution_enabled:true};
        let read=execute(&ctx,"workspace_read_range",
            &json!({"path":"src/engine/target.rs","startLine":1,"endLine":3}),
            "run").await.unwrap();
        assert_eq!(read["path"],"src/engine/target.rs");
        assert!(!read["path"].as_str().unwrap().starts_with('/'));
        assert_eq!(read["content"], "fn target() {\n    good();\n}\n",
            "Line numbers must never leak into editable source content");
        assert_eq!(read["contentFormat"], "raw_source_no_line_prefix");
        assert_eq!(read["sourceSha256"],
            format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap())));
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rsi_can_read_only_a_range_or_replace_a_block() {
        assert!(rsi_allowed("code_search"));
        assert!(!rsi_allowed("workspace_list"));
        assert!(rsi_allowed("workspace_read_range"));
        assert!(rsi_allowed("workspace_replace"));
        assert!(rsi_allowed("workspace_replace_range"));
        assert!(rsi_allowed("workspace_write"));
        assert!(rsi_allowed("algorithm_propose"));
        assert!(rsi_allowed("unsafe_audit"));
        assert!(!rsi_allowed("workspace_read"));
        assert!(!rsi_allowed("shell_exec"));
        assert!(crate::engine::rsi::hidden_rsi_oracle_path(
            "src/engine/rsi_oracle_fixture.rs"));
    }

    #[tokio::test]
    async fn algorithm_propose_is_limited_to_running_rsi_campaigns() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-alg-tool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let proposal = json!({"name":"objective-first","expression":"objectiveMatch * 10 + novelty"});
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state.store.put("session", &json!({"id":"s"})).unwrap();
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true,"rsiCampaignId":"c"}))
            .unwrap();
        assert!(execute(&ctx, "algorithm_propose", &proposal, "run").await.is_err());
        state
            .store
            .put("job", &json!({"id":"c","kind":"rsi_evolve","status":"running"}))
            .unwrap();
        let saved = execute(&ctx, "algorithm_propose", &proposal, "run")
            .await
            .unwrap();
        assert_eq!(saved["proposed"], true);
        assert!(state
            .store
            .get("algorithm", saved["algorithmId"].as_str().unwrap())
            .is_some());
        let invalid = json!({"expression":"unknownVariable * 2"});
        assert!(execute(&ctx, "algorithm_propose", &invalid, "run").await.is_err());
        let both = execute(&ctx, "algorithm_propose", &json!({
            "name":"domain-and-target-v1",
            "expression":"objectiveMatch * 6 + exploration",
            "targetExpression":"pathEvidence * 2 + symbolEvidence - repeatPenalty"
        }), "run").await.unwrap();
        assert_eq!(both["proposed"],true);
        assert_eq!(both["algorithm"]["parameters"]["targetExpression"],
            "pathEvidence * 2 + symbolEvidence - repeatPenalty");
        assert_eq!(both["algorithm"]["status"],"available");
        assert!(execute(&ctx,"algorithm_propose",&json!({
            "expression":"novelty","targetExpression":"untrusted_system_call"
        }),"run").await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unsafe_audit_is_limited_to_rsi_sessions_and_reports_findings() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-unsafe-tool-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "// SAFETY: private process group\nunsafe { kill(-id, 9); }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/other.rs"),
            "unsafe fn undocumented() {}\n",
        )
        .unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(execute(&ctx, "unsafe_audit", &json!({}), "run").await.is_err());
        state
            .store
            .put("session", &json!({"id":"s","rsiRun":true}))
            .unwrap();
        let report = execute(&ctx, "unsafe_audit", &json!({}), "run").await.unwrap();
        assert_eq!(report["scanned"], 2);
        assert_eq!(report["passed"], false);
        let findings = report["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].as_str().unwrap().ends_with("src/other.rs:1"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn code_search_finds_indexed_symbols_and_paths() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-code-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let state = crate::state::AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        );
        state
            .store
            .put(
                "code_index",
                &json!({"id":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),"sourceDir":dir.canonicalize().unwrap(),"generatedAt":"2026-11-09T00:00:00Z","files":1}),
            )
            .unwrap();
        state
            .store
            .put(
                "code_index_file",
                &json!({
                    "id":"idx-test:src/engine/rsi_evolve.rs",
                    "indexId":crate::engine::code_index::index_id(&dir.canonicalize().unwrap()),
                    "path":"src/engine/rsi_evolve.rs",
                    "symbols":[
                        {"name":"run_loop","kind":"fn","line":120},
                        {"name":"record_rsi_lesson","kind":"fn","line":900}
                    ]
                }),
            )
            .unwrap();
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "s",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        let result = search_code_index(&ctx, "run_loop", 10).unwrap();
        assert_eq!(result["matchCount"], 1, "{result}");
        assert_eq!(result["matches"][0]["path"], "src/engine/rsi_evolve.rs");
        assert_eq!(result["matches"][0]["line"], 120);
        state.store.put("code_index",&json!({"id":"foreign","sourceDir":"/another/repository","generatedAt":"2099-01-01","files":1}));
        state.store.put("code_index_file",&json!({"id":"foreign-file","indexId":"foreign","path":"private.rs","symbols":[{"name":"run_loop","kind":"fn","line":999}]}));
        let multi=search_code_index(&ctx,"fn run_loop record_rsi_lesson",10).unwrap();
        assert_eq!(multi["matchCount"],2,"{multi}");
        assert!(multi["matches"].as_array().unwrap().iter().all(|row|row["path"]!="private.rs"));
        let path_result = search_code_index(&ctx, "rsi_evolve.rs", 10).unwrap();
        assert!(path_result["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["kind"] == "file"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delegation_depth_is_bounded_and_cycle_safe() {
        use crate::config::Config;
        use crate::db::Store;
        let dir = std::env::temp_dir().join(format!("dominion-depth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        for (id, parent) in [
            ("root", None),
            ("d1", Some("root")),
            ("d2", Some("d1")),
            ("d3", Some("d2")),
        ] {
            store.put(
                "session",
                &json!({"id":id,"parentSessionId":parent,"delegated":parent.is_some()}),
            );
        }
        assert_eq!(delegation_depth(&store, "root"), 0);
        assert_eq!(delegation_depth(&store, "d1"), 1);
        assert_eq!(delegation_depth(&store, "d3"), 3);
        store.put(
            "session",
            &json!({"id":"loop","parentSessionId":"d3","delegated":true}),
        );
        store.put(
            "session",
            &json!({"id":"d3","parentSessionId":"loop","delegated":true}),
        );
        assert!(delegation_depth(&store, "loop") <= 4);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_results_are_scoped_to_the_owning_or_parent_session() {
        use crate::config::Config;
        use crate::db::Store;
        use crate::vault::Vault;
        let dir = std::env::temp_dir().join(format!("dominion-task-acl-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config {
            port: 0,
            data_dir: dir.clone(),
            web_dir: dir.join("web"),
            workspace: dir.clone(),
        };
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        let state = crate::state::AppState::new(config, store, vault);
        state.store.put(
            "job",
            &json!({
                "id":"j1","kind":"prompt","status":"completed",
                "input":{"sessionId":"child","parentSessionId":"parent"},
                "result":{"answer":"ok"}
            }),
        );
        let ctx = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "parent",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&ctx, &json!({"jobId":"j1"}), true).is_ok());
        let child = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "child",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&child, &json!({"jobId":"j1"}), true).is_ok());
        let stranger = ToolContext {
            workspace: &dir,
            store: &state.store,
            state: &state,
            session_id: "stranger",
            agent_id: None,
            tools_enabled: true,
            tools_execution_enabled: true,
        };
        assert!(task_payload(&stranger, &json!({"jobId":"j1"}), true).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn http_fetch_rejects_internal_targets_and_credential_urls() {
        for url in [
            "http://127.0.0.1:4317/api/rsi/campaigns",
            "http://10.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.5/",
            "http://172.20.1.1/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://localhost/",
            "http://worker.internal/",
            "http://user:pass@example.com/",
            "file:///etc/passwd",
        ] {
            assert!(validated_http_target(url).is_err(), "{url}");
        }
        for url in ["https://example.com/resource", "http://1.1.1.1/"] {
            assert!(validated_http_target(url).is_ok(), "{url}");
        }
        let origin=validated_http_target("https://example.com/root").unwrap().0;
        let public=origin.join("/next").unwrap();
        assert!(validated_http_target(public.as_str()).is_ok());
        let private=origin.join("http://127.0.0.1:4317/").unwrap();
        assert!(validated_http_target(private.as_str()).is_err(),
            "A relative or absolute redirect is subject to the same URL gate");
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_fetch_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["127.0.0.1", "192.0.2.1", "198.51.100.7",
            "203.0.113.2", "fc00::1", "fe80::1", "2001:db8::1"] {
            assert!(!public_fetch_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn tool_definitions_cover_the_engine() {
        let names = definitions()
            .into_iter()
            .filter_map(|definition| definition["function"]["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        for expected in [
            "workspace_read",
            "workspace_write",
            "shell_exec",
            "memory_store",
            "memory_search",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} missing"
            );
        }
        assert!(requires_approval("shell_exec"));
        assert!(!requires_approval("workspace_read"));
    }
    #[cfg(unix)]
    async fn wait_for_marker(path: &Path) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while !path.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "test command did not start"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(unix)]
    fn delayed_child_command(dir: &Path) -> tokio::process::Command {
        // The parent waits while an ordinary child will mutate the temp
        // workspace later. No daemon, user process or external service is used.
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .args([
                "-c",
                "(sleep 0.6; printf leaked > escaped.txt) & printf ready > ready.txt; wait",
            ])
            .current_dir(dir);
        command
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.clone();
        let task = tokio::spawn(async move {
            let mut command = delayed_child_command(&cwd);
            run_command(&mut command, Duration::from_secs(10)).await
        });
        wait_for_marker(&dir.join("ready.txt")).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after Stop"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_tool_command_stops_background_descendants() {
        let dir =
            std::env::temp_dir().join(format!("dominion-process-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut command = delayed_child_command(&dir);
        let result = run_command(&mut command, Duration::from_millis(150)).await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(dir.join("ready.txt").exists());
        tokio::time::sleep(Duration::from_millis(650)).await;
        assert!(
            !dir.join("escaped.txt").exists(),
            "child continued changing files after timeout"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn tool_command_preserves_output_exit_status_and_bounded_capture() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "printf normal; printf warning >&2; exit 7"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        let payload = output_payload(&output);
        assert_eq!(payload["exitCode"], 7);
        assert_eq!(payload["stdout"], "normal");
        assert_eq!(payload["stderr"], "warning");
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args(["-c", "head -c 250000 /dev/zero"]);
        let output = run_command(&mut command, Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.len() <= 4 * 20_001);
        assert!(output_payload(&output)["stdout"]
            .as_str()
            .unwrap()
            .contains("truncated"));
    }
