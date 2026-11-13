    #[tokio::test]
    async fn success_compacted_descendant_edits_source_via_real_indexed_tool_runtime() {
        use axum::{routing::post,Json,Router,response::IntoResponse};
        async fn provider(Json(_body):Json<Value>)->axum::response::Response {
            let response=format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":"완료"}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}));
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],response).into_response()
        }
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move {
            axum::serve(listener,Router::new().route("/chat/completions",post(provider)))
                .await.unwrap()
        });
        let (state,root)=state();
        let _parent=parent(&state);
        let path="src/engine/memory.rs";
        std::fs::create_dir_all(state.config.workspace.join("src/engine")).unwrap();
        std::fs::write(state.config.workspace.join(path),"fn old() {}\n").unwrap();
        let index=crate::engine::code_index::refresh(
            &state,&state.config.workspace).unwrap();
        state.store.put("provider",&json!({"id":"index-provider","kind":"openai",
            "baseUrl":format!("http://{address}"),"model":"test",
            "toolMode":"native","enabled":true})).unwrap();
        let input=json!({"rsiObjective":"memory improvement","rsiDomain":"memory",
            "rsiKnowledgeSource":state.config.workspace,
            "rsiSourceDir":state.config.workspace});
        let parent=json!({"id":"accepted-parent","revision":"p1",
            "name":"source patch","domain":"memory","status":"executable",
            "assistanceMode":"harness_assisted","successfulApplications":1,
            "applications":[{"accepted":true,"postconditionsPassed":true}],
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"read","tool":"workspace_read_range","arguments":{
                    "path":path,"startLine":1,"endLine":1}},
                    {"id":"edit","tool":"workspace_replace","arguments":{
                        "path":path,"startLine":1,"endLine":1,
                        "expectedSha256":"$read.sourceSha256",
                        "newText":"fn improved() {}\n"}}],
                "edges":[{"from":"read","to":"edit"}],
                "after":[{"kind":"file_equals","path":path,
                    "text":"fn improved() {}\n"}]}});
        let child=json!({"id":"compacted-child","revision":"c1",
            "name":"compact patch","domain":"memory","status":"candidate",
            "assistanceMode":"harness_assisted","successfulApplications":0,
            "synthesis":{"kind":"successful_read_ablation",
                "parentMethodId":"accepted-parent","parentRevision":"p1"},
            "verification":{"type":"procedure",
                "appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":path,"startLine":1,"endLine":1,
                    "expectedSha256":null,"newText":"fn improved() {}\n"}}],
                "after":[{"kind":"file_equals","path":path,
                    "text":"fn improved() {}\n"}]}});
        state.store.put("experience",&json!({"id":"compact-experience",
            "status":"active","partitionKey":research_scope(&input).partition_key,
            "executableMethods":[parent,child]})).unwrap();
        state.store.put("job",&json!({"id":"compact-generation",
            "kind":"rsi_generation","status":"running",
            "rsiWorkspace":state.config.workspace,
            "input":{"rsiCampaignId":"campaign",
                "rsiSourceDir":state.config.workspace,"rsiIndexId":index["id"]}})).unwrap();
        let mut session=json!({"id":"compact-session","providerId":"index-provider",
            "rsiRun":true,"rsiObjective":"memory improvement",
            "rsiCampaignId":"campaign",
            "rsiGenerationJobId":"compact-generation",
            "rsiCodeIndexId":index["id"]});
        configure_method_session(&state,&input,&mut session);
        assert_eq!(session["rsiDecisionInput"]["reuseDecision"]["repairMethodId"],
            "compacted-child");
        assert_eq!(method_strategies(&state,&session).len(),1);
        state.store.put("session",&session).unwrap();
        let request=crate::engine::chat::TurnRequest::from_value(&json!({
            "sessionId":"compact-session","input":"memory improvement",
            "toolsEnabled":true,"toolsExecutionEnabled":true,"autoLearning":false
        })).unwrap();
        tokio::time::timeout(Duration::from_secs(10),
            crate::engine::run_quiet(&state,&request)).await.unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(
            state.config.workspace.join(path)).unwrap(),"fn improved() {}\n");
        let operations=state.store.all("execution_operation").into_iter()
            .filter(|op|op["sessionId"]=="compact-session"
                && op["origin"]=="procedure").collect::<Vec<_>>();
        assert_eq!(operations.iter().filter(|op|op["tool"]=="workspace_read_range").count(),0,
            "learned compact program must truly omit the read");
        assert!(operations.iter().any(|op|
            op["tool"]=="workspace_replace" && op["status"]=="completed"
                && op["result"]["written"]==true
                && op["result"]["sourceHashRecoveredFromIndex"]==true));
        collect_method_executions(&state,"compact-session",&state.config.workspace);
        let receipt=state.store.get("session","compact-session").unwrap()
            ["rsiMethodExecutions"].as_array().cloned().unwrap_or_default();
        assert_eq!(receipt.len(),1);
        assert_eq!(receipt[0]["result"]["followed"],true);
        server.abort();
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_method_unknown_verification_is_held_not_failed() {
        let (state,root)=state();
        let workspace=&state.config.workspace;
        std::fs::create_dir_all(workspace.join("unreadable.txt")).unwrap();
        std::fs::write(workspace.join("known.txt"),"ok").unwrap();
        let evaluate=|after:Value| crate::engine::procedure::evaluate(
            &json!({"id":"method","verification":{"type":"procedure","after":after}}),&[],workspace);
        let unknown=evaluate(json!([{"kind":"file_equals","path":"unreadable.txt","text":"ok"}]));
        assert_eq!(unknown["verified"],true);
        assert_eq!(unknown["unknown"],true);
        let mut status_only=unknown.clone();status_only["unknown"]=Value::Null;
        let violated=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        let mixed=evaluate(json!([
            {"kind":"file_equals","path":"unreadable.txt","text":"ok"},
            {"kind":"file_equals","path":"known.txt","text":"wrong"}]));
        assert_eq!(mixed["unknown"],true);
        let passed=evaluate(json!([{"kind":"file_equals","path":"known.txt","text":"ok"}]));
        for (index,(result,status,verdict,postcondition)) in [
            (unknown,"held","inconclusive",Value::Null),
            (status_only,"held","inconclusive",Value::Null),
            (violated,"suspended","violated",json!(false)),
            (mixed,"suspended","violated",json!(false)),
            (passed,"executable","passed",json!(true)),
        ].into_iter().enumerate() {
            let id=format!("experience-{index}");
            state.store.put("experience",&json!({"id":id,"executableMethods":[
                {"id":"method","revision":"r1","status":"candidate","successfulApplications":0}]}));
            let session=format!("s-{index}");
            state.store.put("session",&json!({"id":session,"rsiMethodExecutions":[{
                "methodId":"method","revision":"r1","experienceId":id,
                "executionId":format!("exec-{index}"),"result":result}]}));
            let generation=json!({"jobId":"child","accepted":true,"newFindings":1});
            record_method_feedback(&state,&generation,&session);
            record_method_feedback(&state,&generation,&session);
            let record=state.store.get("experience",&id).unwrap();
            let method=&record["executableMethods"][0];
            assert_eq!(method["status"],status);
            assert_eq!(method["applications"].as_array().unwrap().len(),1);
            assert_eq!(method["applications"][0]["verificationVerdict"],verdict);
            assert_eq!(method["applications"][0]["postconditionsPassed"],postcondition);
            assert_eq!(method["successfulApplications"],0,
                "a passed procedure unrelated to a verified changed path receives no gain");
        }
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn learned_source_rebase_program_executes_a_fresh_hash_binding() {
        let original=json!({"id":"old-method","revision":"old-revision",
            "name":"source edit","domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":3,"endLine":3,
                    "newText":"let fixed = true;","expectedSha256":"old-sha"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let failed=json!({"id":"failed-op","origin":"procedure","status":"failed",
            "procedureStrategyId":"old-method","procedureRevision":"old-revision",
            "procedureNodeId":"edit","tool":"workspace_replace",
            "arguments":{"path":"src/engine/memory.rs","startLine":3,"endLine":3,
                "newText":"let fixed = true;","expectedSha256":"old-sha"},
            "result":{"error":"stale version","staleVersion":true}});
        let learned=synthesize_stale_edit_repair(&original,&failed).unwrap();
        assert_eq!(learned["status"],"candidate");
        assert_eq!(learned["synthesis"]["kind"],"stale_source_rebase");
        assert_ne!(learned["revision"],original["revision"]);
        let mut results=std::collections::HashMap::new();
        let (node,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory improvement",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["startLine"],3);
        let read_result=json!({"sourceSha256":"new-observed-hash",
            "path":"src/engine/memory.rs","content":"3 let old = true;"});
        let read_op=json!({"id":"observed-read","origin":"procedure",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":node,"tool":"workspace_read_range",
            "status":"completed","result":read_result});
        results.insert(node,read_result);
        let (next,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[read_op],"memory improvement",&results).unwrap();
        assert_eq!(next,"edit");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["expectedSha256"],"new-observed-hash");
        assert_eq!(args["newText"],"let fixed = true;");
        let mut fake=failed.clone();
        fake["result"]["staleVersion"]=json!(false);
        assert!(synthesize_stale_edit_repair(&original,&fake).is_none());
        assert!(synthesize_stale_edit_repair(&learned,&failed).is_none());
    }

    #[test]
    fn stale_oldtext_edit_rebases_hash_and_preserves_exact_patch() {
        let method=json!({"id":"old","revision":"old-sha","name":"text patch",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","oldText":"let before = 1;",
                    "newText":"let after = 2;","expectedSha256":"stale"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let op=json!({"id":"failed-oldtext","origin":"procedure",
            "status":"failed","tool":"workspace_replace",
            "procedureStrategyId":"old","procedureRevision":"old-sha",
            "procedureNodeId":"edit","arguments":{"path":"src/engine/memory.rs",
                "oldText":"let before = 1;"},"result":{"staleVersion":true}});
        let learned=synthesize_stale_edit_repair(&method,&op).unwrap();
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["startLine"],1);
        results.insert(read.clone(),json!({"sourceSha256":"fresh-hash"}));
        let done=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":read,"result":{"sourceSha256":"fresh-hash"}});
        let (_,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[done],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["oldText"],"let before = 1;");
        assert_eq!(args["newText"],"let after = 2;");
        assert_eq!(args["expectedSha256"],"fresh-hash");
        assert!(synthesize_stale_edit_repair(&learned,&op).is_none());
    }

    #[test]
    fn existing_file_write_failure_compiles_a_versioned_executable_dag() {
        let method=json!({"id":"write","revision":"write-v1","name":"patch source",
            "domain":"memory","assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"change","tool":"workspace_write","arguments":{
                    "path":"src/engine/memory.rs","content":"fn improved() {}\n"}}],
                "after":[{"kind":"tool_called","tool":"workspace_write",
                    "path":"src/engine/memory.rs"},
                    {"kind":"file_equals","path":"src/engine/memory.rs",
                    "text":"fn improved() {}\n"}]}});
        let failed=json!({"id":"failed-write","origin":"procedure",
            "status":"failed","tool":"workspace_write",
            "procedureStrategyId":"write","procedureRevision":"write-v1",
            "procedureNodeId":"change",
            "arguments":{"path":"src/engine/memory.rs","content":"fn improved() {}\n"},
            "result":{"error":"RSI의 workspace_write는 새 파일 생성 전용입니다."}});
        let learned=synthesize_existing_file_write_repair(&method,&failed).unwrap();
        assert_eq!(learned["synthesis"]["kind"],"existing_file_to_versioned_edit");
        assert_ne!(learned["revision"],method["revision"]);
        assert_eq!(learned["verification"]["after"][0]["tool"],"workspace_replace");
        assert_eq!(learned["verification"]["after"][1]["text"],"fn improved() {}\n");
        assert!(is_synthesized_source_repair(&learned));
        let mut results=std::collections::HashMap::new();
        let (read,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[],"memory",&results).unwrap();
        assert_eq!(tool,"workspace_read_range");
        assert_eq!(args["endLine"],120);
        results.insert(read.clone(),json!({"sourceSha256":"observed-hash",
            "totalLines":3}));
        let read_op=json!({"origin":"procedure","status":"completed",
            "procedureStrategyId":learned["id"],"procedureRevision":learned["revision"],
            "procedureNodeId":read,"result":{"sourceSha256":"observed-hash",
            "totalLines":3}});
        let (node,tool,args)=crate::engine::procedure::ready_intent(
            &learned,&[read_op],"memory",&results).unwrap();
        assert_eq!(node,"change");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["startLine"],1);
        assert_eq!(args["endLine"],3);
        assert_eq!(args["expectedSha256"],"observed-hash");
        assert_eq!(args["newText"],"fn improved() {}\n");
        let mut ungrounded=failed.clone();
        ungrounded["result"]["error"]=json!("permission denied");
        assert!(synthesize_existing_file_write_repair(&method,&ungrounded).is_none());
        ungrounded=failed.clone();
        ungrounded["arguments"]["path"]=json!("src/engine/other.rs");
        assert!(synthesize_existing_file_write_repair(&method,&ungrounded).is_none());
        assert!(synthesize_existing_file_write_repair(&learned,&failed).is_none());
    }

    #[test]
    fn indexed_hash_retry_is_a_different_executable_mutation() {
        let method=json!({"id":"original","revision":"original-rev",
            "name":"small edit","domain":"memory",
            "assistanceMode":"harness_assisted",
            "verification":{"type":"procedure","appliesTo":{"requestContains":["memory"]},
                "nodes":[{"id":"edit","tool":"workspace_replace","arguments":{
                    "path":"src/engine/memory.rs","startLine":3,"endLine":3,
                    "expectedSha256":"stale-hash","newText":"fn fixed() {}"}}],
                "after":[{"kind":"tool_called","tool":"workspace_replace",
                    "path":"src/engine/memory.rs"}]}});
        let op=json!({"id":"failed","origin":"procedure","status":"failed",
            "tool":"workspace_replace","procedureStrategyId":"original",
            "procedureRevision":"original-rev","procedureNodeId":"edit",
            "arguments":{"path":"src/engine/memory.rs","startLine":3,"endLine":3},
            "result":{"error":"stale","staleVersion":true}});
        let observed=synthesize_stale_edit_repair(&method,&op).unwrap();
        let indexed=synthesize_indexed_edit_repair(&method,&op).unwrap();
        assert_ne!(observed["id"],indexed["id"]);
        assert_eq!(indexed["synthesis"]["kind"],"indexed_source_rebase");
        assert_eq!(indexed["verification"]["nodes"].as_array().unwrap().len(),1);
        assert!(indexed["verification"]["nodes"][0]["arguments"]["expectedSha256"].is_null());
        let (id,tool,args)=crate::engine::procedure::ready_intent(
            &indexed,&[],"memory",&std::collections::HashMap::new()).unwrap();
        assert_eq!(id,"edit");
        assert_eq!(tool,"workspace_replace");
        assert_eq!(args["startLine"],3);
        assert!(args["expectedSha256"].is_null(),
            "the tool must use the generation-bound repository index");
        let mut old_text=method.clone();
        old_text["verification"]["nodes"][0]["arguments"]["oldText"]=json!("before");
        let mut old_op=op.clone();
        old_op["arguments"]["oldText"]=json!("before");
        assert!(synthesize_indexed_edit_repair(&old_text,&old_op).is_none(),
            "unversioned oldText replacement is not an allowed alternative");
    }

