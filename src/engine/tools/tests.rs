    use super::*;
    #[test]
    fn rsi_model_edit_contract_recovers_only_unambiguous_source_intent() {
        let complete="pub fn assess(value: u64) -> u64 {\n    value + 1\n}\n";
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"endLine":0}),complete),
            Some(RsiEditIntent::CompleteFunction("assess".into()))
        );
        assert_eq!(
            rsi_unambiguous_edit_intent(&json!({"startLine":"11"}),"let count=1;"),
            Some(RsiEditIntent::SingleLine(11))
        );
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":11}),"let a=1;\nlet b=2;\n"),None);
        assert_eq!(rsi_unambiguous_edit_intent(
            &json!({"startLine":11,"endLine":12}),complete),None);
    }


    #[tokio::test]
    async fn rsi_navigation_finds_literal_diagnostics_and_honors_the_read_contract() {
        use crate::{config::Config,db::Store,vault::Vault};
        let dir=std::env::temp_dir().join(format!("rsi-navigation-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let mut lines=(1..=450).map(|n|format!("// ordinary source line {n}")).collect::<Vec<_>>();
        lines[229]="// unique_runtime_diagnostic_token".into();
        std::fs::write(dir.join("src/subject.rs"),lines.join("\n")).unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"rsi","rsiRun":true}));
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,session_id:"rsi",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        let found=search_code_index(&ctx,"unique_runtime_diagnostic_token",10).unwrap();
        assert_eq!(found["matches"][0]["path"],"src/subject.rs");
        assert_eq!(found["matches"][0]["line"],230);
        let read=execute(&ctx,"workspace_read_range",&json!({"path":"src/subject.rs","startLine":1,"endLine":400}),"test").await.unwrap();
        assert_eq!(read["endLine"],400);
        assert!(read["content"].as_str().unwrap().contains("ordinary source line 400"));
        let _=std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rsi_cannot_read_or_overwrite_the_coordinators_holdout_source() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir=std::env::temp_dir().join(format!("rsi-holdout-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/engine")).unwrap();
        let path="src/engine/rsi_oracle_fixture.rs";
        std::fs::write(dir.join(path),"secret holdout examples").unwrap();
        let config=Config{port:0,data_dir:dir.join("data"),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        state.store.put("session",&json!({"id":"rsi","rsiRun":true})).unwrap();
        let ctx=ToolContext{workspace:&dir,store:&state.store,state:&state,session_id:"rsi",
            agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        for tool in ["workspace_read_range","workspace_write","workspace_replace","workspace_replace_range"] {
            let error=execute(&ctx,tool,&json!({"path":path,"content":"overwrite"}),"run").await.unwrap_err();
            assert!(error.contains("동결 평가"),"{tool}: {error}");
        }
        crate::engine::code_index::refresh(&state,&dir).unwrap();
        assert_eq!(search_code_index(&ctx,"rsi_oracle_fixture",10).unwrap()["matchCount"],0);
        assert_eq!(std::fs::read_to_string(dir.join(path)).unwrap(),"secret holdout examples");
        let _=std::fs::remove_dir_all(dir);
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
            .unwrap();
        assert_eq!(ambiguous["occurrences"], 2, "{ambiguous}");
        assert!(result["bytes"].as_u64().unwrap() > 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn range_replacement_edits_one_repeated_line_with_frozen_source_and_rejects_stale_edits() {
        use crate::{config::Config, db::Store, vault::Vault};
        let dir=std::env::temp_dir().join(format!("dominion-range-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let config=Config {port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()};
        let state=crate::state::AppState::new(config.clone(),
            Store::open(&config.db_path()).unwrap(),Vault::open(&config.key_path()));
        let source="first();\nlet value = 0;\nlet value = 0;\nlast();\n";
        let path=dir.join("src/example.rs");
        std::fs::write(&path,source).unwrap();
        let digest=format!("{:x}",Sha256::digest(source.as_bytes()));
        let ctx=ToolContext {workspace:&dir,store:&state.store,state:&state,
            session_id:"s",agent_id:None,tools_enabled:true,tools_execution_enabled:true};
        state.store.put("session",&json!({"id":"s","rsiRun":true})).unwrap();
        let no_op=execute(&ctx,"workspace_replace_range",
            &json!({"path":"src/example.rs","startLine":3,"endLine":3,
                "newText":"let value = 0;","expectedSha256":digest}),"run").await.unwrap();
        assert_eq!(no_op["unchanged"],true);
        assert!(no_op["error"].is_string());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),source);
        let args=json!({"path":"src/example.rs","startLine":3,"endLine":3,
            "newText":"let value = 42;","expectedSha256":digest});
        let result=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(result["replaced"],true,"{result}");
        assert_eq!(result["replacedLines"],1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet value = 0;\nlet value = 42;\nlast();\n");
        let stale=execute(&ctx,"workspace_replace_range",&args,"run").await.unwrap();
        assert_eq!(stale["staleVersion"],true,"{stale}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(),
            "first();\nlet value = 0;\nlet value = 42;\nlast();\n");
        let mut bad=args.clone();
        bad["endLine"]=json!(999);
        bad["expectedSha256"]=json!(format!("{:x}",Sha256::digest(std::fs::read(&path).unwrap())));
        assert!(execute(&ctx,"workspace_replace_range",&bad,"run").await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
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
        let schema = definitions().into_iter()
            .find(|tool| tool["function"]["name"] == "algorithm_propose").unwrap();
        assert!(schema["function"]["parameters"]["properties"]["targetExpression"].is_object());
        let saved = execute(&ctx, "algorithm_propose", &proposal, "run")
            .await
            .unwrap();
        assert_eq!(saved["proposed"], true);
        assert!(state
            .store
            .get("algorithm", saved["algorithmId"].as_str().unwrap())
            .is_some());
        let source_rule = json!({"name":"source-ranker","expression":"objectiveMatch + novelty",
            "targetExpression":"pathEvidence * 3 + bodyEvidence - repeatPenalty - sourceCost"});
        let ranked = execute(&ctx, "algorithm_propose", &source_rule, "run").await.unwrap();
        assert_eq!(ranked["algorithm"]["parameters"]["targetExpression"],
            source_rule["targetExpression"]);
        let invalid_target = json!({"expression":"novelty",
            "targetExpression":"invalidSourceFeature + pathEvidence"});
        assert!(execute(&ctx, "algorithm_propose", &invalid_target, "run").await.is_err());
        let invalid = json!({"expression":"unknownVariable * 2"});
        assert!(execute(&ctx, "algorithm_propose", &invalid, "run").await.is_err());
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
    fn public_http_fetch_blocks_internal_and_redirect_destinations() {
        for url in ["http://127.0.0.1:4317","http://10.1.2.3","http://172.16.1.1",
            "http://192.168.1.1","http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.100","http://[::1]/","http://[::ffff:127.0.0.1]/",
            "http://router.local/","http://metadata.google.internal/",
            "file:///etc/passwd","http://localhost:80/"] {
            assert!(public_http_url(url).is_err(),"private target passed: {url}");
        }
        assert!(public_http_url("https://example.com/").is_ok());
        assert!(public_http_url("https://1.1.1.1/").is_ok());
        assert!(public_http_url("https://[2606:4700:4700::1111]/").is_ok());
        assert!(public_http_url("https://127.0.0.1@public.example/").is_err());
        let origin=public_http_url("https://example.com/").unwrap();
        let redirected=origin.join("http://127.0.0.1:4317/private").unwrap();
        assert!(public_http_url(redirected.as_str()).is_err(),
            "redirect must not bypass local address protection");
    }

    #[test]
    fn http_response_capture_stops_at_byte_limit() {
        let mut bytes=Vec::new();
        assert!(!append_http_chunk(&mut bytes,&vec![b'a';HTTP_FETCH_LIMIT-4]));
        assert!(append_http_chunk(&mut bytes,b"abcdefghij"));
        assert_eq!(bytes.len(),HTTP_FETCH_LIMIT);
        assert_eq!(&bytes[HTTP_FETCH_LIMIT-4..],b"abcd");
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
