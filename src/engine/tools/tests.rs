//! Tool security and native execution regression suite.
use super::*;

    #[test]
    fn http_fetch_rejects_private_addresses_and_local_hosts() {
        for url in [
            "http://127.0.0.1:4317/", "http://10.1.2.3/", "http://172.16.0.1/",
            "http://192.168.1.8/", "http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.100/", "http://[::1]/", "http://[fc00::1]/",
            "http://[64:ff9b::a9fe:a9fe]/",
            "http://localhost/", "http://service.internal/", "http://printer.local/"
        ] {
            let parsed=reqwest::Url::parse(url).unwrap();
            assert!(validate_http_target(&parsed).is_err(),"{url}");
        }
        assert!(validate_http_target(&reqwest::Url::parse("https://1.1.1.1/").unwrap()).is_ok());
        assert!(validate_http_target(&reqwest::Url::parse("https://example.com/").unwrap()).is_ok());
        assert!(validate_http_target(&reqwest::Url::parse("ftp://example.com/").unwrap()).is_err());
        assert!(validate_http_target(&reqwest::Url::parse("https://user:pass@example.com/").unwrap()).is_err());
    }

    #[tokio::test]
    async fn http_fetch_rejects_loopback_before_connecting() {
        assert!(http_fetch("http://127.0.0.1:4317/admin").await.unwrap_err().contains("non-public"));
        assert!(http_fetch("http://[::1]/").await.is_err());
    }

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
        // Repository indexing intentionally covers source paths, not ad-hoc
        // root files. Use a genuine indexed Rust source for the hash-recovery
        // stage of this otherwise standalone tool test.
        let path = dir.join("src/engine/large.rs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
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
            .unwrap();
        assert_eq!(ambiguous["occurrences"], 2, "{ambiguous}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        // A smaller model can update a numbered span without echoing the
        // entire existing file. Every range write is version-pinned.
        let frozen=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let changed=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":2,"expectedSha256":frozen}),
            "let untouched = 7;","run").await.unwrap();
        assert_eq!(changed["written"],true,"{changed}");
        assert_eq!(changed["editMode"],"versioned_line_span");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 7;\n");
        let stale=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":2,"expectedSha256":frozen}),
            "let untouched = 8;","run").await.unwrap();
        assert_eq!(stale["staleVersion"],true,"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 7;\n");
        // In a real RSI generation the coordinator already indexed the exact
        // file revision. A small model may omit SHA and serialize line numbers
        // as strings; that is repairable only while the indexed bytes match.
        let index=crate::engine::code_index::refresh(&state,&dir).unwrap();
        state.store.put("session",&json!({
            "id":"s","rsiRun":true,"rsiCodeIndexId":index["id"]
        }));
        let grounded=replace_line_span(&ctx,&path,
            &json!({"startLine":"2","endLine":"2"}),
            "let untouched = 9;","run").await.unwrap();
        assert_eq!(grounded["written"],true,"{grounded}");
        assert_eq!(grounded["sourceHashRecoveredFromIndex"],true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 3;\nlet untouched = 9;\n");
        // An actual Qwen local-model campaign failed twice after swapping
        // absolute start/end coordinates. Reordering a bounded, version-pinned
        // span is safe; guessing a relative line count is not.
        let again=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let corrected=replace_line_span(&ctx,&path,
            &json!({"startLine":2,"endLine":1,"expectedSha256":again}),
            "let new_value = 6;\nlet untouched = 9;","run").await.unwrap();
        assert_eq!(corrected["written"],true,"{corrected}");
        assert_eq!(corrected["reorderedLineEndpoints"],true);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 6;\nlet untouched = 9;\n");
        let after=format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap()));
        let rejected=replace_line_span(&ctx,&path,
            &json!({"startLine":1,"endLine":0,"expectedSha256":after}),
            "bad","run").await;
        assert!(rejected.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "let new_value = 6;\nlet untouched = 9;\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn real_model_edit_without_end_line_recovers_only_unambiguous_intent() {
        let function="pub fn evaluate_policy(value: u64) -> u64 {\n    value + 1\n}\n";
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"endLine":0}),function),
            Some(RsiEditIntent::CompleteFunction("evaluate_policy".into()))
        );
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"startLine":"7"}),"let x=3;"),
            Some(RsiEditIntent::SingleLine(7))
        );
        // A multi-line snippet without an explicit final coordinate has no
        // unique range. The model must provide one, not let the harness guess.
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":0}),
            "let x=3;\nlet y=4;\n"),None);
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":0}),
            "pub fn a() {\n}\npub fn b() {\n}\n"),None);
        // A valid explicit span is never rewritten into the fallback path.
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":7,"endLine":8}),function),None);
    }

    #[tokio::test]
    async fn descendant_function_edit_without_end_line_writes_real_sha_pinned_rust() {
        use crate::{config::Config,db::Store,vault::Vault};
        let root=std::env::temp_dir().join(format!(
            "rsi-compiled-descendant-tool-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        let path=root.join("src/engine/decision.rs");
        std::fs::write(&path,
            "pub fn choose(value:u32)->u32 {\n    value + 1\n}\n\npub fn keep()->bool {\n    true\n}\n").unwrap();
        let config=Config{port:0,data_dir:root.join("data"),
            web_dir:root.join("web"),workspace:root.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()));
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        state.store.put("session",&json!({"id":"descendant","rsiRun":true,
            "rsiCodeIndexId":index["id"]})).unwrap();
        let ctx=ToolContext{
            workspace:&root,store:&state.store,state:&state,
            session_id:"descendant",agent_id:None,
            tools_enabled:true,tools_execution_enabled:true
        };
        let arguments=json!({
            "path":"src/engine/decision.rs",
            "startLine":0,"endLine":0,
            "newText":"pub fn choose(value:u32)->u32 {\n    value + 2\n}\n"
        });
        let applied=execute(&ctx,"workspace_replace",&arguments,"run-1")
            .await.unwrap();
        assert_eq!(applied["written"],true,"{applied}");
        assert_eq!(applied["editContractRecovered"],
            "model_authored_complete_function");
        assert_eq!(applied["sourceHashRecoveredFromIndex"],true);
        let changed=std::fs::read_to_string(&path).unwrap();
        assert!(changed.contains("value + 2"));
        assert!(changed.contains("pub fn keep()->bool"));
        // The same index cannot approve a second write against new bytes.
        let replay=execute(&ctx,"workspace_replace",&arguments,"run-2")
            .await.unwrap();
        assert_eq!(replay["staleVersion"],true,"{replay}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),changed);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn rsi_can_read_only_a_range_or_replace_a_block() {
        assert!(rsi_allowed("code_search"));
        assert!(!rsi_allowed("workspace_list"));
        assert!(rsi_allowed("workspace_read_range"));
        assert!(rsi_allowed("workspace_replace"));
        assert!(rsi_allowed("workspace_write"));
        assert!(rsi_allowed("algorithm_propose"));
        assert!(rsi_allowed("unsafe_audit"));
        assert!(!rsi_allowed("workspace_read"));
        assert!(!rsi_allowed("shell_exec"));
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

