    #[test]
    fn capability_usage_is_read_from_real_operation_records() {
        let (state, root) = state();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op1","sessionId":"s1","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op2","sessionId":"s1","tool":"workspace_read_range","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op3","sessionId":"s2","tool":"code_search","status":"completed"}),
            )
            .unwrap();
        state
            .store
            .put(
                "execution_operation",
                &json!({"id":"op4","sessionId":"s1","tool":"workspace_read","status":"failed"}),
            )
            .unwrap();
        let usage = session_tool_usage(&state, "s1");
        assert_eq!(usage.len(), 2, "{usage:?}");
        assert!(usage.iter().any(|row| row["tool"] == "code_search"));
        assert!(usage
            .iter()
            .any(|row| row["tool"] == "workspace_read_range"));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn research_digest_lists_ranges_already_read() {
        let (state, root) = state();
        state
            .store
            .put("job", &json!({"id":"digest-job","sessionId":"digest-session"}));
        state
            .store
            .put("execution", &json!({"id":"digest-run","sessionId":"digest-session"}));
        state.store.put(
            "execution_operation",
            &json!({"id":"digest-op","runId":"digest-run","tool":"workspace_read_range","status":"completed",
                "arguments":{"path":"src/engine/memory.rs","startLine":10,"endLine":20}}),
        );
        let record = json!({"history":[{"jobId":"digest-job"}]});
        let text = research_digest(&state, &json!({}), &record);
        assert!(text.contains("src/engine/memory.rs 10-20"));
        assert!(text.contains("다시 읽지 마세요"));
        assert_eq!(research_digest(&state, &json!({}), &json!({"history":[]})), "");
        // Reuse an earlier campaign's read ledger only for a verified exact
        // descendant. The ID alone is not a source-lineage receipt.
        let prior=json!({"id":"digest-prior","sourceDir":"/repo","sourceBranch":"dominion-sub2",
            "baseCommit":"a".repeat(40),"sourceFingerprint":"b".repeat(64),
            "headCommit":"c".repeat(40),"history":[{
                "jobId":"digest-job","generatorCommit":"c".repeat(40)}]});
        state.store.put("rsi_campaign",&prior);
        let current=json!({"history":[],"sourceDir":"/repo","sourceBranch":"dominion-sub2",
            "baseCommit":"a".repeat(40),"sourceFingerprint":"b".repeat(64),
            "headCommit":"c".repeat(40),
            "lineage":{"reused":true,"selectedCommit":"c".repeat(40)}});
        let previous=json!({"rsiContinueFrom":"digest-prior"});
        let continued=research_digest(&state,&previous,&current);
        assert!(continued.contains("src/engine/memory.rs 10-20"));
        let mut revised=current.clone();
        revised["headCommit"]=json!("d".repeat(40));
        assert!(!research_digest(&state,&previous,&revised).contains("src/engine/memory.rs 10-20"),
            "read offsets from an old source revision must not suppress new inspections");
        let mut foreign=current.clone();
        foreign["sourceBranch"]=json!("dominion-sub3");
        assert!(!research_digest(&state,&previous,&foreign).contains("src/engine/memory.rs 10-20"),
            "a sibling branch must not inject source offsets into a new campaign");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn owner_goal_constrains_domain_even_when_another_domain_is_new_or_profitable() {
        let algorithm=json!({"implementation":"failure-clusters"});
        let history=vec![
            json!({"domain":"memory","accepted":false,"candidateId":null,"tokens":80_000,"newFindings":0}),
            json!({"domain":"memory","accepted":false,"candidateId":null,"tokens":70_000,"newFindings":0}),
            json!({"domain":"learning","accepted":true,"candidateId":"c","tokens":3_000,"newFindings":1})
        ];
        assert_eq!(select_domain(&algorithm,&history,"기억 검색 최적화"),"memory");
        assert_eq!(select_domain(&algorithm,&history,"대화 문맥 검색"),"context");
        assert_eq!(select_domain(&algorithm,&history,"브라우저 검색 도구"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"텔레그램 메신저 연결"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"로컬 모델 다운로드"),"tools");
        assert_eq!(select_domain(&algorithm,&history,"학습 기록 검색"),"learning");
        // With no owner scope, productive low-cost work should beat repeated
        // barren token spend when comparing the two domains directly.
        let productive=search_features(&history,"learning","");
        let expensive=search_features(&history,"memory","");
        assert!(score_domain(&algorithm,&productive)>score_domain(&algorithm,&expensive));
        assert!(expensive["barrenRate"]>0.9);
    }

    #[test]
    fn generated_target_expression_actually_changes_file_selection() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::write(root.join("src/engine/memory_first.rs"),
            "fn memory_dispatch() {}\n").unwrap();
        std::fs::write(root.join("src/engine/memory_second.rs"),
            format!("fn memory_dispatch() {{}}\n{}", "// non-goal padding\n".repeat(45))).unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiObjective":"memory dispatch",
            "rsiTargetDomain":"memory"});
        let baseline=choose_focus_files(&state,&input,&[],None);
        assert_eq!(baseline[0]["path"],"src/engine/memory_first.rs");
        let candidate=crate::engine::algorithms::register(&state,&json!({
            "kind":"selfimprove.search","name":"runtime-target-formula","implementation":"expression",
            "parameters":{"expression":"objectiveMatch","targetExpression":"sourceCost"}
        })).unwrap();
        assert_eq!(candidate["status"],"available");
        let trial=choose_focus_files(&state,&input,&[],Some(&candidate));
        assert_eq!(trial[0]["path"],"src/engine/memory_second.rs",
            "a model-authored formula must change an actual selection, not just be stored");
        assert_eq!(trial[0]["scoring"],"expression");
        // A malformed runtime candidate does not fail a generation or make
        // non-finite ordering decisions. It is surfaced as policyError.
        let mut corrupt=candidate.clone();
        corrupt["parameters"]["targetExpression"]=json!("sourceCost / (novelty - novelty)");
        let safe=choose_focus_files(&state,&input,&[],Some(&corrupt));
        assert_eq!(safe[0]["path"],"src/engine/memory_first.rs");
        assert!(safe[0]["policyError"].as_str().is_some());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn auto_targeting_uses_current_index_and_never_requires_a_fixed_filename() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::write(root.join("src/engine/custom_retrieval.rs"),
            "fn memory_search_rank() { /* BM25 memory recall */ }\n").unwrap();
        std::fs::write(root.join("web/src/RoomPanel.tsx"),
            "export function RoomPanel() { return 'room chat ui'; }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],
            "rsiObjective":"기억 검색 BM25 순위 품질","rsiTargetDomain":"memory"});
        let candidates=choose_focus_files(&state,&input,&[],None);
        assert_eq!(candidates[0]["path"],"src/engine/custom_retrieval.rs");
        assert!(candidates[0]["score"].as_f64().unwrap()>0.0);
        let ui=choose_focus_files(&state,&json!({"rsiIndexId":index["id"],
            "rsiObjective":"room panel UI","rsiTargetDomain":"ui"}),&[],None);
        assert_eq!(ui[0]["path"],"web/src/RoomPanel.tsx");
        std::fs::write(root.join("src/engine/telegram.rs"),
            "fn connect_telegram() {}\n").unwrap();
        std::fs::write(root.join("src/engine/models.rs"),
            "fn download_model() {}\n").unwrap();
        let extended=crate::engine::code_index::refresh(&state,&root).unwrap();
        let messenger=choose_focus_files(&state,&json!({"rsiIndexId":extended["id"],
            "rsiObjective":"텔레그램 연결","rsiTargetDomain":"tools"}),&[],None);
        assert_eq!(messenger[0]["path"],"src/engine/telegram.rs");
        let models=choose_focus_files(&state,&json!({"rsiIndexId":extended["id"],
            "rsiObjective":"로컬 모델 다운로드","rsiTargetDomain":"tools"}),&[],None);
        assert_eq!(models[0]["path"],"src/engine/models.rs");
        let no_match=choose_focus_files(&state,&json!({"rsiIndexId":index["id"],
            "rsiObjective":"quantumqzx uniquezzz","rsiTargetDomain":""}),&[],None);
        assert!(no_match.is_empty(),"an unknown goal must trigger discovery, not a forced source");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_unproductive_targeting_downranks_the_failed_source() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src/engine")).unwrap();
        for name in ["search_first.rs","search_second.rs"] {
            std::fs::write(root.join("src/engine").join(name),
                "fn search_dispatch() { /* same relevant implementation */ }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let first=choose_focus_files(&state,&input,&[],None);
        assert_eq!(first[0]["path"],"src/engine/search_first.rs");
        let history=vec![json!({"accepted":false,"candidateId":null,"newFindings":0,
            "focusFiles":["src/engine/search_first.rs"]})];
        let next=choose_focus_files(&state,&input,&history,None);
        assert_eq!(next[0]["path"],"src/engine/search_second.rs");
        // A verified candidate or a new finding is not a barren attempt.
        let useful=vec![json!({"accepted":false,"candidateId":"patch","newFindings":1,
            "focusFiles":["src/engine/search_first.rs"]})];
        assert_eq!(choose_focus_files(&state,&input,&useful,None)[0]["path"],
            "src/engine/search_first.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn verified_success_becomes_revision_scoped_target_learning_not_a_patch_counter() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let beta=root.join("src/beta.rs");
        let original="fn search_dispatch() { println!(\"search\"); }\n";
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),original).unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        assert_eq!(choose_focus_files(&state,&input,&[],None)[0]["path"],"src/alpha.rs");
        let selected_beta=crate::engine::code_index::rank_focus_files(
            &state,index["id"].as_str().unwrap(),"search dispatch","tools",
            &BTreeMap::new(),None,2).into_iter()
            .find(|v|v["path"]=="src/beta.rs").unwrap();
        // Model really edits beta; its post-adoption hash is different from
        // the revision that the target selector originally examined.
        let improved="fn search_dispatch() { println!(\"search improved\"); }\n";
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let accepted_hash=crate::engine::code_index::indexed_source_hash(
            &state,index["id"].as_str().unwrap(),"src/beta.rs").unwrap();
        assert_ne!(selected_beta["sourceHash"],accepted_hash);
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"verified",
            "paths":["src/beta.rs"],
            "evaluation":{"metrics":{"eligible":true}}
        })).unwrap();
        state.store.put("rsi_generation",&json!({
            "id":"prior-success","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":true,"candidateId":"verified-patch","newFindings":0,
            "focusFiles":["src/beta.rs"],"focusSelection":{"candidates":[selected_beta.clone()]},
            "acceptedSourceHashes":{"src/beta.rs":accepted_hash},
            "createdAt":"2026-11-10T00:00:00Z"
        })).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/beta.rs");
        assert_eq!(learned[0]["evidence"]["priorProductive"],1.0);

        // Reverting to *exactly* the pre-patch content must NOT resurrect
        // reward based on the old focusSelection.sourceHash.
        std::fs::write(&beta,original).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let reverted=choose_focus_files(&state,&input,&[],None);
        assert_eq!(reverted[0]["path"],"src/alpha.rs");
        assert_eq!(reverted.iter().find(|v|v["path"]=="src/beta.rs").unwrap()
            ["evidence"]["priorProductive"],0.0);

        std::fs::write(&beta,"fn search_dispatch() { println!(\"new revision\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let stale=choose_focus_files(&state,&input,&[],None);
        assert_eq!(stale[0]["evidence"]["priorProductive"],0.0);
        state.store.put("rsi_candidate",&json!({
            "id":"verified-patch","status":"rejected",
            "paths":["src/beta.rs"],"evaluation":{"metrics":{"eligible":false}}
        })).unwrap();
        std::fs::write(&beta,improved).unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let unverified=choose_focus_files(&state,&input,&[],None);
        assert_eq!(unverified[0]["evidence"]["priorProductive"],0.0);
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_local_target_revision_does_not_poison_updated_source() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for name in ["alpha","beta"] {
            std::fs::write(root.join("src").join(format!("{name}.rs")),
                "fn search_dispatch() { println!(\"search\"); }\n").unwrap();
        }
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiObjective":"search dispatch",
            "rsiTargetDomain":"tools"});
        let before=choose_focus_files(&state,&input,&[],None);
        let failure=json!({"id":"failed-1","candidateId":null,
            "accepted":false,"newFindings":0,
            "focusFiles":["src/alpha.rs"],"focusSelection":{"candidates":before}});
        assert_eq!(choose_focus_files(&state,&input,&[failure.clone()],None)[0]["path"],
            "src/beta.rs");
        std::fs::write(root.join("src/alpha.rs"),
            "fn search_dispatch() { println!(\"revised search\"); }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let recovered=choose_focus_files(&state,&input,&[failure],None);
        let alpha=recovered.iter().find(|item|item["path"]=="src/alpha.rs").unwrap();
        assert_eq!(alpha["evidence"]["priorUnproductive"],0.0,
            "failure of an old version cannot down-rank the rebuilt code");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn cross_campaign_target_failures_are_reused_only_at_same_source_revision() {
        let (state,root)=state();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let first=root.join("src/search_first.rs");
        let second=root.join("src/search_second.rs");
        std::fs::write(&first,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        std::fs::write(&second,"fn search_dispatch() { /* scan alpha */ }\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&root).unwrap();
        let input=json!({"rsiIndexId":index["id"],"rsiKnowledgeSource":root,
            "rsiObjective":"search dispatch","rsiTargetDomain":"tools"});
        let initial=choose_focus_files(&state,&input,&[],None);
        assert_eq!(initial[0]["path"],"src/search_first.rs");
        let prior=json!({"id":"persisted-prior","knowledgePartition":research_scope(&input).partition_key,
            "objectiveId":crate::engine::memory::stable_id(&["search dispatch"]),
            "accepted":false,"candidateId":null,"newFindings":0,
            "focusFiles":["src/search_first.rs"],
            "focusSelection":{"candidates":initial},
            "createdAt":"2026-11-10T00:00:00Z"});
        state.store.put("rsi_generation",&prior).unwrap();
        let learned=choose_focus_files(&state,&input,&[],None);
        assert_eq!(learned[0]["path"],"src/search_second.rs",
            "the later campaign should not re-spend its tokens on the same unproductive target");
        // Same-length edit is a different revision, so old failure evidence
        // can no longer drive the new source choice.
        std::fs::write(&first,"fn search_dispatch() { /* scan bravo */ }\n").unwrap();
        crate::engine::code_index::refresh(&state,&root).unwrap();
        let refreshed=choose_focus_files(&state,&input,&[],None);
        assert_eq!(refreshed[0]["path"],"src/search_first.rs",
            "stale failure evidence must not penalize changed code");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn automatically_found_source_does_not_redefine_the_users_objective() {
        let input=json!({"rsiObjective":"기억 검색","rsiAutoFocus":true,
            "rsiFocusFiles":["src/engine/tools.rs"]});
        assert_eq!(selection_objective(&input),"기억 검색");
    }

    #[test]
    fn autonomous_campaign_ignores_generic_prompt_domain_words() {
        let input=json!({"rsiAutonomous":true,
            "rsiObjective":"기존 코드 지식과 경험을 재사용해 실제 작업에 필요한 능력 하나를 개선한다",
            "rsiAutoFocus":true,"rsiFocusFiles":["src/engine/memory.rs"]});
        let scope=selection_objective(&input);
        assert_eq!(scope,"");
        assert_eq!(eligible_domains(&scope),DOMAINS.to_vec(),
            "generic bootstrap words must not constrain autonomous domain discovery");
        let explicit=json!({"rsiAutonomous":false,"rsiObjective":"학습 기록 검색"});
        assert_eq!(eligible_domains(&selection_objective(&explicit)),vec!["learning"]);
        let focus=json!({"rsiAutonomous":true,"rsiObjective":"기본 목표",
            "rsiFocusFiles":["web/src/pages/Memory.tsx"]});
        assert_eq!(eligible_domains(&selection_objective(&focus)),vec!["ui"],
            "owner-selected source files still constrain the candidate domain");
    }

