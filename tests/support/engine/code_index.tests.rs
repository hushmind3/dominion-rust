#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};


    #[test]
    fn invalid_target_expression_falls_back_for_all_files_not_a_mixed_ranking() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/memory.rs"),
            "fn memory_dispatch() { println!(\"memory\"); }\n").unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn memory_dispatch() { println!(\"memory\"); }\n").unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let empty=BTreeMap::new();
        let baseline=rank_focus_files(&state,id,"memory dispatch","memory",&empty,None,2);
        let malformed=json!({"revision":"trial","parameters":{
            "targetExpression":"1 / pathEvidence"
        }});
        let fallback=rank_focus_files(&state,id,"memory dispatch","memory",
            &empty,Some(&malformed),2);
        assert_eq!(fallback.iter().map(|row|&row["path"]).collect::<Vec<_>>(),
            baseline.iter().map(|row|&row["path"]).collect::<Vec<_>>());
        assert_eq!(fallback.len(),2);
        for row in &fallback {
            assert_eq!(row["scoring"],"corpus-evidence");
            assert!(row["policyError"].as_str().is_some());
            assert_eq!(row["score"],row["baselineScore"]);
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_productive_feedback_changes_target_ranking_without_hardcoded_paths() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        for name in ["alpha","beta"] {
            std::fs::write(source.join(format!("src/{name}.rs")),
                "fn memory_rank() { println!(\"memory retrieval\"); }\n").unwrap();
        }
        let meta=refresh(&state,&source).unwrap();
        let id=meta["id"].as_str().unwrap();
        let empty=BTreeMap::new();
        let before=rank_focus_files(&state,id,"memory rank","memory",&empty,None,2);
        assert_eq!(before[0]["path"],"src/alpha.rs");
        let learned=BTreeMap::from([("src/beta.rs".into(),1)]);
        let after=rank_focus_files_with_feedback(&state,id,"memory rank","memory",
            &empty,&learned,None,2);
        assert_eq!(after[0]["path"],"src/beta.rs");
        assert_eq!(after[0]["evidence"]["priorProductive"],1.0);
        let control=serde_json::json!({"revision":"trial-1",
            "parameters":{"targetExpression":"pathEvidence + symbolEvidence - productiveEvidence"}});
        let trial=rank_focus_files_with_feedback(&state,id,"memory rank","memory",
            &empty,&learned,Some(&control),2);
        assert_eq!(trial[0]["path"],"src/alpha.rs",
            "an evolvable formula still controls the final file selection");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn ai_target_policy_can_explore_sources_outside_the_builtin_keyword_pool() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn iterate() { println!(\"unrelated\"); }\n").unwrap();
        std::fs::write(source.join("src/neutral.rs"),
            "fn dispatch() { println!(\"generic task\"); }\n").unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let empty=BTreeMap::new();
        let objective="entirely novel architecture";
        assert!(rank_focus_files(&state,id,objective,"",&empty,None,2).is_empty(),
            "builtin still uses evidence gate to avoid guessing files");
        let explorer=json!({"revision":"exploration-1",
            "parameters":{"targetExpression":"novelty - sourceCost"}});
        let selected=rank_focus_files(&state,id,objective,"",
            &empty,Some(&explorer),2);
        assert_eq!(selected.len(),2,
            "user-model exploration must not inherit the built-in lexical filter");
        assert!(selected.iter().all(|row|row["scoring"]=="expression"
            && row["lexicalEvidencePresent"]==false));
        let broken=json!({"revision":"broken-1",
            "parameters":{"targetExpression":"1 / pathEvidence"}});
        assert!(rank_focus_files(&state,id,objective,"",
            &empty,Some(&broken),2).is_empty(),
            "invalid trial must restore builtin *pool*, not only scores");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn file_targeting_ignores_embedded_substring_noise_and_resolves_identifiers() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/unsearchable.rs"),
            "fn unsearchable() { println!(\"unsearchable\"); }\n").unwrap();
        std::fs::write(source.join("src/unranked.rs"),
            "fn unranked() { println!(\"unranked\"); }\n").unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn searchHandler() { println!(\"search\"); }\nfn BM25Ranker() {}\n").unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let search=rank_focus_files(&state,id,"search","",&BTreeMap::new(),None,8);
        assert_eq!(search.len(),1,"substring decoys must not be treated as evidence: {search:?}");
        assert_eq!(search[0]["path"],"src/opaque.rs");
        let rank=rank_focus_files(&state,id,"bm25","",&BTreeMap::new(),None,8);
        assert_eq!(rank.len(),1);
        assert_eq!(rank[0]["path"],"src/opaque.rs");
        let noise=rank_focus_files(&state,id,"rank","",&BTreeMap::new(),None,8);
        assert!(noise.is_empty(),"rank cannot match unranked or ranker as a substring");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn opaque_implementation_is_discovered_from_body_without_a_filename_hint() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/neutral.rs"),
            "/*\nbm25 retrieval appears only in a block comment\n*/\nfn unrelated() {} // bm25 again\n").unwrap();
        std::fs::write(source.join("src/opaque.rs"),
            "fn hidden_worker() {\n    let api_marker = \"bm25\";\n    println!(\"{api_marker}\");\n}\n").unwrap();
        let index = refresh(&state, &source).unwrap();
        let id = index["id"].as_str().unwrap();
        let candidates = rank_focus_files(&state, id, "bm25", "", &BTreeMap::new(), None, 2);
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0]["path"], "src/opaque.rs");
        assert!(candidates[0]["evidence"]["body"].as_f64().unwrap() > 0.0);
        // A file edit invalidates the old body terms on the next index refresh.
        std::fs::write(source.join("src/opaque.rs"),
            "fn hidden_worker() { println!(\"different\"); }\n").unwrap();
        let refreshed = refresh(&state, &source).unwrap();
        assert_eq!(refreshed["changedFiles"], 1);
        assert!(rank_focus_files(&state, id, "bm25", "", &BTreeMap::new(), None, 2).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn large_file_excerpts_recover_implementations_after_the_symbol_index_cap() {
        let (state,root) = temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code=(0..450).map(|n|format!("fn unrelated_{n}() {{}}\n")).collect::<String>();
        code.push_str("fn late_entrypoint() {\n    let marker = \"deep_retrieval_needle\";\n    println!(\"{marker}\");\n}\n");
        std::fs::write(source.join("src/opaque.rs"),code).unwrap();
        let index=refresh(&state,&source).unwrap();
        let id=index["id"].as_str().unwrap();
        let file=state.store.get(FILE_KIND,&file_id(id,"src/opaque.rs")).unwrap();
        assert_eq!(file["symbolsTruncated"],true);
        let excerpt=bounded_focus_context(&state,&source,id,"src/opaque.rs","needle","").unwrap();
        assert!(excerpt.contains("fn late_entrypoint()"),"{excerpt}");
        assert!(excerpt.contains("deep_retrieval_needle"),"{excerpt}");
        assert!(!excerpt.contains("fn unrelated_0()"),"{excerpt}");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_discovers_opaque_function_body_and_its_caller() {
        let (state,root)=temp_state();
        let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code=(0..350).map(|i|format!("fn neutral_{i}() {{}}\n")).collect::<String>();
        code.push_str("fn hidden_worker() {\n    let x = \"BM25_SIGNAL_VISIBLE\";\n    println!(\"{x}\");\n}\n");
        code.push_str("fn caller_entrypoint() {\n    hidden_worker(); // DEPENDENT_CALLER_VISIBLE\n}\n");
        std::fs::write(source.join("src/opaque.rs"),&code).unwrap();
        let meta=refresh(&state,&source).unwrap();
        let excerpt=bounded_focus_context(&state,&source,meta["id"].as_str().unwrap(),
            "src/opaque.rs","bm25 retrieval","memory").unwrap();
        assert!(excerpt.contains("BM25_SIGNAL_VISIBLE"),"{excerpt}");
        assert!(excerpt.contains("DEPENDENT_CALLER_VISIBLE"),"{excerpt}");
        assert!(!excerpt.contains("fn neutral_0()"),"{excerpt}");
        assert!(excerpt.chars().count()<8_100);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_prefers_relevant_spans_without_full_file_retransmission() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut code = (0..180).map(|n| format!("fn unrelated_{n}() {{ println!(\"nope\"); }}\n")).collect::<String>();
        code.push_str("fn retrieve_memory() {\n    let target = \"RELEVANT_MARKER\";\n    println!(\"{target}\");\n}\n");
        code.push_str(&(0..100).map(|n| format!("fn tail_{n}() {{}}\n")).collect::<String>());
        std::fs::write(source.join("src/memory.rs"), &code).unwrap();
        let meta = refresh(&state, &source).unwrap();
        let excerpt = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/memory.rs", "기억 검색 정확도", "memory").unwrap();
        assert!(excerpt.contains("RELEVANT_MARKER"));
        assert!(!excerpt.contains("fn unrelated_0()"));
        assert!(excerpt.chars().count() < 8_100);
        std::fs::write(source.join("src/memory.rs"), format!("{code}\n// changed")).unwrap();
        let stale = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/memory.rs", "기억 검색", "memory").unwrap();
        assert!(stale.contains("해시가 달라"), "a stale index must not fail silently: {stale}");
        assert!(stale.contains("workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_focus_keeps_small_source_complete() {
        let (state, root) = temp_state();
        let source = root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        std::fs::write(source.join("src/small.rs"), "fn small() { println!(\"hello\"); }\n").unwrap();
        let meta = refresh(&state, &source).unwrap();
        let excerpt = bounded_focus_context(&state, &source, meta["id"].as_str().unwrap(),
            "src/small.rs", "small", "memory").unwrap();
        assert!(excerpt.contains("대상 파일 전체"));
        assert!(excerpt.contains("println!(\"hello\")"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rsi_navigation_reuses_the_index_without_injecting_source_or_hotspots() {
        let (state,root)=temp_state();let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let content=format!("pub fn target() {{\n// {}\n}}\n","SOURCE_ONLY_NOT_A_TASK ".repeat(4000));
        std::fs::write(source.join("src/large.rs"),content).unwrap();
        let (first,overview)=prompt_context(&state,&source,&[]).unwrap();
        assert_eq!(first["files"],1);assert!(!overview.contains("SOURCE_ONLY_NOT_A_TASK"));
        assert!(!overview.contains("정적 발견 우선순위"));
        let (next,focused)=prompt_context(&state,&source,&["src/large.rs".into()]).unwrap();
        assert_eq!(next["changedFiles"],0);assert_eq!(next["reusedFiles"],1);
        assert!(focused.contains("target:1"));assert!(focused.contains("src/large.rs"));
        assert!(!focused.contains("SOURCE_ONLY_NOT_A_TASK"));
        assert!(focused.chars().count()<1000);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn focused_navigation_includes_late_implementation_symbols_before_module_overview(){
        let (state,root)=temp_state();let source=root.join("repo");
        std::fs::create_dir_all(source.join("src")).unwrap();
        let mut text=(0..25).map(|i|format!("fn helper_{i}() {{}}\n")).collect::<String>();
        text.push_str("fn actual_search_hot_path() {}\n#[test]\nfn test_should_not_fill_navigation() {}\n");
        std::fs::write(source.join("src/focused.rs"),text).unwrap();
        let (_,block)=prompt_context(&state,&source,&["src/focused.rs".into()]).unwrap();
        assert!(block.contains("actual_search_hot_path:26"));
        assert!(!block.contains("test_should_not_fill_navigation"));
        assert!(block.find("actual_search_hot_path").unwrap()<block.find("### 모듈 지도").unwrap());
        let _=std::fs::remove_dir_all(root);
    }

    fn temp_state() -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!("dominion-index-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let cfg = Config {
            port: 0,
            data_dir: data.clone(),
            web_dir: data.join("web"),
            workspace: data.clone(),
        };
        let state = AppState::new(
            cfg.clone(),
            Store::open(&cfg.db_path()).unwrap(),
            Vault::open(&cfg.key_path()),
        );
        (state, root)
    }

    #[test]
    fn a_full_inspection_is_reused_and_only_reparses_changed_files() {
        let (state, root) = temp_state();
        let source = root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(
            source.join("src/engine/rsi_evolve.rs"),
            "//! rsi\npub fn run_loop() {\n    // TODO: remove the cap\n}\n#[test]\nfn a_test() {}\n",
        )
        .unwrap();
        let first = refresh(&state, &source).unwrap();
        assert_eq!(first["files"], 1);
        assert_eq!(first["symbols"], 2);
        assert_eq!(first["tests"], 1);
        assert_eq!(first["todos"], 1);
        assert_eq!(first["changedFiles"], 1);
        let second = refresh(&state, &source).unwrap();
        assert_eq!(second["changedFiles"], 0);
        assert_eq!(second["reusedFiles"], 1);
        std::fs::write(
            source.join("src/engine/rsi_evolve.rs"),
            "pub fn run_loop() {}\n",
        )
        .unwrap();
        let third = refresh(&state, &source).unwrap();
        assert_eq!(third["changedFiles"], 1);
        assert_eq!(third["todos"], 0);
        let (_, block) = prompt_context(&state, &source, &["src/engine/rsi_evolve.rs".into()]).unwrap();
        assert!(block.contains("src/engine/rsi_evolve.rs"));
        assert!(block.contains("run_loop"));
        let _ = std::fs::remove_dir_all(root);
    }
}
