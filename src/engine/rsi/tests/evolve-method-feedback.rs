    #[test]
    fn real_method_feedback_changes_which_repair_variant_a_descendant_receives() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let scope=research_scope(&input).partition_key;
        let original=json!({"id":"old","revision":"r0","name":"source patch",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":1,"endLine":1,
                    "expectedSha256":"stale","newText":"fn changed() {}"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let failed=json!({"id":"failed","origin":"procedure","status":"failed",
            "tool":"workspace_replace","procedureStrategyId":"old",
            "procedureRevision":"r0","procedureNodeId":"edit",
            "arguments":{"path":"src/engine/memory.rs","startLine":1,"endLine":1},
            "result":{"error":"stale","staleVersion":true}});
        let a=synthesize_stale_edit_repair(&original,&failed).unwrap();
        let b=synthesize_indexed_edit_repair(&original,&failed).unwrap();
        state.store.put("experience",&json!({"id":"two-variants",
            "status":"active","partitionKey":scope,
            "executableMethods":[a.clone(),b.clone()]})).unwrap();
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["methodIds"].as_array().unwrap().len(),1,
            "never execute two competing versions against the same source");
        assert_eq!(decision["repairMethodId"],a["id"],
            "read-and-rebase is the first untried conservative operator");
        let mut experience=state.store.get("experience","two-variants").unwrap();
        experience["executableMethods"][0]["applications"]=json!([{
            "verificationVerdict":"passed","postconditionsPassed":true,
            "accepted":false,"tokens":100}]);
        experience["executableMethods"][0]["status"]=json!("executable");
        state.store.put("experience",&experience).unwrap();
        let next=method_reuse_decision(&state,&input,"memory");
        assert_eq!(next["repairMethodId"],b["id"],
            "one verified method call without adoption is not proof of improvement");
        experience["executableMethods"][1]["successfulApplications"]=json!(1);
        experience["executableMethods"][1]["applications"]=json!([{
            "verificationVerdict":"passed","postconditionsPassed":true,
            "accepted":true,"tokens":60}]);
        experience["executableMethods"][1]["status"]=json!("executable");
        state.store.put("experience",&experience).unwrap();
        let adopted=method_reuse_decision(&state,&input,"memory");
        assert_eq!(adopted["repairMethodId"],b["id"],
            "observed adoption should determine the future execution method");
        assert_eq!(adopted["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_source_read_retargets_the_actual_indexed_file_and_postconditions() {
        let (state,root)=state();
        let repo=root.join("source");
        let source_dir=repo.to_string_lossy().to_string();
        let old="src/legacy/memory.rs";
        let new="src/engine/memory.rs";
        state.store.put("job",&json!({"id":"read-failure","kind":"rsi_generation",
            "status":"running","input":{
                "rsiSourceDir":source_dir,"rsiIndexId":"index-one"}})).unwrap();
        state.store.put("code_index",&json!({"id":"index-one",
            "sourceDir":source_dir,"files":1})).unwrap();
        state.store.put("code_index_file",&json!({"id":"new-file",
            "indexId":"index-one","path":new,"symbols":[]})).unwrap();
        let method=json!({"id":"old-source","revision":"v1","name":"memory refactor",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range","arguments":{
                        "path":old,"startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                        "path":old,"startLine":1,"endLine":1,
                        "expectedSha256":"old-hash","newText":"fn improved() {}"}}
                ],
                "after":[{"kind":"file_contains","path":old,"text":"fn improved()"}]}});
        let failed=json!({"id":"missing-file","origin":"procedure",
            "status":"failed","tool":"workspace_read_range",
            "procedureStrategyId":"old-source","procedureRevision":"v1",
            "procedureNodeId":"read","arguments":{"path":old},
            "result":{"error":"No such file or directory (os error 2)"}});
        let generation=json!({"jobId":"read-failure","domain":"memory",
            "accepted":false,"newFindings":0});
        let repaired=synthesize_indexed_path_repair(&state,&method,&failed,&generation).unwrap();
        assert_eq!(repaired["synthesis"]["kind"],"indexed_path_retarget");
        assert_eq!(repaired["synthesis"]["oldPath"],old);
        assert_eq!(repaired["synthesis"]["targetPath"],new);
        assert_eq!(repaired["verification"]["nodes"][0]["arguments"]["path"],new);
        assert_eq!(repaired["verification"]["nodes"][1]["arguments"]["path"],new);
        assert_eq!(repaired["verification"]["after"][0]["path"],new);
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &repaired,&[],"memory",&results).unwrap();
        assert_eq!(read,"read");
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["path"],new);
        results.insert("read".to_string(),json!({"sourceSha256":"new-sha"}));
        let observed=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":repaired["id"],"procedureRevision":repaired["revision"],
            "procedureNodeId":"read","result":{"sourceSha256":"new-sha"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &repaired,&[observed],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["path"],new);
        assert_eq!(args["expectedSha256"],"new-sha");
        // The indexed identity is part of the compiler's evidence. If there
        // are multiple possible targets it must not rewrite either of them.
        state.store.put("code_index_file",&json!({"id":"ambiguous-file",
            "indexId":"index-one","path":"src/other/memory.rs",
            "symbols":[]})).unwrap();
        assert!(synthesize_indexed_path_repair(
            &state,&method,&failed,&generation).is_none());
        let mut wrong=failed.clone();
        wrong["result"]["error"]=json!("permission denied");
        assert!(synthesize_indexed_path_repair(
            &state,&method,&wrong,&generation).is_none());
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_existing_write_is_learned_for_next_generation_not_credited() {
        let (state,root)=state();
        let input=json!({"rsiSourceDir":root.join("source").to_string_lossy(),
            "rsiObjective":"memory improvement"});
        let partition=research_scope(&input).partition_key;
        let original=json!({"id":"create-only","revision":"write-v1","name":"create source",
            "domain":"memory","status":"executable","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"write","tool":"workspace_write","arguments":{
                    "path":"src/engine/memory.rs","content":"fn improved() {}\n"}}],
                "after":[{"kind":"tool_called","tool":"workspace_write",
                    "path":"src/engine/memory.rs"},
                    {"kind":"file_equals","path":"src/engine/memory.rs",
                    "text":"fn improved() {}\n"}]}});
        state.store.put("experience",&json!({"id":"old-method",
            "status":"active","partitionKey":partition,
            "executableMethods":[original]})).unwrap();
        state.store.put("execution_operation",&json!({"id":"blocked-write",
            "sessionId":"write-session","origin":"procedure","status":"failed",
            "procedureStrategyId":"create-only","procedureRevision":"write-v1",
            "procedureNodeId":"write","tool":"workspace_write",
            "arguments":{"path":"src/engine/memory.rs","content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}})).unwrap();
        state.store.put("session",&json!({"id":"write-session",
            "rsiMethodExecutions":[{"experienceId":"old-method",
                "methodId":"create-only","revision":"write-v1",
                "executionId":"run-one","operationIds":["blocked-write"],
                "result":{"verified":true,"status":"violated",
                "violations":["failed"]}}]})).unwrap();
        let generation=json!({"jobId":"gen-one","domain":"memory",
            "accepted":false,"newFindings":0,"tokens":50,"elapsedMs":1});
        record_method_feedback(&state,&generation,"write-session");
        assert_eq!(state.store.get("experience","old-method").unwrap()
            ["executableMethods"][0]["status"],"suspended");
        let proposal=state.store.get("session","write-session").unwrap()
            ["rsiMethodProposals"][0].clone();
        assert_eq!(proposal["status"],"candidate");
        assert_eq!(proposal["successfulApplications"],0);
        let saved=remember_research(&state,&input,&generation,"write-session").unwrap();
        assert_eq!(state.store.get("experience",&saved).unwrap()
            ["executableMethods"][0]["id"],proposal["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["action"],"execute_methods");
        assert_eq!(decision["repairMethodId"],proposal["id"]);
        assert_eq!(decision["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn adopted_method_can_synthesize_a_smaller_executable_dag_in_multiple_domains() {
        for (domain,path) in [
            ("memory","src/engine/memory.rs"),
            ("tools","src/engine/tools.rs"),
        ] {
            let (state,root)=state();
            let source=root.join("source");
            std::fs::create_dir_all(source.join("src/engine")).unwrap();
            std::fs::write(source.join(path),"fn original() {}\n").unwrap();
            let index=crate::engine::code_index::refresh(&state,&source).unwrap();
            state.store.put("job",&json!({"id":"accepted-generation",
                "kind":"rsi_generation","status":"running",
                "input":{"rsiIndexId":index["id"],"rsiSourceDir":source}})).unwrap();
            let method=json!({"id":"accepted-procedure","revision":"old-method-revision",
                "name":"source edit","domain":domain,
                "assistanceMode":"harness_assisted",
                "verification":{"type":"procedure",
                    "appliesTo":{"requestContains":["improve"]},
                    "nodes":[
                        {"id":"read","tool":"workspace_read_range","arguments":{
                            "path":path,"startLine":1,"endLine":1}},
                        {"id":"edit","tool":"workspace_replace","arguments":{
                            "path":path,"startLine":1,"endLine":1,
                            "expectedSha256":"$read.sourceSha256",
                            "newText":"fn improved() {}"}}
                    ],
                    "edges":[{"from":"read","to":"edit"}],
                    "after":[{"kind":"file_contains","path":path,
                        "text":"fn improved()"}]}});
            let sha="a".repeat(64);
            let read=json!({"id":"op-read","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"read",
                "tool":"workspace_read_range","result":{"sourceSha256":sha}});
            let edit=json!({"id":"op-edit","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"edit",
                "tool":"workspace_replace","arguments":{
                    "path":path,"expectedSha256":sha},
                "result":{"written":true,"replaced":true,
                    "postSha256":"c".repeat(64)}});
            let receipt=json!({"executionId":"execution-1",
                "methodId":"accepted-procedure","revision":"old-method-revision",
                "operationIds":["op-read","op-edit"],
                "result":{"verified":true,"followed":true,"status":"followed"}});
            state.store.put("rsi_candidate",&json!({"id":"accepted-source",
                "status":"verified","paths":[path],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut adopted_hashes=serde_json::Map::new();
            adopted_hashes.insert(path.to_string(),json!("c".repeat(64)));
            let generation=json!({"jobId":"accepted-generation","accepted":true,
                "candidateId":"accepted-source",
                "acceptedSourceHashes":Value::Object(adopted_hashes),
                "domain":domain,"tokens":500,"elapsedMs":5000});
            let candidate=synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&generation).unwrap();
            assert_eq!(candidate["synthesis"]["kind"],"successful_read_ablation");
            assert_eq!(candidate["status"],"candidate");
            assert_eq!(candidate["successfulApplications"],0);
            assert_eq!(candidate["synthesis"]["qualityGain"],Value::Null);
            state.store.put("rsi_candidate",&json!({"id":"unrelated-accepted",
                "status":"verified","paths":["src/engine/unrelated.rs"],
                "evaluation":{"metrics":{"eligible":true}}})).unwrap();
            let mut unrelated=generation.clone();
            unrelated["candidateId"]=json!("unrelated-accepted");
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&unrelated).is_none(),
                "a successful but unrelated method must not inherit a different source change");
            let mut changed_again=generation.clone();
            changed_again["acceptedSourceHashes"][path]=json!("e".repeat(64));
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&changed_again).is_none(),
                "a later write to the same path must not credit this method");
            assert_eq!(candidate["verification"]["nodes"].as_array().unwrap().len(),1);
            assert_eq!(candidate["verification"]["edges"],json!([]));
            let (node,tool,args)=crate::engine::procedure::ready_intent(
                &candidate,&[],"improve",&std::collections::HashMap::new()).unwrap();
            assert_eq!(node,"edit");
            assert_eq!(tool,"workspace_replace");
            assert_eq!(args["path"],path);
            assert!(args["expectedSha256"].is_null(),
                "the next descendant must bind SHA through its own source index");
            let denied=json!({"jobId":"accepted-generation","accepted":false});
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),edit.clone()],&denied).is_none());
            let not_written=json!({"id":"op-edit","status":"completed","origin":"procedure",
                "procedureStrategyId":"accepted-procedure",
                "procedureRevision":"old-method-revision","procedureNodeId":"edit",
                "tool":"workspace_replace","arguments":{
                    "path":path,"expectedSha256":"a".repeat(64)},
                "result":{"written":false}});
            assert!(synthesize_successful_dag_ablation(
                &state,&method,&receipt,&[read.clone(),not_written],&generation).is_none());
            let mut dependent=method.clone();
            dependent["verification"]["after"]=json!([{"kind":"tool_called",
                "tool":"workspace_read_range","path":path}]);
            assert!(synthesize_successful_dag_ablation(
                &state,&dependent,&receipt,&[read.clone(),edit.clone()],&generation).is_none());
            let mut different=method.clone();
            different["verification"]["nodes"][1]["arguments"]["expectedSha256"]=
                json!("manually-pinned-hash");
            assert!(synthesize_successful_dag_ablation(
                &state,&different,&receipt,&[read,edit],&generation).is_none());
            let _=std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn accepted_tool_trace_persists_structural_mutation_for_descendant_execution() {
        let (state,root)=state();
        let source=root.join("source");
        std::fs::create_dir_all(source.join("src/engine")).unwrap();
        std::fs::write(source.join("src/engine/memory.rs"),"fn original() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(&state,&source).unwrap();
        state.store.put("job",&json!({"id":"accepted-generation",
            "kind":"rsi_generation","status":"running",
            "input":{"rsiIndexId":index["id"],"rsiSourceDir":source}})).unwrap();
        let input=json!({"rsiSourceDir":source,"rsiObjective":"memory improve"});
        let parent=json!({"id":"accepted-parent","revision":"p1",
            "status":"candidate","assistanceMode":"harness_assisted",
            "name":"repair memory","domain":"memory",
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[
                    {"id":"read","tool":"workspace_read_range",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace",
                        "arguments":{"path":"src/engine/memory.rs",
                            "startLine":1,"endLine":1,
                            "newText":"fn improved() {}",
                            "expectedSha256":"$read.sourceSha256"}}
                ],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_contains","path":"src/engine/memory.rs",
                    "text":"fn improved()"}]}});
        state.store.put("experience",&json!({"id":"parent-experience",
            "partitionKey":research_scope(&input).partition_key,
            "status":"active","executableMethods":[parent]})).unwrap();
        for op in [
            json!({"id":"completed-read","sessionId":"accepted-session",
                "status":"completed","origin":"procedure",
                "tool":"workspace_read_range","procedureStrategyId":"accepted-parent",
                "procedureRevision":"p1","procedureNodeId":"read",
                "result":{"sourceSha256":"b".repeat(64)}}),
            json!({"id":"completed-edit","sessionId":"accepted-session",
                "status":"completed","origin":"procedure","tool":"workspace_replace",
                "procedureStrategyId":"accepted-parent",
                "procedureRevision":"p1","procedureNodeId":"edit",
                "arguments":{"path":"src/engine/memory.rs",
                    "expectedSha256":"b".repeat(64)},
                "result":{"written":true,"replaced":true,
                    "postSha256":"d".repeat(64)}})
        ] {
            state.store.put("execution_operation",&op).unwrap();
        }
        state.store.put("session",&json!({"id":"accepted-session",
            "rsiMethodExecutions":[{"experienceId":"parent-experience",
                "methodId":"accepted-parent","revision":"p1",
                "executionId":"accepted-execution",
                "operationIds":["completed-read","completed-edit"],
                "result":{"verified":true,"followed":true,"status":"followed"}}]})).unwrap();
        state.store.put("rsi_candidate",&json!({"id":"accepted-source",
            "status":"verified","paths":["src/engine/memory.rs"],
            "evaluation":{"metrics":{"eligible":true}}})).unwrap();
        let generation=json!({"jobId":"accepted-generation",
            "domain":"memory","accepted":true,"candidateId":"accepted-source",
            "acceptedSourceHashes":{"src/engine/memory.rs":"d".repeat(64)},
            "newFindings":1,"tokens":650,"elapsedMs":900});
        let first=record_method_feedback(&state,&generation,"accepted-session");
        assert_eq!(first.len(),1);
        let revised=state.store.get("session","accepted-session").unwrap();
        let candidate=revised["rsiMethodProposals"][0].clone();
        assert_eq!(candidate["synthesis"]["kind"],"successful_read_ablation");
        assert_eq!(candidate["status"],"candidate");
        assert_eq!(candidate["successfulApplications"],0);
        let baseline=state.store.get("experience","parent-experience").unwrap();
        assert_eq!(baseline["executableMethods"][0]["successfulApplications"],1);
        assert_eq!(baseline["executableMethods"][0]["status"],"executable");
        // Restarted coordinators must not count a finished execution twice.
        record_method_feedback(&state,&generation,"accepted-session");
        let saved=state.store.get("session","accepted-session").unwrap();
        assert_eq!(saved["rsiMethodProposals"].as_array().unwrap().len(),1);
        assert_eq!(state.store.get("experience","parent-experience").unwrap()
            ["executableMethods"][0]["applications"].as_array().unwrap().len(),1);
        let experience=remember_research(&state,&input,&generation,"accepted-session").unwrap();
        assert_eq!(state.store.get("experience",&experience).unwrap()
            ["executableMethods"][0]["id"],candidate["id"]);
        let decision=method_reuse_decision(&state,&input,"memory");
        assert_eq!(decision["repairMethodId"],candidate["id"]);
        assert_eq!(decision["repairSelectionEvidence"]["selection"],
            "success_ablation_trial");
        assert_eq!(decision["repairTarget"],"src/engine/memory.rs");
        let _=std::fs::remove_dir_all(root);
    }

